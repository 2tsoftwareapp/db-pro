//! Tests for `../monitoring_service.rs` (moved out of the source file).
use super::*;
use crate::domain::connection::{ConnectionConfig, ConnectionHandle, ConnectionId, DriverType, SslMode};
use crate::domain::monitoring::{LocalMonitorState, MonitorSession};
use crate::ports::{MockConnectionRepository, MockMonitoringPort};
use mockall::predicate::eq;
use std::sync::Arc;

fn config(driver: DriverType) -> ConnectionConfig {
    ConnectionConfig {
        name: "t".into(),
        host: "127.0.0.1".into(),
        port: 5432,
        database: "db".into(),
        username: "u".into(),
        driver,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
    }
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn snapshot_lists_postgres_sessions_via_port() {
    let id = ConnectionId::new();
    let handle = ConnectionHandle::new(1);
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));

    let mut port = MockMonitoringPort::new();
    port.expect_list_sessions().returning(|_| {
        Ok(vec![MonitorSession {
            backend_id: 42,
            database: Some("db".into()),
            username: Some("u".into()),
            application_name: Some("db-pro".into()),
            client_addr: None,
            state: Some("active".into()),
            wait_event_type: None,
            wait_event: None,
            query_text: Some("SELECT 1".into()),
            query_duration_ms: Some(12),
            backend_start: None,
            xact_start: None,
            query_start: None,
            xact_age_ms: None,
            backend_age_ms: None,
            idle_in_transaction: false,
            is_current: false,
        }])
    });
    port.expect_list_locks().returning(|_| Ok(Vec::new()));
    port.expect_relation_sizes().returning(|_, _| Ok(Vec::new()));
    port.expect_server_summary().returning(|_| Ok(None));
    port.expect_stat_statements().returning(|_, _, _| {
        Ok(crate::domain::monitoring::StatStatementsSnapshot {
            extension_present: true,
            extension_version: Some("1.10".into()),
            message: "ok".into(),
            statements: Vec::new(),
            sort: crate::domain::monitoring::StatStatementSort::TotalTime,
            fetched_at_ms: 0,
        })
    });

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, handle);

    let service = MonitoringService::new(
        Box::new(port),
        Box::new(MockMonitoringPort::new()),
        registry,
        Box::new(repo),
    );
    let snap = service
        .snapshot(&id, crate::domain::monitoring::StatStatementSort::TotalTime)
        .await
        .unwrap();
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].backend_id, 42);
    assert_eq!(snap.active_queries().len(), 1);
    assert!(snap.workload.as_ref().is_some_and(|w| w.extension_present));
}

#[tokio::test]
async fn sqlite_snapshot_uses_local_state_not_fake_sessions() {
    let id = ConnectionId::new();
    let handle = ConnectionHandle::new(1);
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::SQLite);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));

    let mut sqlite = MockMonitoringPort::new();
    sqlite.expect_list_sessions().times(0);
    sqlite.expect_local_state().returning(|_| {
        Ok(Some(LocalMonitorState {
            journal_mode: Some("wal".into()),
            page_count: Some(10),
            page_size: Some(4096),
            freelist_count: Some(0),
            file_size_bytes: Some(40960),
            note: "local only".into(),
        }))
    });

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, handle);

    let service = MonitoringService::new(
        Box::new(MockMonitoringPort::new()),
        Box::new(sqlite),
        registry,
        Box::new(repo),
    );
    let snap = service
        .snapshot(&id, crate::domain::monitoring::StatStatementSort::TotalTime)
        .await
        .unwrap();
    assert!(snap.sessions.is_empty());
    assert_eq!(snap.local.as_ref().unwrap().journal_mode.as_deref(), Some("wal"));
    assert!(snap.workload.is_none());
}

