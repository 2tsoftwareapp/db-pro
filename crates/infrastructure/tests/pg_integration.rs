//! PostgreSQL integration tests using the fixture database.
//!
//! These tests connect to a real PostgreSQL instance with fixtures loaded.
//! The CI workflow starts a PostgreSQL 16 service container and loads
//! `fixtures/postgres/001_schema.sql` + `002_seed.sql`.
//!
//! Run locally with:
//!   DATABASE_URL=postgres://dbpro:dbpro_test@localhost:5432/dbpro_fixture \
//!   cargo test --package db-pro-infrastructure --test pg_integration -- --ignored
//!
//! Tests are marked `#[ignored]` so they only run when DATABASE_URL is set.

use db_pro_core::application::query_service::QueryService;
use db_pro_core::application::registry::ConnectionRegistry;
use db_pro_core::application::sql_builder::{build_count, FilterOp, TableFilter};
use db_pro_core::application::with_query_session;
use db_pro_core::domain::connection::{Connection, ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use db_pro_core::domain::query::{CellValue, QueryParam, QueryResult};
use db_pro_core::ports::{
    ConnectionRepository, DbConnector, TransactionFailureOutcome, TransactionFailurePhase, TransactionStatementResult,
};
use db_pro_infrastructure::meta::store::SQLiteMetaStore;
use db_pro_infrastructure::postgres::connector::PostgresConnector;
use std::sync::Arc;

/// Build a ConnectionConfig from the DATABASE_URL environment variable.
/// Returns None if DATABASE_URL is not set (tests will be skipped).
fn pg_config() -> Option<ConnectionConfig> {
    let url = std::env::var("DATABASE_URL").ok()?;
    // Parse postgres://user:pass@host:port/dbname
    let without_scheme = url.strip_prefix("postgres://")?;
    let (auth_host_db, _) = without_scheme.split_once('?').unwrap_or((without_scheme, ""));

    // Split auth from host+db
    let (auth, host_db) = if auth_host_db.contains('@') {
        let (a, rest) = auth_host_db.rsplit_once('@').unwrap();
        (Some(a), rest)
    } else {
        (None, auth_host_db)
    };

    let (username, _password) = auth
        .map(|a| {
            let (u, p) = a.split_once(':').unwrap_or((a, ""));
            (u.to_string(), p.to_string())
        })
        .unwrap_or_else(|| ("postgres".to_string(), String::new()));

    let (host_port, database) = host_db.rsplit_once('/').unwrap_or((host_db, "postgres"));
    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        (h.to_string(), p.parse().unwrap_or(5432u16))
    } else {
        (host_port.to_string(), 5432u16)
    };

    Some(ConnectionConfig {
        name: "pg-test".into(),
        host,
        port,
        database: database.to_string(),
        username,
        driver: DriverType::Postgres,
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
        create_if_missing: false,
})
}

/// The password half of `DATABASE_URL`, for the connect calls that `pg_config()` does not carry.
fn pg_password() -> String {
    std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("postgres://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default()
}

async fn setup() -> (PostgresConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let config = pg_config().expect("DATABASE_URL must be set for PG integration tests");
    let password = pg_password();

    let connector = PostgresConnector::new();
    let handle = connector.connect(&config, &password).await.expect("PG connect failed");
    (connector, handle)
}

#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_parameterized_transaction_failure_rolls_back_replace_delete() {
    let (connector, handle) = setup().await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let table = format!("dbpro_copy_rollback_{}_{}", std::process::id(), suffix);
    let qualified_table = format!("public.{table}");

    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {qualified_table} (id TEXT PRIMARY KEY, name TEXT NOT NULL)"),
            &[],
        )
        .await
        .expect("create isolated copy target");
    connector
        .execute(
            &handle,
            &format!("INSERT INTO {qualified_table} (id, name) VALUES ($1, $2)"),
            &[QueryParam::Text("1".into()), QueryParam::Text("Existing".into())],
        )
        .await
        .expect("seed isolated copy target");

    let error = connector
        .execute_parameterized_transaction(
            &handle,
            &[
                db_pro_core::ports::ParameterizedTransactionStatement {
                    sql: format!("DELETE FROM {qualified_table} WHERE id = $1"),
                    params: vec![QueryParam::Text("1".into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                },
                db_pro_core::ports::ParameterizedTransactionStatement {
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
            &format!("SELECT name FROM {qualified_table} WHERE id = $1"),
            &[QueryParam::Text("1".into())],
        )
        .await
        .expect("read target after rollback");
    assert_eq!(target.rows.len(), 1);
    match &target.rows[0].0[0] {
        CellValue::Text(value) => assert_eq!(value, "Existing"),
        other => panic!("unexpected name cell after rollback: {other:?}"),
    }

    connector
        .execute(&handle, &format!("DROP TABLE {qualified_table}"), &[])
        .await
        .expect("drop isolated copy target");
    connector.disconnect(&handle).await.expect("disconnect fixture");
}

/// The joined run must undo only its own statements. A failed statement inside it is
/// rolled back to the run's savepoint, which is what keeps the tab's already-open
/// transaction usable; without that undo the connection sits in PostgreSQL's
/// aborted-transaction state and the next statement fails with 25P02. The tab's own
/// uncommitted work has to survive both.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_joined_parameterized_transaction_rolls_back_only_its_own_statements() {
    let (connector, handle) = setup().await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let table = format!("public.dbpro_join_tx_{}_{}", std::process::id(), suffix);

    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {table} (id TEXT PRIMARY KEY, name TEXT NOT NULL)"),
            &[],
        )
        .await
        .expect("create joined-run fixture");

    let ids = |rows: &QueryResult| -> Vec<String> {
        rows.rows
            .iter()
            .map(|row| match &row.0[0] {
                CellValue::Text(value) => value.clone(),
                other => panic!("unexpected id cell: {other:?}"),
            })
            .collect()
    };

    with_query_session(Some("pg-join-tab"), async {
        connector
            .execute(&handle, "BEGIN", &[])
            .await
            .expect("BEGIN opens the tab transaction");
        connector
            .execute(
                &handle,
                &format!("INSERT INTO {table} (id, name) VALUES ($1, $2)"),
                &[QueryParam::Text("tab".into()), QueryParam::Text("Tab work".into())],
            )
            .await
            .expect("tab statement inside its own transaction");

        let failure = connector
            .execute_parameterized_transaction(
                &handle,
                &[
                    db_pro_core::ports::ParameterizedTransactionStatement {
                        sql: format!("INSERT INTO {table} (id, name) VALUES ($1, $2)"),
                        params: vec![QueryParam::Text("joined".into()), QueryParam::Text("Joined".into())],
                        expect_affected_rows: true,
                        max_affected_rows: Some(1),
                    },
                    db_pro_core::ports::ParameterizedTransactionStatement {
                        sql: "THIS IS NOT VALID SQL".into(),
                        params: Vec::new(),
                        expect_affected_rows: false,
                        max_affected_rows: None,
                    },
                ],
            )
            .await
            .expect_err("the invalid statement must fail the joined run");

        assert_eq!(failure.phase, TransactionFailurePhase::Statement);
        assert_eq!(failure.statement_index, 1);
        assert_eq!(failure.results.len(), 1, "the statement before the failure did run");
        assert_eq!(
            failure.outcome,
            TransactionFailureOutcome::LeftOpen,
            "a confirmed savepoint undo leaves the tab's transaction open"
        );

        let inside = connector
            .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
            .await
            .expect("the tab transaction must still accept statements after the joined failure");
        assert_eq!(
            ids(&inside),
            vec!["tab".to_string()],
            "the joined write must be undone while the tab's own work is kept"
        );

        connector
            .execute(&handle, "COMMIT", &[])
            .await
            .expect("commit the tab transaction");
    })
    .await;

    let after = connector
        .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
        .await
        .expect("read after commit");
    assert_eq!(
        ids(&after),
        vec!["tab".to_string()],
        "the joined run's write must not survive the tab's commit"
    );

    connector
        .execute(&handle, &format!("DROP TABLE {table}"), &[])
        .await
        .expect("drop joined-run fixture");
    connector.disconnect(&handle).await.expect("disconnect fixture");
}

/// A run staged while the tab holds `BEGIN` belongs to the tab's transaction, not to its own:
/// the tab's `ROLLBACK` must undo the run's writes. On a separate pool connection the run would
/// commit alone and survive the tab's rollback.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_joined_parameterized_transaction_rolls_back_with_the_tab() {
    let (connector, handle) = setup().await;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let table = format!("public.dbpro_join_tx_{}_{}", std::process::id(), suffix);

    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {table} (id TEXT PRIMARY KEY, name TEXT NOT NULL)"),
            &[],
        )
        .await
        .expect("create joined fixture");

    let ids = |rows: &QueryResult| -> Vec<String> {
        rows.rows
            .iter()
            .map(|row| match &row.0[0] {
                CellValue::Text(value) => value.clone(),
                other => panic!("unexpected id cell: {other:?}"),
            })
            .collect()
    };

    with_query_session(Some("pg-join-tab"), async {
        connector
            .execute(&handle, "BEGIN", &[])
            .await
            .expect("BEGIN opens the tab transaction");
        connector
            .execute(
                &handle,
                &format!("INSERT INTO {table} (id, name) VALUES ($1, $2)"),
                &[QueryParam::Text("tab".into()), QueryParam::Text("Tab work".into())],
            )
            .await
            .expect("tab statement inside its own transaction");

        let results = connector
            .execute_parameterized_transaction(
                &handle,
                &[db_pro_core::ports::ParameterizedTransactionStatement {
                    sql: format!("INSERT INTO {table} (id, name) VALUES ($1, $2)"),
                    params: vec![QueryParam::Text("joined".into()), QueryParam::Text("Joined".into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                }],
            )
            .await
            .expect("the joined run succeeds");
        assert!(
            matches!(
                results.as_slice(),
                [TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: _
                }]
            ),
            "the run reports its own affected row, got {results:?}"
        );

        let inside = connector
            .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
            .await
            .expect("read inside the tab transaction");
        assert_eq!(
            ids(&inside),
            vec!["joined".to_string(), "tab".to_string()],
            "the run's write must be visible to the tab that owns the transaction"
        );

        connector
            .execute(&handle, "ROLLBACK", &[])
            .await
            .expect("rollback the tab transaction");
    })
    .await;

    let after = connector
        .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
        .await
        .expect("read after rollback");
    assert!(
        ids(&after).is_empty(),
        "the tab's rollback must undo the joined write too, got {:?}",
        ids(&after)
    );

    connector
        .execute(&handle, &format!("DROP TABLE {table}"), &[])
        .await
        .expect("drop joined fixture");
    connector.disconnect(&handle).await.expect("disconnect fixture");
}

