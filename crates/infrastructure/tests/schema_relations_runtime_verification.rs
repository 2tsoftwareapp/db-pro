use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;

async fn setup_sqlite_connector() -> (SQLiteConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "schema-relations-runtime".into(),
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
        query_timeout_ms: 10_000,
        max_rows: 1_000,
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
async fn composite_foreign_key_preserves_constraint_identity_and_order() {
    let (connector, handle) = setup_sqlite_connector().await;

    connector
        .execute(&handle, "PRAGMA foreign_keys = ON", &[])
        .await
        .unwrap();

    // Verify FK enforcement is actually active
    let fk_status = connector.query(&handle, "PRAGMA foreign_keys", &[]).await.unwrap();
    assert!(
        matches!(
            fk_status.rows.first().map(|r| &r.0[0]),
            Some(db_pro_core::domain::query::CellValue::Int64(1))
        ),
        "PRAGMA foreign_keys must be ON for FK enforcement assertions to be meaningful"
    );

    connector
        .execute(
            &handle,
            "CREATE TABLE parent (tenant_id INTEGER NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (tenant_id, id))",
            &[],
        )
        .await
        .unwrap();

    connector
        .execute(
            &handle,
            "CREATE TABLE child (id INTEGER PRIMARY KEY, tenant_id INTEGER NOT NULL, parent_id INTEGER NOT NULL, FOREIGN KEY (tenant_id, parent_id) REFERENCES parent (tenant_id, id))",
            &[],
        )
        .await
        .unwrap();

    let introspection = connector.introspect(&handle).await.unwrap();
    let fk_rows: Vec<_> = introspection
        .foreign_keys
        .iter()
        .filter(|fk| fk.from_table == "child" && fk.to_table == "parent")
        .collect();

    assert_eq!(fk_rows.len(), 1, "composite FK should be grouped into a single entry");
    assert_eq!(fk_rows[0].from_columns, vec!["tenant_id", "parent_id"]);
    assert_eq!(fk_rows[0].to_columns, vec!["tenant_id", "id"]);

    connector
        .execute(&handle, "INSERT INTO parent (tenant_id, id) VALUES (7, 42)", &[])
        .await
        .unwrap();

    connector
        .execute(
            &handle,
            "INSERT INTO child (id, tenant_id, parent_id) VALUES (1, 7, 42)",
            &[],
        )
        .await
        .unwrap();

    let invalid_insert = connector
        .execute(
            &handle,
            "INSERT INTO child (id, tenant_id, parent_id) VALUES (2, 7, 999)",
            &[],
        )
        .await;

    assert!(
        invalid_insert.is_err(),
        "SQLite foreign-key enforcement must reject invalid relation rows"
    );
}
