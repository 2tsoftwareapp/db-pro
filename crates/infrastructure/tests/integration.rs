//! SQLite integration tests using the fixture database.
//!
//! These tests run against an in-memory SQLite database loaded with
//! the fixture schema and seed data from `fixtures/sqlite/fixture.sql`.
//!
//! Run with: `cargo test --package db-pro-infrastructure --test integration`

use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{CellValue, QueryParam};
use db_pro_core::ports::{
    DbConnector, ParameterizedTransactionStatement, TransactionFailureOutcome, TransactionFailurePhase,
};
use db_pro_infrastructure::sqlite::actor::SqliteActor;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;
use std::sync::Arc;

/// Path to the SQLite fixture file, relative to workspace root.
const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/sqlite/fixture.sql");

/// Create a SQLite connector with an in-memory fixture database.
async fn setup_fixture() -> (SQLiteConnector, db_pro_core::domain::connection::ConnectionHandle) {
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
        query_timeout_ms: 30_000,
        max_rows: 10_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };

    let handle = connector.connect(&config, "").await.expect("connect failed");

    // Load fixture SQL.
    let fixture_sql = std::fs::read_to_string(FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("failed to read fixture at {FIXTURE_PATH}: {e}"));

    // Split SQL into statements, respecting BEGIN...END blocks (triggers).
    for statement in split_sql_statements(&fixture_sql) {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        connector
            .execute(&handle, trimmed, &[])
            .await
            .unwrap_or_else(|e| panic!("fixture SQL failed for: {trimmed:?}: {e}"));
    }

    (connector, handle)
}

