// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Unified table-data toolbar composition and typed intents.

use super::*;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::UiTableColumn;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TableDataPaging {
    pub(super) page_range: String,
    pub(super) total_rows: u64,
    pub(super) total_known: bool,
    pub(super) has_next: bool,
    pub(super) has_previous: bool,
    pub(super) inline_query_result: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TableDataToolbarAction {
    RequestData,
    RefreshBlocked,
    RunSql(String),
    RunSqlBlocked,
    AddRow,
    OpenPendingChanges,
    ApplyStagedChanges,
    ConfirmDiscardChanges,
    DiscardStagedChanges,
    ReloadFailedMutation,
    DiscardFailedMutation,
    ResolveConflict,
    RetryFailedMutation,
    RemoveFilter(usize),
    ClearFilters,
    ResetPage,
    Export,
}

pub(super) struct TableDataToolbarContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) table_name: &'a str,
    pub(super) sql_prefix: &'a str,
    pub(super) column_suggestions: &'a [UiTableColumn],
    pub(super) can_mutate: bool,
    pub(super) connected: bool,
    pub(super) has_primary_key: bool,
    pub(super) read_only_notice: Option<&'a str>,
    pub(super) staged_changes: &'a ChangeSet,
    pub(super) staged_apply_pending: bool,
    pub(super) has_data_edit_error: bool,
    pub(super) failure: Option<&'a MutationFailure>,
    pub(super) selected_rows: usize,
    pub(super) data_query: &'a mut TableDataQueryState,
    pub(super) paging: &'a TableDataPaging,
}

pub(super) fn draw_toolbar(
    context: &mut TableDataToolbarContext<'_>,
    ui: &mut egui::Ui,
) -> Option<TableDataToolbarAction> {
    let mut action = None;
    toolbar_frame(context.theme).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                action = context.draw_mutation_controls(ui);
                let export_clicked = Button::new(context.theme)
                    .text(t!("query.export_results"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .tooltip(t!("query.export_results_desc").into_owned())
                    .show(ui)
                    .clicked();
                if action.is_none() && export_clicked {
                    action = Some(TableDataToolbarAction::Export);
                }
                let query_action = context.draw_sql_query_input(ui);
                if action.is_none() {
                    action = query_action;
                }
            });
            // A pending staged-apply failure renders as its own full-width
            // inline strip — visible until resolved, never pushing the WHERE
            // row around (#610).
            if let Some(banner_action) = context.draw_failure_banner(ui) {
                if action.is_none() {
                    action = Some(banner_action);
                }
            }
        });
    });
    let filter_action = context.draw_filter_chips(ui);
    if action.is_none() {
        action = filter_action;
    }
    action
}

pub(super) fn draw_footer(
    context: &mut TableDataToolbarContext<'_>,
    ui: &mut egui::Ui,
) -> Option<TableDataToolbarAction> {
    let mut action = None;
    toolbar_frame(context.theme).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_XS;
            action = context.draw_footer_mutation_controls(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let pagination_action = context.draw_pagination_controls(ui);
                if action.is_none() {
                    action = pagination_action;
                }
            });
        });
    });
    action
}