// ═══════════════════════════════════════════════════════════════════════════
// Introspection tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_introspect_tables() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    let table_names: Vec<&str> = result.tables.iter().map(|t| t.name.as_str()).collect();
    assert!(table_names.contains(&"categories"), "should find categories table");
    assert!(table_names.contains(&"products"), "should find products table");
    assert!(table_names.contains(&"orders"), "should find orders table");
    assert!(table_names.contains(&"order_items"), "should find order_items table");
    assert!(table_names.contains(&"audit_logs"), "should find audit_logs table");
    assert!(table_names.contains(&"empty_table"), "should find empty_table");
    assert!(table_names.contains(&"employees"), "should find employees table");
    // Unicode and weird-name tables
    assert!(
        table_names.iter().any(|n| n.contains("Ünïcödé")),
        "should find unicode table"
    );
    assert!(
        table_names.iter().any(|n| n.contains("weird")),
        "should find weird-name table"
    );

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_introspect_triggers() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    // The fixture has a `products_updated_at` trigger.
    assert!(!result.triggers.is_empty(), "should find at least one trigger");

    let trigger = result
        .triggers
        .iter()
        .find(|t| t.name == "products_updated_at")
        .expect("fixture trigger products_updated_at should exist");

    assert_eq!(trigger.table_name, "products");
    assert_eq!(trigger.schema, "public");
    assert_eq!(trigger.timing, "BEFORE");
    assert_eq!(trigger.event, "UPDATE");
    assert!(trigger.enabled, "trigger should be enabled by default");
    // function_def should contain the CREATE FUNCTION body
    assert!(
        trigger.function_def.contains("update_timestamp"),
        "function_def should reference the trigger function"
    );

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_introspect_views() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    assert!(result.views.len() >= 2, "should find at least 2 views");
    let view_names: Vec<&str> = result.views.iter().map(|v| v.name.as_str()).collect();
    assert!(view_names.contains(&"active_users"), "should find active_users view");
    assert!(view_names.contains(&"order_summary"), "should find order_summary view");

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_introspect_indexes() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    assert!(result.indexes.len() >= 5, "should find at least 5 indexes");
    let idx_names: Vec<&str> = result.indexes.iter().map(|i| i.name.as_str()).collect();
    assert!(idx_names.contains(&"idx_products_category"));
    assert!(idx_names.contains(&"idx_orders_user"));

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_introspect_foreign_keys() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    // order_items has FKs to orders and products
    let oi_fks: Vec<_> = result
        .foreign_keys
        .iter()
        .filter(|fk| fk.from_table == "order_items")
        .collect();
    assert!(oi_fks.len() >= 2, "order_items should have at least 2 FKs");
    assert!(oi_fks
        .iter()
        .any(|fk| fk.from_columns.contains(&"order_id".to_string()) && fk.to_table == "orders"));
    assert!(oi_fks
        .iter()
        .any(|fk| fk.from_columns.contains(&"product_id".to_string()) && fk.to_table == "products"));

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_query_categories() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(&handle, "SELECT * FROM categories ORDER BY id", &[])
        .await
        .unwrap();

    assert_eq!(result.row_count, 4);

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_query_preserves_numeric_and_enum_values() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(
            &handle,
            "SELECT 12345678901234567890.12345::numeric AS amount, 'shipped'::order_status AS status",
            &[],
        )
        .await
        .unwrap();

    assert!(matches!(&result.rows[0].0[0], CellValue::Decimal(value) if value == "12345678901234567890.12345"));
    assert!(matches!(&result.rows[0].0[1], CellValue::Text(value) if value == "shipped"));

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_date_cell_filter_binds_as_date() {
    let (connector, handle) = setup().await;
    let dialect = connector.dialect(&handle).unwrap();
    let filter = TableFilter {
        column: "hire_date".into(),
        op: FilterOp::Eq,
        value: CellValue::Date("2024-01-15".into()),
    };
    let (sql, params) = build_count(dialect.as_ref(), "public", "employees", &[filter]);

    let result = connector.query(&handle, &sql, &params).await.unwrap();
    assert!(matches!(result.rows[0].0[0], CellValue::Int64(count) if count > 0));
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_query_decodes_native_temporal_and_network_values() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(
            &handle,
            "SELECT TIME '12:34:56.123456' AS time_value, INTERVAL '1 day 2 hours' AS interval_value, INET '192.0.2.1/24' AS inet_value",
            &[],
        )
        .await
        .unwrap();

    assert!(matches!(&result.rows[0].0[0], CellValue::Time(value) if value == "12:34:56.123456"));
    assert!(matches!(&result.rows[0].0[1], CellValue::Interval(value) if value.contains("1 day")));
    assert!(matches!(&result.rows[0].0[2], CellValue::Inet(value) if value == "192.0.2.1/24"));
    connector.disconnect(&handle).await.unwrap();
}