// ═══════════════════════════════════════════════════════════════════════════
// Connection tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn connect_and_query() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.query(&handle, "SELECT 1 AS num", &[]).await.unwrap();
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "num");
    assert_eq!(result.rows.len(), 1);
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
async fn sqlite_typed_temporal_and_network_parameters_remain_text() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(
            &handle,
            "SELECT typeof(?) AS time_type, typeof(?) AS interval_type, typeof(?) AS inet_type",
            &[
                QueryParam::Time("12:34:56.123456".into()),
                QueryParam::Interval("1 days 02:00:00".into()),
                QueryParam::Inet("192.0.2.1/24".into()),
            ],
        )
        .await
        .unwrap();

    assert!(result.rows[0].0.iter().all(|cell| matches!(
        cell,
        CellValue::Text(value) if value == "text"
    )));
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
async fn sqlite_parameterized_table_mutations_roll_back_and_reject_zero_rows() {
    let (connector, handle) = setup_fixture().await;
    let product_id = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
    let failure = connector
        .execute_parameterized_transaction(
            &handle,
            &[
                ParameterizedTransactionStatement {
                    sql: "UPDATE products SET name = ? WHERE id = ?".into(),
                    params: vec![QueryParam::Text("Changed".into()), QueryParam::Text(product_id.into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                },
                ParameterizedTransactionStatement {
                    sql: "UPDATE products SET missing_column = ? WHERE id = ?".into(),
                    params: vec![QueryParam::Text("invalid".into()), QueryParam::Text(product_id.into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                },
            ],
        )
        .await
        .expect_err("a failed statement must roll back the previous update");
    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);

    let restored = connector
        .query(
            &handle,
            "SELECT name FROM products WHERE id = ?",
            &[QueryParam::Text(product_id.into())],
        )
        .await
        .unwrap();
    assert!(matches!(&restored.rows[0].0[0], CellValue::Text(name) if name == "Laptop Pro"));

    let zero_rows = connector
        .execute_parameterized_transaction(
            &handle,
            &[ParameterizedTransactionStatement {
                sql: "UPDATE products SET name = ? WHERE id = ?".into(),
                params: vec![QueryParam::Text("missing".into()), QueryParam::Text("not-a-row".into())],
                expect_affected_rows: true,
                max_affected_rows: Some(1),
            }],
        )
        .await
        .expect_err("an expected mutation affecting zero rows must fail");
    assert_eq!(zero_rows.phase, TransactionFailurePhase::Statement);
    assert_eq!(zero_rows.outcome, TransactionFailureOutcome::RolledBack);
    assert!(matches!(zero_rows.error, DbError::Conflict(message) if message.contains("affected no rows")));

    let too_many_rows = connector
        .execute_parameterized_transaction(
            &handle,
            &[ParameterizedTransactionStatement {
                sql: "UPDATE products SET name = ?".into(),
                params: vec![QueryParam::Text("unsafe broad update".into())],
                expect_affected_rows: true,
                max_affected_rows: Some(1),
            }],
        )
        .await
        .expect_err("a row-identity mutation affecting multiple rows must fail");
    assert_eq!(too_many_rows.phase, TransactionFailurePhase::Statement);
    assert_eq!(too_many_rows.statement_index, 0);
    assert_eq!(too_many_rows.outcome, TransactionFailureOutcome::RolledBack);
    assert!(matches!(too_many_rows.error, DbError::Internal(message) if message.contains("expected at most 1")));
}

#[tokio::test]
async fn test_connection_succeeds() {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "test".into(),
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
        query_timeout_ms: 5_000,
        max_rows: 100,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    connector.test_connection(&config, "").await.unwrap();
}

#[tokio::test]
async fn sqlite_query_timeout_interrupts_vm_and_actor_recovers() {
    // The connector applies one timeout to every query. Use its underlying actor
    // API to isolate the short interrupt deadline from the recovery assertion.
    let actor = SqliteActor::spawn(":memory:").expect("in-memory actor should spawn");

    let query = "WITH RECURSIVE numbers(value) AS (\
        SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 100000\
    ) SELECT count(*) FROM numbers first_numbers CROSS JOIN numbers second_numbers";
    let error = actor
        .execute(query.into(), vec![], 100, 1)
        .await
        .expect_err("the deliberately expensive query should time out");
    assert!(matches!(error, DbError::QueryTimeout { timeout_ms: 1 }));

    let recovery = actor
        .execute("SELECT 1".into(), vec![], 100, 30_000)
        .await
        .expect("the same actor should serve queries after the interrupted VM exits");
    assert_eq!(recovery.row_count, 1);
    assert!(matches!(recovery.rows[0].0[0], CellValue::Int64(1)));
    actor.shutdown().await;
}

#[tokio::test]
async fn sqlite_query_cancel_interrupts_vm_and_waits_for_actor_recovery() {
    let connector = Arc::new(SQLiteConnector::new());
    let config = ConnectionConfig {
        name: "cancel-test".into(),
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
        query_timeout_ms: 30_000,
        max_rows: 100,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    let handle = connector.connect(&config, "").await.unwrap();
    let expensive_query = "WITH RECURSIVE numbers(value) AS (\
        SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 100000\
    ) SELECT count(*) FROM numbers first_numbers CROSS JOIN numbers second_numbers";

    let query_connector = Arc::clone(&connector);
    let query_task = tokio::spawn(async move { query_connector.query(&handle, expensive_query, &[]).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    connector.cancel(&handle).await.unwrap();
    assert!(query_task.await.unwrap().is_err(), "cancelled query must not succeed");

    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
}

#[tokio::test]
async fn sqlite_transaction_timeout_waits_for_rollback_before_returning() {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "transaction-timeout-test".into(),
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
        query_timeout_ms: 10,
        max_rows: 100,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    let handle = connector.connect(&config, "").await.unwrap();
    connector
        .execute(&handle, "CREATE TABLE timeout_probe (id INTEGER)", &[])
        .await
        .unwrap();

    let expensive_read = "WITH RECURSIVE numbers(value) AS (\
        SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 100000\
    ) SELECT count(*) FROM numbers first_numbers CROSS JOIN numbers second_numbers";
    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "INSERT INTO timeout_probe (id) VALUES (1)".into(),
                expensive_read.into(),
            ],
            &[false, true],
        )
        .await
        .expect_err("the transaction should exceed its configured deadline");
    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    assert!(matches!(failure.error, DbError::QueryTimeout { timeout_ms: 10 }));

    let count = connector
        .query(&handle, "SELECT COUNT(*) FROM timeout_probe", &[])
        .await
        .unwrap();
    assert!(matches!(
        count.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Int64(0)
    ));
}

#[tokio::test]
async fn sqlite_transaction_commit_failure_reports_unknown_outcome() {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "transaction-commit-failure-test".into(),
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
        query_timeout_ms: 30_000,
        max_rows: 100,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    let handle = connector.connect(&config, "").await.unwrap();
    connector
        .execute(&handle, "PRAGMA foreign_keys = ON", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE TABLE parent (id INTEGER PRIMARY KEY)", &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "CREATE TABLE child (parent_id INTEGER, FOREIGN KEY(parent_id) REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)",
            &[],
        )
        .await
        .unwrap();

    let failure = connector
        .execute_transaction(
            &handle,
            &["INSERT INTO child (parent_id) VALUES (999)".into(), "SELECT 1".into()],
            &[false, true],
        )
        .await
        .expect_err("deferred foreign-key violation must fail at commit");

    assert_eq!(failure.phase, TransactionFailurePhase::Commit);
    assert_eq!(failure.outcome, TransactionFailureOutcome::Unknown);
    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.results.len(), 2);
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
async fn sqlite_execute_batch_timeout_waits_for_rollback_before_returning() {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "batch-timeout-test".into(),
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
        query_timeout_ms: 10,
        max_rows: 100,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    let handle = connector.connect(&config, "").await.unwrap();
    connector
        .execute(&handle, "CREATE TABLE batch_timeout_probe (id INTEGER)", &[])
        .await
        .unwrap();

    let expensive_read = "WITH RECURSIVE numbers(value) AS (\
        SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 100000\
    ) SELECT count(*) FROM numbers first_numbers CROSS JOIN numbers second_numbers";
    let error = connector
        .execute_batch(
            &handle,
            &[
                "INSERT INTO batch_timeout_probe (id) VALUES (1)".into(),
                expensive_read.into(),
            ],
        )
        .await
        .expect_err("the batch should exceed its configured deadline");
    assert!(matches!(error, DbError::QueryTimeout { timeout_ms: 10 }));

    let count = connector
        .query(&handle, "SELECT COUNT(*) FROM batch_timeout_probe", &[])
        .await
        .unwrap();
    assert!(matches!(
        count.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Int64(0)
    ));
    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Query tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn query_categories() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM categories ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 4);
    assert_eq!(result.columns.len(), 3); // id, name, description
}

#[tokio::test]
async fn query_products_with_json() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT name, metadata FROM products ORDER BY id LIMIT 1", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 1);
    assert_eq!(result.columns[0].name, "name");
    assert_eq!(result.columns[1].name, "metadata");
}

#[tokio::test]
async fn query_unicode_table() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM \"Ünïcödé Üsers\" ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 4);
}

#[tokio::test]
async fn query_weird_name_table() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM \"weird\"\"name\" ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 2);
}