impl TableDataToolbarContext<'_> {
    fn draw_mutation_controls(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        let context = table_data_mutation_toolbar_view::TableDataMutationToolbarContext {
            theme: self.theme,
            can_mutate: self.can_mutate,
            connected: self.connected,
            has_primary_key: self.has_primary_key,
            read_only_notice: self.read_only_notice,
            staged_changes: self.staged_changes,
            staged_apply_pending: self.staged_apply_pending,
            has_data_edit_error: self.has_data_edit_error,
            failure: self.failure,
            selected_rows: self.selected_rows,
        };
        table_data_mutation_toolbar_view::draw_mutation_controls(&context, ui).map(Into::into)
    }

    fn draw_failure_banner(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        self.failure?;
        ui.add_space(SPACE_XS);
        let context = table_data_mutation_toolbar_view::TableDataMutationToolbarContext {
            theme: self.theme,
            can_mutate: self.can_mutate,
            connected: self.connected,
            has_primary_key: self.has_primary_key,
            read_only_notice: None,
            staged_changes: self.staged_changes,
            staged_apply_pending: self.staged_apply_pending,
            has_data_edit_error: self.has_data_edit_error,
            failure: self.failure,
            selected_rows: self.selected_rows,
        };
        table_data_mutation_toolbar_view::draw_failure_banner(&context, ui).map(Into::into)
    }

    fn draw_footer_mutation_controls(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        let context = table_data_mutation_toolbar_view::TableDataMutationToolbarContext {
            theme: self.theme,
            can_mutate: self.can_mutate,
            connected: self.connected,
            has_primary_key: self.has_primary_key,
            read_only_notice: None,
            staged_changes: self.staged_changes,
            staged_apply_pending: self.staged_apply_pending,
            has_data_edit_error: self.has_data_edit_error,
            failure: self.failure,
            selected_rows: self.selected_rows,
        };
        table_data_mutation_toolbar_view::draw_footer_controls(&context, ui).map(Into::into)
    }

    fn draw_sql_query_input(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        let editor_id = egui::Id::new(("table-data-sql-condition", self.table_name));
        let popup_was_open =
            self.data_query.sql_condition_suggestions_open && ui.memory(|memory| memory.has_focus(editor_id));
        let previous_cursor = condition_cursor_char(ui.ctx(), editor_id, &self.data_query.sql_condition_draft);
        let previous_candidates = condition_column_candidates(
            &self.data_query.sql_condition_draft,
            previous_cursor,
            self.column_suggestions,
        );
        let mut accepted_index = None;
        let mut close_popup = false;
        // Mod+Enter runs the filter even while the completion popup is open —
        // consumed before the TextEdit sees it so it never reaches the field
        // or the popup's accept handling (#339).
        let mod_enter_pressed = ui.memory(|memory| memory.has_focus(editor_id))
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        if popup_was_open && !previous_candidates.is_empty() {
            ui.input_mut(|input| {
                for (key, selected) in [(egui::Key::ArrowDown, 1usize), (egui::Key::ArrowUp, usize::MAX)] {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        let current = self.data_query.sql_condition_suggestion_index;
                        self.data_query.sql_condition_suggestion_index = if selected == 1 {
                            (current + 1) % previous_candidates.len()
                        } else {
                            current.checked_sub(1).unwrap_or(previous_candidates.len() - 1)
                        };
                    }
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    close_popup = true;
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                {
                    accepted_index = Some(
                        self.data_query
                            .sql_condition_suggestion_index
                            .min(previous_candidates.len() - 1),
                    );
                }
            });
        }
        let draft_before_edit = self.data_query.sql_condition_draft.clone();
        ui.add_space(SPACE_XS);
        // Keep the WHERE field useful even with the mutation controls visible;
        // it must never shrink below 240 px while an error is pending (#610).
        let editor_width = (ui.available_rect_before_wrap().width() - 92.0).max(240.0);
        let control_height = crate::components::button::SizeTokens::from_size(ButtonSize::Sm).min_height;
        if std::env::var_os("DB_PRO_CAPTURE_TABLE_COMPLETION").is_some() {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), editor_id).unwrap_or_default();
            let cursor = self.data_query.sql_condition_draft.chars().count();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor))));
            state.store(ui.ctx(), editor_id);
            if !ui.memory(|memory| memory.has_focus(editor_id)) {
                ui.memory_mut(|memory| memory.request_focus(editor_id));
            }
        }
        let focused = ui.memory(|memory| memory.has_focus(editor_id));
        let (editor_rect, editor_response) =
            ui.allocate_exact_size(egui::vec2(editor_width, control_height), egui::Sense::click());
        editor_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                t!("table.edit_where_condition").into_owned(),
            )
        });
        // The field chrome is a click-to-focus target; the text input drawn on
        // top of it overrides this with the I-beam while hovering the text.
        let editor_response = if ui.is_enabled() {
            editor_response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            editor_response
        };
        ui.painter().rect(
            editor_rect,
            egui::CornerRadius::same(4.0 as u8),
            self.theme.surface_editor,
            egui::Stroke::new(
                1.0,
                if focused {
                    self.theme.border_focus
                } else {
                    self.theme.border_default
                },
            ),
            egui::StrokeKind::Inside,
        );
        // `new_child` (not `allocate_new_ui`): `editor_rect` is already
        // reserved by `allocate_exact_size`, and nothing inside the editor
        // may push its min_rect back into the toolbar's placer.
        let editor_max = editor_rect.shrink2(egui::vec2(SPACE_XXS, 2.0));
        let mut editor_ui = ui.new_child(egui::UiBuilder::new().max_rect(editor_max));
        editor_ui.set_clip_rect(editor_ui.clip_rect().intersect(editor_max));
        let output = editor_ui
            .horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_XS;
                chip_frame(self.theme).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = crate::tokens::ICON_TEXT_GAP;
                        ui.label(
                            RichText::new(char::from(Icon::ListFilter).to_string())
                                .font(font_icon(ICON_XS))
                                .color(self.theme.accent),
                        );
                        ui.label(
                            RichText::new("WHERE")
                                .font(font_caption())
                                .strong()
                                .color(self.theme.text_primary),
                        );
                    });
                });
                let cond_edit = egui::TextEdit::singleline(&mut self.data_query.sql_condition_draft) // ui-rules: WHERE editor needs TextEditOutput.cursor_range for the completion popup and paints frameless inside the allocated toolbar rect
                    .hint_text(
                        RichText::new(
                            self.column_suggestions
                                .first()
                                .map_or_else(|| t!("table.condition").into_owned(), |column| column.name.clone()),
                        )
                        .color(self.theme.text_muted),
                    )
                    .font(egui::FontId::monospace(12.0))
                    .text_color(self.theme.text_primary)
                    .margin(egui::Margin::ZERO)
                    .frame(egui::Frame::NONE)
                    .id(editor_id)
                    .desired_width(ui.available_width())
                    .show(ui);
                cond_edit.response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        ui.is_enabled(),
                        t!("table.where_condition").into_owned(),
                    )
                });
                cond_edit
            })
            .inner;
        if editor_response.clicked() {
            output.response.request_focus();
        }
        let response = output.response;
        let cursor = if std::env::var_os("DB_PRO_CAPTURE_TABLE_COMPLETION").is_some() {
            self.data_query.sql_condition_draft.chars().count()
        } else {
            output
                .cursor_range
                .map(|range| range.primary.index.0)
                .unwrap_or_else(|| self.data_query.sql_condition_draft.chars().count())
        };
        let candidates =
            condition_column_candidates(&self.data_query.sql_condition_draft, cursor, self.column_suggestions);
        if close_popup {
            self.data_query.sql_condition_suggestions_open = false;
            self.data_query.sql_condition_suggestions_dismissed_for = Some(self.data_query.sql_condition_draft.clone());
        }
        let draft_changed = draft_before_edit != self.data_query.sql_condition_draft;
        if draft_changed {
            self.data_query.sql_condition_suggestion_index = 0;
            self.data_query.sql_condition_suggestions_dismissed_for = None;
        }
        let dismissed = self.data_query.sql_condition_suggestions_dismissed_for.as_ref()
            == Some(&self.data_query.sql_condition_draft);
        let should_suggest = condition_should_suggest_automatically(&self.data_query.sql_condition_draft, cursor);
        self.data_query.sql_condition_suggestions_open =
            response.has_focus() && !close_popup && !dismissed && should_suggest && !candidates.is_empty();
        if !candidates.is_empty() {
            self.data_query.sql_condition_suggestion_index =
                self.data_query.sql_condition_suggestion_index.min(candidates.len() - 1);
        }
        if let Some(index) = accepted_index {
            self.accept_column_suggestion(ui, editor_id, previous_cursor, &previous_candidates, index);
        }
        if self.data_query.sql_condition_suggestions_open && !candidates.is_empty() {
            if let Some(index) = draw_column_suggestion_popup(
                ui.ctx(),
                self.theme,
                response.rect,
                &candidates,
                self.data_query.sql_condition_suggestion_index,
            ) {
                self.accept_column_suggestion(ui, editor_id, cursor, &candidates, index);
            }
        }
        ui.add_space(SPACE_SM);
        let run_clicked = Button::new(self.theme)
            .text(t!("query.run"))
            .icon(Icon::Play)
            .variant(ButtonVariant::Default)
            .size(ButtonSize::Sm)
            .tooltip(t!("table.run_select_tooltip", key = "Enter").into_owned())
            .show(ui)
            .clicked();
        let enter_pressed =
            accepted_index.is_none() && response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if run_clicked || enter_pressed || mod_enter_pressed {
            // Running surrenders the single-line editor's focus; hand it back
            // so the user can keep refining the condition without re-clicking.
            ui.memory_mut(|memory| memory.request_focus(editor_id));
            let sql = select_with_condition(self.sql_prefix, &self.data_query.sql_condition_draft);
            return Some(if self.staged_changes.is_empty() && !self.staged_apply_pending {
                TableDataToolbarAction::RunSql(sql)
            } else {
                TableDataToolbarAction::RunSqlBlocked
            });
        }
        None
    }

    fn accept_column_suggestion(
        &mut self,
        ui: &mut egui::Ui,
        editor_id: egui::Id,
        cursor: usize,
        candidates: &[&UiTableColumn],
        index: usize,
    ) {
        let Some(column) = candidates.get(index) else {
            return;
        };
        if let Some((start, end)) = condition_token_range(&self.data_query.sql_condition_draft, cursor) {
            let insertion = condition_identifier(&column.name);
            self.data_query
                .sql_condition_draft
                .replace_range(start..end, &insertion);
            let insertion_end = start + insertion.len();
            let char_cursor = self.data_query.sql_condition_draft[..insertion_end].chars().count();
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), editor_id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
                    char_cursor,
                ))));
            state.store(ui.ctx(), editor_id);
            ui.memory_mut(|memory| memory.request_focus(editor_id));
            self.data_query.sql_condition_suggestions_open = false;
        }
    }

    fn draw_filter_chips(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        let mut context = table_data_filter_view::TableDataFilterContext {
            theme: self.theme,
            data_query: self.data_query,
        };
        table_data_filter_view::draw_filter_chips(&mut context, ui).map(Into::into)
    }

    fn draw_pagination_controls(&mut self, ui: &mut egui::Ui) -> Option<TableDataToolbarAction> {
        let mut context = table_data_pagination_view::TableDataPaginationContext {
            theme: self.theme,
            table_name: self.table_name,
            paging: self.paging,
            query: self.data_query,
            has_staged_changes: !self.staged_changes.is_empty(),
        };
        table_data_pagination_view::draw_pagination(&mut context, ui).map(Into::into)
    }
}

