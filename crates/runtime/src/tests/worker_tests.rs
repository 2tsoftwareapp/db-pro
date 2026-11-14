//! Tests for `../worker.rs` (moved out of the source file).
use super::*;

#[tokio::test]
async fn cancelling_connection_test_drops_the_active_future() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(AtomicBool::new(false));
    let test_dropped = Arc::clone(&dropped);
    let test = async move {
        let _drop_flag = DropFlag(test_dropped);
        std::future::pending::<()>().await;
        Some(())
    };
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let task = tokio::spawn(await_test_or_cancel(test, cancel_rx));
    tokio::task::yield_now().await;
    cancel_tx.send(()).expect("test should still be waiting");

    assert_eq!(task.await.expect("cancellation task should finish"), None);
    assert!(dropped.load(Ordering::SeqCst));
}
#[tokio::test]
async fn cancel_test_connection_command_closes_the_pending_provider_socket() {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    let data_dir = std::env::temp_dir().join(format!("db-pro-cancel-test-{}", uuid::Uuid::new_v4()));
    let runtime = DbProRuntime::new(&data_dir)
        .await
        .expect("test runtime should initialize");
    let (command_tx, mut event_rx) = spawn_worker(runtime, 8);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), event_rx.recv())
            .await
            .expect("worker should announce readiness"),
        Some(RuntimeEvent::AgentProviderReady { .. })
    ));

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("local stalled provider should bind");
    let port = listener.local_addr().expect("listener address").port();
    let request_id = RuntimeRequestId(41);
    let config = db_pro_core::domain::connection::ConnectionConfig {
        name: "cancellation fixture".to_owned(),
        host: "127.0.0.1".to_owned(),
        port,
        database: "fixture".to_owned(),
        username: "fixture".to_owned(),
        driver: db_pro_core::domain::connection::DriverType::Postgres,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: None,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};
    command_tx
        .send(RuntimeCommand::TestConnection {
            request_id,
            config,
            password: String::new(),
        })
        .await
        .expect("test command should reach worker");
    let (mut provider_socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("test should connect to the stalled provider")
        .expect("provider should accept the test socket");
    let mut startup_message = [0; 1];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), provider_socket.read(&mut startup_message))
            .await
            .expect("PostgreSQL startup message should arrive")
            .expect("startup read should succeed"),
        1
    );

    command_tx
        .send(RuntimeCommand::CancelTestConnection { request_id })
        .await
        .expect("cancel command should reach worker");
    assert!(
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut remaining = [0; 256];
            loop {
                if provider_socket
                    .read(&mut remaining)
                    .await
                    .expect("socket close read should succeed")
                    == 0
                {
                    break;
                }
            }
        })
        .await
        .is_ok(),
        "cancel should close the provider socket"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), event_rx.recv())
            .await
            .is_err(),
        "cancelled request must not emit a completion or failure event"
    );

    drop(command_tx);
    assert!(tokio::time::timeout(Duration::from_secs(3), event_rx.recv())
        .await
        .expect("worker should stop after its command channel closes")
        .is_none());
    std::fs::remove_dir_all(&data_dir).expect("temporary runtime data should be removed");
}
#[test]
fn prediction_backoff_is_bounded_and_only_for_transient_provider_errors() {
    assert_eq!(
        prediction_backoff(&CodexProviderError::Http {
            status: 429,
            body: String::new(),
        }),
        Some(std::time::Duration::from_secs(2))
    );
    assert_eq!(
        prediction_backoff(&CodexProviderError::Http {
            status: 503,
            body: String::new(),
        }),
        Some(std::time::Duration::from_secs(1))
    );
    assert_eq!(prediction_backoff(&CodexProviderError::EmptyResponse), None);
}