/// #57 (B3): the structured and binary decoder paths hold on a live server —
/// JSON/JSONB stay structured, a UUID keeps its canonical text, BYTEA stays
/// byte-exact (a 0xde/0xff byte must not become mojibake), CIDR keeps address and
/// prefix — and a SQL NULL in any of those classes stays a null cell instead of
/// failing the row.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_query_decodes_structured_and_binary_value_classes() {
    let (connector, handle) = setup().await;

    // Real fixture columns, including a NULL and a non-UTF8 byte sequence.
    let result = connector
        .query(
            &handle,
            "SELECT p.id, p.metadata, d.title, d.binary_data \
             FROM products p, documents d \
             WHERE p.sku = 'ELEC-001' AND d.title IN ('Binary Only', 'Empty Doc', 'README') \
             ORDER BY d.title",
            &[],
        )
        .await
        .unwrap();

    assert_eq!(result.row_count, 3);
    let rows = &result.rows;

    // UUID: canonical lowercase text, never a byte blob.
    assert!(
        matches!(&rows[0].0[0], CellValue::Uuid(value) if value == "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
        "UUID must keep canonical text, got {:?}",
        rows[0].0[0]
    );
    // JSONB: structured, keyed access works.
    assert!(
        matches!(&rows[0].0[1], CellValue::Json(value) if value["brand"] == "TechCo" && value["weight_kg"] == 1.8),
        "JSONB must stay structured, got {:?}",
        rows[0].0[1]
    );

    // BYTEA: byte-exact in both directions, and a NULL stays NULL.
    assert!(
        matches!(&rows[0].0[3], CellValue::Bytes(value) if value == &vec![0xde, 0xad, 0xbe, 0xef]),
        "BYTEA must stay byte-exact, got {:?}",
        rows[0].0[3]
    );
    assert!(
        matches!(&rows[1].0[3], CellValue::Null),
        "a NULL BYTEA is a null cell, not empty bytes, got {:?}",
        rows[1].0[3]
    );
    assert!(
        matches!(&rows[2].0[3], CellValue::Bytes(value) if value == b"Hello"),
        "ASCII bytes must decode to their exact bytes, got {:?}",
        rows[2].0[3]
    );

    // Inline classes: JSON vs JSONB agree, CIDR keeps its network meaning, and a
    // SQL NULL is distinguishable from a JSON literal null.
    let classes = connector
        .query(
            &handle,
            "SELECT \
               '{\"a\":1,\"b\":[true,null]}'::json AS as_json, \
               '{\"a\":1,\"b\":[true,null]}'::jsonb AS as_jsonb, \
               '192.0.2.0/24'::cidr AS as_cidr, \
               NULL::jsonb AS as_null, \
               'null'::jsonb AS as_json_null",
            &[],
        )
        .await
        .unwrap();

    let cells = &classes.rows[0].0;
    assert!(
        matches!(&cells[0], CellValue::Json(value) if value["a"] == 1 && value["b"][0] == true),
        "JSON must decode to a structured value, got {:?}",
        cells[0]
    );
    assert!(
        matches!(&cells[1], CellValue::Json(value) if value["b"][1].is_null()),
        "JSONB must decode to a structured value, got {:?}",
        cells[1]
    );
    assert!(
        matches!(&cells[2], CellValue::Inet(value) if value == "192.0.2.0/24"),
        "CIDR must keep address and prefix, got {:?}",
        cells[2]
    );
    assert!(
        matches!(&cells[3], CellValue::Null),
        "a SQL NULL jsonb is a null cell, got {:?}",
        cells[3]
    );
    assert!(
        matches!(&cells[4], CellValue::Json(value) if value.is_null()),
        "a JSON literal null is a JSON value, not a SQL NULL, got {:?}",
        cells[4]
    );

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn pg_typed_temporal_and_network_parameters_bind_without_casts() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(
            &handle,
            "SELECT $1 = TIME '12:34:56.123456' AS time_matches, $2 = INTERVAL '1 day 2 hours' AS interval_matches, $3 = INET '192.0.2.1/24' AS inet_matches, $4 = TIMETZ '12:34:56.123456+02:00' AS timetz_matches, $5 = CIDR '192.0.2.0/24' AS cidr_matches",
            &[
                QueryParam::Time("12:34:56.123456".into()),
                QueryParam::Interval("1 days 02:00:00".into()),
                QueryParam::Inet("192.0.2.1/24".into()),
                QueryParam::Time("12:34:56.123456+02:00".into()),
                QueryParam::Inet("192.0.2.0/24".into()),
            ],
        )
        .await
        .unwrap();

    assert!(result.rows[0]
        .0
        .iter()
        .all(|cell| matches!(cell, CellValue::Bool(true))));
    connector.disconnect(&handle).await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// S7 — Gap-filling tests
// ═══════════════════════════════════════════════════════════════════════════

/// S7: Verify composite FK introspection details (column mapping, constraint identity).
#[tokio::test]
#[ignore]
async fn pg_composite_fk_detail() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    // order_items has a composite PK (order_id, product_id) with FKs to orders and products.
    let oi_fks: Vec<_> = result
        .foreign_keys
        .iter()
        .filter(|fk| fk.from_table == "order_items")
        .collect();
    assert!(oi_fks.len() >= 2, "order_items should have at least 2 FKs");

    // Verify FK to orders: order_id → id
    let fk_to_orders = oi_fks
        .iter()
        .find(|fk| fk.to_table == "orders")
        .expect("FK from order_items to orders should exist");
    assert_eq!(fk_to_orders.from_columns, vec!["order_id"]);
    assert_eq!(fk_to_orders.to_columns, vec!["id"]);
    assert_eq!(fk_to_orders.schema, "public");
    assert_eq!(fk_to_orders.to_schema, "public");

    // Verify FK to products: product_id → id
    let fk_to_products = oi_fks
        .iter()
        .find(|fk| fk.to_table == "products")
        .expect("FK from order_items to products should exist");
    assert_eq!(fk_to_products.from_columns, vec!["product_id"]);
    assert_eq!(fk_to_products.to_columns, vec!["id"]);

    // Verify composite PK is introspected correctly.
    let oi_pk = result
        .primary_keys
        .iter()
        .find(|pk| pk.table_name == "order_items")
        .expect("order_items should have a composite PK");
    assert_eq!(oi_pk.columns.len(), 2);
    assert_eq!(oi_pk.columns[0], "order_id");
    assert_eq!(oi_pk.columns[1], "product_id");

    connector.disconnect(&handle).await.unwrap();
}

/// S7: Verify index lifecycle — CREATE INDEX → introspect → DROP INDEX → verify gone.
#[tokio::test]
#[ignore]
async fn pg_index_lifecycle() {
    let (connector, handle) = setup().await;

    // Pre-cleanup: drop leftover index from a previous failed run.
    connector
        .execute(&handle, "DROP INDEX IF EXISTS idx_s7_test_lifecycle", &[])
        .await
        .unwrap();

    // Ensure the test index does not already exist.
    let result = connector.introspect(&handle).await.unwrap();
    assert!(
        !result.indexes.iter().any(|i| i.name == "idx_s7_test_lifecycle"),
        "test index should not exist before CREATE"
    );

    // CREATE INDEX.
    connector
        .execute(&handle, "CREATE INDEX idx_s7_test_lifecycle ON categories(name)", &[])
        .await
        .unwrap();

    // Introspect and verify the new index exists.
    let result = connector.introspect(&handle).await.unwrap();
    let idx = result
        .indexes
        .iter()
        .find(|i| i.name == "idx_s7_test_lifecycle")
        .expect("test index should exist after CREATE");
    assert_eq!(idx.columns, vec!["name"]);
    assert!(!idx.unique, "test index should not be unique");

    // DROP INDEX.
    connector
        .execute(&handle, "DROP INDEX idx_s7_test_lifecycle", &[])
        .await
        .unwrap();

    // Verify the index is gone.
    let result = connector.introspect(&handle).await.unwrap();
    assert!(
        !result.indexes.iter().any(|i| i.name == "idx_s7_test_lifecycle"),
        "test index should be gone after DROP"
    );

    connector.disconnect(&handle).await.unwrap();
}

/// S7: Verify special identifiers — unicode table/column, quoted names, reserved words.
#[tokio::test]
#[ignore]
async fn pg_special_identifiers() {
    let (connector, handle) = setup().await;
    let result = connector.introspect(&handle).await.unwrap();

    // Unicode table name: "Ünïcödé Üsers"
    let unicode_table = result
        .tables
        .iter()
        .find(|t| t.name.contains("Ünïcödé"))
        .expect("unicode table should exist");
    assert_eq!(unicode_table.schema, "public");

    // Unicode column: "émâil"
    let unicode_col = result
        .columns
        .iter()
        .find(|c| c.table_name.contains("Ünïcödé") && c.name == "émâil")
        .expect("unicode column should exist");
    assert!(!unicode_col.nullable, "émâil should be NOT NULL");

    // Weird-name table: "weird""name"
    let weird_table = result
        .tables
        .iter()
        .find(|t| t.name.contains("weird"))
        .expect("weird-name table should exist");
    assert_eq!(weird_table.schema, "public");

    // Column with spaces: "col with spaces"
    let spaces_col = result
        .columns
        .iter()
        .find(|c| c.table_name.contains("weird") && c.name == "col with spaces")
        .expect("column with spaces should exist");
    assert!(spaces_col.nullable);

    // Reserved word column: "SELECT"
    let reserved_col = result
        .columns
        .iter()
        .find(|c| c.table_name.contains("weird") && c.name == "SELECT")
        .expect("reserved-word column should exist");
    assert!(reserved_col.nullable);

    // Verify we can query these tables.
    let q = connector
        .query(&handle, "SELECT \"émâil\" FROM \"Ünïcödé Üsers\" ORDER BY id", &[])
        .await
        .unwrap();
    assert!(q.row_count > 0, "should be able to query unicode table");

    connector.disconnect(&handle).await.unwrap();
}

/// Core transaction contract: a failed statement must roll back an earlier
/// mutation before the connector reports the failure.
#[tokio::test]
#[ignore]
async fn pg_transaction_failure_rolls_back_prior_mutation() {
    let (connector, handle) = setup().await;
    let table = format!("core_tx_probe_{}", uuid::Uuid::new_v4().simple());
    let create = format!("CREATE TABLE \"{table}\" (id INTEGER)");
    let insert = format!("INSERT INTO \"{table}\" (id) VALUES (1)");
    let statements = vec![create, insert, "SELECT * FROM missing_core_tx_table".into()];

    let failure = connector
        .execute_transaction(&handle, &statements, &[false, false, true])
        .await
        .expect_err("the missing table must fail the transaction");

    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    assert_eq!(failure.results.len(), 2);
    assert!(failure.error.to_string().contains("missing_core_tx_table"));

    let relation = format!("SELECT to_regclass('public.\"{table}\"')");
    let relation_result = connector.query(&handle, &relation, &[]).await.unwrap();
    assert!(matches!(
        relation_result.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Null
    ));
    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{table}\""), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core parameterized transaction contract: parameterized transactions run with
/// search_path set to the active_query_schema.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_parameterized_transaction_respects_active_query_schema() {
    let (connector, handle) = setup().await;

    // Create a temporary schema and table.
    connector
        .execute(&handle, "CREATE SCHEMA IF NOT EXISTS param_schema_test", &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "CREATE TABLE IF NOT EXISTS param_schema_test.scoped_table (id INT PRIMARY KEY, name TEXT)",
            &[],
        )
        .await
        .unwrap();

    let statements = vec![db_pro_core::ports::ParameterizedTransactionStatement {
        sql: "INSERT INTO scoped_table (id, name) VALUES ($1, $2)".into(),
        params: vec![QueryParam::Int64(101), QueryParam::Text("scoped_item".into())],
        expect_affected_rows: true,
        max_affected_rows: Some(1),
    }];

    let results = db_pro_core::application::with_query_schema(
        Some("param_schema_test"),
        connector.execute_parameterized_transaction(&handle, &statements),
    )
    .await
    .expect("parameterized transaction must resolve table inside active query schema");

    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0],
        db_pro_core::ports::TransactionStatementResult::Affected { row_count: 1, .. }
    ));

    // Clean up
    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS param_schema_test CASCADE", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core query contract: a schema selected for the editor applies to an autocommit