#[tokio::test]
async fn query_view_active_users() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM active_users ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 3); // 3 active users (carol is inactive)
}

#[tokio::test]
async fn query_view_order_summary() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM order_summary ORDER BY order_id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 4);
}

#[tokio::test]
async fn query_empty_table() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM empty_table", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 0);
}

#[tokio::test]
async fn query_table_without_pk() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM audit_logs ORDER BY created_at", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 3);
}

#[tokio::test]
async fn query_composite_pk() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT * FROM order_items ORDER BY order_id, product_id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 6);
}

#[tokio::test]
async fn query_with_invalid_sql() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.query(&handle, "SELCT BROKEN", &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn query_documents_with_blob() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(&handle, "SELECT title, binary_data FROM documents ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 3);
}

#[tokio::test]
async fn query_columns_reflect_declared_types() {
    let (connector, handle) = setup_fixture().await;
    let result = connector
        .query(
            &handle,
            "SELECT c.id, c.name, c.description, p.price FROM categories c JOIN products p ON p.category_id = c.id LIMIT 1",
            &[],
        )
        .await
        .unwrap();

    let col_types: Vec<&str> = result.columns.iter().map(|c| c.data_type.as_str()).collect();
    assert_eq!(col_types[0], "INTEGER", "categories.id should be INTEGER");
    assert_eq!(col_types[1], "TEXT", "categories.name should be TEXT");
    assert_eq!(col_types[2], "TEXT", "categories.description should be TEXT");
    assert_eq!(col_types[3], "REAL", "products.price should be REAL");
}

// ═══════════════════════════════════════════════════════════════════════════
// Metadata / Introspection tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn introspect_tables() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    // Should find all fixture tables.
    let table_names: Vec<&str> = result.tables.iter().map(|t| t.name.as_str()).collect();
    assert!(table_names.contains(&"categories"));
    assert!(table_names.contains(&"products"));
    assert!(table_names.contains(&"orders"));
    assert!(table_names.contains(&"order_items"));
    assert!(table_names.contains(&"audit_logs"));
    assert!(table_names.contains(&"empty_table"));
    assert!(table_names.contains(&"documents"));
    assert!(table_names.contains(&"employees"));
    // Unicode and weird-name tables.
    assert!(table_names.iter().any(|n| n.contains("Ünïcödé")));
    assert!(table_names.iter().any(|n| n.contains("weird")));
}

