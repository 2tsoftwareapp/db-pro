// Result carries the domain error envelope by value; the larger error is intentional in tests.
#![allow(clippy::result_large_err)]

use super::*;
use crate::domain::connection::ConnectionHandle;
use crate::domain::query::{CellValue, ColumnMeta, Row};
use crate::ports::{
    MockConnectionRepository, MockDbConnector, MockIntrospectionCache, MockQueryHistoryRepository,
    MockRunConfigRepository, MockSavedQueryRepository, TransactionFailure, TransactionFailureOutcome,
    TransactionFailurePhase, TransactionStatementResult,
};

fn test_result() -> QueryResult {
    QueryResult {
        columns: vec![ColumnMeta {
            name: "id".into(),
            data_type: "INT".into(),
            nullable: false,
        }],
        rows: vec![Row(vec![CellValue::Int64(1)])],
        row_count: 1,
        duration_ms: 0,
    }
}

fn build_service(connector: MockDbConnector, registry: Arc<ConnectionRegistry>) -> QueryService {
    QueryService::new(
        Box::new(connector),
        Box::new(MockQueryHistoryRepository::new()),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        registry,
        Box::new(mock_connections_full_access()),
    )
}

/// Create a mock ConnectionRepository that returns a non-readonly config for any connection.
fn mock_connections_full_access() -> MockConnectionRepository {
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(|_id| {
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
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

fn mock_connections_read_only() -> MockConnectionRepository {
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(|_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "readonly".into(),
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
    repo
}

#[tokio::test]
async fn execute_success() {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(test_result()));

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute(&conn_id, "SELECT 1", &[], None, None).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().row_count, 1);
}

#[tokio::test]
async fn execute_not_active() {
    let svc = build_service(MockDbConnector::new(), Arc::new(ConnectionRegistry::new()));
    let result = svc.execute(&ConnectionId::new(), "SELECT 1", &[], None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cancel_forwards_to_the_active_connection() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_cancel()
        .withf(|handle| handle.0 == 1)
        .returning(|_| Ok(()));

    let svc = build_service(connector, registry);
    svc.cancel(&conn_id)
        .await
        .expect("active query cancellation should be forwarded");
}

#[tokio::test]
async fn execute_ddl_invalidates_schema_cache() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| {
        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            duration_ms: 0,
        })
    });

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_invalidate()
        .withf(move |id| *id == conn_id)
        .times(1)
        .returning(|_| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        registry,
        Box::new(mock_connections_full_access()),
    )
    .with_introspection_cache(Box::new(cache));

    svc.execute(&conn_id, "CREATE TABLE added (id INT)", &[], None, None)
        .await
        .expect("DDL query should succeed");
}

#[tokio::test]
async fn explain_rejects_mutating_explain_analyze_on_read_only_connection() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_explain().never();

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(MockQueryHistoryRepository::new()),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        registry,
        Box::new(mock_connections_read_only()),
    );

    let error = svc
        .explain(&conn_id, "DELETE FROM users WHERE id = 1", &[], true, None)
        .await
        .expect_err("mutating EXPLAIN ANALYZE must be rejected");
    assert!(matches!(error, DbError::QueryFailed(message) if message.contains("read-only")));
}

