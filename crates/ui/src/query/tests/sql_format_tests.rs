//! Tests for `../sql_format.rs` (moved out of the source file).
use super::*;

#[test]
fn formats_clause_boundaries_without_rewriting_literals_or_comments() {
    let sql = "select 'select from', note from users -- where stays\nwhere id = 1";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Postgres);

    assert!(formatted.contains("'select from'"));
    assert!(formatted.contains("-- where stays"));
    assert!(formatted.contains("SELECT 'select from', note\nFROM users"));
    assert!(formatted.ends_with("\nWHERE id = 1"));
}

#[test]
fn preserves_dollar_quoted_postgres_body() {
    let sql = "select $tag$from where select$tag$ from functions";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Postgres);

    assert!(formatted.contains("$tag$from where select$tag$"));
    assert!(formatted.ends_with("\nFROM functions"));
}

#[test]
fn formats_incomplete_sql_without_panicking() {
    assert_eq!(format_sql("select * from"), "SELECT *\nFROM");
}

// --- #601: backslash-escaped literals keep their exact bytes ------------------

#[test]
fn postgres_escape_string_literal_is_copied_byte_for_byte() {
    let sql = "UPDATE t SET note = E'it\\'s from here' WHERE id = 1;";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Postgres);
    assert!(
        formatted.contains("E'it\\'s from here'"),
        "literal was rewritten: {formatted}"
    );
    assert!(!formatted.contains("FROM here"), "{formatted}");
    assert!(formatted.ends_with("\nWHERE id = 1;"), "{formatted}");
}

#[test]
fn generic_dialect_treats_backslash_as_an_escape_inside_strings() {
    // MySQL's default sql_mode does the same thing without the `E` prefix.
    // (The editor maps MySQL and SQL Server onto `Generic`.)
    let sql = "select 'it\\'s from here' as note from t";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Generic);
    assert!(formatted.contains("'it\\'s from here'"), "{formatted}");
    assert!(formatted.ends_with("\nFROM t"), "{formatted}");
}

#[test]
fn doubled_quotes_still_close_the_literal() {
    let sql = "select 'it''s from here' from t";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Postgres);
    assert!(formatted.contains("'it''s from here'"), "{formatted}");
    assert!(formatted.ends_with("\nFROM t"), "{formatted}");
}

#[test]
fn postgres_plain_string_keeps_a_trailing_backslash() {
    // `standard_conforming_strings=on`: `'x\'` is the string `x\`, and the next
    // `from` really is a clause keyword.
    let sql = "select 'x\\' from t";
    let formatted = format_sql_for_dialect(sql, SqlDialect::Postgres);
    assert!(formatted.contains("'x\\'"), "{formatted}");
    assert!(formatted.ends_with("\nFROM t"), "{formatted}");
}
