//! Tests for `../query_history_repo.rs` (moved out of the source file).
use super::parse_metric;

#[test]
fn query_history_metrics_reject_invalid_values() {
    assert_eq!(parse_metric("42", "row_count").unwrap(), 42);
    for value in ["", "-1", "1.5"] {
        assert!(parse_metric(value, "row_count").is_err(), "value {value:?} should fail");
    }
}

/// #600: a failed statement is stored with `success = 0` and its error text, and
/// the migration adds the columns to an existing (v2-shaped) table.
#[tokio::test]
async fn failed_statements_round_trip_through_history_600() {
    use crate::meta::migration::migrate;
    use crate::meta::schema::SCHEMA;
    use crate::sqlite::actor::SqliteActor;
    use db_pro_core::domain::connection::ConnectionId;
    use db_pro_core::domain::query::QueryResult;
    use db_pro_core::ports::QueryHistoryRepository;

    let handle = SqliteActor::spawn(":memory:").unwrap();
    handle.execute_statement(SCHEMA.into()).await.unwrap();
    migrate(&handle).await.unwrap();
    let store = super::SQLiteMetaStore::from_handle(handle);

    let connection_id = ConnectionId::new();
    let failed = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        duration_ms: 12,
    };
    store
        .save(
            &connection_id,
            "SELECT * FROM nope",
            Some(&failed),
            None,
            None,
            Some("relation \"nope\" does not exist".to_owned()),
        )
        .await
        .unwrap();

    let entries = store.list(&connection_id, 10).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].success);
    assert_eq!(
        entries[0].error_message.as_deref(),
        Some("relation \"nope\" does not exist")
    );
    assert_eq!(entries[0].duration_ms, 12);
}