/// run, not just to the statement that installs it. Regression: an unqualified
/// tenant table failed with "relation does not exist" because `search_path` was
/// scoped with `is_local = true`, which dies with the `set_config` statement
/// outside an explicit transaction.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_autocommit_run_resolves_unqualified_tables_in_active_query_schema() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_autocommit_schema CASCADE", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE SCHEMA tenant_autocommit_schema", &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "CREATE TABLE tenant_autocommit_schema.scoped_values (id INT PRIMARY KEY, label TEXT)",
            &[],
        )
        .await
        .unwrap();

    db_pro_core::application::with_query_schema(Some("tenant_autocommit_schema"), async {
        connector
            .execute(
                &handle,
                "INSERT INTO scoped_values (id, label) VALUES (1, 'tenant')",
                &[],
            )
            .await
            .expect("unqualified INSERT must resolve inside the editor schema");
        let result = connector
            .query(&handle, "SELECT label FROM scoped_values", &[])
            .await
            .expect("unqualified SELECT must resolve inside the editor schema");
        assert_eq!(result.row_count, 1);
        assert!(matches!(
            result.rows[0].0[0],
            db_pro_core::domain::query::CellValue::Text(ref label) if label == "tenant"
        ));
    })
    .await;

    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_autocommit_schema CASCADE", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core transaction contract: a schema-scoped run inside an open transaction
/// resolves unqualified names, and the transaction-local setting disappears when
/// the transaction ends (#456).
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_transaction_run_resolves_unqualified_tables_in_active_query_schema() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_tx_schema CASCADE", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE SCHEMA tenant_tx_schema", &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "CREATE TABLE tenant_tx_schema.scoped_values (id INT PRIMARY KEY, label TEXT)",
            &[],
        )
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "INSERT INTO tenant_tx_schema.scoped_values (id, label) VALUES (1, 'tenant')",
            &[],
        )
        .await
        .unwrap();

    connector.execute(&handle, "BEGIN", &[]).await.unwrap();
    db_pro_core::application::with_query_schema(Some("tenant_tx_schema"), async {
        let result = connector
            .query(&handle, "SELECT label FROM scoped_values", &[])
            .await
            .expect("unqualified SELECT must resolve inside the tab transaction");
        assert_eq!(result.row_count, 1);
    })
    .await;
    connector.execute(&handle, "ROLLBACK", &[]).await.unwrap();

    let leaked = connector.query(&handle, "SELECT label FROM scoped_values", &[]).await;
    assert!(
        leaked.is_err(),
        "the transaction-local search_path must not survive the rollback"
    );

    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_tx_schema CASCADE", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// #456 contract: an autocommit run that times out must not hand its
/// session-wide `search_path` to the next query from the pool.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_autocommit_timeout_discards_the_schema_scoped_connection() {
    let mut config = pg_config().expect("DATABASE_URL must be set for PG integration tests");
    config.query_timeout_ms = 1_000;
    let connector = PostgresConnector::new();
    let handle = connector
        .connect(&config, &pg_password())
        .await
        .expect("PG connect failed");

    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_timeout_schema CASCADE", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE SCHEMA tenant_timeout_schema", &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "CREATE TABLE tenant_timeout_schema.scoped_values (id INT PRIMARY KEY, label TEXT)",
            &[],
        )
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            "INSERT INTO tenant_timeout_schema.scoped_values (id, label) VALUES (1, 'tenant')",
            &[],
        )
        .await
        .unwrap();

    let timed_out = db_pro_core::application::with_query_schema(
        Some("tenant_timeout_schema"),
        connector.query(&handle, "SELECT pg_sleep(2) FROM scoped_values", &[]),
    )
    .await;
    assert!(matches!(
        timed_out,
        Err(db_pro_core::domain::error::DbError::QueryTimeout { timeout_ms: 1_000 })
    ));

    let leaked = connector.query(&handle, "SELECT label FROM scoped_values", &[]).await;
    assert!(
        leaked.is_err(),
        "the timed-out run must not leak its search_path into the pool"
    );

    // The timed-out backend keeps its share lock until the sleep wakes and the
    // socket close is noticed; let it terminate before the exclusive DROP.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    connector
        .execute(&handle, "DROP SCHEMA IF EXISTS tenant_timeout_schema CASCADE", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core transaction contract: timeout after a mutation must roll back before
/// the connector returns and leave the pool usable for a subsequent query.
#[tokio::test]
#[ignore]
async fn pg_transaction_timeout_rolls_back_prior_mutation() {
    let mut config = pg_config().expect("DATABASE_URL must be set for PG integration tests");
    config.query_timeout_ms = 1_000;
    let password = std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("postgres://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default();

    let connector = PostgresConnector::new();
    let handle = connector.connect(&config, &password).await.expect("PG connect failed");
    let table = format!("core_tx_timeout_probe_{}", uuid::Uuid::new_v4().simple());
    let create = format!("CREATE TABLE \"{table}\" (id INTEGER)");
    let insert = format!("INSERT INTO \"{table}\" (id) VALUES (1)");
    let statements = vec![create, insert, "SELECT pg_sleep(2)".into()];

    let failure = connector
        .execute_transaction(&handle, &statements, &[false, false, true])
        .await
        .expect_err("the sleep must exceed the transaction deadline");

    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    assert!(matches!(
        failure.error,
        db_pro_core::domain::error::DbError::QueryTimeout { timeout_ms: 1_000 }
    ));
    assert_eq!(failure.results.len(), 2);

    let relation = format!("SELECT to_regclass('public.\"{table}\"')");
    let relation_result = connector.query(&handle, &relation, &[]).await.unwrap();
    assert!(matches!(
        relation_result.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Null
    ));
    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{table}\""), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core transaction contract: a deferred constraint failure at commit must be
/// reported as an unknown outcome rather than a confirmed rollback.
#[tokio::test]
#[ignore]
async fn pg_transaction_commit_failure_reports_unknown_outcome() {
    let (connector, handle) = setup().await;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let parent = format!("core_tx_commit_parent_{suffix}");
    let child = format!("core_tx_commit_child_{suffix}");
    connector
        .execute(
            &handle,
            &format!("CREATE TABLE \"{parent}\" (id INTEGER PRIMARY KEY)"),
            &[],
        )
        .await
        .expect("parent table should be created");
    connector
        .execute(
            &handle,
            &format!(
                "CREATE TABLE \"{child}\" (parent_id INTEGER REFERENCES \"{parent}\"(id) DEFERRABLE INITIALLY DEFERRED)"
            ),
            &[],
        )
        .await
        .expect("child table should be created");

    let failure = connector
        .execute_transaction(
            &handle,
            &[
                format!("INSERT INTO \"{child}\" (parent_id) VALUES (999)"),
                "SELECT 1".into(),
            ],
            &[false, true],
        )
        .await
        .expect_err("deferred foreign-key violation must fail at commit");

    assert_eq!(failure.phase, TransactionFailurePhase::Commit);
    assert_eq!(failure.outcome, TransactionFailureOutcome::Unknown);
    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.results.len(), 2);
    assert!(failure.error.to_string().to_lowercase().contains("foreign key"));

    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{child}\""), &[])
        .await
        .expect("child table cleanup should succeed");
    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{parent}\""), &[])
        .await
        .expect("parent table cleanup should succeed");
    connector.disconnect(&handle).await.unwrap();
}

/// Core batch contract: a failed statement must roll back mutations that ran
/// earlier in the same batch.
#[tokio::test]
#[ignore]
async fn pg_execute_batch_failure_rolls_back_prior_mutation() {
    let (connector, handle) = setup().await;
    let table = format!("core_batch_failure_probe_{}", uuid::Uuid::new_v4().simple());
    let create = format!("CREATE TABLE \"{table}\" (id INTEGER)");
    let insert = format!("INSERT INTO \"{table}\" (id) VALUES (1)");
    let error = connector
        .execute_batch(
            &handle,
            &[create, insert, "SELECT * FROM missing_core_batch_table".into()],
        )
        .await
        .expect_err("the missing table must fail the batch");
    assert!(error.to_string().contains("missing_core_batch_table"));

    let relation = format!("SELECT to_regclass('public.\"{table}\"')");
    let relation_result = connector.query(&handle, &relation, &[]).await.unwrap();
    assert!(matches!(
        relation_result.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Null
    ));
    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{table}\""), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// Core batch contract: a timeout after a mutation must roll back before the