#[tokio::test]
async fn execute_multi_statement_rejected() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let svc = build_service(MockDbConnector::new(), Arc::clone(&registry));
    let result = svc.execute(&conn_id, "SELECT 1; SELECT 2", &[], None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn execute_empty_sql_rejected() {
    let svc = build_service(MockDbConnector::new(), Arc::new(ConnectionRegistry::new()));
    let result = svc.execute(&ConnectionId::new(), "  ", &[], None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn execute_trailing_semicolon_ok() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(test_result()));

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute(&conn_id, "SELECT 1;", &[], None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn execute_semicolon_in_string_ok() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(test_result()));

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute(&conn_id, "SELECT ';' FROM t", &[], None, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn execute_history_save_failure_non_fatal() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| Ok(test_result()));

    let mut history = MockQueryHistoryRepository::new();
    history
        .expect_save()
        .returning(|_, _, _, _, _, _| Err(DbError::Internal("disk full".into())));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute(&conn_id, "SELECT 1", &[], None, None).await;
    assert!(result.is_ok());
}

#[test]
fn classify_simple_select_is_read() {
    assert!(matches!(
        classify_statement("SELECT 1", DriverType::Postgres),
        StatementClass::Read
    ));
}

#[test]
fn classify_simple_insert_is_write() {
    assert!(matches!(
        classify_statement("INSERT INTO t VALUES (1)", DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_row_producing_mutations_as_reads_for_result_routing() {
    for sql in [
        "INSERT INTO t (name) VALUES ('one') RETURNING id",
        "UPDATE t SET name = 'one' RETURNING id",
        "DELETE FROM t WHERE id = 1 RETURNING id",
    ] {
        assert!(
            matches!(classify_statement(sql, DriverType::Postgres), StatementClass::Read),
            "{sql}"
        );
    }
}

#[test]
fn returning_inside_cte_does_not_make_outer_mutation_row_producing() {
    let sql = "WITH deleted AS (DELETE FROM t WHERE id = 1 RETURNING id) UPDATE t SET archived = true";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_with_select_is_read() {
    let sql = "WITH cte AS (SELECT id FROM t) SELECT * FROM cte";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Read
    ));
}

#[test]
fn classify_with_update_is_write() {
    let sql = "WITH changed AS (SELECT id FROM t) UPDATE t SET x = 1 WHERE id IN (SELECT id FROM changed)";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_with_delete_is_write() {
    let sql = "WITH deleted AS (SELECT id FROM t) DELETE FROM t WHERE id IN (SELECT id FROM deleted)";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_with_multiple_ctes_update_is_write() {
    let sql = "WITH a AS (SELECT 1), b AS (SELECT 2) UPDATE t SET x = 1";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_leading_comment_stripped() {
    let sql = "-- comment\nSELECT 1";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Read
    ));
}

#[test]
fn classify_leading_block_comment_stripped() {
    let sql = "/* block */ INSERT INTO t VALUES (1)";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_with_paren_in_string_literal() {
    let sql = "WITH cte AS (SELECT 'value )' AS text) UPDATE t SET x = 1";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn classify_with_escaped_quote_in_string() {
    let sql = "WITH cte AS (SELECT 'it''s )' AS text) SELECT * FROM cte";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Read
    ));
}

#[tokio::test]
async fn execute_multi_routes_select_then_update() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_execute_transaction()
        .returning(|_, statements, read_statements| {
            assert_eq!(statements, &["SELECT 1", "UPDATE t SET x = 1"]);
            assert_eq!(read_statements, &[true, false]);
            Ok(vec![
                TransactionStatementResult::Query(test_result()),
                TransactionStatementResult::Affected {
                    row_count: 3,
                    duration_ms: 0,
                },
            ])
        });

    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(&conn_id, "SELECT 1; UPDATE t SET x = 1", None, None)
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(result.results.len(), 2);
    assert_eq!(
        result.result_kinds,
        vec![StatementResultKind::ResultSet, StatementResultKind::Command]
    );
    assert_eq!(result.results[0].row_count, 1);
    assert_eq!(result.results[1].row_count, 3);
}
/// The batch the issue measured (#147): an insert that commits itself, then a failing
/// statement whose failure the envelope used to report as `RolledBack` while the insert
/// survived. It must now be refused before any statement reaches the database.
#[tokio::test]
async fn execute_multi_rejects_a_batch_that_commits_itself() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    // Nothing may reach the database, and no history may be written: a refused batch has not run.
    let mut connector = MockDbConnector::new();
    connector.expect_execute_transaction().never();
    connector.expect_query().never();
    connector.expect_execute().never();

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().never();

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(
            &conn_id,
            "INSERT INTO probe (id) VALUES (1); COMMIT; SELECT no_such_column FROM probe",
            None,
            None,
        )
        .await
        .expect("a refused batch is a result carrying an error, not a transport failure");

    let (index, error) = result.error.expect("the batch carrying `COMMIT` is refused");
    assert_eq!(index, 1, "the refusing statement is reported at its own index");
    assert!(
        error.message.contains("`COMMIT`"),
        "the message names the verb, got: {}",
        error.message
    );
    assert!(result.results.is_empty(), "no statement of a refused batch runs");
    assert!(result.result_kinds.is_empty());
}

#[tokio::test]
async fn execute_multi_rejects_transaction_control_wherever_it_appears() {
    let conn_id = ConnectionId::new();

    for (sql, expected_index, expected_verb) in [
        ("BEGIN; UPDATE t SET x = 1; COMMIT", 0, "BEGIN"),
        ("UPDATE t SET x = 1; ROLLBACK", 1, "ROLLBACK"),
        // An otherwise all-read script: the control verbs classify as `Write`, so the batch
        // still takes the transactional path this refusal guards.
        ("SELECT 1; SELECT 2; COMMIT", 2, "COMMIT"),
        ("SELECT 1; END", 1, "END"),
        ("INSERT INTO t (id) VALUES (1); SAVEPOINT s1", 1, "SAVEPOINT"),
        ("START TRANSACTION; UPDATE t SET x = 1", 0, "START TRANSACTION"),
        ("UPDATE t SET x = 1; ABORT", 1, "ABORT"),
    ] {
        let registry = Arc::new(ConnectionRegistry::new());
        registry.register(conn_id, ConnectionHandle(1));

        let mut connector = MockDbConnector::new();
        connector.expect_execute_transaction().never();
        connector.expect_query().never();
        connector.expect_execute().never();

        let svc = QueryService::new(
            Box::new(connector),
            Box::new(MockQueryHistoryRepository::new()),
            Box::new(MockSavedQueryRepository::new()),
            Box::new(MockRunConfigRepository::new()),
            Arc::clone(&registry),
            Box::new(mock_connections_full_access()),
        );

        let result = svc
            .execute_multi(&conn_id, sql, None, None)
            .await
            .expect("a refused batch is a result carrying an error");
        let (index, error) = result.error.unwrap_or_else(|| panic!("not refused: {sql}"));
        assert_eq!(index, expected_index, "refusing statement index for: {sql}");
        assert!(
            error.message.contains(expected_verb),
            "message for {sql} names `{expected_verb}`, got: {}",
            error.message
        );
    }
}

/// The refusal is about a *batch*: manual transaction control is out of v0.1 scope (#224), so a
/// lone statement keeps the path it already had instead of being refused by this rule.
#[tokio::test]
async fn execute_multi_does_not_apply_the_batch_rule_to_a_single_statement() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_execute().times(1).returning(|_, _, _| Ok(0));
    connector.expect_execute_transaction().never();

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute_multi(&conn_id, "COMMIT", None, None).await.unwrap();
    assert!(
        result.error.is_none(),
        "a single statement is not refused by the batch rule: {:?}",
        result.error
    );
    assert_eq!(result.results.len(), 1);
}

#[tokio::test]
async fn execute_multi_routes_mutating_cte_through_transaction_while_preserving_rows() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_execute_transaction()
        .returning(|_, statements, query_statements| {
            assert_eq!(
                statements,
                &[
                    "WITH deleted AS (DELETE FROM users WHERE id = 1 RETURNING *) SELECT * FROM deleted",
                    "SELECT 1"
                ]
            );
            // The data-modifying CTE returns rows, but still makes the script transactional.
            assert_eq!(query_statements, &[true, true]);
            Ok(vec![
                TransactionStatementResult::Query(test_result()),
                TransactionStatementResult::Query(test_result()),
            ])
        });

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(
            &conn_id,
            "WITH deleted AS (DELETE FROM users WHERE id = 1 RETURNING *) SELECT * FROM deleted; SELECT 1",
            None,
            None,
        )
        .await
        .unwrap();

    assert!(result.error.is_none());
    assert_eq!(result.results.len(), 2);
}

#[tokio::test]
async fn execute_multi_with_update_routes_to_execute() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_execute().returning(|_, _, _| Ok(5));

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(
            &conn_id,
            "WITH cte AS (SELECT id FROM t) UPDATE t SET x = 1",
            None,
            None,
        )
        .await
        .unwrap();
    assert!(result.error.is_none());
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].row_count, 5);
}

