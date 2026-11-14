use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;

async fn setup_sqlite_connector() -> (SQLiteConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "test-fixture".into(),
        host: String::new(),
        port: 0,
        database: ":memory:".into(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 10000,
        max_rows: 1000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};
    let handle = connector.connect(&config, "").await.unwrap();
    (connector, handle)
}

#[tokio::test]
async fn verify_create_and_drop_index() {
    let (connector, handle) = setup_sqlite_connector().await;

    // Create a table to test indexes on
    connector
        .execute(
            &handle,
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL);",
            &[],
        )
        .await
        .unwrap();

    // Introspect - there should be no indexes (apart from implicit PK stuff which we ignore for this test, or check manually)
    let result = connector.introspect(&handle).await.unwrap();
    assert!(!result.indexes.iter().any(|idx| idx.name == "idx_test_unique_email"));

    // 1. Create a Unique Index
    connector
        .execute(
            &handle,
            "CREATE UNIQUE INDEX idx_test_unique_email ON users(email);",
            &[],
        )
        .await
        .unwrap();

    // 2. Introspect and Verify Unique Index
    let result = connector.introspect(&handle).await.unwrap();
    let email_idx = result
        .indexes
        .iter()
        .find(|idx| idx.name == "idx_test_unique_email")
        .expect("Index should exist");
    assert!(email_idx.unique, "Index should be unique");
    assert_eq!(email_idx.columns, vec!["email"], "Index should be on email column");

    // 3. Create a Composite Index
    connector
        .execute(&handle, "CREATE INDEX idx_test_composite ON users(email, name);", &[])
        .await
        .unwrap();

    // 4. Introspect and Verify Composite Index
    let result = connector.introspect(&handle).await.unwrap();
    let composite_idx = result
        .indexes
        .iter()
        .find(|idx| idx.name == "idx_test_composite")
        .expect("Composite index should exist");
    assert!(!composite_idx.unique, "Index should not be unique");
    assert_eq!(
        composite_idx.columns,
        vec!["email", "name"],
        "Index should be on email and name columns"
    );

    // 5. Drop Index
    connector
        .execute(&handle, "DROP INDEX idx_test_unique_email;", &[])
        .await
        .unwrap();

    // 6. Introspect and Verify dropped index
    let result = connector.introspect(&handle).await.unwrap();
    assert!(
        !result.indexes.iter().any(|idx| idx.name == "idx_test_unique_email"),
        "Index should be dropped"
    );
    assert!(
        result.indexes.iter().any(|idx| idx.name == "idx_test_composite"),
        "Composite index should still exist"
    );
}
