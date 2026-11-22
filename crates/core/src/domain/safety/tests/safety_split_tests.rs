//! Tests for `../safety_split.rs` (moved out of the source file).
use super::*;

#[test]
fn create_prefix_detection_is_ascii_and_bounded() {
    assert!(starts_with_create_keyword("  create trigger x"));
    assert!(starts_with_create_keyword("CREATE TABLE t ()"));
    assert!(!starts_with_create_keyword("CREAT"));
    assert!(!starts_with_create_keyword("SELECT 1"));
}

#[test]
fn trigger_and_atomic_classification() {
    let sql = "CREATE TEMP TRIGGER trg AFTER INSERT ON t";
    let tokens = tokenize_sql(sql, false);
    assert_eq!(classify_create_statement(sql, &tokens, DriverType::SQLite), SplitKind::Trigger);
    let sql = "CREATE FUNCTION f() RETURNS int LANGUAGE sql";
    let tokens = tokenize_sql(sql, false);
    assert_eq!(classify_create_statement(sql, &tokens, DriverType::Postgres), SplitKind::Atomic);
    let sql = "CREATE TABLE t (a int)";
    let tokens = tokenize_sql(sql, false);
    assert_eq!(classify_create_statement(sql, &tokens, DriverType::Postgres), SplitKind::Atomic);
}