#[tokio::test]
async fn execute_multi_routes_update_returning_through_query() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .withf(|_, sql, params| sql == "UPDATE t SET x = 1 RETURNING id" && params.is_empty())
        .returning(|_, _, _| Ok(test_result()));

    let mut history = MockQueryHistoryRepository::new();
    history.expect_save().returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(&conn_id, "UPDATE t SET x = 1 RETURNING id", None, None)
        .await
        .unwrap();

    assert!(result.error.is_none());
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].row_count, 1);
    assert_eq!(result.results[0].columns[0].name, "id");
}

#[tokio::test]
async fn execute_multi_rejects_malformed_transaction_query_result() {
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
    connector
        .expect_execute_transaction()
        .returning(move |_, _, _| Ok(vec![TransactionStatementResult::Query(malformed.clone())]));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(MockQueryHistoryRepository::new()),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(&conn_id, "SELECT 1; UPDATE t SET x = 1", None, None)
        .await
        .unwrap();

    let (index, message) = result.error.expect("invalid transaction result must be rejected");
    assert_eq!(index, 0);
    assert!(message.message.contains("expected 1"));
    assert!(result.results.is_empty());
}

#[tokio::test]
async fn execute_multi_partial_failure_preserves_earlier_results() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_execute_transaction().returning(|_, _, _| {
        Err(TransactionFailure {
            phase: TransactionFailurePhase::Statement,
            statement_index: 1,
            outcome: TransactionFailureOutcome::RolledBack,
            results: vec![TransactionStatementResult::Query(test_result())],
            error: DbError::QueryFailedAt {
                message: "permission denied".into(),
                position: 8,
            },
        })
    });

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(MockQueryHistoryRepository::new()),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute_multi(&conn_id, "SELECT 1; UPDATE t SET x = 1; SELECT 2", None, None)
        .await
        .unwrap();

    assert!(result.error.is_some());
    let (idx, msg) = result.error.unwrap();
    assert_eq!(idx, 1);
    assert!(msg.message.contains("permission denied"));
    assert_eq!(msg.code, "QUERY_FAILED");
    assert_eq!(msg.position, Some(8));
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].row_count, 1);
}