#[tokio::test]
async fn introspect_columns_include_nullable() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    // categories.description is nullable.
    let desc_col = result.columns.iter().find(|c| c.name == "description").unwrap();
    assert!(desc_col.nullable);

    // categories.name is NOT NULL.
    let name_col = result
        .columns
        .iter()
        .find(|c| c.name == "name" && c.table_name == "categories")
        .unwrap();
    assert!(!name_col.nullable);
}

#[tokio::test]
async fn introspect_primary_keys() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    // categories has single-column PK.
    let cat_pk = result
        .primary_keys
        .iter()
        .find(|pk| pk.table_name == "categories")
        .unwrap();
    assert_eq!(cat_pk.columns, vec!["id"]);

    // order_items has composite PK.
    let oi_pk = result
        .primary_keys
        .iter()
        .find(|pk| pk.table_name == "order_items")
        .unwrap();
    assert_eq!(oi_pk.columns.len(), 2);

    // audit_logs has no PK.
    let has_audit_pk = result.primary_keys.iter().any(|pk| pk.table_name == "audit_logs");
    assert!(!has_audit_pk);
}

#[tokio::test]
async fn introspect_indexes() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    // Should find at least the fixture indexes.
    assert!(result.indexes.len() >= 3);
    let idx_names: Vec<&str> = result.indexes.iter().map(|i| i.name.as_str()).collect();
    assert!(idx_names.contains(&"idx_products_category"));
    assert!(idx_names.contains(&"idx_orders_user"));
}

#[tokio::test]
async fn introspect_foreign_keys() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    // order_items has FKs referencing both orders and products.
    let oi_fks: Vec<_> = result
        .foreign_keys
        .iter()
        .filter(|fk| fk.from_table == "order_items")
        .collect();
    assert!(oi_fks.len() >= 2, "expected at least 2 FKs from order_items");
    assert!(oi_fks
        .iter()
        .any(|fk| fk.from_columns.contains(&"order_id".to_string()) && fk.to_table == "orders"));
    assert!(oi_fks
        .iter()
        .any(|fk| fk.from_columns.contains(&"product_id".to_string()) && fk.to_table == "products"));
}

#[tokio::test]
async fn introspect_views() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    assert!(result.views.len() >= 2);
    let view_names: Vec<&str> = result.views.iter().map(|v| v.name.as_str()).collect();
    assert!(view_names.contains(&"active_users"));
    assert!(view_names.contains(&"order_summary"));

    let active_user_cols: Vec<_> = result
        .columns
        .iter()
        .filter(|c| c.table_name == "active_users")
        .collect();
    assert_eq!(active_user_cols.len(), 3);
    let active_user_col_names: Vec<&str> = active_user_cols.iter().map(|c| c.name.as_str()).collect();
    assert!(active_user_col_names.contains(&"id"));
    assert!(active_user_col_names.contains(&"émâil"));
    assert!(active_user_col_names.contains(&"name"));

    let order_summary_cols: Vec<_> = result
        .columns
        .iter()
        .filter(|c| c.table_name == "order_summary")
        .collect();
    assert_eq!(order_summary_cols.len(), 4);
}

#[tokio::test]
async fn introspect_triggers() {
    let (connector, handle) = setup_fixture().await;
    let result = connector.introspect(&handle).await.unwrap();

    assert!(!result.triggers.is_empty());
    let trigger = result
        .triggers
        .iter()
        .find(|t| t.name == "products_updated_at")
        .expect("fixture trigger should exist");

    // Verify all S4 fields are populated.
    assert_eq!(trigger.schema, "main");
    assert_eq!(trigger.table_name, "products");
    assert_eq!(trigger.timing, "AFTER");
    assert_eq!(trigger.event, "UPDATE");
    assert!(
        trigger.definition.contains("products_updated_at"),
        "definition should contain the CREATE TRIGGER SQL"
    );
    assert!(trigger.enabled, "SQLite triggers are always enabled");
}

// ═══════════════════════════════════════════════════════════════════════════
// Data mutation tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn insert_and_query() {
    let (connector, handle) = setup_fixture().await;
    let affected = connector
        .execute(
            &handle,
            "INSERT INTO categories (name, description) VALUES ('Toys', 'Fun stuff')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(affected, 1);

    let result = connector
        .query(&handle, "SELECT COUNT(*) AS cnt FROM categories", &[])
        .await
        .unwrap();
    assert_eq!(result.row_count, 1);
}

