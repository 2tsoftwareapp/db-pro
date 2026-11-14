//! Tests for `../export_service.rs` (moved out of the source file).
use super::*;
use crate::domain::connection::{ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use crate::domain::query::{CellValue, ColumnMeta, Row};
use crate::ports::{MockConnectionRepository, MockDbConnector};

fn sample_result() -> QueryResult {
    QueryResult {
        columns: vec![
            ColumnMeta {
                name: "id".into(),
                data_type: "INT".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "name".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ],
        rows: vec![
            Row(vec![CellValue::Int64(1), CellValue::Text("Alice".into())]),
            Row(vec![CellValue::Int64(2), CellValue::Null]),
        ],
        row_count: 2,
        duration_ms: 0,
    }
}

#[test]
fn excel_indexes_reject_provider_type_overflow() {
    assert!(matches!(excel_column_index(u16::MAX as usize), Ok(u16::MAX)));
    assert!(matches!(
        excel_column_index(u16::MAX as usize + 1),
        Err(DbError::Validation(message)) if message.contains("columns")
    ));
    assert!(matches!(excel_row_index(u32::MAX as usize - 1), Ok(u32::MAX)));
    assert!(matches!(
        excel_row_index(u32::MAX as usize),
        Err(DbError::Validation(message)) if message.contains("rows")
    ));
}

#[test]
fn excel_integer_precision_switches_to_text_outside_exact_f64_range() {
    assert!(excel_integer_is_exact(MAX_EXACT_EXCEL_INTEGER));
    assert!(excel_integer_is_exact(-MAX_EXACT_EXCEL_INTEGER));
    assert!(!excel_integer_is_exact(MAX_EXACT_EXCEL_INTEGER + 1));
    assert!(!excel_integer_is_exact(i64::MAX));
    assert!(!excel_integer_is_exact(i64::MIN));
}

fn build_service(connector: MockDbConnector, registry: Arc<ConnectionRegistry>) -> ExportService {
    let mut connections = MockConnectionRepository::new();
    connections.expect_get_config().returning(|_| {
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
        Ok(Some(ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver: DriverType::Postgres,
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
}))
    });
    ExportService::new(Box::new(connector), registry, Box::new(connections))
}

#[tokio::test]
async fn export_csv_basic() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(sample_result()));

    let svc = build_service(connector, Arc::clone(&registry));
    let result = svc.export_csv(&conn_id, "SELECT * FROM users").await.unwrap();

    let content = String::from_utf8(result.content).unwrap();
    assert!(content.contains("id,name"));
    assert!(content.contains("1,Alice"));
    assert!(content.contains("2,"));
    assert_eq!(result.mime_type, "text/csv");
    assert_eq!(result.row_count, 2);
}

// #429: the service-level CSV render must defuse spreadsheet formulas in both headers and cells.
#[test]
fn csv_render_escapes_formula_leading_fields() {
    let result = QueryResult {
        columns: vec![
            ColumnMeta {
                name: "id".into(),
                data_type: "INT".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "=cmd".into(),
                data_type: "TEXT".into(),
                nullable: true,
            },
        ],
        rows: vec![
            Row(vec![
                CellValue::Int64(1),
                CellValue::Text("=HYPERLINK(\"http://evil\",\"x\")".into()),
            ]),
            Row(vec![CellValue::Int64(-2), CellValue::Text("-1+1".into())]),
        ],
        row_count: 2,
        duration_ms: 0,
    };

    let csv = String::from_utf8(render_csv(&result).expect("csv render")).expect("utf8");

    assert!(csv.contains("'=cmd"), "header must be guarded: {csv}");
    assert!(
        csv.contains("'=HYPERLINK("),
        "formula text cell must be prefixed: {csv}"
    );
    assert!(csv.contains("'-1+1"), "non-numeric `-` text must be prefixed: {csv}");
    assert!(csv.contains("-2"), "a real negative number stays numeric: {csv}");
}

#[tokio::test]
async fn export_json_basic() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(sample_result()));

    let svc = build_service(connector, Arc::clone(&registry));
    let result = svc.export_json(&conn_id, "SELECT * FROM users").await.unwrap();

    let parsed: Vec<serde_json::Value> = serde_json::from_slice(&result.content).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0]["id"], 1);
    assert_eq!(parsed[0]["name"], "Alice");
    assert_eq!(parsed[1]["name"], serde_json::Value::Null);
    assert_eq!(result.mime_type, "application/json");
}

#[tokio::test]
async fn export_json_rejects_duplicate_column_names() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let duplicate_columns = QueryResult {
        columns: vec![
            ColumnMeta {
                name: "value".into(),
                data_type: "INT".into(),
                nullable: false,
            },
            ColumnMeta {
                name: "value".into(),
                data_type: "INT".into(),
                nullable: false,
            },
        ],
        rows: vec![Row(vec![CellValue::Int64(1), CellValue::Int64(2)])],
        row_count: 1,
        duration_ms: 0,
    };
    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .returning(move |_, _, _| Ok(duplicate_columns.clone()));

    let error = build_service(connector, registry)
        .export_json(&conn_id, "SELECT 1 AS value, 2 AS value")
        .await
        .expect_err("JSON export must not drop a duplicate column");

    assert!(matches!(error, DbError::Validation(message)
            if message.contains("duplicate column") && message.contains("value")));
}

