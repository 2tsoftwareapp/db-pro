use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::domain::query::{CellValue, QueryParam};
use db_pro_core::ports::{DbConnector, ParameterizedTransactionStatement, TransactionFailureOutcome};
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;
async fn setup_sqlite_connector() -> (SQLiteConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "schema-columns-atomicity".into(),
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
async fn execute_batch_middle_failure_rolls_back_prior_ddl() {
    let (connector, handle) = setup_sqlite_connector().await;

    let statements = vec![
        "CREATE TABLE rollback_probe (id INTEGER PRIMARY KEY)".to_string(),
        "THIS IS NOT VALID SQL".to_string(),
        "CREATE TABLE should_never_exist (id INTEGER PRIMARY KEY)".to_string(),
    ];

    let result = connector.execute_batch(&handle, &statements).await;
    assert!(result.is_err(), "middle-statement failure must fail the batch");

    let introspection = connector.introspect(&handle).await.unwrap();
    assert!(
        !introspection.tables.iter().any(|table| table.name == "rollback_probe"),
        "the first DDL statement must be rolled back when a later statement fails"
    );
    assert!(
        !introspection
            .tables
            .iter()
            .any(|table| table.name == "should_never_exist"),
        "statements after the failure must not execute"
    );
}

#[tokio::test]
async fn parameterized_transaction_failure_rolls_back_replace_delete() {
    let (connector, handle) = setup_sqlite_connector().await;
    connector
        .execute(
            &handle,
            "CREATE TABLE replace_probe (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
            &[],
        )
        .await
        .expect("create target table");
    connector
        .execute(
            &handle,
            "INSERT INTO replace_probe (id, name) VALUES (?1, ?2)",
            &[QueryParam::Text("1".into()), QueryParam::Text("Existing".into())],
        )
        .await
        .expect("insert existing target row");

    let error = connector
        .execute_parameterized_transaction(
            &handle,
            &[
                ParameterizedTransactionStatement {
                    sql: "DELETE FROM replace_probe WHERE id = ?1".into(),
                    params: vec![QueryParam::Text("1".into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                },
                ParameterizedTransactionStatement {
                    sql: "THIS IS NOT VALID SQL".into(),
                    params: Vec::new(),
                    expect_affected_rows: false,
                    max_affected_rows: None,
                },
            ],
        )
        .await
        .expect_err("the injected invalid statement must fail");

    assert_eq!(error.statement_index, 1);
    assert_eq!(error.outcome, TransactionFailureOutcome::RolledBack);
    let target = connector
        .query(
            &handle,
            "SELECT name FROM replace_probe WHERE id = ?1",
            &[QueryParam::Text("1".into())],
        )
        .await
        .expect("read target after rollback");
    assert_eq!(target.rows.len(), 1);
    match &target.rows[0].0[0] {
        CellValue::Text(value) => assert_eq!(value, "Existing"),
        other => panic!("unexpected name cell after rollback: {other:?}"),
    }
}