#[tokio::test]
async fn disconnect_closes_sqlite_memory_and_reconnect_opens_a_new_database() {
    use std::time::Duration;

    let data_dir = std::env::temp_dir().join(format!("db-pro-disconnect-{}", uuid::Uuid::new_v4()));
    let runtime = DbProRuntime::new(&data_dir)
        .await
        .expect("test runtime should initialize");
    let summary = runtime
        .connection_api()
        .create(
            db_pro_core::domain::connection::ConnectionConfig {
                name: "memory".to_owned(),
                host: String::new(),
                port: 0,
                database: ":memory:".to_owned(),
                username: String::new(),
                driver: db_pro_core::domain::connection::DriverType::SQLite,
                ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
                ssh_tunnel: None,
                ssh_profile_id: None,
                ssl_root_cert_path: None,
                ssl_client_cert_path: None,
                ssl_client_key_path: None,
                query_timeout_ms: 30_000,
                max_rows: 500,
                color: None,
                tags: Vec::new(),
                group: None,
                favorite: false,
                environment: Default::default(),
                readonly: false,
                create_if_missing: false,
},
            "",
        )
        .await
        .expect("sqlite memory connection should be saved");
    let (command_tx, mut event_rx) = spawn_worker(runtime, 8);
    let ready = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("worker should announce readiness");
    assert!(matches!(ready, Some(RuntimeEvent::AgentProviderReady { .. })));

    let connection_id = summary.id;
    let mut request = 1_u64;
    let send_query = |sql: &str, request: &mut u64| {
        *request += 1;
        RuntimeCommand::ExecuteQuery {
            request_id: RuntimeRequestId(*request),
            connection_id: connection_id.clone(),
            sql: sql.to_owned(),
            params: Vec::new(),
            schema: None,
            session_id: None,
        }
    };

    command_tx
        .send(RuntimeCommand::Connect {
            request_id: RuntimeRequestId(request),
            connection_id: connection_id.clone(),
        })
        .await
        .expect("connect should reach the worker");
    let connected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("connect should finish")
        .expect("worker should stay open");
    assert!(matches!(connected, RuntimeEvent::Connected { .. }));

    command_tx
        .send(send_query("CREATE TABLE probe (id INTEGER)", &mut request))
        .await
        .expect("create should reach the worker");
    let created = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("create should finish")
        .expect("worker should stay open");
    assert!(matches!(created, RuntimeEvent::QueryCompleted { .. }), "{created:?}");

    command_tx
        .send(send_query("INSERT INTO probe (id) VALUES (1)", &mut request))
        .await
        .expect("insert should reach the worker");
    let inserted = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("insert should finish")
        .expect("worker should stay open");
    assert!(matches!(inserted, RuntimeEvent::QueryCompleted { .. }), "{inserted:?}");

    command_tx
        .send(RuntimeCommand::Disconnect {
            request_id: RuntimeRequestId(90),
            connection_id: connection_id.clone(),
        })
        .await
        .expect("disconnect should reach the worker");
    let disconnected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("disconnect should finish")
        .expect("worker should stay open");
    assert!(matches!(
        disconnected,
        RuntimeEvent::OperationCompleted {
            operation: "connection.disconnected",
            ..
        }
    ));

    command_tx
        .send(send_query("SELECT id FROM probe", &mut request))
        .await
        .expect("query after disconnect should reach the worker");
    let closed = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("query after disconnect should finish")
        .expect("worker should stay open");
    match closed {
        RuntimeEvent::QueryFailedDetailed { error, .. } => {
            assert!(error.message.contains("is not active"), "{}", error.message);
        }
        other => panic!("closed handle should reject the query, got {other:?}"),
    }

    command_tx
        .send(RuntimeCommand::Connect {
            request_id: RuntimeRequestId(91),
            connection_id: connection_id.clone(),
        })
        .await
        .expect("reconnect should reach the worker");
    let reconnected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("reconnect should finish")
        .expect("worker should stay open");
    assert!(matches!(reconnected, RuntimeEvent::Connected { .. }), "{reconnected:?}");

    command_tx
        .send(send_query("SELECT id FROM probe", &mut request))
        .await
        .expect("query after reconnect should reach the worker");
    let fresh = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("query after reconnect should finish")
        .expect("worker should stay open");
    match fresh {
        RuntimeEvent::QueryFailedDetailed { error, .. } => {
            assert!(
                error.message.to_ascii_lowercase().contains("no such table"),
                "{}",
                error.message
            );
        }
        other => panic!("a new memory database has no probe table, got {other:?}"),
    }

    drop(command_tx);
    std::fs::remove_dir_all(&data_dir).expect("temporary runtime data should be removed");
}