#[tokio::test]
async fn export_json_rejects_non_finite_float() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let non_finite = QueryResult {
        columns: vec![ColumnMeta {
            name: "value".into(),
            data_type: "FLOAT8".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Float64(f64::NAN)])],
        row_count: 1,
        duration_ms: 0,
    };
    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .returning(move |_, _, _| Ok(non_finite.clone()));

    let error = build_service(connector, registry)
        .export_json(&conn_id, "SELECT 'NaN'::float8 AS value")
        .await
        .expect_err("JSON export must not silently turn NaN into null");

    assert!(matches!(error, DbError::Validation(message) if message.contains("non-finite")));
}

#[tokio::test]
async fn export_excel_basic() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(sample_result()));

    let svc = build_service(connector, Arc::clone(&registry));
    let result = svc.export_excel(&conn_id, "SELECT * FROM users").await.unwrap();

    assert!(result.content.len() > 4);
    assert_eq!(&result.content[0..4], b"PK\x03\x04");
    assert_eq!(result.row_count, 2);
}

#[tokio::test]
async fn export_not_active() {
    let svc = build_service(MockDbConnector::new(), Arc::new(ConnectionRegistry::new()));
    let result = svc.export_csv(&ConnectionId::new(), "SELECT 1").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn export_multi_statement_rejected() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let svc = build_service(MockDbConnector::new(), Arc::clone(&registry));
    let result = svc.export_csv(&conn_id, "SELECT 1; SELECT 2").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn export_rejects_mutating_query_on_readonly_connection() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connections = MockConnectionRepository::new();
    connections.expect_get_config().returning(|_| {
        Ok(Some(ConnectionConfig {
            name: "readonly".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver: DriverType::Postgres,
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
            readonly: true,
            create_if_missing: false,
}))
    });

    let svc = ExportService::new(Box::new(MockDbConnector::new()), registry, Box::new(connections));
    let error = svc
        .export_csv(&conn_id, "DELETE FROM users RETURNING id")
        .await
        .expect_err("readonly export must not execute a mutation");
    assert!(matches!(error, DbError::QueryFailed(message) if message.contains("read-only")));
}

#[tokio::test]
async fn export_rejects_malformed_query_result_shape() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let malformed = QueryResult {
        columns: vec![ColumnMeta {
            name: "id".into(),
            data_type: "INT".into(),
            nullable: false,
        }],
        rows: vec![Row(Vec::new())],
        row_count: 1,
        duration_ms: 0,
    };
    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(move |_, _, _| Ok(malformed.clone()));

    let error = build_service(connector, registry)
        .export_json(&conn_id, "SELECT id FROM users")
        .await
        .expect_err("export must reject an invalid provider result");

    assert!(matches!(error, DbError::QueryFailed(message) if message.contains("expected 1")));
}

// #484: results must never materialize more than MAX_EXPORT_ROWS rows for export.
fn oversized_result(row_total: usize) -> QueryResult {
    QueryResult {
        columns: vec![ColumnMeta {
            name: "id".into(),
            data_type: "INT".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Int64(0)]); row_total],
        row_count: row_total as u64,
        duration_ms: 0,
    }
}

#[test]
fn export_cap_allows_exactly_max_rows() {
    assert!(ensure_within_export_cap(&oversized_result(MAX_EXPORT_ROWS)).is_ok());
}

#[test]
fn export_cap_rejects_one_row_over_max() {
    let error = ensure_within_export_cap(&oversized_result(MAX_EXPORT_ROWS + 1))
        .expect_err("one row over the cap must be rejected");
    let DbError::Validation(message) = error else {
        panic!("expected DbError::Validation, got {error:?}");
    };
    assert!(message.contains("500000") || message.contains("500_000"));
    assert!(message.contains("LIMIT"));
}

#[test]
fn csv_render_rejects_result_over_row_cap() {
    // Defense in depth: renderers refuse oversized results even if called directly.
    let error = export_formats::render_csv(&oversized_result(MAX_EXPORT_ROWS + 1))
        .expect_err("csv render must reject oversized results");
    assert!(matches!(error, DbError::Validation(_)));
}

#[tokio::test]
async fn export_csv_rejects_result_over_row_cap() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .returning(|_, _, _| Ok(oversized_result(MAX_EXPORT_ROWS + 1)));

    let error = build_service(connector, registry)
        .export_csv(&conn_id, "SELECT * FROM huge_table")
        .await
        .expect_err("export must reject results over the row cap");

    let DbError::Validation(message) = error else {
        panic!("expected DbError::Validation, got {error:?}");
    };
    assert!(message.contains("LIMIT"));
}
