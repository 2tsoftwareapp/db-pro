//! Tests for `../user_service.rs` (moved out of the source file).
use super::*;
use crate::domain::connection::{ConnectionConfig, SslMode};
use crate::ports::{MockConnectionRepository, MockUserManager};

#[tokio::test]
async fn user_management_rejects_sqlite_before_provider_call() {
    let config = ConnectionConfig {
        name: "local-db".into(),
        host: String::new(),
        port: 0,
        database: "/tmp/db-pro-test.sqlite".into(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
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
    assert!(config.validate().is_ok());

    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get_config()
        .returning(move |_| Ok(Some(config.clone())));

    let service = UserService::new(
        Box::new(MockUserManager::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(connections),
    );
    let error = service
        .list_users(&ConnectionId::new())
        .await
        .expect_err("SQLite must not enter PostgreSQL user-management provider");

    assert!(matches!(error, DbError::Unsupported(message) if message.contains("PostgreSQL")));
}