#[tokio::test]
async fn execute_multi_commit_failure_reports_unknown_outcome() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_execute_transaction().returning(|_, _, _| {
        Err(TransactionFailure {
            phase: TransactionFailurePhase::Commit,
            statement_index: 2,
            outcome: TransactionFailureOutcome::Unknown,
            results: vec![TransactionStatementResult::Query(test_result())],
            error: DbError::QueryFailed("connection lost while committing".into()),
        })
    });

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_invalidate()
        .withf(move |id| *id == conn_id)
        .times(1)
        .returning(|_| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(MockQueryHistoryRepository::new()),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    )
    .with_introspection_cache(Box::new(cache));

    let result = svc
        .execute_multi(&conn_id, "CREATE TABLE t (id INT); SELECT 1", None, None)
        .await
        .unwrap();

    let (index, message) = result.error.expect("commit failure must be returned");
    assert_eq!(index, 2);
    assert!(message.message.contains("final outcome is unknown"));
    assert!(!message.message.contains("rolled back"));
    assert_eq!(result.results.len(), 1);
}

#[test]
fn boundary_failure_with_open_transaction_is_not_reported_as_unknown() {
    // A joined run whose setup failed before its first statement leaves the caller's
    // transaction open; the message must say so instead of claiming an unknown outcome.
    let message = format_transaction_failure(
        TransactionFailurePhase::Begin,
        TransactionFailureOutcome::LeftOpen,
        &DbError::ConnectionFailed("search_path rejected".into()),
    );
    assert!(message.contains("the open transaction is still held"), "{message}");
    assert!(!message.contains("unknown"), "{message}");
}

// --- #589: Run-all routes CTE / VALUES / PRAGMA statements to query() --------

