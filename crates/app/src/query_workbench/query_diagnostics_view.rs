//! Query formatting and SQL diagnostics orchestration.

use super::query_view::{deduplicate_diagnostics, deduplicate_messages, format_query_document};
use super::*;
use crate::editor::{Diagnostic, SqlDialect};
use sqlparser::dialect::{GenericDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use std::time::{Duration, Instant};

pub(super) fn format_active_query(
    query: &mut QueryFeatureState,
    capabilities: CapabilityLookup,
    feedback: &mut FeedbackState,
) -> Option<RequestId> {
    let doc_index = query.session.active_document_index;
    let cancelled_prediction = query.session.invalidate_prediction(doc_index);
    let dialect = if capabilities.allows(|caps| caps.query.numbered_parameters) {
        SqlDialect::Postgres
    } else {
        SqlDialect::SQLite
    };
    if let Some(doc) = query.session.documents.get_mut(doc_index) {
        // #605: formatting an untouched document used to be a silent no-op.
        let changed = format_query_document(doc, dialect);
        feedback.set_runtime_message(if changed {
            "SQL formatted"
        } else {
            "Already well formatted"
        });
    }
    cancelled_prediction
}

#[cfg(test)]
pub(super) fn analyze_sql_diagnostics(sql: &str, driver: &str) -> (Vec<String>, Vec<Diagnostic>) {
    analyze_sql_diagnostics_with_lint(sql, driver, &SqlLintSettings::default())
}

pub(super) fn analyze_sql_diagnostics_with_lint(
    sql: &str,
    driver: &str,
    lint: &SqlLintSettings,
) -> (Vec<String>, Vec<Diagnostic>) {
    let mut string_diagnostics = Vec::new();
    let mut structured_diagnostics = Vec::new();
    let capabilities = match CapabilityLookup::for_driver_label(driver) {
        CapabilityLookup::Supported(caps) => Some(caps),
        CapabilityLookup::NoActiveConnection | CapabilityLookup::UnsupportedDriver { .. } => None,
    };
    let parse_result = if capabilities.as_ref().is_some_and(|caps| caps.query.numbered_parameters) {
        Parser::parse_sql(&PostgreSqlDialect {}, sql)
    } else if driver.eq_ignore_ascii_case("mysql") {
        // MySQL shares the positional editor dialect; GenericDialect is the closest
        // sqlparser stand-in until a dedicated MySQL dialect is wired.
        Parser::parse_sql(&GenericDialect {}, sql)
    } else {
        Parser::parse_sql(&SQLiteDialect {}, sql)
    };
    if let Err(error) = parse_result {
        let msg = format!("SQL parser: {error}");
        string_diagnostics.push(msg.clone());
        structured_diagnostics.push(Diagnostic::error((0, sql.len().clamp(1, 4)), msg));
    }
    if sql.trim().is_empty() {
        string_diagnostics.push("Query is empty".to_owned());
        return (string_diagnostics, structured_diagnostics);
    }
    for issue in crate::editor::brackets::structural_delimiter_issues(sql) {
        let end = issue.offset + sql[issue.offset..].chars().next().map_or(1, char::len_utf8);
        let message = if let Some(expected) = issue.expected {
            format!("Mismatched delimiter {}: expected {}", issue.character, expected)
        } else {
            format!("Unmatched delimiter {}", issue.character)
        };
        string_diagnostics.push(message.clone());
        structured_diagnostics.push(Diagnostic::delimiter((issue.offset, end), message));
    }
    append_sql_lint_diagnostics(sql, lint, &mut string_diagnostics, &mut structured_diagnostics);
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut in_string_escapes = false;
    let mut string_start_byte = 0;

    // E'...' strings (Postgres) and MySQL strings treat backslash as an
    // escape; standard conforming strings do not (#611).
    let mysql_like = driver.eq_ignore_ascii_case("mysql");
    let postgres_like = capabilities
        .as_ref()
        .is_some_and(|caps| caps.query.numbered_parameters);
    let bytes = sql.as_bytes();
    let mut idx = 0usize;
    while idx < sql.len() {
        let Some(ch) = sql[idx..].chars().next() else {
            break;
        };
        let byte_offset = idx;
        idx += ch.len_utf8();
        if ch == '\'' {
            if !in_string {
                in_string = true;
                string_start_byte = byte_offset;
                in_string_escapes = if mysql_like {
                    true
                } else if postgres_like && byte_offset >= 1 {
                    let prev = bytes[byte_offset - 1];
                    matches!(prev, b'E' | b'e')
                        && (byte_offset < 2 || !is_identifier_byte(bytes[byte_offset - 2]))
                } else {
                    false
                };
                current.push(ch);
                continue;
            }
            // Doubled quote is the standard string escape.
            if sql[idx..].starts_with('\'') {
                current.push('\'');
                current.push('\'');
                idx += 1;
                continue;
            }
            in_string = false;
            current.push(ch);
            continue;
        }
        if ch == '\\' && in_string && in_string_escapes {
            current.push(ch);
            if let Some(escaped) = sql[idx..].chars().next() {
                current.push(escaped);
                idx += escaped.len_utf8();
            }
            continue;
        }
        if in_string {
            current.push(ch);
        } else if matches!(ch, '(' | ')') {
            tokens.push((current.to_lowercase(), byte_offset));
            current.clear();
        } else if ch.is_whitespace() || ch == ';' || ch == ',' {
            if !current.is_empty() {
                tokens.push((current.to_lowercase(), byte_offset - current.len()));
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push((current.to_lowercase(), sql.len() - current.len()));
    }
    if in_string {
        let msg = "Unclosed string literal".to_owned();
        string_diagnostics.push(msg.clone());
        structured_diagnostics.push(Diagnostic::error((string_start_byte, sql.len()), msg));
    }
    if lint.allows("lint.update-no-where")
        && tokens.first().map(|(t, _)| t.as_str()) == Some("update")
        && !tokens.iter().any(|(t, _)| t == "where")
    {
        let msg = "UPDATE without WHERE will affect every row".to_owned();
        string_diagnostics.push(msg.clone());
        structured_diagnostics.push(Diagnostic::lint((0, sql.len().min(6)), msg, "lint.update-no-where"));
    }
    // Dialect capability mismatches — distinct from lint; these are provider contract errors.
    if !capabilities.as_ref().is_some_and(|caps| caps.query.ilike) {
        if let Some((_, offset)) = tokens.iter().find(|(t, _)| t == "ilike") {
            let msg = "ILIKE is not supported for this provider; use LIKE or lower()".to_owned();
            string_diagnostics.push(msg.clone());
            structured_diagnostics.push(Diagnostic::error((*offset, offset + 5), msg));
        }
    }
    if !capabilities.as_ref().is_some_and(|caps| caps.query.glob) {
        if let Some((_, offset)) = tokens.iter().find(|(t, _)| t == "glob") {
            let msg = "GLOB is not supported for this provider; use LIKE instead".to_owned();
            string_diagnostics.push(msg.clone());
            structured_diagnostics.push(Diagnostic::error((*offset, offset + 4), msg));
        }
    }
    let lower = sql.to_lowercase();
    if lower.contains("select * from") && lower.contains("select * from select") {
        let msg = "Subquery must be enclosed in parentheses".to_owned();
        string_diagnostics.push(msg.clone());
        structured_diagnostics.push(Diagnostic::error((0, sql.len()), msg));
    }

    (
        deduplicate_messages(string_diagnostics),
        deduplicate_diagnostics(structured_diagnostics),
    )
}
fn append_sql_lint_diagnostics(
    sql: &str,
    lint: &SqlLintSettings,
    string_diagnostics: &mut Vec<String>,
    structured_diagnostics: &mut Vec<Diagnostic>,
) {
    if !lint.enabled {
        return;
    }
    let lower = sql.to_lowercase();
    // SELECT * — warn on the star token when it is a projection wildcard.
    if lint.allows("lint.select-star") {
        if let Some(star_at) = lower.find("select") {
            let after = &lower[star_at..];
            if let Some(rel) = after.find('*') {
                let abs = star_at + rel;
                let before_ok = after[..rel].chars().rev().find(|c| !c.is_whitespace()).is_some();
                let from_follows = after[rel..].contains("from");
                if before_ok && from_follows {
                    let msg = "SELECT * makes column contracts brittle; prefer an explicit column list".to_owned();
                    string_diagnostics.push(msg.clone());
                    structured_diagnostics.push(Diagnostic::lint((abs, abs + 1), msg, "lint.select-star"));
                }
            }
        }
    }
    // = NULL / != NULL / <> NULL — always unknown in SQL; suggest IS [NOT] NULL.
    if lint.allows("lint.null-compare") {
        for (needle, suggestion) in [
            ("= null", "IS NULL"),
            ("!= null", "IS NOT NULL"),
            ("<> null", "IS NOT NULL"),
            ("=null", "IS NULL"),
            ("!=null", "IS NOT NULL"),
            ("<>null", "IS NOT NULL"),
        ] {
            if let Some(at) = lower.find(needle) {
                let msg = format!("Comparing with NULL using {needle} is always unknown; use {suggestion}");
                string_diagnostics.push(msg.clone());
                structured_diagnostics.push(Diagnostic::lint_with_fix(
                    (at, at + needle.len()),
                    msg,
                    "lint.null-compare",
                    suggestion,
                ));
            }
        }
    }
    // DELETE without WHERE.
    let trimmed = lower.trim_start();
    if lint.allows("lint.delete-no-where") && trimmed.starts_with("delete") && !lower.contains("where") {
        let msg = "DELETE without WHERE will remove every row".to_owned();
        string_diagnostics.push(msg.clone());
        structured_diagnostics.push(Diagnostic::lint((0, sql.len().min(6)), msg, "lint.delete-no-where"));
    }
    // ORDER BY n — positional ordinals are brittle across projection changes.
    if lint.allows("lint.order-by-ordinal") {
        if let Some(order_at) = lower.find("order by") {
            let after = &lower[order_at + "order by".len()..];
            let trimmed_after = after.trim_start();
            let skip = after.len() - trimmed_after.len();
            if let Some(first) = trimmed_after.chars().next() {
                if first.is_ascii_digit() {
                    let abs = order_at + "order by".len() + skip;
                    let end = abs
                        + trimmed_after
                            .chars()
                            .take_while(|c| c.is_ascii_digit() || *c == ',' || c.is_whitespace())
                            .map(char::len_utf8)
                            .sum::<usize>();
                    let msg = "ORDER BY ordinal is brittle; prefer an explicit column or expression".to_owned();
                    string_diagnostics.push(msg.clone());
                    structured_diagnostics.push(Diagnostic::lint(
                        (abs, end.max(abs + 1)),
                        msg,
                        "lint.order-by-ordinal",
                    ));
                }
            }
        }
    }
    // FROM a, b — classic comma join / cartesian-product pattern when JOIN is absent.
    if lint.allows("lint.comma-join") {
        if let Some(from_at) = lower.find("from") {
            let after_from = &lower[from_at + 4..];
            let has_join = after_from.contains(" join ")
                || after_from.contains(" join\n")
                || after_from.contains("\njoin ")
                || after_from.starts_with("join ")
                || after_from.contains(" join(");
            if !has_join {
                if let Some(comma_rel) = after_from.find(',') {
                    let between = after_from[..comma_rel].trim();
                    let after_comma = after_from[comma_rel + 1..].trim_start();
                    let looks_like_table = !between.is_empty()
                        && after_comma
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphabetic() || c == '"');
                    if looks_like_table {
                        let abs = from_at + 4 + comma_rel;
                        let msg = "Comma join may produce a cartesian product; prefer explicit JOIN … ON".to_owned();
                        string_diagnostics.push(msg.clone());
                        structured_diagnostics.push(Diagnostic::lint((abs, abs + 1), msg, "lint.comma-join"));
                    }
                }
            }
        }
    }
    // Duplicate projection aliases: `SELECT a AS x, b AS x`.
    if lint.allows("lint.duplicate-alias") {
        if let Some(select_at) = lower.find("select") {
            let after_select = &lower[select_at + "select".len()..];
            let projection = after_select.split(" from ").next().unwrap_or(after_select);
            let mut seen: Vec<(String, usize)> = Vec::new();
            let mut search_from = 0usize;
            while let Some(rel) = projection[search_from..].find(" as ") {
                let abs_in_proj = search_from + rel + " as ".len();
                let alias_slice = projection[abs_in_proj..].trim_start();
                let skip = projection[abs_in_proj..].len() - alias_slice.len();
                let alias: String = alias_slice
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '"')
                    .collect();
                if !alias.is_empty() {
                    let alias_key = alias.trim_matches('"').to_ascii_lowercase();
                    let abs = select_at + "select".len() + abs_in_proj + skip;
                    if let Some((_, first_at)) = seen.iter().find(|(name, _)| name == &alias_key) {
                        let msg = format!("Duplicate projection alias `{alias_key}`");
                        string_diagnostics.push(msg.clone());
                        structured_diagnostics.push(Diagnostic::lint(
                            (abs, abs + alias.len()),
                            msg,
                            "lint.duplicate-alias",
                        ));
                        let _ = first_at;
                    } else {
                        seen.push((alias_key, abs));
                    }
                }
                search_from = abs_in_proj + alias.len().max(1);
            }
        }
    }
}

#[cfg(test)]
pub(super) fn parse_sql_diagnostics(sql: &str, driver: &str) -> Vec<String> {
    analyze_sql_diagnostics_with_lint(sql, driver, &SqlLintSettings::default()).0
}

// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
pub(super) fn refresh_diagnostics(query: &mut QueryFeatureState, driver: &str, lint: &SqlLintSettings) {
    let doc_index = query.session.active_document_index;
    let version = query
        .session
        .documents
        .get(doc_index)
        .map(|doc| doc.buffer.version())
        .unwrap_or(0);
    let cache_key = (doc_index, version);
    let exec_fp = query
        .session
        .documents
        .get(doc_index)
        .and_then(|doc| doc.execution_diagnostic.as_ref())
        .map(|d| d.range);

    if query.editor.diagnostics_cache_key == Some(cache_key) && query.editor.diagnostics_cache_driver == driver {
        // Cheap path: only rematch when execution diagnostic identity changes.
        if query.editor.diagnostics_exec_fp == exec_fp {
            return;
        }
        if let Some(doc) = query.session.documents.get_mut(doc_index) {
            let execution = doc.execution_diagnostic.clone();
            doc.diagnostics = merge_editor_diagnostics(
                query.editor.diagnostics_lint_structured.iter().cloned(),
                execution,
                &doc.diagnostics,
            );
        }
        query.editor.diagnostics_exec_fp = exec_fp;
        return;
    }

    // While typing, defer sqlparser until a short quiet window so keystrokes stay snappy.
    let now = Instant::now();
    if query.editor.diagnostics_debounce_key != Some(cache_key) {
        query.editor.diagnostics_debounce_key = Some(cache_key);
        query.editor.diagnostics_debounce_at = Some(now + Duration::from_millis(180));
        return;
    }
    if let Some(deadline) = query.editor.diagnostics_debounce_at {
        if now < deadline {
            return;
        }
    }

    if let Some(doc) = query.session.documents.get_mut(doc_index) {
        let (raw_diags, structured) = analyze_sql_diagnostics_with_lint(doc.text(), driver, lint);
        query.editor.diagnostics = raw_diags;
        query.editor.diagnostics_lint_structured = structured.clone();
        let execution = doc.execution_diagnostic.clone();
        doc.diagnostics = merge_editor_diagnostics(structured, execution, &doc.diagnostics);
    } else {
        query.editor.diagnostics = analyze_sql_diagnostics_with_lint(query.session.active_text(), driver, lint).0;
        query.editor.diagnostics_lint_structured.clear();
    }
    query.editor.diagnostics_cache_key = Some(cache_key);
    query.editor.diagnostics_cache_driver = driver.to_owned();
    query.editor.diagnostics_exec_fp = exec_fp;
    query.editor.diagnostics_debounce_at = None;
}

fn preserved_syntax_diagnostics(diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("syntax"))
        .cloned()
        .collect()
}

fn merge_editor_diagnostics(
    structured: impl IntoIterator<Item = Diagnostic>,
    execution: Option<Diagnostic>,
    existing: &[Diagnostic],
) -> Vec<Diagnostic> {
    let syntax = preserved_syntax_diagnostics(existing);
    let mut combined: Vec<Diagnostic> = structured.into_iter().collect();
    if !syntax.is_empty() {
        combined.retain(|diagnostic| !diagnostic.message.starts_with("SQL parser:"));
    }
    if let Some(execution) = execution {
        combined.push(execution);
    }
    combined.extend(syntax);
    deduplicate_diagnostics(combined)
}

/// Word-boundary helper for the E-string prefix check (#611).
fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 128
}
