//! Lightweight delimiter matching for the SQL editor.
//!
//! This is deliberately independent from the renderer so pairing and matching
//! stay testable without an egui context. It skips SQL strings/comments while
//! looking for structural parentheses and brackets.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimiterMatch {
    pub delimiter: usize,
    pub matching: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelimiterIssue {
    pub offset: usize,
    pub character: char,
    pub expected: Option<char>,
}

pub fn opening_pair(character: char) -> Option<char> {
    match character {
        '(' => Some(')'),
        '[' => Some(']'),
        '\'' => Some('\''),
        '"' => Some('"'),
        _ => None,
    }
}

pub fn closing_pair(character: char) -> bool {
    matches!(character, ')' | ']' | '\'' | '"')
}

pub fn auto_pair_range(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let (opening_offset, opening) = text[..cursor].char_indices().next_back()?;
    let closing = text[cursor..].chars().next()?;
    (opening_pair(opening) == Some(closing)).then_some((opening_offset, cursor + closing.len_utf8()))
}

/// Finds a delimiter immediately before or at the caret.
pub fn delimiter_near_cursor(text: &str, cursor: usize) -> Option<DelimiterMatch> {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let previous = text[..cursor].char_indices().next_back();
    if let Some((offset, character)) = previous {
        if is_delimiter(character) {
            return Some(DelimiterMatch {
                delimiter: offset,
                matching: matching_delimiter(text, offset),
            });
        }
    }

    let current = text[cursor..].chars().next().map(|character| (cursor, character));
    current.and_then(|(offset, character)| {
        is_delimiter(character).then(|| DelimiterMatch {
            delimiter: offset,
            matching: matching_delimiter(text, offset),
        })
    })
}

pub fn is_unmatched_structural_delimiter(text: &str, offset: usize) -> bool {
    let Some(character) = text[offset..].chars().next() else {
        return false;
    };
    matches!(character, '(' | ')' | '[' | ']') && matching_delimiter(text, offset).is_none()
}

pub fn unmatched_structural_delimiters(text: &str) -> Vec<(usize, char)> {
    structural_delimiter_issues(text)
        .into_iter()
        .map(|issue| (issue.offset, issue.character))
        .collect()
}

// cc-scan:allow DEEP_NESTING — nesting mirrors structure depth
pub fn structural_delimiter_issues(text: &str) -> Vec<DelimiterIssue> {
    let mut issues = Vec::new();
    let mut stack = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        if let Some(end) = protected_end(text, offset) {
            offset = end;
            continue;
        }
        let Some(character) = text[offset..].chars().next() else {
            break;
        };
        match character {
            '(' | '[' => stack.push((character, offset)),
            ')' | ']' => {
                let expected = stack.last().and_then(|(opening, _)| opening_pair(*opening));
                if expected == Some(character) {
                    stack.pop();
                } else {
                    issues.push(DelimiterIssue {
                        offset,
                        character,
                        expected,
                    });
                }
            }
            _ => {}
        }
        offset += character.len_utf8();
    }
    issues.extend(stack.into_iter().map(|(character, offset)| DelimiterIssue {
        offset,
        character,
        expected: None,
    }));
    issues.sort_unstable_by_key(|issue| issue.offset);
    issues
}

fn is_delimiter(character: char) -> bool {
    matches!(character, '(' | ')' | '[' | ']' | '\'' | '"')
}

fn matching_delimiter(text: &str, offset: usize) -> Option<usize> {
    let character = text[offset..].chars().next()?;
    match character {
        '(' | '[' => scan_forward(text, offset, character),
        ')' | ']' => scan_backward(text, offset),
        '\'' | '"' => matching_quote(text, offset, character),
        _ => None,
    }
}

fn scan_forward(text: &str, start: usize, opening: char) -> Option<usize> {
    let mut stack = vec![(opening, start)];
    let mut offset = start + opening.len_utf8();
    while offset < text.len() {
        if let Some(end) = protected_end(text, offset) {
            offset = end;
            continue;
        }
        let character = text[offset..].chars().next()?;
        match character {
            '(' | '[' => stack.push((character, offset)),
            ')' | ']' => {
                let expected = stack.last().and_then(|(opening, _)| opening_pair(*opening));
                if expected != Some(character) {
                    return None;
                }
                stack.pop();
                if stack.is_empty() {
                    return Some(offset);
                }
            }
            _ => {}
        }
        offset += character.len_utf8();
    }
    None
}

fn scan_backward(text: &str, start: usize) -> Option<usize> {
    let mut stack = Vec::new();
    let mut offset = 0;
    while offset <= start && offset < text.len() {
        if let Some(end) = protected_end(text, offset) {
            offset = end;
            continue;
        }
        let character = text[offset..].chars().next()?;
        match character {
            '(' | '[' => stack.push((character, offset)),
            ')' | ']' => {
                let expected = stack.last().and_then(|(opening, _)| opening_pair(*opening));
                if offset == start {
                    return (expected == Some(character))
                        .then(|| stack.pop().map(|(_, offset)| offset))
                        .flatten();
                }
                if expected == Some(character) {
                    stack.pop();
                }
            }
            _ => {}
        }
        offset += character.len_utf8();
    }
    None
}

