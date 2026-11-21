// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::query_editor_support::{close_completion, draw_rich_hover_popup, draw_signature_help};
use super::query_view::{format_query_document, prediction_replacement_range};
use super::*;
use crate::editor::{
    CompletionIntent, CompletionTriggerKind, EditorInteractionPolicy, SqlDialect, SqlEditor, SqlEditorResponse,
};
use crate::query::{CompletionContext, SchemaCompletionProvider};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub(super) struct QueryEditorEffects {
    pub(super) actions: Vec<QueryEditorAction>,
}

#[derive(Debug, Clone)]
pub(super) enum QueryEditorAction {
    CancelPrediction { request_id: RequestId },
    RequestPrediction(Box<QueryPredictionRequest>),
    DispatchStatement,
    DispatchAll,
    SaveQuery,
    OpenIdentified(super::query_editor_support::IdentifierNavigation),
}

#[derive(Debug, Clone)]
pub(super) struct QueryPredictionRequest {
    pub(super) document_id: String,
    pub(super) document_version: u64,
    pub(super) anchor: usize,
    pub(super) replacement_range: (usize, usize),
    pub(super) context: crate::editor::prediction::AiSqlContext,
    pub(super) fingerprint: u64,
}

struct QueryEditorFrame {
    doc_index: usize,
    response: SqlEditorResponse,
    is_completion_open: bool,
    previous_completion_trigger: Option<CompletionTriggerKind>,
    previous_cursor: usize,
    previous_selection: crate::editor::SelectionRange,
}

pub(super) struct QueryEditorSurfaceContext<'view> {
    pub(super) theme: DbProTheme,
    pub(super) query_editor: &'view mut QueryEditorState,
    pub(super) query_session: &'view mut QuerySessionState,
    pub(super) preferences: &'view PreferencesState,
    pub(super) schema: &'view SchemaExplorerState,
    pub(super) active_schema: &'view str,
    pub(super) dialect: SqlDialect,
}