#[tokio::test]
async fn update_row() {
    let (connector, handle) = setup_fixture().await;
    let affected = connector
        .execute(&handle, "UPDATE categories SET name = 'Gadgets' WHERE id = 1", &[])
        .await
        .unwrap();
    assert_eq!(affected, 1);

    let result = connector
        .query(&handle, "SELECT name FROM categories WHERE id = 1", &[])
        .await
        .unwrap();
    // Verify the value via serialization.
    let json = serde_json::to_value(&result.rows[0].0[0]).unwrap();
    assert_eq!(json, serde_json::json!({"type": "text", "value": "Gadgets"}));
}

#[tokio::test]
async fn delete_row() {
    let (connector, handle) = setup_fixture().await;
    // Delete an order_item first (referenced by composite PK).
    let affected = connector
        .execute(
            &handle,
            "DELETE FROM order_items WHERE order_id = 2 AND product_id = 'b1ffcd00-ad1c-4f99-cc7e-7cc0ce491b22'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(affected, 1);

    let result = connector
        .query(&handle, "SELECT COUNT(*) AS cnt FROM order_items", &[])
        .await
        .unwrap();
    // COUNT(*) returns 1 row; check the actual count value.
    let json = serde_json::to_value(&result.rows[0].0[0]).unwrap();
    assert_eq!(json, serde_json::json!({"type": "int64", "value": "5"}));
}

// ═══════════════════════════════════════════════════════════════════════════
// Explain tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn explain_select() {
    let (connector, handle) = setup_fixture().await;
    let plan = connector
        .explain(&handle, "SELECT * FROM categories WHERE id = 1", &[], false)
        .await
        .unwrap();
    // SQLite returns a JSON array of plan steps.
    assert!(plan.is_array() || plan.is_object() || plan.is_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// SQL statement splitter (handles BEGIN...END blocks)
// ═══════════════════════════════════════════════════════════════════════════

/// Split SQL text into individual statements, respecting BEGIN...END blocks
/// and string literals. This is a simplified parser sufficient for fixture loading.
// cc-scan:allow HARD_COMPLEXITY — SQL statement splitter for tests: quote/comment state ladder is one cohesive unit
fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut in_string = false;
    let mut begin_depth: u32 = 0;

    while let Some(ch) = chars.next() {
        if in_string {
            current.push(ch);
            if ch == '\'' {
                // Check for escaped quote ('').
                if chars.peek() == Some(&'\'') {
                    current.push(chars.next().unwrap());
                } else {
                    in_string = false;
                }
            }
            continue;
        }

        match ch {
            '\'' => {
                in_string = true;
                current.push(ch);
            }
            '-' if chars.peek() == Some(&'-') => {
                // Line comment: skip to end of line.
                current.push(ch);
                current.push(chars.next().unwrap());
                while let Some(&c) = chars.peek() {
                    current.push(chars.next().unwrap());
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {
                current.push(ch);
                // Track BEGIN/END for trigger bodies.
                let upper_so_far = current.trim().to_ascii_uppercase();
                if upper_so_far.ends_with("BEGIN") && begin_depth == 0 {
                    // Check it's actually the keyword BEGIN.
                    let before_begin = &upper_so_far[..upper_so_far.len() - 5];
                    if before_begin.is_empty() || before_begin.ends_with(|c: char| !c.is_alphanumeric()) {
                        begin_depth = 1;
                    }
                } else if begin_depth > 0 {
                    // Check for nested BEGIN.
                    let trimmed = current.trim().to_ascii_uppercase();
                    if trimmed.ends_with("BEGIN") && !trimmed.ends_with("END BEGIN") {
                        let before = &trimmed[..trimmed.len() - 5];
                        if before.is_empty() || before.ends_with(|c: char| !c.is_alphanumeric()) {
                            begin_depth += 1;
                        }
                    }
                }

                if ch == ';' && begin_depth == 0 {
                    let stmt = current.trim().to_string();
                    if !stmt.is_empty() {
                        statements.push(stmt);
                    }
                    current.clear();
                }

                // Check for END; that closes a BEGIN block.
                if ch == ';' && begin_depth > 0 {
                    let trimmed = current.trim().to_ascii_uppercase();
                    // Count ENDs in the current statement.
                    let ends_count = trimmed.matches("END").count();
                    let begins_count = trimmed.matches("BEGIN").count();
                    if ends_count >= begins_count {
                        begin_depth = 0;
                        let stmt = current.trim().to_string();
                        if !stmt.is_empty() {
                            statements.push(stmt);
                        }
                        current.clear();
                    }
                }
            }
        }
    }

    // Handle final statement without trailing semicolon.
    let remaining = current.trim().to_string();
    if !remaining.is_empty() {
        statements.push(remaining);
    }

    statements
}
