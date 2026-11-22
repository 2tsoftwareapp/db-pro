use crate::editor::SqlDialect;

const CLAUSE_KEYWORDS: &[&str] = &[
    "INSERT INTO",
    "DELETE FROM",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "INNER JOIN",
    "CROSS JOIN",
    "GROUP BY",
    "ORDER BY",
    "SELECT",
    "RETURNING",
    "FROM",
    "WHERE",
    "HAVING",
    "JOIN",
    "SET",
    "VALUES",
    "LIMIT",
    "OFFSET",
];

/// Formats common SQL clause boundaries without rewriting protected SQL text.
///
/// This intentionally remains a conservative formatter for incomplete editor input:
/// comments, quoted identifiers, string literals, and PostgreSQL dollar-quoted bodies
/// are copied byte-for-byte. The dialect is accepted at this boundary so callers do not
/// accidentally format a document using a different provider context as the formatter grows.
pub fn format_sql(sql: &str) -> String {
    format_sql_for_dialect(sql, SqlDialect::Postgres)
}

/// `format_sql` honouring the dialect's string-escape rules (#601).
///
/// A string literal is copied byte-for-byte, so the only thing that can go wrong
/// is failing to recognise where the literal ends: `E'it\'s from here'` and the
/// MySQL `'it\'s from here'` both close one quote later than a scanner that only
/// understands `''` believes. Getting that wrong made `from` look like a clause
/// keyword *inside* the value, and Format rewrote the value that would be stored.
pub fn format_sql_for_dialect(sql: &str, dialect: SqlDialect) -> String {
    // `Generic` is the only non-PostgreSQL value the editor produces (MySQL and
    // SQL Server both map to it), and their default string rules agree on
    // backslash escapes. PostgreSQL's `E'…'` prefix is handled per literal.
    let backslash_escapes = matches!(dialect, SqlDialect::Generic);
    let mut formatted = String::with_capacity(sql.len() + 32);
    let mut offset = 0;
    let mut parenthesis_depth = 0usize;

    while offset < sql.len() {
        if let Some(end) = protected_sql_end(sql, offset, backslash_escapes) {
            formatted.push_str(&sql[offset..end]);
            offset = end;
            continue;
        }

        let Some(character) = sql[offset..].chars().next() else {
            break;
        };
        if character == '(' {
            parenthesis_depth = parenthesis_depth.saturating_add(1);
            formatted.push(character);
            offset += character.len_utf8();
            continue;
        }
        if character == ')' {
            parenthesis_depth = parenthesis_depth.saturating_sub(1);
            formatted.push(character);
            offset += character.len_utf8();
            continue;
        }

        if let Some(keyword) = matching_clause_keyword(sql, offset) {
            if parenthesis_depth == 0 && needs_clause_break(&formatted) {
                trim_trailing_horizontal_whitespace(&mut formatted);
                if !formatted.ends_with('\n') {
                    formatted.push('\n');
                }
            }
            formatted.push_str(keyword);
            offset += keyword.len();
            continue;
        }

        formatted.push(character);
        offset += character.len_utf8();
    }

    formatted.trim().to_owned()
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
fn protected_sql_end(sql: &str, offset: usize, backslash_escapes: bool) -> Option<usize> {
    let rest = &sql[offset..];
    if rest.starts_with("--") {
        return Some(sql[offset..].find('\n').map_or(sql.len(), |line_end| offset + line_end));
    }
    if rest.starts_with("/*") {
        return Some(
            sql[offset..]
                .find("*/")
                .map_or(sql.len(), |comment_end| offset + comment_end + 2),
        );
    }

    // `E'…'` is PostgreSQL's opt-in escape string regardless of the dialect:
    // the prefix decides, not the connection's other string rules.
    if let Some(end) = escape_string_end(sql, offset) {
        return Some(end);
    }

    // The dialect flag covers MySQL's default `sql_mode`, where a backslash
    // escapes inside a plain `'…'` string.
    let quote = rest.chars().next()?;
    if quote == '\'' || quote == '"' || quote == '`' {
        return Some(quoted_end(
            sql,
            offset + quote.len_utf8(),
            quote,
            backslash_escapes && quote == '\'',
        ));
    }

    if quote == '$' {
        let tag_end = sql[offset + 1..]
            .char_indices()
            .find(|(_, character)| *character == '$')
            .map(|(index, _)| offset + 1 + index)?;
        let tag = &sql[offset..=tag_end];
        let body_start = tag_end + 1;
        return sql[body_start..]
            .find(tag)
            .map_or(Some(sql.len()), |end| Some(body_start + end + tag.len()));
    }

    None
}

fn matching_clause_keyword(sql: &str, offset: usize) -> Option<&'static str> {
    CLAUSE_KEYWORDS.iter().copied().find(|keyword| {
        let end = offset + keyword.len();
        end <= sql.len()
            && sql[offset..end].eq_ignore_ascii_case(keyword)
            && is_left_word_boundary(sql, offset)
            && is_right_word_boundary(sql, end)
    })
}

fn is_left_word_boundary(sql: &str, offset: usize) -> bool {
    sql[..offset]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

fn is_right_word_boundary(sql: &str, offset: usize) -> bool {
    sql[offset..]
        .chars()
        .next()
        .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
}

fn needs_clause_break(formatted: &str) -> bool {
    !formatted.is_empty() && !formatted.ends_with('\n')
}

fn trim_trailing_horizontal_whitespace(formatted: &mut String) {
    while formatted.ends_with([' ', '\t']) {
        formatted.pop();
    }
}

/// Index just past the closing `quote` of the run starting at `cursor`.
///
/// A doubled quote (`''`) always stays inside the literal. `escapes_enabled`
/// additionally hides the character after a backslash, which is what `E'…'`
/// (PostgreSQL) and default-`sql_mode` MySQL strings do.
fn quoted_end(sql: &str, cursor: usize, quote: char, escapes_enabled: bool) -> usize {
    let mut cursor = cursor;
    while cursor < sql.len() {
        let Some(character) = sql[cursor..].chars().next() else {
            break;
        };
        cursor += character.len_utf8();
        if escapes_enabled && quote == '\'' && character == '\\' {
            if let Some(escaped) = sql[cursor..].chars().next() {
                cursor += escaped.len_utf8();
            }
            continue;
        }
        if character == quote {
            if sql[cursor..].starts_with(quote) {
                cursor += quote.len_utf8();
            } else {
                break;
            }
        }
    }
    cursor
}

/// `E'…'` starting at `offset`: the literal closes one quote later than a plain
/// string when a backslash precedes the quote. This is PostgreSQL's own opt-in
/// escape syntax, so it applies for every dialect value.
fn escape_string_end(sql: &str, offset: usize) -> Option<usize> {
    let mut characters = sql[offset..].chars();
    let prefix = characters.next()?;
    if (prefix != 'E' && prefix != 'e') || characters.next() != Some('\'') {
        return None;
    }
    // `someE'…'` is an identifier followed by a plain string, not an escape string.
    if sql[..offset]
        .chars()
        .next_back()
        .is_some_and(|previous| previous.is_alphanumeric() || previous == '_')
    {
        return None;
    }
    Some(quoted_end(sql, offset + 2, '\'', true))
}

#[cfg(test)]
#[path = "tests/sql_format_tests.rs"]
mod tests;
