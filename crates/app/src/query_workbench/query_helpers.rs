//! Query document helpers: atomic export I/O, diagnostics dedupe, and SQL formatting.
use super::query_editor_support::close_completion;
use super::*;
use crate::editor::{Diagnostic, EditorSnapshot, SelectionRange, SqlDialect};

/// Distinguishes concurrent temp files of one process (see [`write_file_atomically`]).
static ATOMIC_WRITE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Write `contents` to `path`, publishing the file in one step (#244, E-1).
///
/// `std::fs::write(path, …)` streams straight at the destination, so a failure part-way through —
/// disk full, I/O error, volume removed — leaves a **truncated file that looks like a complete
/// export**. This writes a sibling temp file first (same directory, so the rename stays on one
/// filesystem), removes it if anything fails, and only then renames it over the destination: the
/// destination either keeps its previous content or holds the complete new bytes, never a prefix of
/// them.
pub(crate) fn write_file_atomically(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    };
    let Some(file_name) = path.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "export path has no file name",
        ));
    };
    let temp_path = parent.join(format!(
        ".{}.tmp.{}.{}",
        file_name.to_string_lossy(),
        std::process::id(),
        ATOMIC_WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));

    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)
    })();

    if result.is_err() {
        // Best effort: a failed export must not leave its scratch file behind either.
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// Raw service errors like `connection 7fd5a510-… is not active` (#603) name internal
/// ids the user cannot act on. Rewrite them to a human instruction; anything else is
/// passed through untouched.
pub(crate) fn friendly_connection_error(message: &str) -> String {
    const NEEDLE: &str = " is not active";
    const PREFIX: &str = "connection ";
    let mut rest = message;
    let mut out = String::with_capacity(message.len() + 48);
    while let Some(end) = rest.find(NEEDLE) {
        let head = &rest[..end];
        match head.rfind(PREFIX) {
            Some(start) if is_uuid_like(&head[start + PREFIX.len()..]) => {
                out.push_str(&rest[..start]);
                out.push_str(
                    "the connection was closed — reconnect it from the Connections list and run again",
                );
                rest = &rest[end + NEEDLE.len()..];
            }
            _ => {
                out.push_str(head);
                out.push_str(NEEDLE);
                rest = &rest[end + NEEDLE.len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Exactly the canonical UUID shape the registry emits (`xxxxxxxx-xxxx-…`).
fn is_uuid_like(token: &str) -> bool {
    token.len() == 36
        && token.char_indices().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn deduplicate_messages(messages: Vec<String>) -> Vec<String> {
    let mut unique = Vec::with_capacity(messages.len());
    for message in messages {
        if !unique.iter().any(|existing| existing == &message) {
            unique.push(message);
        }
    }
    unique
}

pub(crate) fn deduplicate_diagnostics(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut unique = Vec::with_capacity(diagnostics.len());
    for diagnostic in diagnostics {
        let duplicate = unique.iter().any(|existing: &Diagnostic| {
            existing.source == diagnostic.source
                && existing.message == diagnostic.message
                && ranges_overlap(existing.range, diagnostic.range)
        });
        if !duplicate {
            unique.push(diagnostic);
        }
    }
    unique
}

pub(crate) fn database_error_diagnostic(
    message: &str,
    executed_sql: &str,
    executed_range: (usize, usize),
    position: Option<usize>,
    code: Option<&str>,
) -> Option<Diagnostic> {
    if executed_sql.is_empty() || executed_range.0 >= executed_range.1 {
        return None;
    }
    let (start, end) = position
        .filter(|position| *position > 0)
        .map(|position| char_position_to_byte_offset(executed_sql, position))
        .filter(|start| *start < executed_sql.len())
        .and_then(|start| {
            executed_sql[start..]
                .chars()
                .next()
                .map(|character| (start, start + character.len_utf8()))
        })
        .unwrap_or((0, executed_sql.len()));
    let local_limit = executed_range.1 - executed_range.0;
    let document_start = executed_range.0 + start.min(local_limit);
    let document_end = (executed_range.0 + end)
        .min(executed_range.1)
        .max((document_start + 1).min(executed_range.1));
    Some(match code {
        Some(code) => Diagnostic::database_with_code((document_start, document_end), message, code),
        None => Diagnostic::database((document_start, document_end), message),
    })
}

fn char_position_to_byte_offset(sql: &str, position: usize) -> usize {
    let index = position.saturating_sub(1);
    sql.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .unwrap_or(sql.len())
}

fn ranges_overlap(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 < right.1 && right.0 < left.1
}

/// Format the selected range, or — with no selection — the whole document.
/// Returns `true` when the buffer changed, so callers can tell the user
/// something happened instead of failing silently (#605).
pub(crate) fn format_query_document(doc: &mut QueryDocument, dialect: SqlDialect) -> bool {
    let original_selection = doc.selection;
    let had_selection = !original_selection.is_empty();
    if !had_selection {
        return format_whole_document(doc, dialect);
    }
    let (start, end) = original_selection.normalized();
    let original_text = doc.buffer.slice(start, end).to_owned();
    let formatted_text = crate::query::sql_format::format_sql_for_dialect(&original_text, dialect);
    if formatted_text == original_text {
        return false;
    }

    let cursor_after = if had_selection {
        if original_selection.active >= original_selection.anchor {
            start + formatted_text.len()
        } else {
            start
        }
    } else {
        start + doc.cursor.offset.saturating_sub(start).min(formatted_text.len())
    };
    let anchor_after = if had_selection {
        if original_selection.anchor <= original_selection.active {
            start
        } else {
            start + formatted_text.len()
        }
    } else {
        cursor_after
    };

    doc.buffer.replace_with_snapshot(
        start,
        end,
        &formatted_text,
        EditorSnapshot {
            cursor_offset: doc.cursor.offset,
            anchor_offset: original_selection.anchor,
        },
        EditorSnapshot {
            cursor_offset: cursor_after,
            anchor_offset: anchor_after,
        },
    );
    doc.cursor.set_offset(&doc.buffer, cursor_after);
    doc.selection = SelectionRange::new(anchor_after, cursor_after);
    close_completion(doc);
    doc.invalidate_prediction();
    doc.reanalyze(dialect);
    doc.search.update_matches(doc.buffer.text());
    doc.dirty = true;
    true
}

/// No selection: format the entire buffer and carry the caret to the same
/// line/column it had before (clamped), instead of silently doing nothing (#605).
fn format_whole_document(doc: &mut QueryDocument, dialect: SqlDialect) -> bool {
    let original_text = doc.buffer.slice(0, doc.buffer.len_bytes()).to_owned();
    let trimmed = original_text.trim();
    let formatted_text = crate::query::sql_format::format_sql_for_dialect(trimmed, dialect);
    if formatted_text == trimmed {
        return false;
    }

    let (line, col) = doc.buffer.offset_to_line_col(doc.cursor.offset);
    let mut cursor_after = formatted_text.len();
    let mut byte_acc = 0usize;
    for (index, text_line) in formatted_text.split('\n').enumerate() {
        if index == line {
            let col_bytes = text_line
                .char_indices()
                .nth(col)
                .map(|(offset, _)| offset)
                .unwrap_or(text_line.len());
            cursor_after = byte_acc + col_bytes;
            break;
        }
        byte_acc += text_line.len() + 1;
    }

    doc.buffer.replace_with_snapshot(
        0,
        doc.buffer.len_bytes(),
        &formatted_text,
        EditorSnapshot {
            cursor_offset: doc.cursor.offset,
            anchor_offset: doc.selection.anchor,
        },
        EditorSnapshot {
            cursor_offset: cursor_after,
            anchor_offset: cursor_after,
        },
    );
    doc.cursor.set_offset(&doc.buffer, cursor_after);
    doc.selection = SelectionRange::new(cursor_after, cursor_after);
    close_completion(doc);
    doc.invalidate_prediction();
    doc.reanalyze(dialect);
    doc.search.update_matches(doc.buffer.text());
    doc.dirty = true;
    true
}

/// Toggles `-- ` line comments on the caret line, or on every line the selection touches.
pub(crate) fn toggle_query_comment(doc: &mut QueryDocument, dialect: SqlDialect) {
    let (selection_start, selection_end) = doc.selection.normalized();
    let start_line = doc.buffer.offset_to_line_col(selection_start).0;
    let (end_line, end_col) = doc.buffer.offset_to_line_col(selection_end);
    let end_line = if selection_start != selection_end && end_col == 0 && end_line > start_line {
        end_line - 1
    } else {
        end_line
    };
    let range_start = doc.buffer.line_start_offset(start_line);
    let range_end = doc.buffer.line_end_offset(end_line);
    let original = doc.buffer.slice(range_start, range_end).to_owned();
    let toggled = toggle_sql_line_comments(&original);
    if toggled == original {
        return;
    }
    let cursor_after = range_start + toggled.len();
    doc.buffer.replace_with_snapshot(
        range_start,
        range_end,
        &toggled,
        EditorSnapshot {
            cursor_offset: doc.cursor.offset,
            anchor_offset: doc.selection.anchor,
        },
        EditorSnapshot {
            cursor_offset: cursor_after,
            anchor_offset: cursor_after,
        },
    );
    doc.cursor.set_offset(&doc.buffer, cursor_after);
    doc.selection = SelectionRange::new(cursor_after, cursor_after);
    close_completion(doc);
    doc.invalidate_prediction();
    doc.reanalyze(dialect);
    doc.search.update_matches(doc.buffer.text());
    doc.dirty = true;
}

fn toggle_sql_line_comments(source: &str) -> String {
    let uncomment = source.lines().any(|line| !line.trim().is_empty())
        && source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .all(|line| line.trim_start().starts_with("--"));
    source
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                line.to_owned()
            } else if uncomment {
                uncomment_sql_line(line)
            } else {
                comment_sql_line(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn comment_sql_line(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    format!("{}-- {}", &line[..indent], &line[indent..])
}

fn uncomment_sql_line(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    let rest = line[indent..].strip_prefix("--").unwrap_or(&line[indent..]);
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    format!("{}{rest}", &line[..indent])
}

pub(crate) fn prediction_replacement_range(
    buffer: &crate::editor::buffer::TextBuffer,
    anchor: usize,
    manual: bool,
) -> (usize, usize) {
    if !manual {
        return (anchor, anchor);
    }
    let before = buffer.slice(0, anchor);
    let start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(offset, ch)| offset + ch.len_utf8());
    (start, anchor)
}

#[cfg(test)]
#[path = "tests/query_helpers_tests.rs"]
mod tests;

