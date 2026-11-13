//! Tests for `../connector.rs` (moved out of the source file).
use super::*;

use db_pro_core::ports::{TransactionFailureOutcome, TransactionFailurePhase};

#[tokio::test]
async fn execute_transaction_reports_validation_failure_on_mismatched_read_statements_length() {
    let connector = MySqlConnector::new();
    let handle = ConnectionHandle::new(999);
    let statements = vec!["SELECT 1".to_string(), "SELECT 2".to_string()];
    let read_statements = vec![true]; // Mismatched length: 2 statements vs 1 read flag

    let failure = connector
        .execute_transaction(&handle, &statements, &read_statements)
        .await
        .expect_err("mismatched statement lengths must fail validation");

    assert_eq!(failure.phase, TransactionFailurePhase::Validation);
    assert_eq!(
        failure.statement_index, 0,
        "statement_index must be 0 for Validation phase failure"
    );
    assert_eq!(failure.outcome, TransactionFailureOutcome::NotStarted);
}

/// Schema `execute_batch` delegates to `execute_transaction`, which publishes
/// `CONNECTION_ID()` before the statements. No live server is required for this check.
fn batch_publishes_connection_before_statements() {
    let source = include_str!("../connector.rs");
    let batch_at = source.find("async fn execute_batch(").expect("execute_batch");
    let tx_at = source[batch_at..]
        .find("async fn execute_transaction(")
        .expect("execute_transaction");
    let batch = &source[batch_at..batch_at + tx_at];
    assert!(
        batch.contains("self\n            .execute_transaction(") || batch.contains("self.execute_transaction("),
        "schema apply must run on the transaction that records the connection id"
    );
    let tx_start = batch_at + tx_at;
    let param_at = source[tx_start..]
        .find("async fn execute_parameterized_transaction(")
        .expect("parameterized transaction follows");
    let body = &source[tx_start..tx_start + param_at];
    let id = body.find("SELECT CONNECTION_ID()").expect("connection id is read");
    let track = body.find("self.track_guard(").expect("id is published for Stop");
    let statements = body
        .find("for (idx, (stmt, is_read))")
        .expect("statements follow the publish");
    assert!(id < track && track < statements);
}

#[test]
fn execute_parameterized_transaction_publishes_connection_id_before_statements() {
    let source = include_str!("../connector.rs");
    let param_at = source
        .find("async fn execute_parameterized_transaction(")
        .expect("execute_parameterized_transaction");
    let body = &source[param_at..];
    let id = body.find("SELECT CONNECTION_ID()").expect("connection id is read");
    let track = body.find("self.track_guard(").expect("id is published for Stop");
    let statements = body
        .find("for (index, statement) in statements.iter().enumerate()")
        .expect("statements follow the publish");
    assert!(id < track && track < statements);
}

#[tokio::test]
async fn execute_parameterized_transaction_unconnected_handle_fails_validation() {
    let connector = MySqlConnector::new();
    let failure = connector
        .execute_parameterized_transaction(&ConnectionHandle::new(99), &[])
        .await
        .expect_err("unconnected handle must return failure");
    assert_eq!(failure.phase, TransactionFailurePhase::Validation);
    assert_eq!(failure.outcome, TransactionFailureOutcome::NotStarted);
}

#[test]
fn execute_parameterized_transaction_dispatches_joined_lease_before_pool_begin() {
    let source = include_str!("../connector.rs");
    let start = source
        .find("async fn execute_parameterized_transaction(")
        .expect("owned execute_parameterized_transaction");
    let rest = &source[start..];
    let end = rest.find("\n    async fn introspect(").expect("introspect follows");
    let body = &rest[..end];
    let checkout = body
        .find("checkout_if_held")
        .expect("joined path checks out the tab lease");
    let joined = body
        .find("execute_parameterized_transaction_joined")
        .expect("joined savepoint path");
    let begin = body.find("pool.begin()").expect("standalone begin remains");
    assert!(checkout < joined && joined < begin);
}

#[test]
fn parameterized_joined_savepoints_use_text_protocol_not_prepared_query() {
    let source = include_str!("../connector.rs");
    let start = source
        .find("async fn execute_parameterized_transaction_joined(")
        .expect("joined parameterized transaction");
    let rest = &source[start..];
    let end = rest.find("\n    fn untrack_connection(").expect("untrack follows");
    let body = &rest[..end];
    assert!(
        body.contains("crate::transaction_session::savepoint_name()"),
        "savepoint names are generated, never taken from input"
    );
    assert!(
        body.contains("mysql_text_execute"),
        "SAVEPOINT/ROLLBACK/RELEASE must use the COM_QUERY helper"
    );
    assert!(
        !body.contains("sqlx::query(&format!(\"SAVEPOINT") && !body.contains("sqlx::query(\"SAVEPOINT"),
        "prepared sqlx::query SAVEPOINT is ERROR 1295 on MySQL"
    );
    assert!(body.contains("ROLLBACK TO SAVEPOINT"));
    assert!(body.contains("RELEASE SAVEPOINT"));
    assert!(body.contains("discard_if_abandoned"));
    assert!(body.contains("lease.succeed()"));
}

