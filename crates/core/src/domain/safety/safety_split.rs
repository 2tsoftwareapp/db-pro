//! Block-aware statement boundary detection (#586).
//!
//! `split_statements_for_driver` terminates a statement on every top-level `;`.
//! That is wrong for the multi-statement bodies SQL delimits with keywords
//! rather than with the statement terminator:
//!
//! ```text
//! CREATE TRIGGER trg AFTER INSERT ON t BEGIN UPDATE t SET b = 'x'; SELECT 1; END;   -- SQLite
//! CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END; -- PostgreSQL 14+
//! ```
//!
//! Both use `BEGIN … END`, where the inner `;` separators belong to the body.
//! The splitter used to cut them into fragments (`… BEGIN UPDATE t …`,
//! `SELECT 1`, `END`), and the trailing `END` fragment then read as a
//! transaction-control verb, so the batch was rejected before reaching the
//! server (`batch refused: END cannot run inside an atomic batch`) and a
//! generated `CREATE TRIGGER` DDL could not be replayed.
//!
//! Scope is deliberately narrow:
//!
//! * only a `CREATE` statement can open a body, so every other statement keeps
//!   the previous, well-tested `;` behaviour (`SplitKind::None` is decided from
//!   the leading keyword, without tokenizing the whole script);
//! * a `BEGIN` opens a block for a `CREATE … TRIGGER` statement (SQLite,
//!   PostgreSQL, MySQL, SQL Server) or when it is spelled `BEGIN ATOMIC`
//!   (PostgreSQL SQL-standard function bodies);
//! * inside a block, `CASE … END` and `BEGIN … END` nest and a closing `END`
//!   absorbs its `IF`/`LOOP`/`CASE`/`WHILE`/`REPEAT`/`FOR` suffix (`END IF;`);
//! * a bare `BEGIN;` never balances a later `END;`, so `BEGIN; … END;` is still
//!   two statements and transaction-control detection is unchanged.
//!
//! `;` inside strings, quoted identifiers, comments and dollar quotes never
//! reach this code: `split_statements_for_driver` consumes them first through
//! `special_token_end`.

use crate::domain::connection::DriverType;

use super::safety_lexer::{token_is_word, tokenize_sql, SqlToken, SqlTokenKind};

/// Words that follow a closing `END` as part of the same keyword group, so
/// `END IF;` / `END LOOP;` do not shift the nesting depth.
const END_SUFFIXES: [&str; 8] = ["IF", "LOOP", "CASE", "WHILE", "REPEAT", "FOR", "TRY", "CATCH"];
/// How many leading words are inspected to classify a `CREATE` statement.
const PREFIX_WORDS: usize = 6;

/// One `;` that belongs to the statement being accumulated instead of ending it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatementBodyTerminator {
    /// Byte offset of the `;` inside the statement text.
    pub(crate) offset: usize,
    /// Whether everything before this `;` is a complete statement on its own.
    /// A `;` inside an open `CASE … END` expression is not, so handing that
    /// prefix to the engine would be a syntax error.
    pub(crate) after_complete_statement: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitKind {
    /// No keyword-delimited body: every top-level `;` terminates a statement.
    None,
    /// `CREATE … TRIGGER`: a bare `BEGIN` opens the trigger body.
    Trigger,
    /// Any other `CREATE`: only `BEGIN ATOMIC` opens a body.
    Atomic,
}

#[derive(Debug)]
pub(super) struct BlockSplitter {
    driver: DriverType,
    /// Byte offset where the statement currently being accumulated starts.
    statement_start: usize,
    /// Classification of the statement being accumulated, decided once its
    /// prefix is complete.
    kind: SplitKind,
    /// `;` offsets that belong to the current body, in script coordinates.
    body_terminators: Vec<StatementBodyTerminator>,
}

impl BlockSplitter {
    pub(super) fn new(driver: DriverType) -> Self {
        Self {
            driver,
            statement_start: 0,
            kind: SplitKind::None,
            body_terminators: Vec::new(),
        }
    }

    /// `true` when the `;` at `semicolon` terminates the current statement.
    ///
    /// Stateful: a caller must pass every top-level `;` of `sql` in order. The
    /// statement prefix is re-tokenized only for `CREATE` statements, so an
    /// ordinary `SELECT …;` script costs one ASCII prefix check per statement.
    pub(super) fn terminates_statement(&mut self, sql: &str, semicolon: usize, backslash_escapes: bool) -> bool {
        let statement = &sql[self.statement_start..semicolon];
        if self.kind == SplitKind::None {
            if !starts_with_create_keyword(statement) {
                self.statement_start = semicolon + 1;
                return true;
            }
            let tokens = tokenize_sql(statement, backslash_escapes);
            self.kind = classify_create_statement(statement, &tokens, self.driver);
        }

        if self.kind == SplitKind::None {
            self.statement_start = semicolon + 1;
            return true;
        }

        let tokens = tokenize_sql(statement, backslash_escapes);
        let state = last_block_depth(statement, &tokens, self.kind);
        if state.depth == 0 {
            self.kind = SplitKind::None;
            self.statement_start = semicolon + 1;
            return true;
        }
        // The body may or may not know this `;` as a statement of its own: only a
        // delimiter directly after a complete inner statement (`… SET a = 1;` or
        // a nested `END;`) is legal to hand to the engine alone.
        self.body_terminators.push(StatementBodyTerminator {
            offset: semicolon,
            after_complete_statement: !state.case_expression_open,
        });
        false
    }

