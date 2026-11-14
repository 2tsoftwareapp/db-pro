use super::connection::DriverType;
use safety_cte::classify_cte_safety;
use safety_lexer::{
    is_escape_string_start, is_identifier_continue, is_identifier_start, matching_parenthesis_end, scan_identifier,
    skip_block_comment, skip_dollar_quote, skip_line_comment, skip_quoted, special_token_end, token_is_word,
    tokenize_sql, SqlTokenKind,
};
use safety_split::{BlockSplitter, StatementBodyTerminator};
pub use safety_policy::{validate_against_policy, ConnectionSafetyPolicy};

#[path = "safety/safety_cte.rs"]
mod safety_cte;
#[path = "safety/safety_lexer.rs"]
mod safety_lexer;

#[path = "safety/safety_policy.rs"]
mod safety_policy;

#[path = "safety/safety_split.rs"]
mod safety_split;

/// Classification of a SQL statement for safety enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementSafety {
    /// SELECT, SHOW, EXPLAIN, WITH ... SELECT
    Read,
    /// INSERT, UPDATE, DELETE (with WHERE)
    Write,
    /// CREATE, ALTER
    Ddl,
    /// DROP, TRUNCATE, DELETE without WHERE
    Destructive,
}

impl StatementSafety {
    /// Severity of this classification relative to the others, ordered by how
    /// hard the effect is to undo: read < write < DDL < destructive.
    ///
    /// A script is reduced to its most dangerous statement with this ranking,
    /// so no statement can hide behind a harmless one.
    pub fn severity_rank(self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Write => 1,
            Self::Ddl => 2,
            Self::Destructive => 3,
        }
    }
}

/// Classify a SQL statement for safety enforcement.
/// This is a best-effort heuristic, not a full SQL parser.
pub fn classify_statement_safety(sql: &str) -> Option<StatementSafety> {
    classify_statement_safety_for_driver(sql, DriverType::Postgres)
}

/// `classify_statement_safety` honouring the driver's string-escape rules.
/// Only MySQL turns on backslash escapes inside `'…'` strings; PostgreSQL's
/// `standard_conforming_strings` (on by default) does not (#477).
pub fn classify_statement_safety_for_driver(sql: &str, driver: DriverType) -> Option<StatementSafety> {
    classify_statement_safety_lexed(sql, backslash_escapes_for_driver(driver))
}

/// The verb that decides a statement's *result shape*: the leading keyword, with
/// `WITH` resolved past its CTE list (#589).
///
/// `WITH RECURSIVE t(n) AS (…) SELECT …` returns rows; `WITH d AS (DELETE …)
/// DELETE …` does not. Run-all routes on this verb to pick `query` (grid) over
/// `execute` (row count), so resolving the CTE list is what keeps Run and Run-all
/// returning the same thing for the same statement. The CTE bodies themselves
/// are ignored: data-modifying CTEs stay a safety question, answered by
/// `classify_statement_safety_for_driver`.
pub fn effective_statement_keyword(sql: &str, driver: DriverType) -> Option<String> {
    let backslash_escapes = backslash_escapes_for_driver(driver);
    let trimmed = strip_leading_comments(sql.trim_start()).trim_start();
    let first = first_sql_word(trimmed)?;
    if !first.eq_ignore_ascii_case("WITH") {
        return Some(first.to_owned());
    }
    scan_with_statement(trimmed, backslash_escapes).or_else(|| Some(first.to_owned()))
}

fn first_sql_word(sql: &str) -> Option<&str> {
    sql.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .find(|word| !word.is_empty())
}

