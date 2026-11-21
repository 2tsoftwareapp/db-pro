use super::*;
use std::time::Instant;

#[path = "query_actions_surface_view.rs"]
mod query_actions_surface_view;

pub(super) use super::query_view::AI_PREDICTION_EGRESS_NOTE;

impl DbProApp {
    /// Query actions menu anchored to the compact query context strip.
    pub(super) fn draw_query_actions_menu(&mut self, ctx: &egui::Context, anchor: egui::Rect) {
        let actions = {
            let mut context = self.query_actions_surface_context();
            context.draw(ctx, anchor)
        };
        self.apply_query_actions(actions, ctx);
    }

    /// Compatibility adapter for the egress disclosure test and small callers.
    #[cfg(test)]
    pub(super) fn draw_query_editor_actions(&mut self, ui: &mut egui::Ui) -> bool {
        let actions = {
            let mut context = self.query_actions_surface_context();
            context.draw_editor_only(ui)
        };
        let close = actions
            .iter()
            .any(|action| matches!(action, query_actions_surface_view::QueryActionsSurfaceAction::Close));
        let ctx = ui.ctx().clone();
        self.apply_query_actions(actions, &ctx);
        close
    }

    fn query_actions_surface_context(&mut self) -> query_actions_surface_view::QueryActionsSurfaceContext<'_> {
        query_actions_surface_view::QueryActionsSurfaceContext {
            theme: self.theme,
            editor: &mut self.query.editor,
            execution: &mut self.query.execution,
            library: &mut self.query.library,
            session: &self.query.session,
            prediction_mode: &mut self.preferences.prediction_mode,
        }
    }

    fn apply_query_actions(
        &mut self,
        actions: Vec<query_actions_surface_view::QueryActionsSurfaceAction>,
        ctx: &egui::Context,
    ) {
        use query_actions_surface_view::QueryActionsSurfaceAction as Action;

        for action in actions {
            match action {
                Action::Run => {
                    self.dispatch_query();
                }
                Action::RunScript => {
                    self.dispatch_query_all();
                }
                Action::Format => {
                    let capabilities = self.query_capabilities();
                    if let Some(request_id) = query_diagnostics_view::format_active_query(
                        &mut self.query,
                        capabilities,
                        &mut self.feedback,
                    ) {
                        self.send_command_best_effort(UiCommand::CancelSqlPrediction { request_id });
                    }
                }
                Action::Explain => self.explain_query(),
                Action::AskAgent => self.open_agent_prompt(
                    if self.query.session.selected_text.trim().is_empty() {
                        "Explain the current SQL and suggest improvements"
                    } else {
                        "Explain the selected SQL and suggest improvements"
                    },
                    ctx,
                ),
                Action::Save => self.save_query_document(),
                Action::SaveAs => self.open_save_as_dialog(),
                Action::ToggleVisualBuilder => {
                    self.query.editor.visual_builder.open = !self.query.editor.visual_builder.open;
                }
                Action::ToggleSearch => {
                    self.query.editor.editor_search_open = !self.query.editor.editor_search_open;
                }
                Action::ToggleTransaction => {
                    self.query.execution.query_txn_bar_open = !self.query.execution.query_txn_bar_open;
                }
                Action::DecreaseFont => {
                    self.query.editor.editor_font_size = (self.query.editor.editor_font_size - 1.0).max(10.0);
                }
                Action::IncreaseFont => {
                    self.query.editor.editor_font_size = (self.query.editor.editor_font_size + 1.0).min(24.0);
                }
                Action::GeneratePrediction => {
                    if self.preferences.prediction_mode != PredictionMode::Off {
                        if let Some(document) = self
                            .query
                            .session
                            .documents
                            .get_mut(self.query.session.active_document_index)
                        {
                            document.schedule_prediction_with_mode(Instant::now(), true);
                        }
                    }
                }
                Action::SetPredictionMode(mode) => {
                    self.preferences.prediction_mode = mode;
                    if mode == PredictionMode::Off {
                        self.cancel_prediction_for_document(self.query.session.active_document_index);
                    }
                }
                Action::ToggleSnippets => {
                    self.query.editor.snippets_open = !self.query.editor.snippets_open;
                }
                Action::CreateFolder => self.create_query_folder(),
                Action::Close => self.query.editor.query_tools_open = false,
            }
        }
    }

    fn create_query_folder(&mut self) {
        let Some(connection) = self.active_connection().cloned() else {
            return;
        };
        let request_id = self.next_request_id();
        let command = match query_save_commands::create_folder_command(&self.query.library, request_id, connection.id) {
            Ok(command) => command,
            Err(error) => {
                self.feedback.runtime_message = error;
                return;
            }
        };
        if self.dispatch_command(command) {
            self.feedback.runtime_message = "Creating query folder…".to_owned();
        }
    }

    pub(crate) fn insert_snippet(&mut self, snippet: &str) {
        self.cancel_prediction_for_document(self.query.session.active_document_index);
        if let Some(document) = self
            .query
            .session
            .documents
            .get_mut(self.query.session.active_document_index)
        {
            let offset = document.cursor.offset.min(document.buffer.len_bytes());
            let (body, mut session) = crate::editor::SnippetSession::expand(snippet);
            let needs_break = offset > 0 && !document.buffer.text()[..offset].ends_with('\n');
            let insertion = if needs_break { format!("\n{body}") } else { body };
            if let Some(session) = session.as_mut() {
                session.translate(offset + usize::from(needs_break));
            }
            document.buffer.insert(offset, &insertion);
            document.snippet = session;
            if let Some((start, end)) = document.snippet.as_ref().and_then(|session| session.active_range()) {
                document.cursor = crate::editor::CursorPosition::from_offset(&document.buffer, end);
                document.selection = crate::editor::SelectionRange::new(start, end);
            } else {
                let new_offset = offset + insertion.len();
                document.cursor = crate::editor::CursorPosition::from_offset(&document.buffer, new_offset);
                document.selection = crate::editor::SelectionRange::point(new_offset);
            }
            document.dirty = true;
            self.query.editor.query_cursor_line = document.cursor.line + 1;
            self.query.editor.query_cursor_column = document.cursor.col + 1;
        }
        self.workspace.active_tab = WorkspaceTab::Query;
        let driver = self.active_driver().to_owned();
        let lint = self.preferences.settings.editor.lint.clone();
        query_diagnostics_view::refresh_diagnostics(&mut self.query, &driver, &lint);
        self.feedback.runtime_message = "Snippet inserted".to_owned();
    }
}