fn matching_quote(text: &str, target: usize, quote: char) -> Option<usize> {
    // Track string state so escape handling mirrors the rest of the editor
    // (#611): conforming strings never treat `\` as an escape, doubled quotes
    // always do, and E'...' strings (Postgres) do too.
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut in_string = false;
    let mut in_string_escapes = false;
    let mut positions = Vec::new();
    while offset < text.len() {
        if let Some(end) = non_quote_protected_end(text, offset) {
            offset = end;
            continue;
        }
        let character = text[offset..].chars().next()?;
        if in_string {
            if in_string_escapes && character == '\\' {
                offset += character.len_utf8();
                if let Some(next) = text[offset..].chars().next() {
                    offset += next.len_utf8();
                }
                continue;
            }
            if character == quote {
                let next_offset = offset + quote.len_utf8();
                if text[next_offset..].starts_with(quote) {
                    offset = next_offset + quote.len_utf8();
                    continue;
                }
                positions.push(offset);
                in_string = false;
                offset = next_offset;
                continue;
            }
            offset += character.len_utf8();
            continue;
        }
        if character == quote {
            in_string = true;
            in_string_escapes = quote == '\''
                && offset >= 1
                && matches!(bytes[offset - 1], b'E' | b'e')
                && (offset < 2 || {
                    let before = bytes[offset - 2];
                    !before.is_ascii_alphanumeric() && before != b'_'
                });
            positions.push(offset);
            offset += quote.len_utf8();
            continue;
        }
        offset += character.len_utf8();
    }
    let index = positions.iter().position(|position| *position == target)?;
    if index % 2 == 0 {
        positions.get(index + 1).copied()
    } else {
        Some(positions[index - 1])
    }
}

// cc-scan:allow HARD_COMPLEXITY — delimiter-end scanner — quote/comment states form one ladder
fn protected_end(text: &str, offset: usize) -> Option<usize> {
    let rest = &text[offset..];
    if rest.starts_with("--") {
        return Some(text[offset..].find('\n').map_or(text.len(), |end| offset + end));
    }
    if rest.starts_with("/*") {
        return Some(text[offset..].find("*/").map_or(text.len(), |end| offset + end + 2));
    }
    if rest.starts_with('\'') {
        // Only E'...' strings (Postgres) treat backslash as an escape; a
        // standard conforming string ends at the first quote, with '' as the
        // doubling escape (#611).
        let bytes = text.as_bytes();
        let escapes = offset >= 1
            && matches!(bytes[offset - 1], b'E' | b'e')
            && (offset < 2 || {
                let before = bytes[offset - 2];
                !before.is_ascii_alphanumeric() && before != b'_'
            });
        let mut cursor = offset + 1;
        while cursor < text.len() {
            let character = text[cursor..].chars().next()?;
            cursor += character.len_utf8();
            if character == '\\' && escapes {
                if let Some(escaped) = text[cursor..].chars().next() {
                    cursor += escaped.len_utf8();
                }
                continue;
            }
            if character == '\'' {
                if text[cursor..].starts_with('\'') {
                    cursor += 1;
                } else {
                    break;
                }
            }
        }
        return Some(cursor);
    }
    if rest.starts_with('"') || rest.starts_with('`') {
        let quote = rest.chars().next()?;
        let mut cursor = offset + quote.len_utf8();
        while cursor < text.len() {
            let character = text[cursor..].chars().next()?;
            cursor += character.len_utf8();
            if character == quote {
                if text[cursor..].starts_with(quote) {
                    cursor += quote.len_utf8();
                } else {
                    break;
                }
            }
        }
        return Some(cursor);
    }
    if rest.starts_with('$') {
        let tag_end = text[offset + 1..]
            .char_indices()
            .find(|(_, character)| *character == '$')
            .map(|(index, _)| offset + 1 + index)?;
        let tag = &text[offset..=tag_end];
        if tag[1..tag.len() - 1]
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
        {
            let body_start = tag_end + 1;
            return Some(
                text[body_start..]
                    .find(tag)
                    .map_or(text.len(), |end| body_start + end + tag.len()),
            );
        }
    }
    None
}

fn non_quote_protected_end(text: &str, offset: usize) -> Option<usize> {
    let rest = &text[offset..];
    if rest.starts_with("--") {
        return Some(text[offset..].find('\n').map_or(text.len(), |end| offset + end));
    }
    if rest.starts_with("/*") {
        return Some(text[offset..].find("*/").map_or(text.len(), |end| offset + end + 2));
    }
    if rest.starts_with('$') {
        let tag_end = text[offset + 1..]
            .char_indices()
            .find(|(_, character)| *character == '$')
            .map(|(index, _)| offset + 1 + index)?;
        let tag = &text[offset..=tag_end];
        if tag[1..tag.len() - 1]
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
        {
            let body_start = tag_end + 1;
            return Some(
                text[body_start..]
                    .find(tag)
                    .map_or(text.len(), |end| body_start + end + tag.len()),
            );
        }
    }
    None
}

#[cfg(test)]
#[path = "tests/brackets_tests.rs"]
mod tests;