/// connector reports the timeout and leave the pool usable.
#[tokio::test]
#[ignore]
async fn pg_execute_batch_timeout_rolls_back_prior_mutation() {
    let mut config = pg_config().expect("DATABASE_URL must be set for PG integration tests");
    config.query_timeout_ms = 1_000;
    let password = std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("postgres://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default();

    let connector = PostgresConnector::new();
    let handle = connector.connect(&config, &password).await.expect("PG connect failed");
    let table = format!("core_batch_timeout_probe_{}", uuid::Uuid::new_v4().simple());
    let create = format!("CREATE TABLE \"{table}\" (id INTEGER)");
    let insert = format!("INSERT INTO \"{table}\" (id) VALUES (1)");
    let error = connector
        .execute_batch(&handle, &[create, insert, "SELECT pg_sleep(2)".into()])
        .await
        .expect_err("the batch sleep must exceed its configured deadline");
    assert!(matches!(
        error,
        db_pro_core::domain::error::DbError::QueryTimeout { timeout_ms: 1_000 }
    ));

    let relation = format!("SELECT to_regclass('public.\"{table}\"')");
    let relation_result = connector.query(&handle, &relation, &[]).await.unwrap();
    assert!(matches!(
        relation_result.rows[0].0[0],
        db_pro_core::domain::query::CellValue::Null
    ));
    let recovery = connector.query(&handle, "SELECT 1", &[]).await.unwrap();
    assert_eq!(recovery.row_count, 1);
    connector
        .execute(&handle, &format!("DROP TABLE IF EXISTS \"{table}\""), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// Gate 5 B2 — temporal value classes decode without timezone invention (#56)
// ═══════════════════════════════════════════════════════════════════════════

/// A naive `timestamp` must keep its wall-clock reading and its own variant: the
/// decoder must not reinterpret it as an instant, and the session timezone must not
/// reach the cell. Since #52 the class also keeps a dedicated `Timestamp` variant.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_timestamp_without_time_zone_keeps_wall_clock_value() {
    let (connector, handle) = setup().await;
    connector
        .execute(&handle, "SET TIME ZONE 'America/New_York'", &[])
        .await
        .unwrap();

    let result = connector
        .query(
            &handle,
            "SELECT TIMESTAMP '2024-03-15 10:20:30.123456' AS naive_stamp",
            &[],
        )
        .await
        .unwrap();

    assert_eq!(
        format!("{:?}", result.rows[0].0[0]),
        "Timestamp(\"2024-03-15T10:20:30.123456\")",
        "a naive timestamp must keep its wall-clock reading and its own variant"
    );
    connector.disconnect(&handle).await.unwrap();
}

/// The whole temporal class matrix must decode to the canonical strings, and the
/// session timezone must not move any of them.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_temporal_classes_decode_to_canonical_strings() {
    let (connector, handle) = setup().await;
    connector
        .execute(&handle, "SET TIME ZONE 'America/New_York'", &[])
        .await
        .unwrap();

    let result = connector
        .query(
            &handle,
            "SELECT DATE '2024-03-15', \
                    TIME '10:20:30.123456', \
                    TIMETZ '10:20:30.123456+05:30', \
                    TIMESTAMP '2024-03-15 10:20:30.123456', \
                    TIMESTAMPTZ '2024-03-15 10:20:30.123456+00', \
                    TIMESTAMP '2024-03-15 10:20:30', \
                    NULL::TIMESTAMP",
            &[],
        )
        .await
        .unwrap();

    let cells: Vec<String> = result.rows[0].0.iter().map(|cell| format!("{cell:?}")).collect();
    assert_eq!(
        cells,
        vec![
            "Date(\"2024-03-15\")".to_string(),
            "Time(\"10:20:30.123456\")".to_string(),
            "TimeTz(\"10:20:30.123456+05:30\")".to_string(),
            "Timestamp(\"2024-03-15T10:20:30.123456\")".to_string(),
            "TimestampTz(\"2024-03-15T10:20:30.123456Z\")".to_string(),
            "Timestamp(\"2024-03-15T10:20:30.000000\")".to_string(),
            "Null".to_string(),
        ]
    );
    connector.disconnect(&handle).await.unwrap();
}

/// #58 (B4): a class the mapper has no explicit arm for must never be served as
/// mojibake and must never take the readable columns of the row down with it.
/// The wire format decides the representation: provider text stays text (canonical
/// enum/domain values), binary stays byte-exact (`\x` hex in the UI, read-only).
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_unsupported_binary_classes_stay_exact_and_keep_the_row_readable() {
    let (connector, handle) = setup().await;

    // Two classes that used to fail the whole query (money, a composite record),
    // one that used to come back as binary noise (a range), one array, and the
    // ordinary columns of the same row that must survive all of it.
    let result = connector
        .query(
            &handle,
            "SELECT                '[1,10)'::int4range AS range_value,                '12.34'::money AS money_value,                ROW(1, 'x')::record AS record_value,                '{a,b}'::text[] AS text_array,                'ok'::text AS note,                42 AS answer",
            &[],
        )
        .await
        .expect("an unsupported class must not fail the readable columns of the row");

    let cells = &result.rows[0].0;

    // int4range: flags 0x02 (lower bound inclusive) then big-endian bounds 1 and 10.
    match &cells[0] {
        CellValue::Bytes(bytes) => assert_eq!(
            bytes.as_slice(),
            [0x02, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 10],
            "a range must stay byte-exact"
        ),
        other => panic!("a range must not be decoded as text, got {other:?}"),
    }

    // money in binary form is an int64 count of cents.
    match &cells[1] {
        CellValue::Bytes(bytes) => {
            assert_eq!(bytes.len(), 8, "money is an int64 on the wire: {bytes:?}");
            let cents = i64::from_be_bytes(bytes.as_slice().try_into().expect("8 bytes"));
            assert_eq!(cents, 1234, "12.34 in cents");
        }
        other => panic!("money must not be decoded as text, got {other:?}"),
    }

    // A composite record has no flat representation: bytes, not text.
    match &cells[2] {
        CellValue::Bytes(bytes) => assert!(!bytes.is_empty(), "a record carries field data"),
        other => panic!("a record must not be decoded as text, got {other:?}"),
    }

    // Arrays arrive in the binary wire format and have no element parsing in v0.1,
    // so the fallback is byte-exact rather than an invented element rendering.
    match &cells[3] {
        CellValue::Bytes(bytes) => assert!(!bytes.is_empty(), "an array carries element data"),
        other => panic!("an array must not be decoded as text, got {other:?}"),
    }

    // The unrelated readable columns of the same row are intact.
    assert!(
        matches!(&cells[4], CellValue::Text(value) if value == "ok"),
        "the text column of the row must survive, got {:?}",
        cells[4]
    );
    assert!(
        matches!(&cells[5], CellValue::Int64(42)),
        "the integer column of the row must survive, got {:?}",
        cells[5]
    );

    connector.disconnect(&handle).await.unwrap();
}

/// #58 (B4): enum labels and domains decode deterministically — the enum label as
/// canonical text, a domain through its base type (PostgreSQL resolves the domain
/// to the underlying type on the wire), and a NULL stays NULL for both.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_enum_and_domain_values_decode_to_canonical_values() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(
            &handle,
            "SELECT                'shipped'::order_status AS enum_literal,                (SELECT status FROM orders WHERE id = 1) AS enum_column,                'YES'::information_schema.yes_or_no AS domain_over_text,                42::information_schema.cardinal_number AS domain_over_int,                NULL::information_schema.yes_or_no AS domain_null",
            &[],
        )
        .await
        .expect("enum and domain values must decode");

    let cells = &result.rows[0].0;
    assert!(
        matches!(&cells[0], CellValue::Text(value) if value == "shipped"),
        "an enum literal must keep its canonical label, got {:?}",
        cells[0]
    );
    assert!(
        matches!(&cells[1], CellValue::Text(value) if value == "delivered"),
        "an enum column must keep its canonical label, got {:?}",
        cells[1]
    );
    assert!(
        matches!(&cells[2], CellValue::Text(value) if value == "YES"),
        "a domain over text keeps canonical text, got {:?}",
        cells[2]
    );
    assert!(
        matches!(&cells[3], CellValue::Int64(42)),
        "a domain over integer decodes through its base type, got {:?}",
        cells[3]
    );
    assert!(
        matches!(&cells[4], CellValue::Null),
        "a NULL domain value is a null cell, got {:?}",
        cells[4]
    );

    connector.disconnect(&handle).await.unwrap();
}

/// #59 (B5): the decoder against the full fixture matrix — one column per value
/// class the contract defines, in `fixtures/postgres/decoder_matrix` (DDL in
/// `001_schema.sql`, row in `002_seed.sql`), asserted expected-vs-actual on a live
/// server, plus a second row that is NULL in every nullable column, so each class
/// is proven to survive a NULL.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
          // cc-scan:allow HARD_COMPLEXITY — decoder matrix test: one assertion per value class, table-flat by design