#[tokio::test]
async fn snapshot_uses_selected_sort_reports_lock_failure_and_unsupported_drivers() {
    let id = ConnectionId::new();
    let handle = ConnectionHandle::new(1);
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));

    let mut port = MockMonitoringPort::new();
    port.expect_list_sessions().returning(|_| Ok(Vec::new()));
    port.expect_list_locks()
        .returning(|_| Err(DbError::QueryFailed("permission denied for pg_locks".into())));
    port.expect_relation_sizes().returning(|_, _| Ok(Vec::new()));
    port.expect_server_summary().returning(|_| Ok(None));
    port.expect_stat_statements()
        .withf(|_, sort, limit| *sort == crate::domain::monitoring::StatStatementSort::Calls && *limit == 100)
        .returning(|_, sort, _| {
            Ok(crate::domain::monitoring::StatStatementsSnapshot {
                extension_present: true,
                extension_version: None,
                message: String::new(),
                statements: Vec::new(),
                sort,
                fetched_at_ms: 0,
            })
        });

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, handle);
    let service = MonitoringService::new(
        Box::new(port),
        Box::new(MockMonitoringPort::new()),
        registry,
        Box::new(repo),
    );
    let snap = service
        .snapshot(&id, crate::domain::monitoring::StatStatementSort::Calls)
        .await
        .unwrap();
    assert_eq!(
        snap.workload.as_ref().map(|workload| workload.sort),
        Some(crate::domain::monitoring::StatStatementSort::Calls)
    );
    let lock_error = snap.lock_error.expect("lock failure is kept");
    assert!(lock_error.contains("permission denied for pg_locks"), "{lock_error}");

    for (driver, expected) in [
        (DriverType::Mysql, "MySQL has no sessions"),
        (DriverType::SqlServer, "SQL Server has no sessions"),
    ] {
        let mut repo = MockConnectionRepository::new();
        let cfg = config(driver);
        repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));
        let service = MonitoringService::new(
            Box::new(MockMonitoringPort::new()),
            Box::new(MockMonitoringPort::new()),
            Arc::new(ConnectionRegistry::new()),
            Box::new(repo),
        );
        let error = service
            .snapshot(&id, crate::domain::monitoring::StatStatementSort::TotalTime)
            .await
            .expect_err("unsupported driver");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[tokio::test]
async fn reset_stat_statements_requires_confirmation() {
    let id = ConnectionId::new();
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));
    let service = MonitoringService::new(
        Box::new(MockMonitoringPort::new()),
        Box::new(MockMonitoringPort::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(repo),
    );
    let err = service.reset_stat_statements(&id, false).await.unwrap_err();
    assert!(err.to_string().contains("confirmation"));
}

#[tokio::test]
async fn cancel_rejects_non_positive_backend_id() {
    let id = ConnectionId::new();
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));
    let port = MockMonitoringPort::new();
    let registry = Arc::new(ConnectionRegistry::new());
    let service = MonitoringService::new(
        Box::new(port),
        Box::new(MockMonitoringPort::new()),
        registry,
        Box::new(repo),
    );
    let err = service
        .cancel_backend(&id, 0, "2026-10-17 08:15:00+00")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("positive"));
}

#[tokio::test]
async fn cancel_and_terminate_reject_missing_backend_start() {
    // Without the snapshot's backend_start the pid is ambiguous — signalling a
    // possibly-recycled pid could kill an innocent session (#501).
    let id = ConnectionId::new();
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));
    let service = MonitoringService::new(
        Box::new(MockMonitoringPort::new()),
        Box::new(MockMonitoringPort::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(repo),
    );
    let err = service.cancel_backend(&id, 42, "").await.unwrap_err();
    assert!(err.to_string().contains("backend_start"), "{err}");
    let err = service.terminate_backend(&id, 42, "   ").await.unwrap_err();
    assert!(err.to_string().contains("backend_start"), "{err}");
}

#[tokio::test]
async fn cancel_targets_exact_backend_id() {
    let id = ConnectionId::new();
    let handle = ConnectionHandle::new(7);
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));

    let mut port = MockMonitoringPort::new();
    port.expect_cancel_backend()
        .with(eq(handle), eq(99_i64), eq("2026-10-17 08:15:00+00"))
        .returning(|_, _, _| Ok(true));

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, handle);
    let service = MonitoringService::new(
        Box::new(port),
        Box::new(MockMonitoringPort::new()),
        registry,
        Box::new(repo),
    );
    assert!(service.cancel_backend(&id, 99, "2026-10-17 08:15:00+00").await.unwrap());
}

#[tokio::test]
async fn maintenance_requires_confirmation() {
    let id = ConnectionId::new();
    let mut repo = MockConnectionRepository::new();
    let cfg = config(DriverType::Postgres);
    repo.expect_get_config().returning(move |_| Ok(Some(cfg.clone())));
    let service = MonitoringService::new(
        Box::new(MockMonitoringPort::new()),
        Box::new(MockMonitoringPort::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(repo),
    );
    let err = service
        .run_maintenance(
            &id,
            Some("public"),
            Some("t"),
            crate::domain::monitoring::MaintenanceAction::Analyze,
            false,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("confirmation"));
}