/// The first top-level word after the CTE list of a `WITH …` statement.
///
/// Walked structurally rather than by parenthesis counting: `WITH t(n) AS (…)`
/// closes its *column list* first, and returning `AS` as the verb is exactly the
/// defect this replaces. `AS`/`NOT`/`MATERIALIZED` are CTE syntax, never verbs.
fn scan_with_statement(sql: &str, backslash_escapes: bool) -> Option<String> {
    let tokens = tokenize_sql(sql, backslash_escapes);
    let mut index = tokens.iter().position(|token| token_is_word(sql, *token, "WITH"))? + 1;
    if tokens
        .get(index)
        .is_some_and(|token| token_is_word(sql, *token, "RECURSIVE"))
    {
        index += 1;
    }

    let mut depth = 0usize;
    for token in tokens.iter().skip(index) {
        if depth == 0 && token.kind == SqlTokenKind::Word {
            let word = &sql[token.start..token.end];
            if !word.eq_ignore_ascii_case("AS")
                && !word.eq_ignore_ascii_case("NOT")
                && !word.eq_ignore_ascii_case("MATERIALIZED")
            {
                return Some(word.to_owned());
            }
        }
        match token.kind {
            SqlTokenKind::OpenParen => depth += 1,
            SqlTokenKind::CloseParen => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// `true` only for drivers whose `'…'` strings treat `\` as an escape by
/// default. The lexers enable the same behaviour for `E'…'` literals on their
/// own, without this flag.
pub(crate) fn backslash_escapes_for_driver(driver: DriverType) -> bool {
    matches!(driver, DriverType::Mysql)
}

fn classify_statement_safety_lexed(sql: &str, backslash_escapes: bool) -> Option<StatementSafety> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip leading comments
    let trimmed = strip_leading_comments(trimmed);
    let upper = trimmed.to_ascii_uppercase();
    let keyword = upper.split_whitespace().next()?;

    match keyword {
        "SHOW" | "TABLE" => Some(StatementSafety::Read),
        "SELECT" => {
            // SELECT … INTO creates a table (Postgres) or writes a file (MySQL OUTFILE):
            // a top-level INTO is DDL-shaped, not a plain read.
            if has_top_level_sql_keyword(trimmed, "INTO", backslash_escapes) {
                Some(StatementSafety::Ddl)
            } else if has_row_lock_clause(trimmed, backslash_escapes) {
                // `FOR UPDATE` / `FOR NO KEY UPDATE` / `FOR SHARE` / `FOR KEY SHARE`
                // take row locks and hold them for the transaction: a production
                // read-only connection must not be able to block writers (#588).
                Some(StatementSafety::Write)
            } else {
                Some(StatementSafety::Read)
            }
        }
        "EXPLAIN" => classify_explain_safety(trimmed, backslash_escapes),
        "WITH" => classify_cte_safety(trimmed, backslash_escapes),
        "INSERT" => Some(StatementSafety::Write),
        "UPDATE" => {
            if statement_lacks_effective_where(trimmed, backslash_escapes) {
                Some(StatementSafety::Destructive)
            } else {
                Some(StatementSafety::Write)
            }
        }
        "DELETE" => {
            if statement_lacks_effective_where(trimmed, backslash_escapes) {
                Some(StatementSafety::Destructive)
            } else {
                Some(StatementSafety::Write)
            }
        }
        "CREATE" | "ALTER" => Some(StatementSafety::Ddl),
        // Privilege mutations belong in the DDL/audit tier, not with ordinary writes.
        "GRANT" | "REVOKE" | "SECURITY" => Some(StatementSafety::Ddl),
        "DROP" => Some(StatementSafety::Destructive),
        "TRUNCATE" => Some(StatementSafety::Destructive),
        // Full-table rewrites and session teardown: VACUUM FULL/CLUSTER rewrite and lock the
        // table, REINDEX rebuilds under a lock, DISCARD ALL wipes session state.
        "VACUUM" | "CLUSTER" | "REINDEX" | "DISCARD" => Some(StatementSafety::Destructive),
        // These statements can execute server-side or dynamically prepared
        // mutations that the client-side classifier cannot inspect safely.
        "DO" | "CALL" | "EXECUTE" => Some(StatementSafety::Destructive),
        "MERGE" => classify_merge_safety(trimmed, backslash_escapes),
        _ => Some(StatementSafety::Write),
    }
}

/// Whether a `SELECT` carries a row-lock clause (`FOR UPDATE`, `FOR SHARE`,
/// `FOR NO KEY UPDATE`, `FOR KEY SHARE`, optionally behind `OF …`/`NOWAIT`).
///
/// `FOR UPDATE` inside a subquery locks that subquery's rows just the same, so
/// the whole statement is scanned rather than only its top level.
fn has_row_lock_clause(sql: &str, backslash_escapes: bool) -> bool {
    let tokens = tokenize_sql(sql, backslash_escapes);
    tokens.iter().enumerate().any(|(index, token)| {
        token_is_word(sql, *token, "FOR")
            && tokens.get(index + 1).is_some_and(|next| {
                ["UPDATE", "SHARE", "NO", "KEY"]
                    .iter()
                    .any(|keyword| token_is_word(sql, *next, keyword))
            })
    })
}

/// Classify a SQL script that may hold several statements by its **most
/// dangerous** statement.
///
/// A script must never be classified as read-only merely because it starts with
/// `SELECT`: the decision that can auto-execute a script has to see every
/// statement in it (#147, #129). Returns `None` only when the script holds no
/// statement at all.
pub fn classify_script_safety(sql: &str) -> Option<StatementSafety> {
    classify_script_safety_for_driver(sql, DriverType::Postgres)
}

/// `classify_script_safety` honouring the driver's string-escape rules (#477).
pub fn classify_script_safety_for_driver(sql: &str, driver: DriverType) -> Option<StatementSafety> {
    let backslash_escapes = backslash_escapes_for_driver(driver);
    split_statements_for_driver(sql, driver)
        .into_iter()
        .filter_map(|statement| classify_statement_safety_lexed(&statement, backslash_escapes))
        .max_by_key(|safety| safety.severity_rank())
}

/// The transaction-control verb a statement starts with, if it is one.
///
/// `BEGIN`/`START TRANSACTION`, `COMMIT`/`END`, `ROLLBACK`/`ABORT`, `SAVEPOINT` and `RELEASE`
/// take control of a transaction explicitly. A multi-statement batch that contains any mutation is
/// executed inside a transaction of its own (`Connector::execute_transaction`, `#129`), so a
/// statement like this one does not join that transaction — it ends or re-scopes it. The statements
/// after it run outside the wrapper, and the wrapper's rollback then has nothing left to undo while
/// the failure envelope still reports `RolledBack` (#147).
///
/// Only the leading keyword is inspected: a `BEGIN` inside `DO $body$ … $body$` belongs to that
/// statement's own body and is classified by its leading `DO`, and quoted text is not a keyword.
pub fn transaction_control_verb(sql: &str) -> Option<&'static str> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let trimmed = strip_leading_comments(trimmed);
    if trimmed.is_empty() {
        return None;
    }

    let upper = trimmed.to_ascii_uppercase();
    let mut words = upper.split_whitespace();
    match words.next()? {
        "BEGIN" => Some("BEGIN"),
        "COMMIT" => Some("COMMIT"),
        "END" => Some("END"),
        "ROLLBACK" => Some("ROLLBACK"),
        "ABORT" => Some("ABORT"),
        "SAVEPOINT" => Some("SAVEPOINT"),
        "RELEASE" => Some("RELEASE"),
        "START" if words.next() == Some("TRANSACTION") => Some("START TRANSACTION"),
        _ => None,
    }
}

/// Split a SQL script into its statements on statement-terminating semicolons.
///
/// Semicolons inside quoted strings, quoted identifiers, comments and
/// dollar-quoted bodies do not terminate a statement, and comment-only
/// fragments are dropped rather than returned as empty statements.
pub fn split_statements(sql: &str) -> Vec<String> {
    split_statements_for_driver(sql, DriverType::Postgres)
}

/// `split_statements` honouring the driver's string-escape rules.
///
/// PostgreSQL's `standard_conforming_strings` (on by default) means a trailing
/// `\` inside `'…'` is plain text, so `'x\'; …` is a closed string followed by a
/// real statement boundary. MySQL's default sql_mode treats `\'` as an escaped
/// quote instead. The default `split_statements` keeps the PostgreSQL reading,
/// which is the safe interpretation for every other driver (#477).
pub fn split_statements_for_driver(sql: &str, driver: DriverType) -> Vec<String> {
    split_with_body_terminators(sql, driver).0
}

/// Split a script into units the driver can execute one at a time, and report
/// where statement separators fell *inside* a `BEGIN … END` body (#586).
///
/// The body delimiters are needed by engines that re-split what they receive
/// (SQLite runs a batch one statement at a time); the splitter already knows
/// which `;` values those are, and a second, simpler splitter is exactly how the
/// two paths drifted apart before.
fn split_with_body_terminators(
    sql: &str,
    driver: DriverType,
) -> (Vec<String>, Vec<StatementBodyTerminator>) {
    let backslash_escapes = backslash_escapes_for_driver(driver);
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut splitter = BlockSplitter::new(driver);
    let mut body_terminators = Vec::new();
    let bytes = sql.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if let Some(end) = special_token_end(bytes, index, backslash_escapes) {
            current.push_str(&sql[index..end]);
            index = end;
        } else if bytes[index] == b';' && splitter.terminates_statement(sql, index, backslash_escapes) {
            body_terminators.append(&mut splitter.take_body_terminators());
            push_statement(&mut statements, &current);
            current.clear();
            index += 1;
        } else {
            let Some(character) = sql[index..].chars().next() else {
                break;
            };
            current.push(character);
            index += character.len_utf8();
        }
    }

    body_terminators.append(&mut splitter.take_body_terminators());
    push_statement(&mut statements, &current);
    (statements, body_terminators)
}

