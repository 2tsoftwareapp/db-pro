//! Tests for `../migration.rs` (moved out of the source file).
use super::*;

#[test]
fn migrations_are_ordered() {
    for window in MIGRATIONS.windows(2) {
        assert!(
            window[0].version < window[1].version,
            "migrations must be strictly ordered: {} >= {}",
            window[0].version,
            window[1].version,
        );
    }
}

#[test]
fn latest_version_matches_registry() {
    if let Some(last) = MIGRATIONS.last() {
        assert_eq!(
            LATEST_VERSION, last.version,
            "LATEST_VERSION must match the last migration"
        );
    }
}

#[test]
fn migration_descriptions_non_empty() {
    for m in MIGRATIONS {
        assert!(
            !m.description.is_empty(),
            "migration {} has empty description",
            m.version
        );
    }
}

#[tokio::test]
async fn test_migration_execution() {
    use crate::meta::schema::SCHEMA;
    use crate::sqlite::actor::SqliteActor;

    // Use in-memory SQLite
    let handle = SqliteActor::spawn(":memory:").unwrap();

    // Setup schema baseline
    handle.execute_statement(SCHEMA.into()).await.unwrap();

    // Run migrate
    let final_version = migrate(&handle).await.unwrap();
    assert_eq!(final_version, LATEST_VERSION);

    // Introspect schema_version to ensure it is inserted properly
    let rows = handle
        .raw_query(
            "SELECT version, description FROM schema_version ORDER BY version ASC".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(rows.len() as u32, LATEST_VERSION);
    assert_eq!(rows[0][0], "1");
    assert_eq!(rows[0][1], "initial schema with versioning table");
    assert_eq!(rows[1][0], "2");
    assert_eq!(rows[1][1], "add database and schema columns to query_history");
}

#[tokio::test]
async fn test_migration_idempotency() {
    use crate::meta::schema::SCHEMA;
    use crate::sqlite::actor::SqliteActor;

    let handle = SqliteActor::spawn(":memory:").unwrap();
    handle.execute_statement(SCHEMA.into()).await.unwrap();

    // Run migrate twice
    let v1 = migrate(&handle).await.unwrap();
    let v2 = migrate(&handle).await.unwrap();

    // Should be idempotent
    assert_eq!(v1, LATEST_VERSION);
    assert_eq!(v2, LATEST_VERSION);

    // Should only have LATEST_VERSION rows in schema_version
    let rows = handle
        .raw_query("SELECT COUNT(*) FROM schema_version".into(), vec![])
        .await
        .unwrap();
    assert_eq!(rows[0][0], LATEST_VERSION.to_string());
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn test_migration_upgrade_from_old_schema() {
    use crate::sqlite::actor::SqliteActor;

    let handle = SqliteActor::spawn(":memory:").unwrap();

    // Simulate old schema WITHOUT database/schema columns
    let old_schema = r#"
            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS query_history (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                sql TEXT NOT NULL,
                executed_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                row_count INTEGER NOT NULL
            );
        "#;
    handle.execute_statement(old_schema.into()).await.unwrap();

    // Insert v1 as already applied
    handle
            .raw_query(
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                "INSERT INTO schema_version (version, applied_at, description) VALUES (1, '2024-01-01T00:00:00Z', 'initial')".into(),
                vec![],
            )
            .await
            .unwrap();

    // Run migrate - should apply v2 successfully
    let final_version = migrate(&handle).await.unwrap();
    assert_eq!(final_version, LATEST_VERSION);

    // Verify columns were added
    let rows = handle
        .raw_query(
            "SELECT sql FROM sqlite_master WHERE name = 'query_history'".into(),
            vec![],
        )
        .await
        .unwrap();
    let schema_sql = &rows[0][0];
    assert!(
        schema_sql.contains("database"),
        "query_history should have database column"
    );
    assert!(schema_sql.contains("schema"), "query_history should have schema column");
}

#[tokio::test]
async fn malformed_schema_version_fails_closed() {
    use crate::sqlite::actor::SqliteActor;

    let handle = SqliteActor::spawn(":memory:").unwrap();
    handle
        .execute_statement("CREATE TABLE schema_version (version TEXT, applied_at TEXT, description TEXT)".into())
        .await
        .unwrap();
    handle
        .execute_statement(
            "INSERT INTO schema_version (version, applied_at, description) VALUES ('not-a-version', '', '')".into(),
        )
        .await
        .unwrap();

    let error = current_version(&handle).await.expect_err("malformed version must fail");
    assert!(error.to_string().contains("invalid schema version"));
}

#[tokio::test]
async fn unsupported_future_schema_version_is_rejected() {
    use crate::meta::schema::SCHEMA;
    use crate::sqlite::actor::SqliteActor;

    let handle = SqliteActor::spawn(":memory:").unwrap();
    handle.execute_statement(SCHEMA.into()).await.unwrap();
    handle
        .execute_param(
            "INSERT INTO schema_version (version, applied_at, description) VALUES (?1, ?2, ?3)".into(),
            vec![
                QueryParam::Int64(i64::from(LATEST_VERSION) + 1),
                QueryParam::Text("2026-01-01T00:00:00Z".into()),
                QueryParam::Text("future".into()),
            ],
        )
        .await
        .unwrap();

    let error = migrate(&handle).await.expect_err("future version must be rejected");
    assert!(error.to_string().contains("newer than supported version"));
}

/// #600: v3/v4 add `success`/`error_message` to a v2-shaped table, and re-running
/// the migration must not fail on the columns it already added.
#[tokio::test]
async fn migration_adds_history_status_columns_idempotently_600() {
    use crate::sqlite::actor::SqliteActor;

    let handle = SqliteActor::spawn(":memory:").unwrap();
    // v2 shape: the base table without the new columns.
    handle
        .execute_statement(
            "CREATE TABLE query_history (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                sql TEXT NOT NULL,
                executed_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                row_count INTEGER NOT NULL,
                database TEXT,
                schema TEXT
            )"
            .into(),
        )
        .await
        .unwrap();
    // Pretend v2 was applied, so only v3/v4 run.
    handle
        .execute_statement(
            "CREATE TABLE schema_version (
                version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL, description TEXT NOT NULL
            )"
            .into(),
        )
        .await
        .unwrap();
    for version in 1..=2 {
        handle
            .execute_param(
                "INSERT INTO schema_version (version, applied_at, description) VALUES (?, ?, ?)".into(),
                vec![
                    db_pro_core::domain::query::QueryParam::Int64(version),
                    db_pro_core::domain::query::QueryParam::Text("2024-01-01T00:00:00Z".into()),
                    db_pro_core::domain::query::QueryParam::Text("seed".into()),
                ],
            )
            .await
            .unwrap();
    }

    assert_eq!(migrate(&handle).await.unwrap(), LATEST_VERSION);
    // A second run is a no-op rather than a "duplicate column" error.
    assert_eq!(migrate(&handle).await.unwrap(), LATEST_VERSION);

    let columns = handle
        .raw_query("PRAGMA table_info(query_history)".into(), vec![])
        .await
        .unwrap();
    let names: Vec<&str> = columns
        .iter()
        .filter_map(|column| column.get(1).map(String::as_str))
        .collect();
    assert!(names.contains(&"success"), "columns: {names:?}");
    assert!(names.contains(&"error_message"), "columns: {names:?}");
}
