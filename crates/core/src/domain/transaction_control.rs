//! Recognize a transaction-control statement the editor can pin to one connection.
//!
//! Only forms that open, close, or roll back the whole transaction are recognized.
//! `COMMIT AND CHAIN` and `ROLLBACK TO SAVEPOINT` stay unrecognized so the caller
//! keeps the connection and the open-transaction flag.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionControl {
    Begin,
    Commit,
    Rollback,
}

/// `Some` when `sql` is a single transaction-control statement.
pub fn transaction_control(sql: &str) -> Option<TransactionControl> {
    let words = control_words(sql);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["begin"] | ["begin", "work"] | ["begin", "transaction"] | ["start", "transaction"] => {
            Some(TransactionControl::Begin)
        }
        ["commit"]
        | ["commit", "work"]
        | ["commit", "transaction"]
        | ["end"]
        | ["end", "work"]
        | ["end", "transaction"] => Some(TransactionControl::Commit),
        ["rollback"] | ["rollback", "work"] | ["rollback", "transaction"] | ["abort"] | ["abort", "work"] => {
            Some(TransactionControl::Rollback)
        }
        ["begin", _, ..] | ["start", "transaction", _, ..] => Some(TransactionControl::Begin),
        _ => None,
    }
}

fn control_words(sql: &str) -> Vec<String> {
    sql.trim()
        .trim_end_matches(';')
        .split_whitespace()
        .map(|word| word.trim_matches(';').to_ascii_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "tests/transaction_control_tests.rs"]
mod tests;