/// Split one statement at the `;` separators that belong to its `BEGIN … END`
/// body, so an engine with no block-aware parser can run each inner statement
/// (SQLite `execute_batch`).
///
/// A statement without a body comes back as the single original string, byte for
/// byte: this must never reformat or rejoin SQL that the caller wrote.
pub fn split_statement_body_for_driver(statement: &str, driver: DriverType) -> Vec<String> {
    let (_, terminators) = split_with_body_terminators(statement, driver);
    let mut parts = Vec::new();
    let mut start = 0;
    for terminator in terminators.iter().filter(|terminator| terminator.after_complete_statement) {
        let part = statement[start..terminator.offset].trim();
        if !part.is_empty() {
            parts.push(part.to_owned());
        }
        start = terminator.offset + 1;
    }
    if parts.is_empty() {
        return vec![statement.to_owned()];
    }
    let tail = statement[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_owned());
    }
    parts
}

/// True when `sql` is a single, self-contained expression fragment — no
/// statement terminator, no comments, balanced parentheses. Used to vet raw
/// expression text (RLS `USING`/`WITH CHECK` bodies) before it is embedded in
/// a generated statement (#507).
///
/// `backslash_escapes` must match the target driver's string rules
/// (`backslash_escapes_for_driver`).
pub fn single_expression_fragment(sql: &str, backslash_escapes: bool) -> bool {
    let bytes = sql.as_bytes();
    let mut depth = 0i64;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => index = skip_quoted(bytes, index, b'\'', backslash_escapes),
            b'"' => index = skip_quoted(bytes, index, b'"', backslash_escapes),
            byte if is_escape_string_start(bytes, index, byte) => index = skip_quoted(bytes, index + 1, b'\'', true),
            // Comment introducers are rejected outright: comment text inside a
            // generated policy statement can swallow the rest of the SQL.
            b'-' if bytes.get(index + 1) == Some(&b'-') => return false,
            b'/' if bytes.get(index + 1) == Some(&b'*') => return false,
            b'$' => index = skip_dollar_quote(bytes, index).unwrap_or(index + 1),
            b';' => return false,
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    depth == 0
}

