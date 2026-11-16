use egui::Pos2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionTriggerKind {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    Keyword,
    Table,
    View,
    Column,
    Function,
    Schema,
    Cte,
    Snippet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub insert_text: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub replacement_range: (usize, usize),
    pub sort_score: i32,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionState {
    pub is_open: bool,
    pub anchor_offset: usize,
    pub document_version: u64,
    pub popup_position: Pos2,
    pub query_prefix: String,
    pub filter_text: String,
    pub items: Vec<CompletionItem>,
    pub selected_index: usize,
    pub trigger_kind: Option<CompletionTriggerKind>,
}

impl CompletionState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(
        &mut self,
        anchor_offset: usize,
        document_version: u64,
        popup_position: Pos2,
        query_prefix: String,
        items: Vec<CompletionItem>,
        trigger_kind: CompletionTriggerKind,
    ) {
        self.is_open = !items.is_empty();
        self.anchor_offset = anchor_offset;
        self.document_version = document_version;
        self.popup_position = popup_position;
        self.query_prefix = query_prefix.clone();
        self.filter_text = query_prefix;
        self.items = items;
        self.selected_index = 0;
        self.trigger_kind = Some(trigger_kind);
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.document_version = 0;
        self.items.clear();
        self.query_prefix.clear();
        self.filter_text.clear();
        self.selected_index = 0;
        self.trigger_kind = None;
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.items.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.items.len().saturating_sub(1);
            } else {
                self.selected_index -= 1;
            }
        }
    }

    pub fn select_page_down(&mut self, page_size: usize) {
        if !self.items.is_empty() {
            self.selected_index = (self.selected_index + page_size).min(self.items.len().saturating_sub(1));
        }
    }

    pub fn select_page_up(&mut self, page_size: usize) {
        if !self.items.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(page_size);
        }
    }

    pub fn can_apply_to_version(&self, document_version: u64) -> bool {
        self.is_open && self.document_version == document_version
    }

    pub fn current_item(&self) -> Option<&CompletionItem> {
        if self.is_open {
            self.items.get(self.selected_index)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "tests/completion_tests.rs"]
mod tests;
