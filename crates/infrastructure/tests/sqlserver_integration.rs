//! Live SQL Server verification for issue #260.
//!
//! The tests are intentionally ignored in the normal workspace run. They need
//! a real SQL Server fixture and a `SQLSERVER_URL`, for example:
//!
//!   SQLSERVER_URL='sqlserver://sa:Pass%40123@127.0.0.1:1433/dbpro_fixture' \
//!   cargo test -p db-pro-infrastructure --test sqlserver_integration -- --ignored

use db_pro_core::domain::cloud_presets::parse_connection_snippet;
use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{CellValue, QueryParam};
use db_pro_core::ports::{DbConnector, TransactionFailureOutcome, TransactionFailurePhase};
use db_pro_infrastructure::sqlserver::connector::SqlServerConnector;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn sqlserver_config() -> Option<(ConnectionConfig, String)> {
    let raw = std::env::var("SQLSERVER_URL").ok()?;
    let parsed = parse_connection_snippet(&raw).ok()?;
    if parsed.driver != DriverType::SqlServer {
        return None;
    }
    let password = parsed.password.unwrap_or_default();
    Some((
        ConnectionConfig {
            name: "sqlserver-test".into(),
            host: parsed.host,
            port: parsed.port,
            database: parsed.database,
            username: parsed.username,
            driver: DriverType::SqlServer,
            ssl_mode: parsed.ssl_mode.unwrap_or(SslMode::Require),
            ssh_tunnel: None,
            ssh_profile_id: None,
            ssl_root_cert_path: None,
            ssl_client_cert_path: None,
            ssl_client_key_path: None,
            query_timeout_ms: 30_000,
            max_rows: 10_000,
            color: None,
            tags: vec![],
            group: None,
            favorite: false,
            environment: Default::default(),
            readonly: false,
            create_if_missing: false,
},
        password,
    ))
}

async fn setup() -> Option<(SqlServerConnector, ConnectionHandle)> {
    let (config, password) = sqlserver_config()?;
    let connector = SqlServerConnector::new();
    let handle = connector.connect(&config, &password).await.ok()?;
    Some((connector, handle))
}

#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_connects_and_queries() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };

    let result = connector
        .query(&handle, "SELECT CAST(1 AS bigint) AS one", &[])
        .await
        .expect("SQL Server query");
    assert_eq!(result.columns[0].name, "one");
    assert!(matches!(result.rows[0].0[0], CellValue::Int64(1)));
    connector.disconnect(&handle).await.expect("disconnect");
}

#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_preserves_parameterized_decimal_and_bigint() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };

    // tiberius declares positional RPC parameters as `@P1..@PN` (1-based),
    // matching the product convention in core `sql_builder::PlaceholderWriter`.
    let result = connector
        .query(
            &handle,
            "SELECT CAST(@P1 AS decimal(38, 10)) AS exact_decimal, CAST(@P2 AS bigint) AS exact_bigint",
            &[
                QueryParam::Decimal("123456789012345678.1234567890".into()),
                QueryParam::Int64(9_223_372_036_854_775_000),
            ],
        )
        .await
        .expect("SQL Server parameterized query");
    assert!(matches!(
        &result.rows[0].0[0],
        CellValue::Decimal(value) if value == "123456789012345678.1234567890"
    ));
    assert!(matches!(
        result.rows[0].0[1],
        CellValue::Int64(9_223_372_036_854_775_000)
    ));
    connector.disconnect(&handle).await.expect("disconnect");
}

#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_introspects_catalog() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };

    let result = connector.introspect(&handle).await.expect("SQL Server introspection");
    assert!(!result.schemas.is_empty(), "SQL Server fixture should expose schemas");
    connector.disconnect(&handle).await.expect("disconnect");
}

#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_transaction_rolls_back_on_failure() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };

    connector
        .execute(&handle, "DROP TABLE IF EXISTS dbo.dbpro_transaction_probe", &[])
        .await
        .ok();
    connector
        .execute(
            &handle,
            "CREATE TABLE dbo.dbpro_transaction_probe (id bigint NOT NULL)",
            &[],
        )
        .await
        .expect("create transaction probe");
    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "INSERT INTO dbo.dbpro_transaction_probe VALUES (1)".into(),
                "INSERT INTO dbo.dbpro_transaction_probe (missing_column) VALUES (2)".into(),
            ],
            &[false, false],
        )
        .await
        .expect_err("transaction should fail");
    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    connector
        .execute(&handle, "DROP TABLE dbo.dbpro_transaction_probe", &[])
        .await
        .expect("drop transaction probe");
    connector.disconnect(&handle).await.expect("disconnect");
}