fn push_statement(statements: &mut Vec<String>, current: &str) {
    let statement = current.trim();
    if !strip_leading_comments(statement).trim().is_empty() {
        statements.push(statement.to_owned());
    }
}

fn classify_merge_safety(sql: &str, backslash_escapes: bool) -> Option<StatementSafety> {
    if contains_sql_keyword(sql, "DELETE", backslash_escapes) {
        Some(StatementSafety::Destructive)
    } else {
        Some(StatementSafety::Write)
    }
}

/// For EXPLAIN statements: plain EXPLAIN is Read, but EXPLAIN ANALYZE actually
/// executes the inner statement, so its safety depends on the inner statement.
fn classify_explain_safety(sql: &str, backslash_escapes: bool) -> Option<StatementSafety> {
    let mut remainder = strip_leading_comments(consume_keyword(sql, "EXPLAIN")?.1).trim_start();

    // PostgreSQL's parenthesized EXPLAIN options are metadata, not the query
    // being explained. Only ANALYZE causes the inner statement to execute.
    if remainder.starts_with('(') {
        let end = matching_parenthesis_end(remainder, backslash_escapes)?;
        let options = &remainder[1..end - 1];
        if !contains_sql_keyword(options, "ANALYZE", backslash_escapes)
            && !contains_sql_keyword(options, "ANALYSE", backslash_escapes)
        {
            return Some(StatementSafety::Read);
        }
        remainder = remainder[end..].trim_start();
        return classify_statement_safety_lexed(remainder, backslash_escapes);
    }

    // The legacy spelling is EXPLAIN [ANALYZE] [VERBOSE] statement. Match
    // whole keywords in the original SQL so literals/identifiers cannot alter
    // the decision and Unicode case conversion cannot invalidate byte offsets.
    let Some((_, after_analyze)) =
        consume_keyword(remainder, "ANALYZE").or_else(|| consume_keyword(remainder, "ANALYSE"))
    else {
        return Some(StatementSafety::Read);
    };

    let mut inner_sql = strip_leading_comments(after_analyze).trim_start();
    if let Some((_, after_verbose)) = consume_keyword(inner_sql, "VERBOSE") {
        inner_sql = strip_leading_comments(after_verbose).trim_start();
    }

    classify_statement_safety_lexed(inner_sql, backslash_escapes)
}