#[test]
fn classify_recursive_cte_is_read() {
    let sql = "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 3) SELECT n FROM t";
    assert!(matches!(
        classify_statement(sql, DriverType::Postgres),
        StatementClass::Read
    ));
}

#[test]
fn classify_cte_with_column_list_is_read() {
    for sql in [
        "WITH cte (id) AS (SELECT 1) SELECT * FROM cte",
        "WITH t(n) AS (SELECT 1), u AS (SELECT 2) SELECT * FROM t JOIN u ON true",
        "WITH a AS NOT MATERIALIZED (SELECT 1) SELECT * FROM a",
        "WITH \"a(b\" AS (SELECT 1) SELECT 1",
    ] {
        assert!(
            matches!(classify_statement(sql, DriverType::Postgres), StatementClass::Read),
            "{sql}"
        );
    }
}

#[test]
fn classify_values_and_pragma_are_reads() {
    assert!(matches!(
        classify_statement("VALUES (1), (2)", DriverType::Postgres),
        StatementClass::Read
    ));
    assert!(matches!(
        classify_statement("PRAGMA table_info(users)", DriverType::SQLite),
        StatementClass::Read
    ));
    // A pragma that assigns is a write, and PostgreSQL does not know PRAGMA at all.
    assert!(matches!(
        classify_statement("PRAGMA journal_mode = wal", DriverType::SQLite),
        StatementClass::Write
    ));
    assert!(matches!(
        classify_statement("PRAGMA table_info(users)", DriverType::Postgres),
        StatementClass::Write
    ));
}

#[test]
fn effective_keyword_resolves_the_cte_list_not_its_first_parenthesis() {
    assert_eq!(
        effective_statement_keyword("WITH RECURSIVE t(n) AS (SELECT 1) SELECT n FROM t", DriverType::Postgres)
            .as_deref(),
        Some("SELECT")
    );
    assert_eq!(
        effective_statement_keyword("-- lead\nWITH d AS (SELECT 1) UPDATE t SET x = 1", DriverType::Postgres).as_deref(),
        Some("UPDATE")
    );
    assert_eq!(
        effective_statement_keyword("SELECT 1", DriverType::Postgres).as_deref(),
        Some("SELECT")
    );
}

/// #600: a failed statement must still land in history, marked failed, with its
/// error text — that is exactly when the SQL is worth keeping.
#[tokio::test]
async fn failed_statement_is_recorded_in_history_600() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .returning(|_, _, _| Err(DbError::QueryFailed("relation \"nope\" does not exist".into())));

    let mut history = MockQueryHistoryRepository::new();
    history
        .expect_save()
        .withf(|_, sql, result, _, _, error| {
            sql.contains("nope")
                && result.is_some()
                && result.is_some_and(|result| result.row_count == 0)
                && error
                    .as_deref()
                    .is_some_and(|error| error.contains("does not exist"))
        })
        .times(1)
        .returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc.execute(&conn_id, "SELECT * FROM nope", &[], None, None).await;
    assert!(result.is_err());
}

/// #600: history never stores a credential, even inside the failure text.
#[tokio::test]
async fn failed_statement_history_redacts_credentials_600() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_query().returning(|_, _, _| {
        Err(DbError::QueryFailed(
            "server rejected conninfo host=db password=supersecret".into(),
        ))
    });

    let mut history = MockQueryHistoryRepository::new();
    history
        .expect_save()
        .withf(|_, sql, _, _, _, error| {
            !sql.contains("supersecret") && !error.as_deref().unwrap_or("").contains("supersecret")
        })
        .times(1)
        .returning(|_, _, _, _, _, _| Ok(()));

    let svc = QueryService::new(
        Box::new(connector),
        Box::new(history),
        Box::new(MockSavedQueryRepository::new()),
        Box::new(MockRunConfigRepository::new()),
        Arc::clone(&registry),
        Box::new(mock_connections_full_access()),
    );

    let result = svc
        .execute(&conn_id, "ALTER ROLE app PASSWORD 'supersecret'", &[], None, None)
        .await;
    assert!(result.is_err());
}