fn condition_cursor_char(ctx: &egui::Context, editor_id: egui::Id, text: &str) -> usize {
    egui::text_edit::TextEditState::load(ctx, editor_id)
        .and_then(|state| state.cursor.char_range())
        .map(|range| range.primary.index.0)
        .unwrap_or_else(|| text.chars().count())
}

fn condition_token_range(text: &str, cursor_char: usize) -> Option<(usize, usize)> {
    let cursor_byte = text
        .char_indices()
        .nth(cursor_char)
        .map_or(text.len(), |(byte, _)| byte);
    if inside_sql_quoted_text(&text[..cursor_byte]) {
        return None;
    }
    let is_identifier = |character: char| character.is_alphanumeric() || matches!(character, '_' | '$');
    let start = text[..cursor_byte]
        .char_indices()
        .rev()
        .take_while(|(_, character)| is_identifier(*character))
        .last()
        .map_or(cursor_byte, |(byte, _)| byte);
    let end = text[cursor_byte..]
        .char_indices()
        .take_while(|(_, character)| is_identifier(*character))
        .last()
        .map_or(cursor_byte, |(byte, character)| {
            byte + character.len_utf8() + cursor_byte
        });
    Some((start, end))
}

fn inside_sql_quoted_text(text: &str) -> bool {
    let mut quote = None;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(quote_char) = quote {
            if character == quote_char {
                if characters.peek() == Some(&quote_char) {
                    characters.next();
                } else {
                    quote = None;
                }
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == '[' {
            quote = Some(']');
        }
    }
    quote.is_some()
}

fn condition_should_suggest_automatically(text: &str, cursor_char: usize) -> bool {
    let Some((start, _)) = condition_token_range(text, cursor_char) else {
        return false;
    };
    let cursor_byte = text
        .char_indices()
        .nth(cursor_char)
        .map_or(text.len(), |(byte, _)| byte);
    if cursor_byte > start {
        return true;
    }
    let before = text[..start].trim_end();
    !before.is_empty()
        && (before.ends_with('(')
            || ["AND", "OR"].iter().any(|keyword| {
                before
                    .split_whitespace()
                    .last()
                    .is_some_and(|word| word.eq_ignore_ascii_case(keyword))
            }))
}

fn condition_identifier(identifier: &str) -> String {
    let mut bytes = identifier.bytes();
    let simple_unquoted = bytes
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'$'));
    let uppercase = identifier.to_ascii_uppercase();
    if simple_unquoted && !sqlparser::keywords::ALL_KEYWORDS.contains(&uppercase.as_str()) {
        identifier.to_owned()
    } else {
        super::result_grid_export::quote_sql_identifier(identifier)
    }
}