#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live writable SQL Server database"]
async fn sqlserver_foreign_key_actions_round_trip_through_schema_service() {
    use db_pro_core::application::{ConnectionRegistry, SchemaService};
    use db_pro_core::domain::connection::Connection;
    use db_pro_core::ports::ConnectionRepository;
    use db_pro_infrastructure::meta::store::SQLiteMetaStore;
    use std::sync::Arc;

    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };
    let connector = Arc::new(connector);
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let parent = format!("dbpro442_action_parent_{suffix}");
    connector
        .execute(
            &handle,
            &format!("CREATE TABLE dbo.[{parent}] (id INTEGER NOT NULL PRIMARY KEY)"),
            &[],
        )
        .await
        .unwrap();
    let mut children = Vec::new();
    for (label, action) in [("none", "NO ACTION"), ("null", "SET NULL"), ("default", "SET DEFAULT")] {
        let child = format!("dbpro442_action_{label}_{suffix}");
        connector
            .execute(
                &handle,
                &format!(
                    "CREATE TABLE dbo.[{child}] (id INTEGER NOT NULL PRIMARY KEY, parent_id INTEGER NULL DEFAULT 0, \
                     CONSTRAINT [fk_{child}] FOREIGN KEY (parent_id) REFERENCES dbo.[{parent}] (id) \
                     ON UPDATE {action} ON DELETE {action})"
                ),
                &[],
            )
            .await
            .expect("create FK action fixture");
        children.push((child, action));
    }
    let model = connector.introspect(&handle).await.unwrap();
    for (child, action) in &children {
        let foreign_key = model
            .foreign_keys
            .iter()
            .find(|item| item.from_table == *child)
            .unwrap();
        assert_eq!(foreign_key.on_update, *action);
        assert_eq!(foreign_key.on_delete, *action);
    }
    let (config, _) = sqlserver_config().unwrap();
    let connection = Connection::new(config);
    let store = SQLiteMetaStore::new(":memory:").await.unwrap();
    ConnectionRepository::save(&store, &connection).await.unwrap();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(connection.id, handle);
    let service = SchemaService::new(
        Box::new(Arc::clone(&connector)),
        Box::new(store.clone()),
        registry,
        Box::new(store),
    );
    for (child, _) in &children {
        let ddl = service.get_table_ddl(&connection.id, "dbo", child).await.unwrap();
        connector
            .execute(&handle, &format!("DROP TABLE dbo.[{child}]"), &[])
            .await
            .unwrap();
        service
            .execute_ddl(&connection.id, &ddl)
            .await
            .expect("recreate canonical FK actions");
        connector
            .execute(&handle, &format!("DROP TABLE dbo.[{child}]"), &[])
            .await
            .unwrap();
    }
    connector
        .execute(&handle, &format!("DROP TABLE dbo.[{parent}]"), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Issue #260: `cancel()` opens a second tiberius connection, issues `KILL <spid>`
/// against the session running `WAITFOR DELAY`, and adopts that connection as the
/// handle's new primary. The killed statement must error promptly (not after the
/// 10-second delay elapses) and the handle must remain usable afterwards.
#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_cancel_kills_running_query_via_out_of_band_spid() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };
    let connector = Arc::new(connector);
    let runner = Arc::clone(&connector);
    let running = tokio::spawn(async move { runner.query(&handle, "WAITFOR DELAY '00:00:10'", &[]).await });

    // Single-session runtime: give the spawned task a moment to acquire the
    // connection lock and put WAITFOR on the wire before Stop is pressed.
    tokio::time::sleep(Duration::from_millis(1_000)).await;

    let started = Instant::now();
    connector.cancel(&handle).await.expect("KILL <spid> should succeed");
    let result = running.await.expect("query task should return, not hang");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "WAITFOR ran for {elapsed:?}; the session was never killed (a 10s delay would keep the query alive)"
    );
    let error = result.expect_err("killed session must surface a query error");
    assert!(
        matches!(error, DbError::QueryFailed(_)),
        "expected a cancellation-flavoured query error, got {error:?}"
    );

    // The killed connection is replaced transparently: the next query succeeds.
    let probe = connector
        .query(&handle, "SELECT CAST(1 AS bigint) AS one", &[])
        .await
        .expect("handle should work on the rotated session after cancel");
    assert!(matches!(probe.rows[0].0[0], CellValue::Int64(1)));
    connector.disconnect(&handle).await.expect("disconnect");
}

