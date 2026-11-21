// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Query output composition and the result-grid shell.
use super::*;

impl DbProApp {
    /// Selects an output pane and applies the pane's explicit action at the root boundary.
    pub(super) fn draw_output_pane(&mut self, ui: &mut egui::Ui, result: Option<&UiQueryResult>) {
        let active_tab = self.query.output.active_tab_for_document(
            self.query
                .session
                .active_document()
                .map(|document| document.id.as_str()),
        );
        match active_tab {
            OutputTab::Results => self.draw_results_pane(ui, result),
            OutputTab::Chart => self.draw_chart_pane(ui, result),
            OutputTab::Messages => self.draw_messages_pane(ui),
            OutputTab::Explain => self.draw_explain_pane(ui),
            OutputTab::History => self.draw_history_pane(ui),
        }
    }

    /// Results grid plus its row-count / export header.
    pub(super) fn draw_results_pane(&mut self, ui: &mut egui::Ui, result: Option<&UiQueryResult>) {
        let (active_result_index, result_names, pinned_results, row_limit, results_stale, results_running) =
            if let Some(active_doc) = self
                .query
                .session
                .documents
                .get(self.query.session.active_document_index)
            {
                (
                    active_doc.active_result_index,
                    active_doc.query_result_names.clone(),
                    active_doc.pinned_results.clone(),
                    active_doc.query_result_row_limit,
                    active_doc.results_stale,
                    matches!(active_doc.execution_state, QueryExecutionState::Running(_)),
                )
            } else {
                (
                    0,
                    std::collections::HashMap::new(),
                    std::collections::BTreeSet::new(),
                    None,
                    false,
                    false,
                )
            };
        let context = query_results_surface_view::QueryResultsSurfaceContext {
            theme: self.theme,
            result_count: self.query.session.active_result_count(),
            active_result_index,
            result_names: &result_names,
            pinned_results: &pinned_results,
            result,
            row_limit,
            results_stale,
            results_running,
        };

        if let Some(action) = query_results_surface_view::draw_results(&context, ui, |ui, result| {
            self.draw_result_grid(ui, result);
        }) {
            match action {
                query_results_surface_view::QueryResultsSurfaceAction::SelectResult(index) => {
                    self.set_active_query_result(index);
                }
                query_results_surface_view::QueryResultsSurfaceAction::TogglePin(index) => {
                    if let Some(doc) = self.query.session.documents.get_mut(self.query.session.active_document_index) {
                        if !doc.pinned_results.remove(&index) {
                            doc.pinned_results.insert(index);
                        }
                    }
                }
                query_results_surface_view::QueryResultsSurfaceAction::CloseResult(index) => {
                    if let Some(doc) = self.query.session.documents.get_mut(self.query.session.active_document_index) {
                        if index < doc.query_results.len() {
                            doc.query_results.remove(index);
                            if doc.active_result_index >= doc.query_results.len() && !doc.query_results.is_empty() {
                                doc.active_result_index = doc.query_results.len() - 1;
                            }
                        }
                    }
                }
                query_results_surface_view::QueryResultsSurfaceAction::CloseOtherResults(keep_index) => {
                    if let Some(doc) = self.query.session.documents.get_mut(self.query.session.active_document_index) {
                        if keep_index < doc.query_results.len() {
                            let kept = doc.query_results.remove(keep_index);
                            doc.query_results = vec![kept];
                            doc.active_result_index = 0;
                            doc.pinned_results.clear();
                            doc.query_result_names.clear();
                        }
                    }
                }
                query_results_surface_view::QueryResultsSurfaceAction::OpenExport => {
                    self.overlay.export_open = true;
                }
                query_results_surface_view::QueryResultsSurfaceAction::OpenInTab => {
                    self.open_results_tab();
                }
            }
        }
        self.draw_export_dialog(ui, result);
    }

    fn draw_messages_pane(&mut self, ui: &mut egui::Ui) {
        let mut context = query_output_panes_view::QueryOutputPanesContext {
            theme: self.theme,
            session: &mut self.query.session,
        };
        query_output_panes_view::draw_messages_pane(&mut context, ui);
    }

    fn draw_chart_pane(&mut self, ui: &mut egui::Ui, result: Option<&UiQueryResult>) {
        let mut context = query_output_panes_view::QueryOutputPanesContext {
            theme: self.theme,
            session: &mut self.query.session,
        };
        query_output_panes_view::draw_chart_pane(&mut context, ui, result);
    }

    fn draw_explain_pane(&mut self, ui: &mut egui::Ui) {
        let analyze_executes = query_explain_actions::explain_analyze_executes(self.active_query_driver());
        let action = {
            let mut context = query_output_actions_view::QueryOutputActionsContext {
                theme: self.theme,
                session: &self.query.session,
                editor: &mut self.query.editor,
                execution: &mut self.query.execution,
                feedback: &mut self.feedback,
                analyze_executes,
            };
            query_output_actions_view::draw_explain_pane(&mut context, ui)
        };
        if let Some(action) = action {
            self.apply_query_output_action(action);
        }
    }

    fn draw_history_pane(&mut self, ui: &mut egui::Ui) {
        let action = {
            let mut context = query_output_actions_view::QueryOutputActionsContext {
                theme: self.theme,
                session: &self.query.session,
                editor: &mut self.query.editor,
                execution: &mut self.query.execution,
                feedback: &mut self.feedback,
                analyze_executes: false,
            };
            query_output_actions_view::draw_history_pane(&mut context, ui)
        };
        if let Some(action) = action {
            self.apply_query_output_action(action);
        }
    }

    fn apply_query_output_action(&mut self, action: query_output_actions_view::QueryOutputAction) {
        match action {
            query_output_actions_view::QueryOutputAction::Explain => self.explain_query(),
            query_output_actions_view::QueryOutputAction::ExplainAnalyze => self.explain_query_analyze(),
            query_output_actions_view::QueryOutputAction::OpenHistory(entry, run) => {
                self.open_history_entry(&entry, run);
            }
        }
    }
}