impl<'view> QueryEditorSurfaceContext<'view> {
    pub(super) fn draw_query_editor(&mut self, ui: &mut egui::Ui) -> QueryEditorEffects {
        let editor_width = ui.max_rect().width();
        // Editor owns the allocated region from draw_query — no permanent output reserve.
        let editor_height = ui.available_height().max(120.0);
        let active_schema = self.active_schema.to_owned();
        let mut effects = QueryEditorEffects::default();
        let available_size = egui::vec2(editor_width.max(280.0), editor_height);
        let Some(frame) = self.render_editor(ui, available_size) else {
            return effects;
        };
        let cursor_in_string_or_comment = self.apply_editor_response(&frame, &mut effects);
        if frame.response.wants_identifier_typed
            && !frame.is_completion_open
            && self.query_session.documents[frame.doc_index].completion_due_at.is_some()
        {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
        self.maybe_request_prediction(&frame, cursor_in_string_or_comment, &active_schema, &mut effects);
        let completion_timer_due = {
            let doc = &mut self.query_session.documents[frame.doc_index];
            if doc.completion_due_at.is_some_and(|due| Instant::now() >= due) {
                doc.completion_due_at = None;
                !frame.is_completion_open && doc.selection.is_empty() && !cursor_in_string_or_comment
            } else {
                false
            }
        };
        self.resolve_completion(&frame, &active_schema, completion_timer_due);
        self.append_terminal_actions(&frame, &mut effects);
        let signature_visible = self.draw_signature_help(ui, &frame, &active_schema);
        self.draw_hover_popup(ui, &frame, &active_schema, signature_visible);
        effects
    }

    fn render_editor(&mut self, ui: &mut egui::Ui, available_size: egui::Vec2) -> Option<QueryEditorFrame> {
        let doc_index = self.query_session.active_document_index;
        let doc = self.query_session.documents.get_mut(doc_index)?;
        let is_completion_open = doc.completion.is_open;
        let previous_completion_trigger = doc.completion.trigger_kind;
        let execution_range = doc.executing_range;
        let previous_cursor = doc.cursor.offset;
        let previous_selection = doc.selection;
        let search_query = self.query_editor.editor_search.clone();
        let mut editor = SqlEditor::new(
            &mut doc.buffer,
            &mut doc.cursor,
            &mut doc.selection,
            self.dialect,
            &self.theme,
            &doc.diagnostics,
            doc.prediction.as_ref(),
            "active_sql_editor",
        )
        .with_auto_focus(self.query_editor.query_focus_editor_on_open)
        .with_cached_tokens(&mut doc.cached_tokens)
        .with_search(&search_query, doc.search.active_match_index)
        .with_completion_open(is_completion_open)
        .with_rich_hover(doc.hover_state.confirmed_token())
        .with_snippet(
            doc.snippet.is_some(),
            doc.snippet.as_ref().and_then(|session| session.active_range()),
        )
        .with_execution_range(execution_range)
        .with_prediction_visible(self.preferences.prediction_mode == PredictionMode::Eager || doc.prediction_reveal);
        editor.font_size = self.query_editor.editor_font_size;
        let response = editor.show(ui, available_size);
        self.query_editor.query_focus_editor_on_open = false;
        self.query_editor.query_editor_rect = response.rect;
        Some(QueryEditorFrame {
            doc_index,
            response,
            is_completion_open,
            previous_completion_trigger,
            previous_cursor,
            previous_selection,
        })
    }

    fn apply_editor_response(&mut self, frame: &QueryEditorFrame, effects: &mut QueryEditorEffects) -> bool {
        let response = &frame.response;
        let doc = &mut self.query_session.documents[frame.doc_index];
        let cursor_context_changed =
            frame.previous_cursor != doc.cursor.offset || frame.previous_selection != doc.selection;
        let completion_intent =
            EditorInteractionPolicy::completion_intent(response, frame.is_completion_open, cursor_context_changed);
        if completion_intent == CompletionIntent::Close {
            close_completion(doc);
        }
        if cursor_context_changed {
            if let Some(request_id) = doc.pending_prediction_request {
                effects.actions.push(QueryEditorAction::CancelPrediction { request_id });
            }
            doc.invalidate_prediction();
        }
        self.query_editor.query_editor_focused = response.focused;
        self.query_editor.query_cursor_line = doc.cursor.line + 1;
        self.query_editor.query_cursor_column = doc.cursor.col + 1;
        Self::apply_prediction_response(doc, response);
        doc.cached_tokens.get_or_recompute(&doc.buffer, self.dialect);
        if response.wants_format {
            if let Some(request_id) = doc.pending_prediction_request {
                effects.actions.push(QueryEditorAction::CancelPrediction { request_id });
            }
            format_query_document(doc, self.dialect);
        }
        let selected_text = if response.changed || response.wants_format {
            if doc.selection.is_empty() {
                Some(String::new())
            } else {
                let (start, end) = doc.selection.normalized();
                Some(doc.buffer.slice(start, end).to_owned())
            }
        } else {
            None
        };
        if response.end_snippet {
            doc.snippet = None;
        } else if response.snippet_delta != 0 {
            if let Some(session) = doc.snippet.as_mut() {
                match session.move_by(response.snippet_delta as i32) {
                    crate::editor::SnippetMove::Select(start, end) => {
                        doc.cursor = crate::editor::CursorPosition::from_offset(&doc.buffer, end);
                        doc.selection = crate::editor::SelectionRange::new(start, end);
                    }
                    crate::editor::SnippetMove::Finished => doc.snippet = None,
                }
            }
        } else if response.changed {
            if let Some(mut session) = doc.snippet.take() {
                if session.note_edit(&mut doc.buffer) {
                    if let Some((_, end)) = session.active_range() {
                        doc.cursor = crate::editor::CursorPosition::from_offset(&doc.buffer, end);
                        doc.selection = crate::editor::SelectionRange::point(end);
                    }
                    doc.snippet = Some(session);
                }
            }
        }
        self.query_editor.query_cursor_line = doc.cursor.line + 1;
        self.query_editor.query_cursor_column = doc.cursor.col + 1;
        if response.changed {
            doc.reanalyze(self.dialect);
            doc.dirty = true;
            doc.execution_diagnostic = None;
        }
        // reanalyze gates on buffer.version(): a cursor move into a different
        // statement must still re-sync the statement syntax diagnostic (#510).
        doc.sync_cursor_diagnostic(self.dialect);
        let cursor_in_string_or_comment = doc
            .cached_tokens
            .is_in_string_or_comment(doc.cursor.offset.saturating_sub(1));
        if frame.is_completion_open {
            doc.completion_due_at = None;
        } else if response.wants_identifier_typed && doc.selection.is_empty() && !cursor_in_string_or_comment {
            doc.completion_due_at = Some(Instant::now() + Duration::from_millis(150));
        } else if response.changed || cursor_context_changed || !doc.selection.is_empty() {
            doc.completion_due_at = None;
        }
        if let Some(selected_text) = selected_text {
            self.query_session.selected_text = selected_text;
        }
        if let Some(name) = response.clicked_identifier.as_deref() {
            effects.actions.push(QueryEditorAction::OpenIdentified(
                query_editor_support::navigate_identifier(&self.schema.schema, self.active_schema, name),
            ));
        }
        cursor_in_string_or_comment
    }

    fn apply_prediction_response(doc: &mut QueryDocument, response: &SqlEditorResponse) {
        if let Some(accepted_len) = response.accepted_prediction_len {
            if let Some(prediction) = doc.prediction.as_mut() {
                let was_partial = accepted_len < prediction.text.len();
                prediction.consume(accepted_len);
                doc.prediction_accepted = doc.prediction_accepted.saturating_add(1);
                if was_partial {
                    doc.prediction_partially_accepted = doc.prediction_partially_accepted.saturating_add(1);
                }
                if prediction.is_empty() {
                    doc.prediction = None;
                }
            }
        }
        if response.wants_dismiss_prediction {
            doc.prediction = None;
        }
        if doc
            .prediction
            .as_ref()
            .is_some_and(|prediction| prediction.anchor != doc.cursor.offset)
        {
            doc.prediction = None;
        }
    }

    fn maybe_request_prediction(
        &mut self,
        frame: &QueryEditorFrame,
        cursor_in_string_or_comment: bool,
        active_schema: &str,
        effects: &mut QueryEditorEffects,
    ) {
        let doc = &mut self.query_session.documents[frame.doc_index];
        if EditorInteractionPolicy::should_schedule_prediction(
            &frame.response,
            self.preferences.prediction_mode,
            doc.selection.is_empty(),
            cursor_in_string_or_comment,
        ) {
            doc.schedule_prediction_with_mode(Instant::now(), true);
        }
        if let Some(request) = self.take_prediction_request(frame.doc_index, active_schema, cursor_in_string_or_comment)
        {
            effects
                .actions
                .push(QueryEditorAction::RequestPrediction(Box::new(request)));
        }
    }

    fn take_prediction_request(
        &mut self,
        doc_index: usize,
        active_schema: &str,
        cursor_in_string_or_comment: bool,
    ) -> Option<QueryPredictionRequest> {
        let doc = &mut self.query_session.documents[doc_index];
        if self.preferences.prediction_mode == PredictionMode::Off
            || !doc.prediction_is_due(Instant::now())
            || doc.pending_prediction_request.is_some()
            || doc.completion.is_open
            || !doc.selection.is_empty()
            || cursor_in_string_or_comment
        {
            return None;
        }
        doc.take_prediction_schedule();
        let manual = doc.take_prediction_manual();
        if let Some(scheduled_at) = doc.prediction_scheduled_at.take() {
            doc.prediction_last_debounce_ms = Some(scheduled_at.elapsed().as_millis() as u64);
        }
        let (before_cursor, after_cursor) = doc.buffer.split_at(doc.cursor.offset);
        let context = SchemaCompletionProvider::build_ai_sql_context(
            before_cursor,
            after_cursor,
            doc.cursor.offset,
            active_schema,
            &self.schema.schema,
            !matches!(self.dialect, SqlDialect::Postgres),
        );
        let document_version = doc.buffer.version();
        let anchor = doc.cursor.offset;
        let fingerprint = context.fingerprint(document_version, anchor);
        if !manual {
            if let Some(cached) = doc.take_cached_prediction(fingerprint, Instant::now()) {
                doc.prediction_context_fingerprint = Some(fingerprint);
                doc.prediction = Some(cached);
            }
        }
        if doc.prediction.is_none() && !manual && doc.should_dedupe_prediction(fingerprint, Instant::now()) {
            doc.prediction_requests_deduped = doc.prediction_requests_deduped.saturating_add(1);
            return None;
        }
        if doc.prediction.is_some() {
            return None;
        }
        Some(QueryPredictionRequest {
            document_id: doc.id.clone(),
            document_version,
            anchor,
            replacement_range: prediction_replacement_range(&doc.buffer, anchor, manual),
            context,
            fingerprint,
        })
    }

    fn resolve_completion(
        &mut self,
        frame: &QueryEditorFrame,
        active_schema: &str,
        completion_timer_due: bool,
    ) {
        let document = &self.query_session.documents[frame.doc_index];
        let cursor_context_changed =
            frame.previous_cursor != document.cursor.offset || frame.previous_selection != document.selection;
        let mut intent = EditorInteractionPolicy::completion_intent(
            &frame.response,
            frame.is_completion_open,
            cursor_context_changed,
        );
        if completion_timer_due && intent == CompletionIntent::None {
            intent = CompletionIntent::Open(CompletionTriggerKind::Automatic);
        }
        let Some((trigger_kind, is_manual_trigger)) = (match intent {
            CompletionIntent::Open(trigger) => Some((trigger, trigger == CompletionTriggerKind::Manual)),
            CompletionIntent::Refresh => {
                let trigger = frame
                    .previous_completion_trigger
                    .unwrap_or(CompletionTriggerKind::Automatic);
                Some((trigger, trigger == CompletionTriggerKind::Manual))
            }
            CompletionIntent::None | CompletionIntent::Close => None,
        }) else {
            return;
        };
        let doc = &mut self.query_session.documents[frame.doc_index];
        let (before_cursor, after_cursor) = doc.buffer.split_at(doc.cursor.offset);
        let context = CompletionContext {
            text_before_cursor: before_cursor,
            text_after_cursor: after_cursor,
            cursor_offset: doc.cursor.offset,
            active_schema,
            schema_summary: &self.schema.schema,
            cached_tokens: Some(&doc.cached_tokens),
            is_sqlite: !matches!(self.dialect, SqlDialect::Postgres),
            is_manual_trigger,
        };
        let (prefix, items) = SchemaCompletionProvider::provide(&context);
        if items.is_empty() {
            close_completion(doc);
        } else {
            doc.completion.open(
                doc.cursor.offset,
                doc.buffer.version(),
                frame.response.cursor_screen_pos,
                prefix,
                items,
                trigger_kind,
            );
        }
    }

    fn append_terminal_actions(&self, frame: &QueryEditorFrame, effects: &mut QueryEditorEffects) {
        if frame.response.wants_execute_statement {
            effects.actions.push(QueryEditorAction::DispatchStatement);
        } else if frame.response.wants_execute_all {
            effects.actions.push(QueryEditorAction::DispatchAll);
        }
        if frame.response.wants_save {
            effects.actions.push(QueryEditorAction::SaveQuery);
        }
    }

    fn draw_signature_help(&self, ui: &egui::Ui, frame: &QueryEditorFrame, active_schema: &str) -> bool {
        let doc = &self.query_session.documents[frame.doc_index];
        if !frame.response.focused || doc.completion.is_open {
            return false;
        }
        if let Some(signature) = self.schema.schema_symbol_index.signature_help(
            doc.buffer.text(),
            doc.cursor.offset,
            active_schema,
            self.dialect,
            &doc.cached_tokens,
        ) {
            draw_signature_help(ui.ctx(), frame.response.cursor_screen_pos, &signature, &self.theme);
            return true;
        }
        false
    }

    fn draw_hover_popup(
        &mut self,
        ui: &egui::Ui,
        frame: &QueryEditorFrame,
        active_schema: &str,
        signature_visible: bool,
    ) {
        let doc = &mut self.query_session.documents[frame.doc_index];
        let raw_hover = if !doc.completion.is_open && !signature_visible {
            frame.response.hovered_token
        } else {
            None
        };
        let now = Instant::now();
        if doc.hover_state.update(raw_hover, now) {
            ui.ctx().request_repaint();
        }
        if let Some(remaining) = doc.hover_state.pending_repaint_after(now) {
            ui.ctx().request_repaint_after(remaining);
        }
        let Some(confirmed) = doc.hover_state.confirmed_token() else {
            return;
        };
        let Some(rich) = self.schema.schema_symbol_index.rich_hover(
            doc.buffer.text(),
            confirmed.range,
            active_schema,
            self.dialect,
            &self.schema.schema,
        ) else {
            return;
        };
        draw_rich_hover_popup(ui.ctx(), confirmed.anchor_rect, confirmed.range, &rich, &self.theme);
    }
}