async fn pg_decoder_matrix_covers_every_value_class() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(&handle, "SELECT * FROM decoder_matrix ORDER BY id", &[])
        .await
        .expect("every fixture column must decode without unintended query failure");

    assert_eq!(result.row_count, 2, "one populated row and one NULL row");
    let row = &result.rows[0].0;
    assert_eq!(row.len(), 26, "one cell per decoder_matrix column");

    let checks: Vec<(&str, bool)> = vec![
        ("id int2", matches!(&row[0], CellValue::Int64(1))),
        ("flag bool", matches!(&row[1], CellValue::Bool(true))),
        ("count int4", matches!(&row[2], CellValue::Int64(2_147_483_647))),
        (
            "big_count int8",
            matches!(&row[3], CellValue::Int64(9_223_372_036_854_775_807)),
        ),
        (
            "ratio float4",
            matches!(&row[4], CellValue::Float64(value) if (*value - 1.5).abs() < f64::EPSILON),
        ),
        (
            "precise_ratio float8",
            matches!(&row[5], CellValue::Float64(value) if (*value - 0.1).abs() < f64::EPSILON),
        ),
        (
            "amount numeric(24,4)",
            matches!(&row[6], CellValue::Decimal(value) if value == "12345678901234567890.1234"),
        ),
        (
            "calendar_date",
            matches!(&row[7], CellValue::Date(value) if value == "2024-03-15"),
        ),
        (
            "wall_time",
            matches!(&row[8], CellValue::Time(value) if value == "10:20:30.123456"),
        ),
        (
            "zoned_time timetz",
            matches!(&row[9], CellValue::TimeTz(value) if value == "10:20:30.123456+07:00"),
        ),
        (
            "local_stamp timestamp",
            matches!(&row[10], CellValue::Timestamp(value) if value == "2024-03-15T10:20:30.123456"),
        ),
        (
            "instant timestamptz",
            matches!(&row[11], CellValue::TimestampTz(value) if value == "2024-03-15T10:20:30.123456Z"),
        ),
        (
            "span interval",
            matches!(&row[12], CellValue::Interval(value) if value == "1 mons 2 days 03:04:05.000006"),
        ),
        (
            "token uuid",
            matches!(&row[13], CellValue::Uuid(value) if value == "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
        ),
        (
            "doc json",
            matches!(&row[14], CellValue::Json(value) if value["a"] == 1),
        ),
        (
            "payload jsonb",
            matches!(&row[15], CellValue::Json(value) if value["brand"] == "TechCo"),
        ),
        (
            "blob bytea",
            matches!(&row[16], CellValue::Bytes(value) if value == &vec![0xde, 0xad, 0xbe, 0xef]),
        ),
        (
            "address inet",
            matches!(&row[17], CellValue::Inet(value) if value.starts_with("192.0.2.1")),
        ),
        (
            "network cidr",
            matches!(&row[18], CellValue::Inet(value) if value == "192.0.2.0/24"),
        ),
        (
            "status enum",
            matches!(&row[19], CellValue::Text(value) if value == "shipped"),
        ),
        ("quantity domain", matches!(&row[20], CellValue::Int64(7))),
        (
            "postal domain",
            matches!(&row[21], CellValue::Text(value) if value == "SW1A 1AA"),
        ),
        ("labels array", matches!(&row[22], CellValue::Bytes(_))),
        ("slot range", matches!(&row[23], CellValue::Bytes(_))),
        ("pair composite", matches!(&row[24], CellValue::Bytes(_))),
        ("missing null", matches!(&row[25], CellValue::Null)),
    ];

    let wrong: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    assert!(wrong.is_empty(), "decoder matrix mismatch: {wrong:?}");

    // Provider type identity stays available to the policy layer for each column.
    let declared: Vec<&str> = result.columns.iter().map(|c| c.data_type.as_str()).collect();
    for expected in ["INT2", "NUMERIC", "TIMETZ", "TEXT[]", "INT4RANGE", "decoder_pair"] {
        assert!(
            declared.iter().any(|name| name.contains(expected)),
            "the declared type {expected} must reach the caller: {declared:?}"
        );
    }

    // Every nullable class in the second row is a null cell, not an error and not
    // empty bytes. The key and the flag are NOT NULL by DDL and keep their values.
    assert!(
        matches!(&result.rows[1].0[0], CellValue::Int64(2)),
        "the NOT NULL key column keeps its value"
    );
    assert!(
        matches!(&result.rows[1].0[1], CellValue::Bool(false)),
        "the NOT NULL flag column keeps its value"
    );
    for (index, cell) in result.rows[1].0.iter().enumerate().skip(2) {
        assert!(
            matches!(cell, CellValue::Null),
            "column {index} of the NULL row must be Null, got {cell:?}"
        );
    }

    connector.disconnect(&handle).await.unwrap();
}

