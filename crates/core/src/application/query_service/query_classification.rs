use crate::domain::connection::DriverType;
use crate::domain::safety::{
    backslash_escapes_for_driver, effective_statement_keyword, has_top_level_operator, has_top_level_sql_keyword,
};

pub(super) enum StatementClass {
    Read,
    Write,
}

pub(super) fn classify_statement(sql: &str, driver: DriverType) -> StatementClass {
    let backslash_escapes = backslash_escapes_for_driver(driver);
    match effective_statement_keyword(sql, driver) {
        Some(keyword) if is_read_keyword(&keyword, sql, driver) => StatementClass::Read,
        Some(keyword) if is_row_producing_mutation(&keyword, sql, backslash_escapes) => StatementClass::Read,
        _ => StatementClass::Write,
    }
}

fn is_row_producing_mutation(keyword: &str, sql: &str, backslash_escapes: bool) -> bool {
    matches!(keyword, "INSERT" | "UPDATE" | "DELETE" | "MERGE")
        && has_top_level_sql_keyword(sql, "RETURNING", backslash_escapes)
}

/// Keywords whose statement returns a result set on every supported driver.
const UNIVERSAL_READ_KEYWORDS: [&str; 5] = ["SELECT", "SHOW", "EXPLAIN", "TABLE", "VALUES"];

/// `true` when a statement starting with `keyword` returns rows (#589).
///
/// `PRAGMA` returns rows for the introspection spellings SQLite users run
/// (`PRAGMA table_info(t)`, `PRAGMA index_list(t)`), but `PRAGMA name = value`
/// writes a setting, so only the reader spelling counts as a read. Other drivers
/// never see `PRAGMA`.
fn is_read_keyword(keyword: &str, sql: &str, driver: DriverType) -> bool {
    if UNIVERSAL_READ_KEYWORDS.contains(&keyword) {
        return true;
    }
    matches!(driver, DriverType::SQLite) && keyword == "PRAGMA" && !has_top_level_operator(sql, b'=', driver)
}
