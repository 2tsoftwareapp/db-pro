use std::collections::VecDeque;

use unicode_segmentation::GraphemeCursor;

const MAX_UNDO_HISTORY: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoAction {
    Insert { offset: usize, text: String },
    Delete { offset: usize, text: String },
    Group(Vec<UndoAction>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorSnapshot {
    pub cursor_offset: usize,
    pub anchor_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoStep {
    pub action: UndoAction,
    pub before: EditorSnapshot,
    pub after: EditorSnapshot,
}

#[derive(Debug, Clone, Default)]
pub struct UndoStack {
    pub undo_list: VecDeque<UndoStep>,
    pub redo_list: VecDeque<UndoStep>,
    pub group_open: bool,
    pub current_group_actions: Vec<UndoAction>,
    pub group_before_snapshot: Option<EditorSnapshot>,
    pub typing_group_active: bool,
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_group(&mut self, before: EditorSnapshot) {
        if !self.group_open {
            self.break_typing_group();
            self.group_open = true;
            self.current_group_actions.clear();
            self.group_before_snapshot = Some(before);
        }
    }

    pub fn end_group(&mut self, after: EditorSnapshot) {
        if self.group_open {
            self.group_open = false;
            let before = self.group_before_snapshot.take().unwrap_or(after);
            if !self.current_group_actions.is_empty() {
                let actions = std::mem::take(&mut self.current_group_actions);
                let step = UndoStep {
                    action: UndoAction::Group(actions),
                    before,
                    after,
                };
                self.push_step(step);
            }
        }
    }

    pub fn break_typing_group(&mut self) {
        self.typing_group_active = false;
    }

    pub fn push_action(&mut self, action: UndoAction, before: EditorSnapshot, after: EditorSnapshot) {
        if self.group_open {
            self.current_group_actions.push(action);
        } else {
            self.typing_group_active = false;
            let step = UndoStep { action, before, after };
            self.push_step(step);
        }
    }

    // cc-scan:allow TOO_MANY_PARAMS — context params passed through
    pub fn push_typing_insert(&mut self, offset: usize, text: &str, before: EditorSnapshot, after: EditorSnapshot) {
        if self.group_open {
            self.current_group_actions.push(UndoAction::Insert {
                offset,
                text: text.to_owned(),
            });
            return;
        }

        // Check if we can merge with the previous insert in a continuous typing streak
        if self.typing_group_active {
            if let Some(last_step) = self.undo_list.back_mut() {
                if let UndoAction::Insert {
                    offset: prev_off,
                    text: prev_txt,
                } = &mut last_step.action
                {
                    if *prev_off + prev_txt.len() == offset
                        && !text.contains('\n')
                        && !text.contains('\t')
                        && !prev_txt.ends_with(' ')
                    {
                        prev_txt.push_str(text);
                        last_step.after = after;
                        self.redo_list.clear();
                        return;
                    }
                }
            }
        }

        // Otherwise start a new typing step
        self.typing_group_active = !text.contains('\n') && !text.contains('\t');
        let step = UndoStep {
            action: UndoAction::Insert {
                offset,
                text: text.to_owned(),
            },
            before,
            after,
        };
        self.push_step(step);
    }

    pub fn push_step(&mut self, step: UndoStep) {
        self.redo_list.clear();
        if self.undo_list.len() >= MAX_UNDO_HISTORY {
            self.undo_list.pop_front();
        }
        self.undo_list.push_back(step);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_list.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_list.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_list.clear();
        self.redo_list.clear();
        self.current_group_actions.clear();
        self.group_before_snapshot = None;
        self.group_open = false;
        self.typing_group_active = false;
    }
}

/// TextBuffer provides indexed text storage with line/column/offset conversions,
/// UTF-8 boundary snapping, and snapshot-aware undo/redo history.
#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    content: String,
    /// Byte offsets of line starts (line 0 starts at 0).
    line_starts: Vec<usize>,
    /// Longest line in Unicode scalars — refreshed with the line index (not per frame).
    max_line_chars: usize,
    /// Per-line char counts, parallel to `line_starts` (#580). Kept so an
    /// edit can update `max_line_chars` from the touched lines instead of
    /// re-decoding the whole buffer.
    line_char_lens: Vec<usize>,
    pub undo_stack: UndoStack,
    version: u64,
    /// `(start, old_end, new_end)` of the latest edit, in bytes.
    /// `None` after undo/redo or when the edit covers more than two lines,
    /// so the highlighter retokenizes the whole buffer instead of splicing.
    pub(crate) last_edit: Option<(usize, usize, usize)>,
}

impl TextBuffer {
    pub fn new() -> Self {
        let mut buffer = Self {
            content: String::new(),
            line_starts: vec![0],
            max_line_chars: 0,
            line_char_lens: vec![0],
            undo_stack: UndoStack::new(),
            version: 0,
            last_edit: None,
        };
        buffer.rebuild_line_index();
        buffer
    }

    pub fn from_string(text: impl Into<String>) -> Self {
        let mut buffer = Self {
            content: text.into(),
            line_starts: vec![0],
            max_line_chars: 0,
            line_char_lens: vec![0],
            undo_stack: UndoStack::new(),
            version: 0,
            last_edit: None,
        };
        buffer.rebuild_line_index();
        buffer
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len_bytes(&self) -> usize {
        self.content.len()
    }

    pub fn is_char_boundary(&self, offset: usize) -> bool {
        self.content.is_char_boundary(offset)
    }

    pub fn len_chars(&self) -> usize {
        self.content.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len().max(1)
    }

    pub fn max_line_len_chars(&self) -> usize {
        self.max_line_chars
    }

    /// Byte offsets of line starts — exposed for the #580 differential tests.
    #[cfg(test)]
    pub(crate) fn line_starts_for_test(&self) -> Vec<usize> {
        self.line_starts.clone()
    }

    // --- UTF-8 Safety Helpers ---

    pub fn floor_char_boundary(&self, mut offset: usize) -> usize {
        if offset >= self.content.len() {
            return self.content.len();
        }
        while offset > 0 && !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    pub fn ceil_char_boundary(&self, mut offset: usize) -> usize {
        let len = self.content.len();
        if offset >= len {
            return len;
        }
        while offset < len && !self.content.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }

    pub fn prev_char_boundary(&self, offset: usize) -> usize {
        if offset == 0 {
            return 0;
        }
        let mut idx = offset.saturating_sub(1);
        while idx > 0 && !self.content.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    pub fn next_char_boundary(&self, offset: usize) -> usize {
        let len = self.content.len();
        if offset >= len {
            return len;
        }
        let mut idx = offset + 1;
        while idx < len && !self.content.is_char_boundary(idx) {
            idx += 1;
        }
        idx
    }

    /// Start of the grapheme cluster containing or ending at `offset` — caret
    /// steps must not land inside ZWJ emoji or base+combining-mark sequences.
    /// `offset` inside a cluster resolves to that cluster's start.
    pub fn prev_grapheme_boundary(&self, offset: usize) -> usize {
        let len = self.content.len();
        let offset = self.floor_char_boundary(offset.min(len));
        if offset == 0 {
            return 0;
        }
        let mut cursor = GraphemeCursor::new(offset, len, true);
        match cursor.prev_boundary(&self.content, 0) {
            Ok(Some(boundary)) => boundary,
            Ok(None) => 0,
            Err(_) => self.prev_char_boundary(offset),
        }
    }

    /// End of the grapheme cluster starting at `offset`; `offset` inside a
    /// cluster resolves to that cluster's end.
    pub fn next_grapheme_boundary(&self, offset: usize) -> usize {
        let len = self.content.len();
        let offset = self.floor_char_boundary(offset.min(len));
        if offset >= len {
            return len;
        }
        let mut cursor = GraphemeCursor::new(offset, len, true);
        match cursor.next_boundary(&self.content, 0) {
            Ok(Some(boundary)) => boundary,
            Ok(None) => len,
            Err(_) => self.next_char_boundary(offset),
        }
    }

    pub fn char_at(&self, offset: usize) -> Option<char> {
        let valid = self.floor_char_boundary(offset);
        self.content[valid..].chars().next()
    }

    pub fn prev_char(&self, offset: usize) -> Option<char> {
        if offset == 0 {
            return None;
        }
        let prev_boundary = self.prev_char_boundary(offset);
        self.content[prev_boundary..offset].chars().next()
    }

    // --- Line / Col / Offset Mapping ---

    pub fn line_at(&self, line_idx: usize) -> Option<&str> {
        if line_idx >= self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line_idx];
        let end = if line_idx + 1 < self.line_starts.len() {
            let mut next_start = self.line_starts[line_idx + 1];
            if next_start > 0 && self.content.as_bytes().get(next_start - 1) == Some(&b'\n') {
                next_start -= 1;
                if next_start > 0 && self.content.as_bytes().get(next_start - 1) == Some(&b'\r') {
                    next_start -= 1;
                }
            }
            next_start
        } else {
            self.content.len()
        };
        self.content.get(start..end)
    }

    pub fn split_at(&self, byte_offset: usize) -> (&str, &str) {
        let clamped = self.floor_char_boundary(byte_offset.min(self.content.len()));
        self.content.split_at(clamped)
    }

    pub fn line_start_offset(&self, line_idx: usize) -> usize {
        if line_idx >= self.line_starts.len() {
            self.content.len()
        } else {
            self.line_starts[line_idx]
        }
    }

    pub fn line_end_offset(&self, line_idx: usize) -> usize {
        if line_idx >= self.line_starts.len() {
            self.content.len()
        } else if line_idx + 1 < self.line_starts.len() {
            let mut next = self.line_starts[line_idx + 1];
            if next > 0 && self.content.as_bytes().get(next - 1) == Some(&b'\n') {
                next -= 1;
                if next > 0 && self.content.as_bytes().get(next - 1) == Some(&b'\r') {
                    next -= 1;
                }
            }
            next
        } else {
            self.content.len()
        }
    }

    pub fn offset_to_line_col(&self, byte_offset: usize) -> (usize, usize) {
        let clamped = self.floor_char_boundary(byte_offset.min(self.content.len()));
        let line_idx = match self.line_starts.binary_search(&clamped) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0);
        let col_char_idx = self.content[line_start..clamped].chars().count();
        (line_idx, col_char_idx)
    }

    pub fn line_col_to_offset(&self, line_idx: usize, col_char_idx: usize) -> usize {
        if line_idx >= self.line_starts.len() {
            return self.content.len();
        }
        let line_start = self.line_starts[line_idx];
        let line_text = self.line_at(line_idx).unwrap_or("");
        let mut byte_advance = 0;
        for (char_count, (idx, ch)) in line_text.char_indices().enumerate() {
            if char_count == col_char_idx {
                return line_start + idx;
            }
            byte_advance = idx + ch.len_utf8();
        }
        line_start + byte_advance
    }

    pub fn slice(&self, start: usize, end: usize) -> &str {
        let clamped_start = self.floor_char_boundary(start.min(self.content.len()));
        let clamped_end = self.ceil_char_boundary(end.min(self.content.len()).max(clamped_start));
        &self.content[clamped_start..clamped_end]
    }

    // --- Editing Operations ---

    pub fn insert(&mut self, offset: usize, text: &str) {
        self.insert_with_snapshot(
            offset,
            text,
            EditorSnapshot {
                cursor_offset: offset,
                anchor_offset: offset,
            },
            EditorSnapshot {
                cursor_offset: offset + text.len(),
                anchor_offset: offset + text.len(),
            },
        );
    }

    // cc-scan:allow TOO_MANY_PARAMS — context params passed through
    pub fn insert_with_snapshot(&mut self, offset: usize, text: &str, before: EditorSnapshot, after: EditorSnapshot) {
        if text.is_empty() {
            return;
        }
        let clamped = self.floor_char_boundary(offset.min(self.content.len()));
        self.undo_stack.push_action(
            UndoAction::Insert {
                offset: clamped,
                text: text.to_owned(),
            },
            before,
            after,
        );
        self.content.insert_str(clamped, text);
        self.version = self.version.wrapping_add(1);
        self.last_edit = bounded_edit(clamped, clamped, clamped + text.len(), 0, newline_count(text));
        self.refresh_after_edit();
    }

    // cc-scan:allow TOO_MANY_PARAMS — context params passed through
    pub fn type_text(&mut self, offset: usize, text: &str, before: EditorSnapshot, after: EditorSnapshot) {
        if text.is_empty() {
            return;
        }
        let clamped = self.floor_char_boundary(offset.min(self.content.len()));
        self.undo_stack.push_typing_insert(clamped, text, before, after);
        self.content.insert_str(clamped, text);
        self.version = self.version.wrapping_add(1);
        self.last_edit = bounded_edit(clamped, clamped, clamped + text.len(), 0, newline_count(text));
        self.refresh_after_edit();
    }

    pub fn delete(&mut self, start: usize, end: usize) -> String {
        let clamped_start = self.floor_char_boundary(start.min(self.content.len()));
        let clamped_end = self.ceil_char_boundary(end.min(self.content.len()).max(clamped_start));
        self.delete_with_snapshot(
            clamped_start,
            clamped_end,
            EditorSnapshot {
                cursor_offset: clamped_end,
                anchor_offset: clamped_start,
            },
            EditorSnapshot {
                cursor_offset: clamped_start,
                anchor_offset: clamped_start,
            },
        )
    }

    pub fn delete_with_snapshot(
        &mut self,
        start: usize,
        end: usize,
        before: EditorSnapshot,
        after: EditorSnapshot,
    ) -> String {
        let clamped_start = self.floor_char_boundary(start.min(self.content.len()));
        let clamped_end = self.ceil_char_boundary(end.min(self.content.len()).max(clamped_start));
        if clamped_start == clamped_end {
            return String::new();
        }
        let deleted = self.content[clamped_start..clamped_end].to_owned();
        self.undo_stack.break_typing_group();
        self.undo_stack.push_action(
            UndoAction::Delete {
                offset: clamped_start,
                text: deleted.clone(),
            },
            before,
            after,
        );
        self.content.replace_range(clamped_start..clamped_end, "");
        self.version = self.version.wrapping_add(1);
        self.last_edit = bounded_edit(clamped_start, clamped_end, clamped_start, newline_count(&deleted), 0);
        self.refresh_after_edit();
        deleted
    }

    pub fn replace(&mut self, start: usize, end: usize, text: &str) {
        self.replace_with_snapshot(
            start,
            end,
            text,
            EditorSnapshot {
                cursor_offset: end,
                anchor_offset: start,
            },
            EditorSnapshot {
                cursor_offset: start + text.len(),
                anchor_offset: start + text.len(),
            },
        );
    }

    pub fn replace_with_snapshot(
        &mut self,
        start: usize,
        end: usize,
        text: &str,
        before: EditorSnapshot,
        after: EditorSnapshot,
    ) {
        let clamped_start = self.floor_char_boundary(start.min(self.content.len()));
        let clamped_end = self.ceil_char_boundary(end.min(self.content.len()).max(clamped_start));
        if clamped_start == clamped_end && text.is_empty() {
            return;
        }
        let removed_newlines = if clamped_start != clamped_end {
            newline_count(&self.content[clamped_start..clamped_end])
        } else {
            0
        };
        self.undo_stack.break_typing_group();
        self.undo_stack.begin_group(before);
        if clamped_start != clamped_end {
            let deleted = self.content[clamped_start..clamped_end].to_owned();
            self.undo_stack.push_action(
                UndoAction::Delete {
                    offset: clamped_start,
                    text: deleted,
                },
                before,
                after,
            );
            self.content.replace_range(clamped_start..clamped_end, "");
        }
        if !text.is_empty() {
            self.undo_stack.push_action(
                UndoAction::Insert {
                    offset: clamped_start,
                    text: text.to_owned(),
                },
                before,
                after,
            );
            self.content.insert_str(clamped_start, text);
        }
        self.undo_stack.end_group(after);
        self.version = self.version.wrapping_add(1);
        self.last_edit = bounded_edit(
            clamped_start,
            clamped_end,
            clamped_start + text.len(),
            removed_newlines,
            newline_count(text),
        );
        self.refresh_after_edit();
    }

    /// Keep the line index in sync after an edit: incremental when the edit
    /// stayed inside two lines, full rebuild otherwise (#580).
    fn refresh_after_edit(&mut self) {
        match self.last_edit {
            Some((start, old_end, new_end)) => self.update_line_index(start, old_end, new_end),
            None => self.rebuild_line_index(),
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        let new_text = text.into();
        let len = self.content.len();
        self.replace(0, len, &new_text);
    }

    pub fn set_text_initial(&mut self, text: impl Into<String>) {
        self.content = text.into();
        self.version = self.version.wrapping_add(1);
        self.last_edit = None;
        self.undo_stack.clear();
        self.rebuild_line_index();
    }

    pub fn break_typing_group(&mut self) {
        self.undo_stack.break_typing_group();
    }

    pub fn undo(&mut self) -> Option<(usize, usize)> {
        let step = self.undo_stack.undo_list.pop_back()?;
        self.apply_undo_action(&step.action, true);
        let restored_cursor = (step.before.cursor_offset, step.before.anchor_offset);
        self.undo_stack.redo_list.push_back(step);
        self.version = self.version.wrapping_add(1);
        self.last_edit = None;
        self.rebuild_line_index();
        Some(restored_cursor)
    }

    pub fn redo(&mut self) -> Option<(usize, usize)> {
        let step = self.undo_stack.redo_list.pop_back()?;
        self.apply_undo_action(&step.action, false);
        let restored_cursor = (step.after.cursor_offset, step.after.anchor_offset);
        self.undo_stack.undo_list.push_back(step);
        self.version = self.version.wrapping_add(1);
        self.last_edit = None;
        self.rebuild_line_index();
        Some(restored_cursor)
    }

    fn apply_undo_action(&mut self, action: &UndoAction, is_undo: bool) {
        match action {
            UndoAction::Insert { offset, text } => {
                if is_undo {
                    let end = *offset + text.len();
                    if end <= self.content.len() {
                        self.content.replace_range(*offset..end, "");
                    }
                } else {
                    let off = (*offset).min(self.content.len());
                    self.content.insert_str(off, text);
                }
            }
            UndoAction::Delete { offset, text } => {
                if is_undo {
                    let off = (*offset).min(self.content.len());
                    self.content.insert_str(off, text);
                } else {
                    let end = *offset + text.len();
                    if end <= self.content.len() {
                        self.content.replace_range(*offset..end, "");
                    }
                }
            }
            UndoAction::Group(actions) => {
                if is_undo {
                    for sub in actions.iter().rev() {
                        self.apply_undo_action(sub, true);
                    }
                } else {
                    for sub in actions.iter() {
                        self.apply_undo_action(sub, false);
                    }
                }
            }
        }
    }

    /// Rebuild the whole line index (#580).
    ///
    /// Only used on construction, `set_text_initial`, undo/redo and edits that
    /// span more than two lines; ordinary typing goes through
    /// [`Self::update_line_index`] and touches only the edited lines.
    fn rebuild_line_index(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (idx, b) in self.content.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                self.line_starts.push(idx + 1);
            }
        }
        self.line_char_lens = (0..self.line_count())
            .map(|line| self.line_at(line).unwrap_or("").chars().count())
            .collect();
        self.max_line_chars = self.line_char_lens.iter().copied().max().unwrap_or(0);
    }

    /// Incrementally update the line index after a bounded edit (#580).
    ///
    /// `start`, `old_end` and `new_end` are the same triple stored in
    /// [`Self::last_edit`]: `old_end` is an offset into the *pre-edit* content,
    /// `new_end` into the current one. Because such an edit spans at most two
    /// lines, only the touched lines are re-measured — no whole-buffer UTF-8
    /// decode per keystroke.
    pub(crate) fn update_line_index(&mut self, start: usize, old_end: usize, new_end: usize) {
        let Some(first) = self.line_index_at(start) else {
            return self.rebuild_line_index();
        };
        if new_end > self.content.len() {
            return self.rebuild_line_index();
        }
        self.splice_line_starts(first, old_end, new_end);
        self.recount_touched_lines(first, new_end);
    }

    /// Replace the line starts covered by the edit and shift the rest (#580).
    fn splice_line_starts(&mut self, first: usize, old_end: usize, new_end: usize) {
        let first_start = self.line_starts[first];
        if first_start > old_end.min(new_end) {
            return self.rebuild_line_index();
        }
        // New line starts introduced anywhere in the touched window.
        let mut fresh: Vec<usize> = Vec::new();
        for (offset, byte) in self.content.as_bytes()[first_start..new_end].iter().enumerate() {
            if *byte == b'\n' {
                fresh.push(first_start + offset + 1);
            }
        }
        // Old line starts the edit consumed: entries in `(first_start, old_end]`.
        let removed = self.line_starts[first + 1..]
            .iter()
            .take_while(|offset| **offset <= old_end)
            .count();
        let inserted = fresh.len();
        self.line_starts
            .splice(first + 1..first + 1 + removed, fresh.iter().copied());
        // Everything past the replaced window moves by the byte delta.
        let delta = new_end as isize - old_end as isize;
        let limit = self.content.len();
        for offset in &mut self.line_starts[first + 1 + inserted..] {
            let shifted = *offset as isize + delta;
            *offset = shifted.clamp(0, limit as isize) as usize;
        }
    }

    /// Re-measure only the lines the edit could have changed (#580).
    fn recount_touched_lines(&mut self, first: usize, new_end: usize) {
        let Some(last) = self.line_index_at(new_end) else {
            return self.rebuild_line_index();
        };
        let tail = (last + 1).min(self.line_starts.len());
        self.line_char_lens.resize(self.line_starts.len(), 0);
        for line in first..tail {
            let len = self.line_at(line).unwrap_or("").chars().count();
            self.line_char_lens[line] = len;
        }
        // Integer-only running max over the per-line cache; no re-decode.
        self.max_line_chars = self.line_char_lens.iter().copied().max().unwrap_or(0);
    }

    /// Binary-search the line containing `offset`.
    fn line_index_at(&self, offset: usize) -> Option<usize> {
        if self.line_starts.is_empty() {
            return None;
        }
        Some(match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        })
    }
}

fn newline_count(text: &str) -> usize {
    text.bytes().filter(|byte| *byte == b'\n').count()
}

/// One newline stays inside two adjacent lines. More than that cannot be spliced safely.
fn bounded_edit(
    start: usize,
    old_end: usize,
    new_end: usize,
    removed_newlines: usize,
    inserted_newlines: usize,
) -> Option<(usize, usize, usize)> {
    if removed_newlines > 1 || inserted_newlines > 1 {
        None
    } else {
        Some((start, old_end, new_end))
    }
}

#[cfg(test)]
#[path = "tests/buffer_tests.rs"]
mod tests;