fn condition_column_candidates<'a>(
    text: &str,
    cursor_char: usize,
    columns: &'a [UiTableColumn],
) -> Vec<&'a UiTableColumn> {
    let Some((start, end)) = condition_token_range(text, cursor_char) else {
        return Vec::new();
    };
    let prefix = &text[start
        ..text
            .char_indices()
            .nth(cursor_char)
            .map_or(text.len(), |(byte, _)| byte)];
    let before = text[..start].trim_end();
    let at_column_position = before.is_empty()
        || before.ends_with('(')
        || ["AND", "OR"].iter().any(|keyword| {
            before
                .split_whitespace()
                .last()
                .is_some_and(|word| word.eq_ignore_ascii_case(keyword))
        });
    if !at_column_position {
        return Vec::new();
    }
    let token = &text[start..end];
    let normalized_prefix = prefix.to_ascii_lowercase();
    columns
        .iter()
        .filter(|column| {
            (token.is_empty() || !token.eq_ignore_ascii_case(&column.name))
                && column.name.to_ascii_lowercase().starts_with(&normalized_prefix)
        })
        .collect()
}

fn draw_column_suggestion_popup(
    ctx: &egui::Context,
    theme: DbProTheme,
    editor_rect: egui::Rect,
    candidates: &[&UiTableColumn],
    selected_index: usize,
) -> Option<usize> {
    let popup_width = editor_rect.width().clamp(250.0, 420.0);
    let visible_rows = candidates.len().min(7);
    let popup_height = 28.0 + visible_rows as f32 * 28.0 + 8.0;
    let position = crate::components::clamp_popup_to_screen(
        editor_rect.left_bottom() + egui::vec2(0.0, 4.0),
        egui::vec2(popup_width, popup_height),
        ctx.content_rect(),
        8.0,
    );
    let mut clicked = None;
    FloatingPopup::new(egui::Id::new("table_data_column_suggestions"), theme, position)
        .surface_elevated()
        .radius(RADIUS_SM)
        .margin(egui::Margin::same(SPACE_XS as i8))
        .show(ctx, |ui| {
            ui.set_width(popup_width - 8.0);
            ScrollArea::new(theme)
                .max_height(visible_rows as f32 * 28.0)
                .show(ui, |ui| {
                    for (index, column) in candidates.iter().enumerate() {
                        let selected = index == selected_index;
                        let row = list_row_frame(theme, selected).show(ui, |ui| {
                            ui.set_width(popup_width - 24.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&column.name)
                                        .font(egui::FontId::monospace(12.0))
                                        .color(theme.text_primary),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if column.is_primary_key {
                                        ui.label(RichText::new("PK").small().color(theme.warning));
                                    }
                                    ui.label(RichText::new(&column.data_type).small().color(theme.text_muted));
                                });
                            });
                        });
                        let row_response = ui.interact(
                            row.response.rect,
                            egui::Id::new(("table_data_column_suggestion", index)),
                            egui::Sense::click(),
                        );
                        row_response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                column.name.as_str(),
                            )
                        });
                        let row_response = if ui.is_enabled() {
                            row_response.on_hover_cursor(egui::CursorIcon::PointingHand)
                        } else {
                            row_response
                        };
                        if row_response.clicked() {
                            clicked = Some(index);
                        }
                    }
                });
        });
    clicked
}

