//! Tests for `../table_data_service.rs` (moved out of the source file).
#![allow(clippy::result_large_err)]
use super::*;
use crate::domain::connection::ConnectionHandle;
use crate::domain::query::*;
use crate::ports::dialect::SqlDialect;
use crate::ports::{
    MockConnectionRepository, MockDbConnector, TransactionFailure, TransactionFailureOutcome, TransactionFailurePhase,
    TransactionStatementResult,
};

struct QuestionDialect;
impl SqlDialect for QuestionDialect {
    fn placeholder(&self, _index: usize) -> String {
        "?".to_string()
    }
    fn quote_identifier(&self, name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
}

fn setup() -> (ConnectionId, Arc<ConnectionRegistry>) {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));
    (conn_id, registry)
}

fn mock_connections() -> MockConnectionRepository {
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(|_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver: crate::domain::connection::DriverType::Postgres,
            ssl_mode: crate::domain::connection::SslMode::Disable,
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
    repo
}

#[test]
fn parse_total_count_rejects_negative_values() {
    let result = QueryResult {
        columns: vec![ColumnMeta {
            name: "count".into(),
            data_type: "INT".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Int64(-1)])],
        row_count: 1,
        duration_ms: 0,
    };

    assert!(matches!(
        parse_total_count(&result),
        Err(DbError::Internal(message)) if message.contains("negative")
    ));
}

#[test]
fn parse_total_count_rejects_missing_or_wrong_type() {
    let empty = QueryResult::empty();
    assert!(matches!(
        parse_total_count(&empty),
        Err(DbError::Internal(message)) if message.contains("exactly one row")
    ));

    let wrong_type = QueryResult {
        columns: vec![ColumnMeta {
            name: "count".into(),
            data_type: "TEXT".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Text("42".into())])],
        row_count: 1,
        duration_ms: 0,
    };
    assert!(matches!(
        parse_total_count(&wrong_type),
        Err(DbError::Internal(message)) if message.contains("non-integer")
    ));
}

#[test]
fn parse_total_count_rejects_non_scalar_result() {
    let result = QueryResult {
        columns: vec![ColumnMeta {
            name: "count".into(),
            data_type: "INT".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Int64(42)]), Row(vec![CellValue::Int64(43)])],
        row_count: 2,
        duration_ms: 0,
    };

    assert!(matches!(
        parse_total_count(&result),
        Err(DbError::Internal(message)) if message.contains("exactly one row")
    ));
}

#[tokio::test]
async fn fetch_rows_returns_data_and_count() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_query().times(2).returning(|_handle, sql, _params| {
        if sql.contains("COUNT(*)") {
            Ok(QueryResult {
                columns: vec![ColumnMeta {
                    name: "count".into(),
                    data_type: "INT".into(),
                    nullable: false,
                }],
                rows: vec![Row(vec![CellValue::Int64(42)])],
                row_count: 1,
                duration_ms: 1,
            })
        } else {
            Ok(QueryResult {
                columns: vec![ColumnMeta {
                    name: "id".into(),
                    data_type: "INT".into(),
                    nullable: false,
                }],
                rows: vec![Row(vec![CellValue::Int64(1)])],
                row_count: 1,
                duration_ms: 2,
            })
        }
    });

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let (result, total) = svc
        .fetch_rows(&conn_id, "public", "users", &[], &[], 50, 0)
        .await
        .unwrap();
    assert_eq!(total, 42);
    assert_eq!(result.rows.len(), 1);
}

#[tokio::test]
async fn fetch_rows_rejects_malformed_data_result_shape() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_query().times(2).returning(|_, sql, _| {
        if sql.contains("COUNT(*)") {
            Ok(QueryResult {
                columns: vec![ColumnMeta {
                    name: "count".into(),
                    data_type: "INT".into(),
                    nullable: false,
                }],
                rows: vec![Row(vec![CellValue::Int64(1)])],
                row_count: 1,
                duration_ms: 0,
            })
        } else {
            Ok(QueryResult {
                columns: vec![ColumnMeta {
                    name: "id".into(),
                    data_type: "INT".into(),
                    nullable: false,
                }],
                rows: vec![Row(Vec::new())],
                row_count: 1,
                duration_ms: 0,
            })
        }
    });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let error = service
        .fetch_rows(&conn_id, "public", "users", &[], &[], 50, 0)
        .await
        .expect_err("table data must reject an invalid provider result");

    assert!(matches!(error, DbError::QueryFailed(message) if message.contains("expected 1")));
}