    /// The `;` offsets recorded inside bodies since the last [`Self::take_body_terminators`].
    pub(super) fn take_body_terminators(&mut self) -> Vec<StatementBodyTerminator> {
        std::mem::take(&mut self.body_terminators)
    }
}

/// Cheap prefix test, so non-`CREATE` statements never pay for tokenization.
fn starts_with_create_keyword(statement: &str) -> bool {
    let trimmed = statement.trim_start();
    trimmed.len() >= 6 && trimmed.as_bytes()[..6].eq_ignore_ascii_case(b"CREATE")
}

/// `CREATE [OR REPLACE|OR ALTER] [TEMP|TEMPORARY] <kind>` classification.
fn classify_create_statement(statement: &str, tokens: &[SqlToken], driver: DriverType) -> SplitKind {
    let mut words = Vec::with_capacity(PREFIX_WORDS);
    let mut depth = 0usize;
    for token in tokens {
        match token.kind {
            SqlTokenKind::OpenParen => depth += 1,
            SqlTokenKind::CloseParen => depth = depth.saturating_sub(1),
            SqlTokenKind::Word if depth == 0 => {
                words.push(statement[token.start..token.end].to_ascii_uppercase());
                if words.len() == PREFIX_WORDS {
                    break;
                }
            }
            _ => {}
        }
    }

    let mut words = words.into_iter();
    if !words.next().is_some_and(|word| word == "CREATE") {
        return SplitKind::None;
    }
    for word in words {
        match word.as_str() {
            "TRIGGER" if driver != DriverType::SqlServer => return SplitKind::Trigger,
            // `CREATE [OR REPLACE|OR ALTER] [TEMP|TEMPORARY] [CONSTRAINT] …`
            // modifiers keep the prefix scan going until the object kind shows up.
            "OR" | "REPLACE" | "ALTER" | "TEMP" | "TEMPORARY" | "CONSTRAINT" | "UNIQUE" | "DEFERRABLE" => {}
            _ => return SplitKind::Atomic,
        }
    }
    SplitKind::Atomic
}

/// Block nesting at the point where the statement text ends.
#[derive(Debug, Clone, Copy, Default)]
struct BlockDepth {
    /// `BEGIN`/`CASE`/`LOOP` depth. `0` means no open body, so the next `;`
    /// terminates the statement.
    depth: usize,
    /// `true` when the last thing before the end of the text is an open
    /// `BEGIN … CASE` *expression* (`CASE WHEN …`), where a `;` cannot legally
    /// appear until the `END` closes it.
    case_expression_open: bool,
}

/// Walk `statement` and report its block state at the end of the text.
// cc-scan:allow COMPLEXITY — one arm per token kind that can open or close a block
fn last_block_depth(statement: &str, tokens: &[SqlToken], kind: SplitKind) -> BlockDepth {
    let mut state = BlockDepth::default();
    let mut case_ends = Vec::new();
    let mut depth = 0usize;
    let mut parentheses = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token.kind {
            // A column list (`CREATE TABLE t (case int)`) is not a body: only
            // words at parenthesis depth 0 can open or close a block.
            SqlTokenKind::OpenParen => parentheses += 1,
            SqlTokenKind::CloseParen => parentheses = parentheses.saturating_sub(1),
            _ => {}
        }
        if token.kind != SqlTokenKind::Word || parentheses > 0 {
            index += 1;
            continue;
        }
        let word = &statement[token.start..token.end];
        if depth == 0 {
            if token_is_word(statement, token, "BEGIN")
                && (kind == SplitKind::Trigger
                    || next_token_is_word(statement, tokens, index, "ATOMIC"))
            {
                depth = 1;
                if kind != SplitKind::Trigger {
                    index += 1; // consume ATOMIC
                }
            }
        } else if token_is_word(statement, token, "BEGIN") || token_is_word(statement, token, "LOOP") {
            depth += 1;
        } else if token_is_word(statement, token, "CASE") {
            depth += 1;
            case_ends.push(depth);
        } else if token_is_word(statement, token, "END") {
            if case_ends.last() == Some(&depth) {
                case_ends.pop();
            }
            depth = depth.saturating_sub(1);
            if next_token_is_end_suffix(statement, tokens, index) {
                index += 1;
            }
        }
        state.depth = depth;
        state.case_expression_open = case_ends.last() == Some(&depth);
        index += 1;
    }
    state
}

fn next_token_is_word(statement: &str, tokens: &[SqlToken], index: usize, expected: &str) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|token| token_is_word(statement, *token, expected))
}

fn next_token_is_end_suffix(statement: &str, tokens: &[SqlToken], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|token| END_SUFFIXES.iter().any(|suffix| token_is_word(statement, *token, suffix)))
}

#[cfg(test)]
#[path = "tests/safety_split_tests.rs"]
mod tests;
