use super::*;
use std::time::Instant;

#[path = "query_layout_surface_view.rs"]
mod query_layout_surface_view;

#[path = "query_snippets_surface_view.rs"]
mod query_snippets_surface_view;

/// Egress note shown with the AI prediction control (#242).
///
/// Inline prediction can still schedule without a click — default is `Subtle`
/// (`crates/ui/src/editor/prediction.rs`) so ghost text stays quieter until reveal,
/// and a request follows 300 ms after an edit or cursor move (`query_document.rs`).
/// The control that chooses the mode is where its data flow has to be stated. The
/// registry entry recording the same facts is `docs/release/known-limitations.md`
/// LIM-019; with no key configured the runtime answers "AI provider is not configured"
/// and nothing leaves the machine (`crates/runtime/src/worker.rs:1118`).
pub(super) const AI_PREDICTION_EGRESS_NOTE: &str =
    "Sends the SQL around your cursor and its schema context to your configured AI provider.";

#[path = "query_helpers.rs"]
mod query_helpers;
pub(crate) use query_helpers::{
    database_error_diagnostic, deduplicate_diagnostics, deduplicate_messages, format_query_document,
    friendly_connection_error, prediction_replacement_range, toggle_query_comment, write_file_atomically,
};

impl DbProApp {
    pub(super) fn draw_query(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: (SPACE_XS) as i8,
                right: (SPACE_XS) as i8,
                top: (SPACE_XS) as i8,
                bottom: 0.0 as i8,
            })
            .show(ui, |ui| self.draw_query_content(ui));
    }

    fn draw_query_content(&mut self, ui: &mut egui::Ui) {
        self.refresh_query_diagnostics(ui);
        self.draw_query_context_chrome(ui);
        self.draw_query_transaction_chrome(ui);
        self.draw_visual_query_builder_surface(ui);
        let layout = query_layout_surface_view::calculate(query_layout_surface_view::QueryPanelLayoutContext {
            available_width: ui.available_width(),
            available_height: ui.available_height(),
            bottom_panel_open: self.workspace.bottom_panel_open,
            output_dock_maximized: self.query.editor.query_output_dock_maximized,
            bottom_panel_height: self.workspace.bottom_panel_height,
            right_dock_width: self.workspace.right_dock_width,
            dock_position: self.workspace.output_dock_position,
        });
        if layout.dock_open && layout.dock_position == OutputDockPosition::Right {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(layout.editor_width, layout.editor_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        self.draw_query_editor_stack(ui, layout.editor_height);
                    },
                );
                let mut splitter_context = query_output_dock_surface_view::QueryOutputDockContext {
                    theme: self.theme,
                    workspace: &mut self.workspace,
                    output: &mut self.query.output,
                    session: &self.query.session,
                    editor: &mut self.query.editor,
                };
                splitter_context.draw_horizontal_splitter(ui, layout.dock_height);
                ui.allocate_ui_with_layout(
                    egui::vec2(layout.dock_width.min(ui.available_width()), layout.dock_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        self.draw_query_output_dock(ui, layout.dock_height);
                    },
                );
            });
        } else {
            self.draw_query_editor_stack(ui, layout.editor_height);
            self.draw_query_optional_panels(ui);
            if layout.dock_open {
                self.draw_query_output_dock(ui, layout.dock_height);
            }
        }
        self.draw_query_status_bar(ui);
        self.draw_dirty_close_dialog(ui.ctx());
        self.draw_save_as_dialog(ui.ctx());
    }

    fn refresh_query_diagnostics(&mut self, ui: &egui::Ui) {
        let driver = self.active_driver().to_owned();
        let lint = self.preferences.settings.editor.lint.clone();
        query_diagnostics_view::refresh_diagnostics(&mut self.query, &driver, &lint);
        if let Some(deadline) = self.query.editor.diagnostics_debounce_at {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                ui.ctx().request_repaint_after(remaining);
            }
        }
    }

    // cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
    fn draw_query_context_chrome(&mut self, ui: &mut egui::Ui) {
        let doc_idx = self.query.session.active_document_index;
        let file_path = self
            .query
            .session
            .documents
            .get(doc_idx)
            .and_then(|document| document.file_path.clone());
        let connected = self.active_query_connection_id().is_some() && self.connection.lifecycle.is_connected();
        let connection_name = if connected {
            self.active_query_connection_name().to_owned()
        } else {
            "No connection".to_owned()
        };
        let schema = self.active_query_schema().to_owned();
        let environment = self
            .active_query_connection()
            .map(|connection| connection.environment)
            .unwrap_or_default();
        let active_request_id = self.active_query_request_id();
        let execution_started_at = self
            .query
            .session
            .active_document()
            .and_then(|document| document.execution_started_at);
        let capabilities = self.query_capabilities();
        let cancel_reason =
            capabilities.feature_limitation(db_pro_core::domain::capabilities::CapabilityFeature::Cancel);
        let cancel_supported = capabilities.allows(|value| value.query.cancel);
        let modifier = Self::primary_modifier_label();
        let chrome = {
            let mut context = query_context_view::QueryContextViewContext {
                theme: self.theme,
                editor: &mut self.query.editor,
                execution: &mut self.query.execution,
                file_path: file_path.as_deref(),
                connected,
                connection_name: &connection_name,
                schema: &schema,
                environment,
                active_request_id,
                execution_started_at,
                cancel_supported,
                cancel_reason: cancel_reason.as_deref(),
                modifier,
            };
            query_context_view::draw_context_strip(&mut context, ui)
        };
        if let Some(action) = chrome.action {
            self.apply_query_chrome_action(action);
        }
        let anchors = chrome.anchors;
        if self.query.editor.query_context_picker_open {
            if let Some(anchor) = anchors.context_anchor {
                self.draw_query_context_picker(ui.ctx(), anchor);
            }
        }
        if self.query.editor.query_tools_open {
            if let Some(anchor) = anchors.more_anchor {
                self.draw_query_actions_menu(ui.ctx(), anchor);
            }
        }
    }

    fn draw_query_transaction_chrome(&mut self, ui: &mut egui::Ui) {
        let document_id = self
            .query
            .session
            .active_document()
            .map(|document| document.id.clone());
        let actions = query_transaction_surface_view::QueryTransactionSurfaceContext {
            theme: self.theme,
            execution: &mut self.query.execution,
            document_id: document_id.as_deref(),
        }
        .draw(ui);
        for action in actions {
            match action {
                query_transaction_surface_view::QueryTransactionAction::Transaction(action) => {
                    self.handle_transaction_action(action);
                }
                query_transaction_surface_view::QueryTransactionAction::DismissDisconnectGuard => {
                    self.query.execution.disconnect_txn_guard = false;
                }
            }
        }
    }

    fn draw_visual_query_builder_surface(&mut self, ui: &mut egui::Ui) {
        query_shell_surface_view::draw_visual_builder(ui, self.theme, self.query.editor.visual_builder.open, |ui| {
            self.draw_visual_query_builder(ui)
        });
    }

    fn draw_query_editor_stack(&mut self, ui: &mut egui::Ui, editor_height: f32) {
        query_shell_surface_view::draw_editor_stack(ui, editor_height, |ui| self.draw_query_editor(ui));
        self.draw_floating_completion_popup(ui.ctx());
        let mut context = query_search_view::QuerySearchContext {
            theme: self.theme,
            editor: &mut self.query.editor,
            session: &mut self.query.session,
        };
        query_search_view::draw_editor_search_overlay(&mut context, ui.ctx());
    }

    fn draw_query_optional_panels(&mut self, ui: &mut egui::Ui) {
        if self.query.editor.snippets_open {
            self.draw_sql_snippets(ui);
        }
        if self.query.editor.query_params_panel_open {
            self.draw_sql_parameters_panel(ui);
        }
    }

    fn draw_query_context_picker(&mut self, ctx: &egui::Context, anchor: egui::Rect) {
        let doc_idx = self.query.session.active_document_index;
        let current_conn_id = self
            .query
            .session
            .documents
            .get(doc_idx)
            .and_then(|d| d.connection_id.clone())
            .or_else(|| self.connection.lifecycle.active_connection_id().map(str::to_owned));
        let current_schema = self.active_query_schema().to_owned();
        // UAT-8: the loaded schema explorer only applies when it describes the
        // same connection the document is bound to — otherwise a PG-bound doc
        // would offer SQLite's ["main"].
        let doc_conn = self
            .query
            .session
            .documents
            .get(doc_idx)
            .and_then(|d| d.connection_id.as_deref());
        let global_conn = self.connection.lifecycle.active_connection_id();
        let explorer_is_current = doc_conn.is_none() || doc_conn == global_conn;
        let available_schemas = if explorer_is_current && !self.schema.explorer.schema.schemas.is_empty() {
            self.schema.explorer.schema.schemas.clone()
        } else if !self.query_capabilities().allows(|caps| caps.schema.schemas) {
            vec!["main".to_string()]
        } else {
            vec!["public".to_string()]
        };
        let connections: Vec<(String, String, String)> = self
            .connection
            .catalog
            .iter()
            .map(|c| (c.id.clone(), c.name.clone(), c.environment.as_label().to_owned()))
            .collect();

        let context = query_context_picker_view::QueryContextPickerContext {
            theme: self.theme,
            current_connection_id: current_conn_id.as_deref(),
            current_schema: &current_schema,
            available_schemas: &available_schemas,
            connections: &connections,
        };
        if let Some(action) = query_context_picker_view::draw_picker(&context, ctx, anchor) {
            match action {
                query_context_picker_view::QueryContextPickerAction::SelectConnection(id) => {
                    self.set_document_connection(doc_idx, Some(id));
                    self.query.editor.query_context_picker_open = false;
                }
                query_context_picker_view::QueryContextPickerAction::SelectSchema(schema) => {
                    self.set_document_schema(doc_idx, Some(schema));
                    self.query.editor.query_context_picker_open = false;
                }
                query_context_picker_view::QueryContextPickerAction::Close => {
                    self.query.editor.query_context_picker_open = false;
                }
            }
        }
    }

    fn draw_query_output_dock(&mut self, ui: &mut egui::Ui, dock_height: f32) {
        let body_height = {
            let mut context = query_output_dock_surface_view::QueryOutputDockContext {
                theme: self.theme,
                workspace: &mut self.workspace,
                output: &mut self.query.output,
                session: &self.query.session,
                editor: &mut self.query.editor,
            };
            context.draw_chrome(ui, dock_height)
        };
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), body_height),
            Layout::top_down(Align::Min),
            |ui| {
                // #541: the draw path takes a refcount-bump handle and paints by
                // reference — a deep `active_result().cloned()` here cost 58 ms per
                // frame at 100k x 8 and defeated grid virtualization.
                let result = self.query.session.active_result_handle();
                self.draw_output_pane(ui, result.as_deref());
            },
        );
    }

    fn draw_query_status_bar(&mut self, ui: &mut egui::Ui) {
        self.refresh_query_parameter_count();
        if let Some(action) = self.draw_query_status_surface(ui) {
            self.apply_query_status_bar_action(action);
        }
    }

    fn refresh_query_parameter_count(&mut self) {
        let driver = self.active_driver().to_owned();
        let param_key = (
            self.query.session.active_document_index,
            self.query.session.active_buffer_version(),
        );
        if self.query.editor.param_count_cache_key != Some(param_key)
            || self.query.editor.param_count_cache_driver != driver
        {
            self.query.editor.param_count_cache_key = Some(param_key);
            self.query.editor.param_count_cache =
                crate::query::discover_sql_parameters_for_driver(self.query.session.active_text(), &driver).len();
            self.query.editor.param_count_cache_driver = driver;
        }
    }

    fn draw_query_status_surface(
        &self,
        ui: &mut egui::Ui,
    ) -> Option<query_status_bar_surface_view::QueryStatusBarAction> {
        let document_id = self.query.session.active_document().map(|document| document.id.as_str());
        query_status_bar_surface_view::draw_status_bar(
            &query_status_bar_surface_view::QueryStatusBarContext {
                theme: self.theme,
                bottom_panel_open: self.workspace.bottom_panel_open,
                in_transaction: self.query.execution.in_transaction(document_id),
                transaction_pending: self.query.execution.txn_pending(document_id),
                auto_commit: self.query.execution.auto_commit(document_id),
                cursor_line: self.query.editor.query_cursor_line,
                cursor_column: self.query.editor.query_cursor_column,
                selected_line_range: self
                    .query
                    .session
                    .documents
                    .get(self.query.session.active_document_index)
                    .filter(|document| !document.selection.is_empty())
                    .map(|document| {
                        let (start, end) = document.selection.normalized();
                        let (mut end_line, end_col) = document.buffer.offset_to_line_col(end);
                        // A selection ending at column 0 does not visually cover
                        // that last line — match standard editor telemetry.
                        if end_col == 0 && end_line > 0 {
                            end_line -= 1;
                        }
                        (
                            document.buffer.offset_to_line_col(start).0 + 1,
                            end_line + 1,
                        )
                    }),
                driver: self.active_query_driver(),
                schema: self.active_query_schema(),
                parameter_count: self.query.editor.param_count_cache,
                diagnostic_count: self.query.editor.diagnostics.len(),
            },
            ui,
        )
    }

    fn active_query_request_id(&self) -> Option<RequestId> {
        self.query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .and_then(|document| match document.execution_state {
                QueryExecutionState::Running(request_id) => Some(request_id),
                _ => None,
            })
    }

    fn apply_query_chrome_action(&mut self, action: query_context_view::QueryChromeAction) {
        use query_context_view::QueryChromeAction;
        match action {
            QueryChromeAction::RunControl(action) => self.apply_run_control_action(action),
            QueryChromeAction::Explain => self.explain_query(),
            QueryChromeAction::Format => {
                let capabilities = self.query_capabilities();
                if let Some(request_id) = query_diagnostics_view::format_active_query(
                    &mut self.query,
                    capabilities,
                    &mut self.feedback,
                ) {
                    self.send_command_best_effort(UiCommand::CancelSqlPrediction { request_id });
                }
            }
        }
    }

    fn apply_run_control_action(&mut self, action: query_run_control_view::QueryRunControlAction) {
        use query_run_control_view::QueryRunControlAction;
        match action {
            QueryRunControlAction::Run => {
                self.dispatch_query();
            }
            QueryRunControlAction::Cancel(request_id) => self.cancel_query(request_id),
            QueryRunControlAction::ReportUnsupportedCancel(reason) => {
                self.feedback.runtime_message = reason;
            }
            QueryRunControlAction::ReportDisconnected => {
                self.feedback.runtime_message = "Connect to a database before running a query".to_owned();
            }
        }
    }

    fn apply_query_status_bar_action(&mut self, action: query_status_bar_surface_view::QueryStatusBarAction) {
        use query_status_bar_surface_view::QueryStatusBarAction as Action;
        match action {
            Action::ShowOutput => self.workspace.bottom_panel_open = true,
            Action::ToggleTransaction => {
                self.query.execution.query_txn_bar_open = !self.query.execution.query_txn_bar_open;
            }
            Action::ToggleParameters => {
                self.query.editor.query_params_panel_open = !self.query.editor.query_params_panel_open;
            }
            Action::ShowDiagnostics => self.show_query_diagnostics(),
        }
    }

    fn show_query_diagnostics(&mut self) {
        self.workspace.bottom_panel_open = true;
        if let Some(doc_id) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|document| document.id.clone())
        {
            self.query.output.set_for_document_and_activate_if_active(
                &doc_id,
                self.query
                    .session
                    .active_document()
                    .map(|document| document.id.as_str()),
                OutputTab::Messages,
            );
        }
    }
}

impl DbProApp {
    /// Quick SQL snippet inserters.
    fn draw_sql_snippets(&mut self, ui: &mut egui::Ui) {
        let context = query_snippets_surface_view::QuerySnippetsContext { theme: self.theme };
        if let Some(query_snippets_surface_view::QuerySnippetsAction::Insert(snippet)) = context.draw(ui) {
            self.insert_snippet(snippet);
            self.query.editor.snippets_open = false;
        }
    }

    /// Discovered bind placeholders for the active document (#225 discovery slice).
    fn draw_sql_parameters_panel(&mut self, ui: &mut egui::Ui) {
        let supports_parameters = self.query_capabilities().allows(|caps| caps.query.parameters);
        let driver = self.active_driver().to_owned();
        let mut context = query_parameters_view::QueryParametersContext {
            theme: self.theme,
            session: &mut self.query.session,
            supports_parameters,
            driver: &driver,
        };
        query_parameters_view::draw_parameters_panel(&mut context, ui);
    }
}

#[cfg(test)]
#[path = "tests/query_view_tests.rs"]
mod query_view_tests;