#[tokio::test]
async fn execute_query_returns_a_row_from_a_sqlite_file() {
    use std::time::Duration;

    let data_dir = std::env::temp_dir().join(format!("db-pro-file-query-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_dir).expect("query fixture dir");
    let database = data_dir.join("shop.sqlite");
    let runtime = DbProRuntime::new(&data_dir)
        .await
        .expect("test runtime should initialize");
    let summary = runtime
        .connection_api()
        .create(
            db_pro_core::domain::connection::ConnectionConfig {
                name: "shop".to_owned(),
                host: String::new(),
                port: 0,
                database: database.display().to_string(),
                username: String::new(),
                driver: db_pro_core::domain::connection::DriverType::SQLite,
                ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
                ssh_tunnel: None,
                ssh_profile_id: None,
                ssl_root_cert_path: None,
                ssl_client_cert_path: None,
                ssl_client_key_path: None,
                query_timeout_ms: 30_000,
                max_rows: 500,
                color: None,
                tags: Vec::new(),
                group: None,
                favorite: false,
                environment: Default::default(),
                readonly: false,
                create_if_missing: false,
},
            "",
        )
        .await
        .expect("sqlite file connection should be saved");
    let (command_tx, mut event_rx) = spawn_worker(runtime, 8);
    let _ = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("worker should announce readiness");
    let connection_id = summary.id;
    command_tx
        .send(RuntimeCommand::Connect {
            request_id: RuntimeRequestId(1),
            connection_id: connection_id.clone(),
        })
        .await
        .expect("connect should reach the worker");
    let connected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("connect should finish")
        .expect("worker should stay open");
    assert!(matches!(connected, RuntimeEvent::Connected { .. }), "{connected:?}");

    for (request_id, sql) in [
        (2_u64, "CREATE TABLE products (name TEXT)"),
        (3, "INSERT INTO products (name) VALUES ('keyboard')"),
    ] {
        command_tx
            .send(RuntimeCommand::ExecuteQuery {
                request_id: RuntimeRequestId(request_id),
                connection_id: connection_id.clone(),
                sql: sql.to_owned(),
                params: Vec::new(),
                schema: None,
                session_id: None,
            })
            .await
            .expect("statement should reach the worker");
        let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("statement should finish")
            .expect("worker should stay open");
        assert!(matches!(event, RuntimeEvent::QueryCompleted { .. }), "{sql}: {event:?}");
    }

    command_tx
        .send(RuntimeCommand::ExecuteQuery {
            request_id: RuntimeRequestId(4),
            connection_id,
            sql: "SELECT name FROM products".to_owned(),
            params: Vec::new(),
            schema: None,
            session_id: None,
        })
        .await
        .expect("select should reach the worker");
    let selected = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("select should finish")
        .expect("worker should stay open");
    match selected {
        RuntimeEvent::QueryCompleted { result, .. } => {
            assert_eq!(result.row_count, 1, "{result:?}");
            assert!(
                matches!(result.rows[0].0.first(), Some(db_pro_core::domain::query::CellValue::Text(value)) if value == "keyboard"),
                "{result:?}"
            );
        }
        other => panic!("read-only query did not complete: {other:?}"),
    }
    drop(command_tx);
    std::fs::remove_dir_all(&data_dir).expect("temporary runtime data should be removed");
}
