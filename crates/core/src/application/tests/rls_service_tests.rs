//! Tests for `../rls_service.rs` (moved out of the source file).
use super::*;
use crate::domain::connection::SslMode;
use crate::ports::{MockConnectionRepository, MockRlsManager};

#[tokio::test]
async fn rls_rejects_sqlite_before_provider_call() {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
    let config = ConnectionConfig {
        name: "local".into(),
        host: String::new(),
        port: 0,
        database: "/tmp/t.sqlite".into(),
        username: String::new(),
        driver: DriverType::SQLite,
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
};
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get_config()
        .returning(move |_| Ok(Some(config.clone())));
    let service = RlsService::new(
        Box::new(MockRlsManager::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(connections),
    );
    let err = service
        .table_rls_state(&ConnectionId::new(), "public", "t")
        .await
        .expect_err("sqlite gated");
    assert!(matches!(err, DbError::Unsupported(_)));
}