/// Issue #260: `config.query_timeout_ms` is now enforced on SQL Server query paths
/// (it was previously ignored). The timeout returns the canonical `QueryTimeout`
/// variant AND stops the runaway query, because tiberius would otherwise block the
/// next statement behind the unconsumed response.
#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_query_timeout_is_enforced_and_recovers_the_session() {
    let Some((mut config, password)) = sqlserver_config() else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };
    config.query_timeout_ms = 1_500;
    let connector = SqlServerConnector::new();
    let handle = connector.connect(&config, &password).await.expect("connect");

    let started = Instant::now();
    let error = connector
        .query(&handle, "WAITFOR DELAY '00:00:10'", &[])
        .await
        .expect_err("a 1.5s deadline must cut a 10s WAITFOR");
    let elapsed = started.elapsed();
    assert!(
        matches!(error, DbError::QueryTimeout { timeout_ms: 1_500 }),
        "expected the canonical QueryTimeout {{ timeout_ms: 1500 }}, got {error:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout fired after {elapsed:?}; the runaway query was not killed server-side"
    );

    // Same recovery guarantee as cancel: the rotated session serves new queries.
    let probe = connector
        .query(&handle, "SELECT CAST(1 AS bigint) AS one", &[])
        .await
        .expect("handle should work on the rotated session after timeout");
    assert!(matches!(probe.rows[0].0[0], CellValue::Int64(1)));
    connector.disconnect(&handle).await.expect("disconnect");
}

/// Stop on an idle session must not kill it (PostgreSQL connector parity): the
/// active-statement guard is empty, so `cancel()` succeeds without a round-trip and
/// the very next query still runs on the same SPID.
#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_cancel_on_idle_session_keeps_the_connection() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };
    let before = connector
        .query(&handle, "SELECT CAST(@@SPID AS int) AS spid", &[])
        .await
        .expect("spid probe");
    connector.cancel(&handle).await.expect("idle cancel should succeed");
    let after = connector
        .query(&handle, "SELECT CAST(@@SPID AS int) AS spid", &[])
        .await
        .expect("spid probe after idle cancel");
    assert!(
        matches!(
            (&before.rows[0].0[0], &after.rows[0].0[0]),
            (CellValue::Int64(a), CellValue::Int64(b)) if a == b
        ),
        "idle Stop must not rotate the session"
    );
    connector.disconnect(&handle).await.expect("disconnect");
}

/// Issue #260 (audit item 6): `query.explain: true` needs live evidence beyond the
/// capability flag — `SqlServerConnector::explain` wraps the statement in a single
/// `SET SHOWPLAN_TEXT ON; …; SET SHOWPLAN_TEXT OFF` batch (the only shape that takes
/// effect, since SHOWPLAN does not reach `sp_executesql` child sessions) and must
/// return an actual plan row without executing the query.
#[tokio::test]
#[ignore = "requires SQLSERVER_URL and a live SQL Server fixture"]
async fn sqlserver_showplan_text_explain_returns_a_plan_row() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping SQL Server integration test: SQLSERVER_URL is unavailable");
        return;
    };

    let plan = connector
        .explain(&handle, "SELECT CAST(1 AS int) AS one", &[], false)
        .await
        .expect("SHOWPLAN_TEXT explain should return a plan result set");
    let rendered = plan.to_string();
    assert!(
        rendered.to_ascii_uppercase().contains("SELECT"),
        "the plan JSON must describe the SELECT operator, got: {rendered}"
    );

    // SHOWPLAN was switched off inside the same batch: the session executes normally.
    let probe = connector
        .query(&handle, "SELECT CAST(1 AS bigint) AS one", &[])
        .await
        .expect("session should execute queries again after explain");
    assert!(matches!(probe.rows[0].0[0], CellValue::Int64(1)));
    connector.disconnect(&handle).await.expect("disconnect");
}