/// A catalog function in the introspection query (`pg_get_indexdef`,
/// `pg_get_constraintdef`) is evaluated with a *fresh* catalog snapshot, so an index
/// or constraint created or dropped by another session after this query's snapshot
/// makes the function return NULL for a row the snapshot still shows. That is the
/// failure CI hit — `pg_introspect_tables … ColumnDecode { index: "definition",
/// UnexpectedNullError }` at `crates/infrastructure/src/postgres/introspect.rs:337`
/// — and it must degrade the one row, never fail the whole refresh.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_introspection_survives_concurrent_index_churn() {
    let (connector, handle) = setup().await;

    // The mechanism, on one concrete index: once it is gone the function this query
    // calls returns NULL, which is exactly what the decoder has to tolerate.
    connector
        .query(&handle, "CREATE TABLE IF NOT EXISTS churn_probe(id integer)", &[])
        .await
        .unwrap();
    connector
        .query(
            &handle,
            "CREATE INDEX IF NOT EXISTS churn_probe_i ON churn_probe(id)",
            &[],
        )
        .await
        .unwrap();
    let oid = connector
        .query(
            &handle,
            "SELECT c.oid FROM pg_class c WHERE c.relname = 'churn_probe_i'",
            &[],
        )
        .await
        .unwrap();
    let oid = match &oid.rows[0].0[0] {
        CellValue::Int64(oid) => *oid,
        other => panic!("an oid column must decode as Int64, got {other:?}"),
    };
    connector.query(&handle, "DROP INDEX churn_probe_i", &[]).await.unwrap();
    let probe = connector
        .query(
            &handle,
            &format!("SELECT pg_get_indexdef({oid}) IS NULL AS definition_is_null"),
            &[],
        )
        .await
        .unwrap();
    assert!(
        matches!(&probe.rows[0].0[0], CellValue::Bool(true)),
        "a dropped index must have no definition — that is the NULL the decoder sees"
    );

    // The race itself: drop indexes from a second session while the first one
    // introspects repeatedly. Before the fix this failed within a fraction of a
    // second (`UnexpectedNullError`); it must now report the schema either way.
    for i in 0..40 {
        connector
            .query(
                &handle,
                &format!("CREATE INDEX IF NOT EXISTS churn_probe_i{i} ON churn_probe(id)"),
                &[],
            )
            .await
            .unwrap();
    }
    let (dropper, dropper_handle) = setup().await;
    let dropper = tokio::spawn(async move {
        for i in 0..40 {
            let _ = dropper
                .query(&dropper_handle, &format!("DROP INDEX IF EXISTS churn_probe_i{i}"), &[])
                .await;
        }
        dropper.disconnect(&dropper_handle).await.unwrap();
    });

    let mut refreshes = 0;
    while !dropper.is_finished() || refreshes < 20 {
        connector
            .introspect(&handle)
            .await
            .expect("introspection must survive concurrent index churn");
        refreshes += 1;
        if refreshes > 500 {
            break;
        }
    }
    dropper.await.unwrap();

    connector
        .query(&handle, "DROP TABLE IF EXISTS churn_probe", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// #147 — multi-statement batches may not carry transaction control
//
// The product path is `QueryService::execute_multi`, which refuses a batch whose statements
// contain `BEGIN` / `COMMIT` / `ROLLBACK` before any of them reaches the connector. These tests
// pin, on live PostgreSQL:
//   1. the product refusal, with nothing written,
//   2. the documented rollback contract for a batch without control,
//   3. the defect that made the refusal necessary, measured on the connector itself (a batch that
//      carries its own transaction control reports `RolledBack` while its earlier writes survive).
// ═══════════════════════════════════════════════════════════════════════════

/// Build the product path used by the app: `QueryService` over a real connector, a real meta store
/// and a real registry entry for `connection.id`. The meta store lives in a fresh temp directory so
/// tests stay isolated from whatever `db-pro` state happens to be on the machine.
async fn product_path(
    connection: &Connection,
) -> (QueryService, ConnectionHandle, SQLiteMetaStore, std::path::PathBuf) {
    let meta_dir = std::env::temp_dir().join(format!("db-pro-pg-ms-tx-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&meta_dir).expect("temp dir for the meta store");

    let meta_store = SQLiteMetaStore::new(&meta_dir.join("meta.db").to_string_lossy())
        .await
        .expect("meta store");
    ConnectionRepository::save(&meta_store, connection)
        .await
        .expect("save the connection");

    let registry = Arc::new(ConnectionRegistry::new());
    let connector = PostgresConnector::new();
    let config = pg_config().expect("DATABASE_URL must be set for PG integration tests");
    let handle = connector
        .connect(&config, &pg_password())
        .await
        .expect("PG connect failed");
    registry.register(connection.id, handle);

    let service = QueryService::new(
        Box::new(connector),
        Box::new(meta_store.clone()),
        Box::new(meta_store.clone()),
        Box::new(meta_store.clone()),
        Arc::clone(&registry),
        Box::new(meta_store.clone()),
    );

    (service, handle, meta_store, meta_dir)
}

fn count_cell(result: &QueryResult) -> i64 {
    match &result.rows[0].0[0] {
        CellValue::Int64(count) => *count,
        other => panic!("unexpected count cell {other:?}"),
    }
}

/// The product refusal: a batch that carries its own `COMMIT` is rejected before dispatch and
/// nothing is written. Without this rule the same batch on PostgreSQL commits the insert and then
/// reports `RolledBack` (see `pg_connector_alone_reports_rolled_back_for_a_batch_that_committed`).
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_multi_statement_batch_with_commit_is_refused_and_writes_nothing() {
    let mut connection = Connection::new(pg_config().expect("DATABASE_URL must be set for PG integration tests"));
    connection.config.name = "multi-statement-refusal".to_owned();
    let connection_id = connection.id;

    let (reader, reader_handle) = setup().await;
    reader
        .execute(&reader_handle, "DROP TABLE IF EXISTS ms_tx_refused", &[])
        .await
        .unwrap();
    reader
        .execute(&reader_handle, "CREATE TABLE ms_tx_refused (id INT)", &[])
        .await
        .unwrap();

    let (service, _service_handle, _meta_store, _meta_dir) = product_path(&connection).await;

    let result = service
        .execute_multi(
            &connection_id,
            "INSERT INTO ms_tx_refused (id) VALUES (1); COMMIT; SELECT no_such_column FROM ms_tx_refused",
            None,
            None,
        )
        .await
        .expect("a refused batch is a result carrying an error");

    let (index, error) = result.error.expect("the batch is refused");
    assert_eq!(index, 1, "the offending statement is reported by its index");
    assert!(
        error.message.contains("`COMMIT`"),
        "the refusal names the offending statement, got: {}",
        error.message
    );
    assert!(result.results.is_empty(), "no results are returned for a refused batch");

    let count = reader
        .query(&reader_handle, "SELECT count(*) FROM ms_tx_refused", &[])
        .await
        .unwrap();
    assert_eq!(
        count_cell(&count),
        0,
        "the refusal happens before dispatch, so the insert must not be written"
    );

    reader
        .execute(&reader_handle, "DROP TABLE IF EXISTS ms_tx_refused", &[])
        .await
        .unwrap();
    reader.disconnect(&reader_handle).await.unwrap();
}

/// The contract for a batch without transaction control is unchanged: a failure rolls the whole
/// batch back and the envelope reports it.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_multi_statement_batch_without_transaction_control_still_rolls_back() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "DROP TABLE IF EXISTS ms_tx_control", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE TABLE ms_tx_control (id INT)", &[])
        .await
        .unwrap();

    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "INSERT INTO ms_tx_control (id) VALUES (1)".to_owned(),
                "SELECT no_such_column FROM ms_tx_control".to_owned(),
            ],
            &[false, true],
        )
        .await
        .expect_err("the second statement fails");

    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.statement_index, 1);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);

    let count = connector
        .query(&handle, "SELECT count(*) FROM ms_tx_control", &[])
        .await
        .unwrap();
    assert_eq!(count_cell(&count), 0, "the failure must roll the whole batch back");

    connector
        .execute(&handle, "DROP TABLE IF EXISTS ms_tx_control", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// The defect that made the dispatch rule necessary, measured on the connector itself and pinned on
/// purpose (the #147 fix lives one layer up, so this is what shows the assumption behind the rule is
/// real on PostgreSQL — and what fails loudly if someone removes the refusal without giving the
/// connector the guarantee itself). Measured on this host: the batch `BEGIN; INSERT; COMMIT; SELECT
/// bad` fails on the `SELECT`, and the envelope reports `RolledBack` — but the insert the user's own
/// `COMMIT` committed is still in the table. The connector is not changed by the fix; its
/// `execute_transaction` documents that assumption instead.
#[tokio::test]
#[ignore] // Requires DATABASE_URL
async fn pg_connector_alone_reports_rolled_back_for_a_batch_that_committed() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "DROP TABLE IF EXISTS ms_tx_characterisation", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "CREATE TABLE ms_tx_characterisation (id INT)", &[])
        .await
        .unwrap();

    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "BEGIN".to_owned(),
                "INSERT INTO ms_tx_characterisation (id) VALUES (1)".to_owned(),
                "COMMIT".to_owned(),
                "SELECT no_such_column FROM ms_tx_characterisation".to_owned(),
            ],
            &[false, false, false, true],
        )
        .await
        .expect_err("the statement after the COMMIT fails");

    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.statement_index, 3);
    assert_eq!(
        failure.outcome,
        TransactionFailureOutcome::RolledBack,
        "the envelope reports rolled back although the user's COMMIT already ended the transaction"
    );

    let count = connector
        .query(&handle, "SELECT count(*) FROM ms_tx_characterisation", &[])
        .await
        .unwrap();
    assert_eq!(
        count_cell(&count),
        1,
        "the user's COMMIT ended the wrapper transaction, so the insert survived"
    );

    connector
        .execute(&handle, "DROP TABLE IF EXISTS ms_tx_characterisation", &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

/// INTEGER keys are deliberately not BIGINT: the public service supplies Int64
/// values, so PostgreSQL must prepare using their wire types rather than infer INT4.
#[tokio::test]
#[ignore] // Requires DATABASE_URL, no preloaded fixtures
async fn pg_typed_query_integer_composite_pk_via_table_data_service() {
    use db_pro_core::application::sql_builder::build_select_by_pk;
    use db_pro_core::application::table_data_service::TableDataService;

    let (connector, handle) = setup().await;
    let connector = Arc::new(connector);
    // Pin one backend so every call exercises the same statement cache.
    connector.execute(&handle, "BEGIN", &[]).await.unwrap();
    let table = format!("typed_pk_{}", uuid::Uuid::new_v4().simple());
    connector
        .execute(
            &handle,
            &format!(
                "CREATE TABLE public.\"{table}\" (tenant INTEGER NOT NULL, part INTEGER NOT NULL, \
                 amount BIGINT NOT NULL, note TEXT, PRIMARY KEY (tenant, part))"
            ),
            &[],
        )
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            &format!("INSERT INTO public.\"{table}\" VALUES (10, 21, 9223372036854775807, NULL)"),
            &[],
        )
        .await
        .unwrap();

    let connection = Connection::new(pg_config().unwrap());
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(connection.id, handle);
    let meta_dir = std::env::temp_dir().join(format!("db-pro-typed-pk-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&meta_dir).unwrap();
    let meta_store = SQLiteMetaStore::new(&meta_dir.join("meta.db").to_string_lossy())
        .await
        .unwrap();
    ConnectionRepository::save(&meta_store, &connection).await.unwrap();
    let service = TableDataService::new(Box::new(Arc::clone(&connector)), registry, Box::new(meta_store.clone()));
    let keys = vec!["tenant".into(), "part".into()];
    let missing = [CellValue::Int64(10), CellValue::Int64(22)];
    let matching = [CellValue::Int64(10), CellValue::Int64(21)];
    let dialect = connector.dialect(&handle).unwrap();
    let (sql, params) = build_select_by_pk(dialect.as_ref(), "public", &table, &keys, &missing).unwrap();

    // First execution has no PgRow from which to obtain column metadata.
    let empty = connector.query(&handle, &sql, &params).await.unwrap();
    assert!(empty.rows.is_empty());
    assert_eq!(empty.row_count, 0);
    assert_eq!(
        empty
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.data_type.as_str(), column.nullable))
            .collect::<Vec<_>>(),
        vec![
            ("tenant", "INT4", false),
            ("part", "INT4", false),
            ("amount", "INT8", false),
            ("note", "TEXT", true),
        ]
    );
    for values in [&matching, &missing, &matching] {
        let result = service
            .fetch_row_by_pk(&connection.id, "public", &table, &keys, values)
            .await
            .unwrap();
        if matches!(values[1], CellValue::Int64(22)) {
            assert!(result.is_none());
        } else {
            let result = result.unwrap();
            assert_eq!(result.row_count, 1);
            assert!(matches!(result.rows[0].0[0], CellValue::Int64(10)));
            assert!(matches!(result.rows[0].0[1], CellValue::Int64(21)));
            assert!(matches!(result.rows[0].0[2], CellValue::Int64(i64::MAX)));
            assert!(matches!(result.rows[0].0[3], CellValue::Null));
            assert!(!result.columns[2].nullable);
            assert!(result.columns[3].nullable);
        }
    }

    // Execute and explain already let bound arguments drive preparation.
    assert_eq!(
        connector
            .execute(
                &handle,
                &format!("UPDATE public.\"{table}\" SET amount = $1 WHERE tenant = $2 AND part = $3"),
                &[
                    QueryParam::Int64(i64::MAX - 1),
                    QueryParam::Int64(10),
                    QueryParam::Int64(21)
                ],
            )
            .await
            .unwrap(),
        1
    );
    connector.explain(&handle, &sql, &params, false).await.unwrap();
    let updated = service
        .fetch_row_by_pk(&connection.id, "public", &table, &keys, &matching)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(updated.rows[0].0[2], CellValue::Int64(value) if value == i64::MAX - 1));

    connector.execute(&handle, "ROLLBACK", &[]).await.unwrap();
    connector.disconnect(&handle).await.unwrap();
    drop(service);
    drop(meta_store);
    std::fs::remove_dir_all(meta_dir).unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL, no preloaded fixtures
async fn pg_typed_query_uncast_parameter_preserves_bigint_precision_and_empty_metadata() {
    let (connector, handle) = setup().await;
    connector.execute(&handle, "BEGIN", &[]).await.unwrap();
    let sql = "SELECT $1 AS typed_value WHERE $2";
    for (value, present) in [(i64::MAX, true), (i64::MIN, false), (i64::MIN, true)] {
        let result = connector
            .query(&handle, sql, &[QueryParam::Int64(value), QueryParam::Bool(present)])
            .await
            .unwrap();
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "typed_value");
        assert_eq!(result.columns[0].data_type, "INT8");
        assert_eq!(result.row_count, u64::from(present));
        if present {
            assert!(matches!(result.rows[0].0[0], CellValue::Int64(actual) if actual == value));
        } else {
            assert!(result.rows.is_empty());
        }
    }
    connector.execute(&handle, "ROLLBACK", &[]).await.unwrap();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL, no preloaded fixtures