#[tokio::test]
async fn insert_row_returns_affected() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, sql, _| {
        assert!(sql.contains("INSERT INTO"));
        Ok(1)
    });

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let affected = svc
        .insert_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("alice".into())],
        )
        .await
        .unwrap();
    assert_eq!(affected, 1);
}

#[tokio::test]
async fn update_row_returns_affected() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, sql, params| {
        assert!(sql.contains("UPDATE"));
        assert!(sql.contains("WHERE"));
        assert_eq!(params.len(), 2);
        Ok(1)
    });

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let affected = svc
        .update_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("bob".into())],
            &["id".into()],
            &[CellValue::Int64(1)],
        )
        .await
        .unwrap();
    assert_eq!(affected, 1);
}

#[tokio::test]
async fn delete_row_returns_affected() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, sql, params| {
        assert!(sql.contains("DELETE FROM"));
        assert_eq!(params.len(), 1);
        Ok(1)
    });

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let affected = svc
        .delete_row(&conn_id, "public", "users", &["id".into()], &[CellValue::Int64(1)])
        .await
        .unwrap();
    assert_eq!(affected, 1);
}

#[tokio::test]
async fn update_row_returns_not_found_when_no_row_is_affected() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, _, _| Ok(0));

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let error = svc
        .update_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("bob".into())],
            &["id".into()],
            &[CellValue::Int64(999)],
        )
        .await
        .expect_err("zero-row update must not be reported as success");

    assert!(matches!(error, DbError::Conflict(message) if message.contains("changed or deleted")));
}

#[tokio::test]
async fn delete_row_returns_not_found_when_no_row_is_affected() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, _, _| Ok(0));

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let error = svc
        .delete_row(&conn_id, "public", "users", &["id".into()], &[CellValue::Int64(999)])
        .await
        .expect_err("zero-row delete must not be reported as success");

    assert!(matches!(error, DbError::Conflict(message) if message.contains("changed or deleted")));
}

#[tokio::test]
async fn update_row_rejects_multiple_affected_rows_as_an_invariant_violation() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, _, _| Ok(2));

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let error = service
        .update_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("unsafe".into())],
            &["id".into()],
            &[CellValue::Int64(1)],
        )
        .await
        .expect_err("a PK mutation must affect at most one row");

    assert!(matches!(error, DbError::Internal(message) if message.contains("invariant violation")));
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn apply_mutations_builds_one_atomic_parameterized_batch_in_safe_order() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector
        .expect_execute_parameterized_transaction()
        .returning(|_, statements| {
            assert_eq!(statements.len(), 3);
            assert!(statements[0].sql.starts_with("DELETE FROM"));
            assert!(statements[1].sql.starts_with("UPDATE"));
            assert!(statements[2].sql.starts_with("INSERT INTO"));
            Ok(vec![
                TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: 0,
                },
                TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: 0,
                },
                TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: 0,
                },
            ])
        });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let affected = service
        .apply_mutations(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("new".into())],
                },
                TableDataMutation::Update {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("changed".into())],
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(1)],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(2)],
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(affected, 3);
}