fn consume_keyword<'a>(sql: &'a str, expected: &str) -> Option<(&'a str, &'a str)> {
    let value = strip_leading_comments(sql).trim_start();
    let bytes = value.as_bytes();
    let first = *bytes.first()?;
    if !is_identifier_start(first) {
        return None;
    }

    let mut end = 1;
    while end < bytes.len() && is_identifier_continue(bytes[end]) {
        end += 1;
    }
    let keyword = &value[..end];
    if keyword.eq_ignore_ascii_case(expected) {
        Some((keyword, &value[end..]))
    } else {
        None
    }
}

/// Check whether a DELETE/UPDATE's top-level WHERE clause is missing or is a
/// trivially-true predicate (`WHERE TRUE`, `WHERE 1=1`, `WHERE 'a'='a'`).
fn statement_lacks_effective_where(sql: &str, backslash_escapes: bool) -> bool {
    let Some(clause) = top_level_where_clause(sql, backslash_escapes) else {
        return true;
    };
    where_clause_is_tautology(clause, backslash_escapes)
}

/// The text of the depth-0 WHERE clause, up to the next clause keyword
/// (`RETURNING`, `ORDER BY`, `LIMIT`, …). `None` when no top-level WHERE exists —
/// a WHERE inside a subquery does not restrict the outer mutation.
fn top_level_where_clause(sql: &str, backslash_escapes: bool) -> Option<&str> {
    const CLAUSE_ENDERS: [&str; 6] = ["returning", "order", "group", "limit", "offset", "output"];
    let mut depth = 0usize;
    let mut where_start = None;
    let mut clause_end = sql.len();
    for token in tokenize_sql(sql, backslash_escapes) {
        match token.kind {
            SqlTokenKind::OpenParen => depth += 1,
            SqlTokenKind::CloseParen => depth = depth.saturating_sub(1),
            SqlTokenKind::Word if depth == 0 => {
                if where_start.is_none() {
                    if token_is_word(sql, token, "where") {
                        where_start = Some(token.end);
                    }
                } else if CLAUSE_ENDERS.iter().any(|w| token_is_word(sql, token, w)) {
                    clause_end = token.start;
                    break;
                }
            }
            _ => {}
        }
    }
    where_start.map(|start| &sql[start..clause_end])
}
/// True when the clause text is a single literal truthy expression:
/// `TRUE`, a non-zero number, or `LITERAL = LITERAL` with equal literals.
/// Anything more complex stays effective — false negatives keep writes gated.
fn where_clause_is_tautology(clause: &str, backslash_escapes: bool) -> bool {
    let compact = compact_clause(clause, backslash_escapes);
    let compact = strip_outer_parentheses(compact.as_str(), backslash_escapes);
    if compact.eq_ignore_ascii_case("true") {
        return true;
    }
    if let Ok(number) = compact.parse::<f64>() {
        return number != 0.0;
    }
    if let Some(eq) = compact.find('=') {
        let left = compact[..eq].trim();
        let right = compact[eq + 1..].trim();
        return is_sql_literal(left) && left == right;
    }
    false
}

fn is_sql_literal(text: &str) -> bool {
    (text.len() > 1 && text.starts_with('\'') && text.ends_with('\''))
        || text.parse::<f64>().is_ok()
        || text.eq_ignore_ascii_case("true")
        || text.eq_ignore_ascii_case("false")
}

