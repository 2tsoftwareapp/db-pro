//! Tests for `../connector.rs` (moved out of the source file).
use super::super::actor::SqliteOpenMode;
use super::{sqlite_target, SqliteDialect};
use db_pro_core::application::sql_builder::{build_select, SortClause, SortDir};
use db_pro_core::domain::connection::ConnectionConfig;

#[test]
fn table_pagination_uses_sqlite_placeholders() {
    let (sql, params) = build_select(
        &SqliteDialect,
        "main",
        "customers",
        &[],
        &[SortClause {
            column: "id".to_owned(),
            direction: SortDir::Desc,
        }],
        100,
        200,
    )
    .expect("SQLite pagination should build");

    assert_eq!(
        sql,
        r#"SELECT * FROM "main"."customers" ORDER BY "id" DESC LIMIT ? OFFSET ?"#
    );
    assert_eq!(params.len(), 2);
}

fn config(database: &str, readonly: bool, create_if_missing: bool) -> ConnectionConfig {
    ConnectionConfig {
        database: database.to_owned(),
        readonly,
        create_if_missing,
        ..ConnectionConfig::default()
    }
}

#[test]
fn empty_database_path_is_an_error_not_a_memory_database() {
    // #597: an empty path used to open `:memory:`, so a blank form silently
    // produced a database that vanished on disconnect.
    let error = sqlite_target(&config("   ", false, false))
        .err()
        .expect("an empty path must be rejected");
    assert!(error.to_string().contains("empty"), "{error}");
}

#[test]
fn in_memory_must_be_requested_explicitly() {
    for explicit in [":memory:", "MEMORY:"] {
        let (target, mode) = sqlite_target(&config(explicit, false, false)).expect("explicit memory");
        assert_eq!(target, ":memory:");
        assert_eq!(mode, SqliteOpenMode::Memory);
    }
}

#[test]
fn file_target_defaults_to_existing_only() {
    let (target, mode) = sqlite_target(&config("/tmp/dbpro.db", false, false)).expect("target");
    assert_eq!(target, "/tmp/dbpro.db");
    assert_eq!(mode, SqliteOpenMode::Existing, "a missing file is not created implicitly");

    let (_, mode) = sqlite_target(&config("/tmp/dbpro.db", false, true)).expect("target");
    assert_eq!(mode, SqliteOpenMode::CreateIfMissing, "the opt-in is honoured");
}

#[test]
fn readonly_wins_over_create_if_missing() {
    // #588: a read-only connection must never create the file as a side effect.
    let (_, mode) = sqlite_target(&config("/tmp/dbpro.db", true, true)).expect("target");
    assert_eq!(mode, SqliteOpenMode::ReadOnly);
}