fn select_with_condition(prefix: &str, condition: &str) -> String {
    let condition = condition.trim();
    if condition.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix} WHERE {condition}")
    }
}

pub(super) fn select_prefix(schema: Option<&str>, table: &str) -> String {
    let table = super::result_grid_export::quote_sql_identifier(table);
    let qualified_table = schema
        .filter(|schema| !schema.is_empty())
        .map(|schema| format!("{}.{}", super::result_grid_export::quote_sql_identifier(schema), table))
        .unwrap_or(table);
    format!("SELECT * FROM {qualified_table}")
}

impl From<table_data_mutation_toolbar_view::TableDataMutationToolbarAction> for TableDataToolbarAction {
    fn from(action: table_data_mutation_toolbar_view::TableDataMutationToolbarAction) -> Self {
        use table_data_mutation_toolbar_view::TableDataMutationToolbarAction as Action;
        match action {
            Action::Refresh => Self::RequestData,
            Action::RefreshBlocked => Self::RefreshBlocked,
            Action::AddRow => Self::AddRow,
            Action::OpenPendingChanges => Self::OpenPendingChanges,
            Action::ApplyStagedChanges => Self::ApplyStagedChanges,
            Action::ConfirmDiscardChanges => Self::ConfirmDiscardChanges,
            Action::DiscardStagedChanges => Self::DiscardStagedChanges,
            Action::ReloadFailedMutation => Self::ReloadFailedMutation,
            Action::DiscardFailedMutation => Self::DiscardFailedMutation,
            Action::ResolveConflict => Self::ResolveConflict,
            Action::RetryFailedMutation => Self::RetryFailedMutation,
        }
    }
}

impl From<table_data_filter_view::TableDataFilterAction> for TableDataToolbarAction {
    fn from(action: table_data_filter_view::TableDataFilterAction) -> Self {
        match action {
            table_data_filter_view::TableDataFilterAction::Remove(index) => Self::RemoveFilter(index),
            table_data_filter_view::TableDataFilterAction::ClearAll => Self::ClearFilters,
        }
    }
}

impl From<table_data_pagination_view::TableDataPaginationAction> for TableDataToolbarAction {
    fn from(action: table_data_pagination_view::TableDataPaginationAction) -> Self {
        match action {
            table_data_pagination_view::TableDataPaginationAction::RequestData => Self::RequestData,
            table_data_pagination_view::TableDataPaginationAction::ResetPage => Self::ResetPage,
        }
    }
}

#[cfg(test)]
#[path = "tests/table_data_toolbar_surface_view_tests.rs"]
mod tests;