/// Remove whitespace, comments and `;` outside of quoted strings, then collapse
/// runs of real whitespace to a single space so tokens stay separated.
fn compact_clause(clause: &str, backslash_escapes: bool) -> String {
    let bytes = clause.as_bytes();
    let mut out = String::with_capacity(clause.len());
    let mut index = 0;
    let mut pending_space = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                let end = skip_quoted(bytes, index, bytes[index], backslash_escapes);
                out.push_str(&clause[index..end]);
                index = end;
            }
            byte if is_escape_string_start(bytes, index, byte) => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                let end = skip_quoted(bytes, index + 1, b'\'', true);
                out.push_str(&clause[index..end]);
                index = end;
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => index = skip_line_comment(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => index = skip_block_comment(bytes, index),
            byte if byte.is_ascii_whitespace() || byte == b';' => {
                pending_space |= byte.is_ascii_whitespace();
                index += 1;
            }
            _ => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(bytes[index] as char);
                index += 1;
            }
        }
    }
    out
}

/// Strip balanced outer parentheses: `((1=1))` reduces to `1=1`.
fn strip_outer_parentheses(text: &str, backslash_escapes: bool) -> &str {
    let mut text = text.trim();
    loop {
        if text.starts_with('(')
            && text.ends_with(')')
            && matching_parenthesis_end(text, backslash_escapes) == Some(text.len())
        {
            text = text[1..text.len() - 1].trim();
        } else {
            return text;
        }
    }
}

/// Find a keyword while ignoring quoted strings, quoted identifiers, and SQL
/// comments. This is intentionally not a full SQL parser; it prevents a
/// keyword-like substring from changing the destructive-operation decision.
fn contains_sql_keyword(sql: &str, expected: &str, backslash_escapes: bool) -> bool {
    let bytes = sql.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        index = match bytes[index] {
            b'\'' => skip_quoted(bytes, index, b'\'', backslash_escapes),
            b'"' => skip_quoted(bytes, index, b'"', backslash_escapes),
            byte if is_escape_string_start(bytes, index, byte) => skip_quoted(bytes, index + 1, b'\'', true),
            b'-' if bytes.get(index + 1) == Some(&b'-') => skip_line_comment(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => skip_block_comment(bytes, index),
            b'$' => skip_dollar_quote(bytes, index).unwrap_or(index + 1),
            current if is_identifier_start(current) => {
                let end = scan_identifier(bytes, index);
                if bytes[index..end].eq_ignore_ascii_case(expected.as_bytes()) {
                    return true;
                }
                end
            }
            _ => index + 1,
        }
    }

    false
}

pub(crate) fn has_top_level_sql_keyword(sql: &str, expected: &str, backslash_escapes: bool) -> bool {
    let mut depth = 0usize;
    for token in tokenize_sql(sql, backslash_escapes) {
        match token.kind {
            SqlTokenKind::OpenParen => depth += 1,
            SqlTokenKind::CloseParen => depth = depth.saturating_sub(1),
            SqlTokenKind::Word if depth == 0 && token_is_word(sql, token, expected) => return true,
            _ => {}
        }
    }
    false
}

/// Whether `character` appears at parenthesis depth 0 outside strings, quoted
/// identifiers, comments and dollar quotes, honouring the driver's string rules.
///
/// Only `;` and `=` matter to callers today; the byte-level walk is what keeps a
/// `PRAGMA` setter (`PRAGMA journal_mode = wal`) distinguishable from its reader
/// spelling (`PRAGMA table_info(t)`) without a second tokenizer (#589).
pub(crate) fn has_top_level_operator(sql: &str, character: u8, driver: DriverType) -> bool {
    let backslash_escapes = backslash_escapes_for_driver(driver);
    let bytes = sql.as_bytes();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = special_token_end(bytes, index, backslash_escapes) {
            index = end;
            continue;
        }
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            byte if byte == character && depth == 0 => return true,
            _ => {}
        }
        index += 1;
    }
    false
}

fn strip_leading_comments(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if s.starts_with("--") {
            s = s.find('\n').map(|i| &s[i + 1..]).unwrap_or("").trim_start();
        } else if s.starts_with("/*") {
            s = s.find("*/").map(|i| &s[i + 2..]).unwrap_or("").trim_start();
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
#[path = "tests/safety_tests.rs"]
mod tests;