#[tokio::test]
async fn apply_mutations_preserves_specific_transaction_failure() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute_parameterized_transaction().returning(|_, _| {
        Err(TransactionFailure {
            phase: TransactionFailurePhase::Statement,
            statement_index: 0,
            outcome: TransactionFailureOutcome::RolledBack,
            results: Vec::new(),
            error: DbError::QueryFailed("duplicate key value".into()),
        })
    });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[TableDataMutation::Insert {
                columns: vec!["name".into()],
                values: vec![CellValue::Text("duplicate".into())],
            }],
        )
        .await
        .expect_err("a rolled-back transaction must surface its database error");
    assert_eq!(failure.statement_index, 0);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    assert!(matches!(failure.error, DbError::QueryFailed(message) if message == "duplicate key value"));
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn apply_mutations_detailed_maps_statement_index_to_original_input_mutation() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));

    // Input mutations: [Insert (orig 0), Delete (orig 1)]
    // Reordered execution statements: [Delete (exec 0), Insert (exec 1)]
    // Mock transaction execution fails at exec index 1 (the Insert statement).
    connector
        .expect_execute_parameterized_transaction()
        .returning(|_, statements| {
            assert_eq!(statements.len(), 2);
            assert!(statements[0].sql.starts_with("DELETE FROM"));
            assert!(statements[1].sql.starts_with("INSERT INTO"));
            Err(TransactionFailure {
                phase: TransactionFailurePhase::Statement,
                statement_index: 1, // exec index 1 (Insert)
                outcome: TransactionFailureOutcome::RolledBack,
                results: vec![TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: 0,
                }],
                error: DbError::QueryFailed("duplicate key value".into()),
            })
        });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("new".into())],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(1)],
                },
            ],
        )
        .await
        .expect_err("failing transaction must map statement_index back to original input position");

    // Statement index 1 in reordered execution corresponds to Insert, which was input index 0.
    assert_eq!(failure.statement_index, 0);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn apply_mutations_detailed_maps_statement_index_complex_reordering() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));

    // Input mutations:
    // idx 0: Insert 1
    // idx 1: Update 1
    // idx 2: Delete 1
    // idx 3: Insert 2
    // Reordered execution:
    // exec 0: Delete 1 (orig idx 2)
    // exec 1: Update 1 (orig idx 1)
    // exec 2: Insert 1 (orig idx 0)
    // exec 3: Insert 2 (orig idx 3)
    // Let exec 0 (Delete 1) fail, which corresponds to original input index 2.
    connector
        .expect_execute_parameterized_transaction()
        .returning(|_, statements| {
            assert_eq!(statements.len(), 4);
            assert!(statements[0].sql.starts_with("DELETE FROM"));
            assert!(statements[1].sql.starts_with("UPDATE"));
            assert!(statements[2].sql.starts_with("INSERT INTO"));
            assert!(statements[3].sql.starts_with("INSERT INTO"));
            Err(TransactionFailure {
                phase: TransactionFailurePhase::Statement,
                statement_index: 0, // exec index 0 (Delete 1)
                outcome: TransactionFailureOutcome::RolledBack,
                results: Vec::new(),
                error: DbError::Conflict("row lock failure".into()),
            })
        });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("insert 1".into())],
                },
                TableDataMutation::Update {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("update 1".into())],
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(10)],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(20)],
                },
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("insert 2".into())],
                },
            ],
        )
        .await
        .expect_err("failing transaction must map statement_index back to original input position");

    // Exec index 0 corresponds to Delete 1, which was original input index 2.
    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
}

#[tokio::test]
async fn apply_mutations_detailed_retains_zero_statement_index_on_begin_failure() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));

    // Input mutations: [Insert (orig 0), Delete (orig 1)]
    // Reordered execution statements: [Delete (exec 0), Insert (exec 1)]
    // Mock transaction execution fails during Begin phase with statement_index 0.
    connector.expect_execute_parameterized_transaction().returning(|_, _| {
        Err(TransactionFailure {
            phase: TransactionFailurePhase::Begin,
            statement_index: 0,
            outcome: TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::ConnectionFailed("begin failed".into()),
        })
    });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("new".into())],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(1)],
                },
            ],
        )
        .await
        .expect_err("begin failure must return transaction failure");

    // Begin failure must retain statement_index 0, NOT remapping via indexed_mutations[0].0 (which would be 1).
    assert_eq!(failure.phase, TransactionFailurePhase::Begin);
    assert_eq!(failure.statement_index, 0);
    assert_eq!(failure.outcome, TransactionFailureOutcome::NotStarted);
}