async fn pg_typed_query_keeps_metadata_at_streaming_row_limits() {
    for max_rows in [0, 2] {
        let mut config = pg_config().unwrap();
        config.max_rows = max_rows;
        let connector = PostgresConnector::new();
        let handle = connector.connect(&config, &pg_password()).await.unwrap();
        let result = connector
            .query(
                &handle,
                "SELECT $1 AS typed_value FROM generate_series(1, 100)",
                &[QueryParam::Int64(i64::MAX)],
            )
            .await
            .unwrap();
        assert_eq!(result.row_count, max_rows);
        assert_eq!(result.rows.len() as u64, max_rows);
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].data_type, "INT8");
        for row in result.rows {
            assert!(matches!(row.0[0], CellValue::Int64(i64::MAX)));
        }
        // A dropped, bounded stream must leave the connection usable.
        connector.query(&handle, "SELECT 1", &[]).await.unwrap();
        connector.disconnect(&handle).await.unwrap();
    }
}

/// PostgreSQL flushes the wide initial rows before sleeping on the tail. A
/// post-stream Describe would drain that tail and turn this capped result into
/// QueryTimeout; metadata must be prepared before executing the row stream.
#[tokio::test]
#[ignore] // Requires DATABASE_URL, no preloaded fixtures
async fn pg_typed_query_cap_returns_before_slow_tail_without_schema_override() {
    let mut config = pg_config().unwrap();
    config.max_rows = 2;
    config.query_timeout_ms = 10_000;
    let connector = PostgresConnector::new();
    let handle = connector.connect(&config, &pg_password()).await.unwrap();
    let result = connector
        .query(
            &handle,
            "SELECT $1 AS typed_value, repeat('x', 32768) AS payload, \
             CASE WHEN n > 2 THEN pg_sleep(5) END AS tail_delay \
             FROM generate_series(1, 3) AS series(n)",
            &[QueryParam::Int64(i64::MAX)],
        )
        .await
        .expect("the two flushed rows must return before the unused five-second tail");
    assert_eq!(result.row_count, 2);
    assert_eq!(result.columns[0].data_type, "INT8");
    for row in &result.rows {
        assert!(matches!(row.0[0], CellValue::Int64(i64::MAX)));
        assert!(matches!(&row.0[1], CellValue::Text(payload) if payload.len() == 32768));
    }
    // Pool release may finish draining asynchronously; do not issue a query on
    // the capped backend before observing the successful result above.
    connector.disconnect(&handle).await.unwrap();
}

/// A domain NULL has no SQL type; fresh statements must infer it from context
/// rather than receiving the mapper's storage-only Option<String> hint.
#[tokio::test]
#[ignore] // Requires DATABASE_URL, no preloaded fixtures
async fn pg_typed_query_null_parameters_preserve_contextual_inference() {
    let (connector, handle) = setup().await;
    for sql in [
        "SELECT 1 = $1 AS matched",
        "SELECT TRUE = $1 AS matched",
        "SELECT DATE '2026-08-17' = $1 AS matched",
    ] {
        let result = connector.query(&handle, sql, &[QueryParam::Null]).await.unwrap();
        assert_eq!(result.row_count, 1);
        assert_eq!(result.columns[0].data_type, "BOOL");
        assert!(matches!(result.rows[0].0[0], CellValue::Null));
    }
    let mixed = connector
        .query(
            &handle,
            "SELECT 1 = $1 AND 2 = $2 AS matched",
            &[QueryParam::Null, QueryParam::Int64(2)],
        )
        .await
        .unwrap();
    assert_eq!(mixed.row_count, 1);
    assert!(matches!(mixed.rows[0].0[0], CellValue::Null));
    connector.disconnect(&handle).await.unwrap();
}

/// #252: the PL/pgSQL debug-capability probe must run cleanly against a real server and
/// report an honest "extension absent" — never a faked capability. The CI fixture (PG16)
/// and the local fixture (PG18) both ship without plugin_debugger preloaded.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn pg_routine_debug_probe_reports_extension_absent_cleanly() {
    let (connector, handle) = setup().await;
    let result = connector
        .query(
            &handle,
            db_pro_core::domain::routine_debug::DEBUG_CAPABILITY_PROBE_SQL,
            &[],
        )
        .await
        .expect("catalog probe SQL must be valid on any supported PostgreSQL");
    let probe = db_pro_core::domain::routine_debug::RoutineDebugProbe::from_probe_result(&result);
    assert!(
        probe.probe_error.is_none(),
        "probe row must be readable: {:?}",
        probe.probe_error
    );
    assert!(!probe.extension_installed, "fixture never installs plugin_debugger");
    assert_eq!(
        probe.state,
        db_pro_core::domain::routine_debug::RoutineDebugState::ExtensionMissing,
        "absent extension must map to ExtensionMissing, not a faked capability"
    );
    assert!(
        !probe.server_version.is_empty(),
        "server_version must come back from the probe"
    );
    connector.disconnect(&handle).await.unwrap();
}

/// #252 LIVE: the typed routine-invocation path binds every argument as a parameter
/// (`SELECT * FROM routine($1::integer, $2)`) instead of interpolating literals. On a real server this
/// proves that a typed NULL reaches the routine as SQL NULL (not the string "NULL", not magic),
/// that a value full of quotes survives verbatim (no escaping bug, no injection), and that a
/// PL/pgSQL failure surfaces its DETAIL/CONTEXT frame trace in the single error message the
/// Messages pane renders.
#[tokio::test]
#[ignore = "requires the disposable PostgreSQL integration fixture"]
async fn dbpro252_typed_routine_execution_binds_parameters_and_surfaces_plpgsql_trace() {
    let (connector, handle) = setup().await;

    connector
        .execute(
            &handle,
            "CREATE OR REPLACE FUNCTION dbpro252_probe(a int, b text) \
             RETURNS text LANGUAGE sql AS $$ SELECT a::text || ' | ' || b $$",
            &[],
        )
        .await
        .expect("create dbpro252_probe");
    connector
        .execute(
            &handle,
            "CREATE OR REPLACE FUNCTION dbpro252_raise(a int) RETURNS int \
             LANGUAGE plpgsql AS $fn$ \
             BEGIN \
                 IF a IS NULL THEN \
                     RAISE EXCEPTION 'dbpro252 typed null reached the routine' \
                         USING DETAIL = 'probe call from dbpro252'; \
                 END IF; \
                 RETURN a; \
             END \
             $fn$",
            &[],
        )
        .await
        .expect("create dbpro252_raise");

    // Exactly the SQL shape build_routine_invoke_sql generates for a Postgres
    // function: `int` binds as bigint, so its placeholder carries the declared
    // `::integer` resolution cast; `text` binds exactly and stays bare.
    let typed_null = connector
        .query(
            &handle,
            "SELECT * FROM dbpro252_probe($1::integer, $2)",
            &[QueryParam::Null, QueryParam::Text("kept".into())],
        )
        .await
        .expect("typed NULL must bind as SQL NULL");
    assert_eq!(typed_null.row_count, 1);
    assert!(
        matches!(typed_null.rows[0].0[0], CellValue::Null),
        "int(NULL) || ... must evaluate to SQL NULL, got {:?}",
        typed_null.rows[0].0[0]
    );

    let quote_payload = "O'Brien \"'; DROP TABLE users; --\"".to_owned();
    let quoted = connector
        .query(
            &handle,
            "SELECT * FROM dbpro252_probe($1::integer, $2)",
            &[QueryParam::Int64(7), QueryParam::Text(quote_payload.clone())],
        )
        .await
        .expect("quote-heavy text must bind as an ordinary value");
    assert_eq!(quoted.row_count, 1);
    assert!(
        matches!(&quoted.rows[0].0[0], CellValue::Text(value)
            if value == &format!("7 | {quote_payload}")),
        "argument must reach the routine verbatim, with no escaping or injection effect, got {:?}",
        quoted.rows[0].0[0]
    );

    let error = connector
        .query(
            &handle,
            "SELECT * FROM dbpro252_raise($1::integer)",
            &[QueryParam::Null],
        )
        .await
        .expect_err("RAISE EXCEPTION must fail the query");
    let message = error.to_string();
    assert!(
        message.contains("dbpro252 typed null reached the routine"),
        "top-line message missing: {message}"
    );
    assert!(
        message.contains("DETAIL:") && message.contains("probe call from dbpro252"),
        "DETAIL must be surfaced in the failure payload: {message}"
    );
    assert!(
        message.contains("CONTEXT:") && message.contains("dbpro252_raise"),
        "PL/pgSQL frame trace (CONTEXT ... line N) must be surfaced: {message}"
    );

    connector
        .execute(&handle, "DROP FUNCTION IF EXISTS dbpro252_probe(int, text)", &[])
        .await
        .expect("drop dbpro252_probe");
    connector
        .execute(&handle, "DROP FUNCTION IF EXISTS dbpro252_raise(int)", &[])
        .await
        .expect("drop dbpro252_raise");
    connector.disconnect(&handle).await.unwrap();
}