#[test]
fn mysql_text_execute_uses_raw_sql_not_prepared_query() {
    let source = include_str!("../connector.rs");
    let start = source.find("fn mysql_text_execute<").expect("text-protocol helper");
    let rest = &source[start..];
    let end = rest.find("\nfn left_open(").expect("left_open follows");
    let body = &rest[..end];
    assert!(body.contains("sqlx::raw_sql(sql)"));
    assert!(!body.contains("sqlx::query("));
}

#[tokio::test]
async fn recorded_batch_connection_is_what_cancel_targets() {
    batch_publishes_connection_before_statements();
    let connector = MySqlConnector::new();
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_millis(200))
        .connect_lazy("mysql://127.0.0.1:1/none")
        .expect("lazy pool does not connect");
    let handle_id = 7;
    connector.pools.write().await.insert(
        handle_id,
        MySqlEntry {
            pool,
            max_rows: 10,
            query_timeout: std::time::Duration::from_secs(5),
            active_connections: std::sync::Mutex::new(Vec::new()),
            open_session: crate::transaction_session::SessionSlot::default(),
        },
    );
    let handle = ConnectionHandle::new(handle_id);
    connector
        .cancel(&handle)
        .await
        .expect("an idle batch has nothing to cancel");

    let guard = connector.track_guard(handle_id, 4242);
    let error = connector
        .cancel(&handle)
        .await
        .expect_err("Stop must send KILL QUERY for the published connection");
    assert!(
        matches!(error, DbError::QueryFailed(_)),
        "cancel reached KILL QUERY instead of treating the batch as idle: {error:?}"
    );
    drop(guard);
    connector
        .cancel(&handle)
        .await
        .expect("the connection id is dropped when the batch ends");
}

#[tokio::test]
async fn execute_batch_that_never_starts_does_not_leave_a_connection_published() {
    let connector = MySqlConnector::new();
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_millis(200))
        .connect_lazy("mysql://127.0.0.1:1/none")
        .expect("lazy pool does not connect");
    let handle_id = 8;
    connector.pools.write().await.insert(
        handle_id,
        MySqlEntry {
            pool,
            max_rows: 10,
            query_timeout: std::time::Duration::from_secs(5),
            active_connections: std::sync::Mutex::new(Vec::new()),
            open_session: crate::transaction_session::SessionSlot::default(),
        },
    );
    let handle = ConnectionHandle::new(handle_id);
    let error = connector
        .execute_batch(&handle, &["CREATE TABLE batch_pid (id int)".to_owned()])
        .await
        .expect_err("begin cannot reach a server");
    assert!(matches!(
        error,
        DbError::QueryFailed(_) | DbError::ConnectionFailed(_) | DbError::QueryTimeout { .. }
    ));
    connector
        .cancel(&handle)
        .await
        .expect("a batch that never started must not leave a connection for Stop");
}

#[tokio::test]
async fn execute_batch_reports_error_on_unknown_handle() {
    let connector = MySqlConnector::new();
    let handle = ConnectionHandle::new(999);
    let statements = vec!["INSERT INTO users VALUES (1)".to_string()];

    let error = connector
        .execute_batch(&handle, &statements)
        .await
        .expect_err("execute_batch on unknown handle must fail");

    assert!(matches!(error, DbError::ConnectionFailed(_)));
}

#[tokio::test]
async fn execute_transaction_reports_begin_failure_on_unknown_handle() {
    let connector = MySqlConnector::new();
    let handle = ConnectionHandle::new(999); // Handle not connected
    let statements = vec!["SELECT 1".to_string(), "SELECT 2".to_string()];
    let read_statements = vec![true, true];

    let failure = connector
        .execute_transaction(&handle, &statements, &read_statements)
        .await
        .expect_err("begin transaction on unknown handle must fail validation/connection lookup");

    assert_eq!(failure.phase, TransactionFailurePhase::Validation);
    assert_eq!(
        failure.statement_index, 0,
        "statement_index must be 0 for Validation phase failure"
    );
    assert_eq!(failure.outcome, TransactionFailureOutcome::NotStarted);
}