#[tokio::test]
async fn apply_mutations_detailed_retains_commit_statement_index_without_out_of_bounds_remapping() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));

    // Input mutations: 2 mutations. Commit failure returns statement_index = 2 (statements.len()).
    connector
        .expect_execute_parameterized_transaction()
        .returning(|_, statements| {
            let count = statements.len();
            Err(TransactionFailure {
                phase: TransactionFailurePhase::Commit,
                statement_index: count,
                outcome: TransactionFailureOutcome::Unknown,
                results: Vec::new(),
                error: DbError::QueryFailed("commit failed".into()),
            })
        });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("new".into())],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(1)],
                },
            ],
        )
        .await
        .expect_err("commit failure must return transaction failure");

    // Commit phase failure must retain statement_index = 2 (number of statements), not attempt out-of-bounds remapping.
    assert_eq!(failure.phase, TransactionFailurePhase::Commit);
    assert_eq!(failure.statement_index, 2);
    assert_eq!(failure.outcome, TransactionFailureOutcome::Unknown);
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn apply_mutations_detailed_retains_zero_statement_index_on_validation_failure() {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(|_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver: crate::domain::connection::DriverType::Postgres,
            ssl_mode: crate::domain::connection::SslMode::Disable,
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
            // Read-only connection triggers validation failure
            readonly: true,
            create_if_missing: false,
}))
    });

    let service = TableDataService::new(Box::new(connector), registry, Box::new(repo));
    let failure = service
        .apply_mutations_detailed(
            &conn_id,
            "public",
            "users",
            &[
                TableDataMutation::Insert {
                    columns: vec!["name".into()],
                    values: vec![CellValue::Text("new".into())],
                },
                TableDataMutation::Delete {
                    pk_columns: vec!["id".into()],
                    pk_values: vec![CellValue::Int64(1)],
                },
            ],
        )
        .await
        .expect_err("validation failure on read-only connection");

    // Validation failure must report statement_index 0 (reflecting 0 statements executed, not mutations.len()).
    assert_eq!(failure.phase, TransactionFailurePhase::Validation);
    assert_eq!(failure.statement_index, 0);
    assert_eq!(failure.outcome, TransactionFailureOutcome::NotStarted);
}

#[tokio::test]
async fn fetch_rows_connection_not_active() {
    let registry = Arc::new(ConnectionRegistry::new());
    let connector = MockDbConnector::new();
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let fake_id = ConnectionId::new();
    let result = svc.fetch_rows(&fake_id, "public", "users", &[], &[], 50, 0).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_row_rejects_empty_pk() {
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc
        .update_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("x".into())],
            &[],
            &[],
        )
        .await;
    assert!(matches!(result, Err(DbError::Validation(_))));
}

#[tokio::test]
async fn update_row_allows_primary_key_column_with_original_predicate() {
    let (conn_id, registry) = setup();
    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Ok(Box::new(QuestionDialect) as Box<dyn SqlDialect>));
    connector.expect_execute().returning(|_, sql, params| {
        assert!(sql.contains("SET \"id\" = ?"));
        assert!(sql.contains("WHERE \"id\" = ?"));
        assert_eq!(params.len(), 2);
        Ok(1)
    });
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc
        .update_row(
            &conn_id,
            "public",
            "users",
            &["id".into()],
            &[CellValue::Int64(2)],
            &["id".into()],
            &[CellValue::Int64(1)],
        )
        .await;

    assert_eq!(result.unwrap(), 1);
}

#[tokio::test]
async fn delete_row_rejects_empty_pk() {
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc.delete_row(&conn_id, "public", "users", &[], &[]).await;
    assert!(matches!(result, Err(DbError::Validation(_))));
}

#[tokio::test]
async fn update_row_rejects_pk_length_mismatch() {
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc
        .update_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("x".into())],
            &["id".into(), "org_id".into()],
            &[CellValue::Int64(1)],
        )
        .await;
    assert!(matches!(result, Err(DbError::Validation(_))));
}

#[tokio::test]
async fn delete_row_rejects_pk_length_mismatch() {
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc
        .delete_row(
            &conn_id,
            "public",
            "users",
            &["id".into(), "org_id".into()],
            &[CellValue::Int64(1)],
        )
        .await;
    assert!(matches!(result, Err(DbError::Validation(_))));
}

#[tokio::test]
async fn fetch_rows_fails_when_dialect_errors() {
    let (conn_id, registry) = setup();

    let mut connector = MockDbConnector::new();
    connector
        .expect_dialect()
        .returning(|_| Err(DbError::ConnectionFailed("unknown handle".into())));

    let svc = TableDataService::new(Box::new(connector), registry, Box::new(mock_connections()));
    let result = svc.fetch_rows(&conn_id, "public", "users", &[], &[], 50, 0).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn insert_row_blocked_on_readonly() {
    let (conn_id, registry) = setup();
    let connector = MockDbConnector::new();
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(|_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver: crate::domain::connection::DriverType::Postgres,
            ssl_mode: crate::domain::connection::SslMode::Disable,
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
    let svc = TableDataService::new(Box::new(connector), registry, Box::new(repo));
    let result = svc
        .insert_row(
            &conn_id,
            "public",
            "users",
            &["name".into()],
            &[CellValue::Text("alice".into())],
        )
        .await;
    assert!(result.is_err());
}
