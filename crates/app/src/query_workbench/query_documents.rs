// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Query document tab lifecycle (new / close / duplicate / history open).
use super::*;

/// Explicit dependencies for query-document lifecycle transitions.
///
/// The lifecycle owns query-tab state and coordinates only the stateful side
/// effects that are part of closing/opening a document. App-level commands
/// such as executing a query remain at the composition root.
pub(crate) struct QueryDocumentContext<'state, 'bridge> {
    pub(super) query_session: &'state mut QuerySessionState,
    pub(super) query_editor: &'state mut QueryEditorState,
    pub(super) workspace: &'state mut WorkspaceFeatureState,
    pub(super) agent: &'state mut AgentState,
    pub(super) query_output: &'state mut QueryOutputState,
    pub(super) schema_explorer: &'state mut SchemaExplorerState,
    pub(super) command_dispatcher: command_dispatch::RuntimeCommandDispatcher<'bridge>,
    pub(super) feedback: &'state mut FeedbackState,
    pub(super) execution: &'state mut QueryExecutionPolicyState,
    active_connection_id: Option<String>,
    active_schema: String,
}

impl QueryDocumentContext<'_, '_> {
    pub(crate) fn new_document(&mut self, scratch: bool) {
        if !self.evict_oldest_document_if_capped() {
            return;
        }
        let (document_id, index) = self.next_document_identity();
        let prefix = if scratch { "Scratch" } else { "Query" };
        let mut document = QueryDocument::new(document_id, format!("{prefix} {index}"), String::new());
        document.connection_id = self.active_connection_id.clone();
        document.schema = Some(self.active_schema.clone());
        self.query_session.add_document(document);
        self.activate_query_surface();
        if scratch {
            self.feedback.set_runtime_message("Opened scratch SQL tab");
        }
    }

    pub(crate) fn open_history_entry(&mut self, entry: &UiQueryHistoryEntry) {
        if !self.evict_oldest_document_if_capped() {
            return;
        }
        let (document_id, document_number) = self.next_document_identity();
        let mut document = QueryDocument::new(document_id, format!("History {document_number}"), entry.sql.clone());
        document.connection_id = entry.connection_id.clone();
        document.schema = entry.schema.clone();
        self.query_session.add_document(document);
        self.activate_query_surface();
    }
    /// Open `text` in a **new** query document instead of replacing the active
    /// buffer (#419/#420): generated and saved SQL must never silently
    /// overwrite the document the user is editing.
    pub(crate) fn open_text_document(&mut self, text: impl Into<String>, connection_id: Option<String>) {
        if !self.evict_oldest_document_if_capped() {
            return;
        }
        let (document_id, document_number) = self.next_document_identity();
        let mut document = QueryDocument::new(document_id, format!("Query {document_number}"), text.into());
        // Preselect the template: the first keystroke replaces it instead of
        // prepending to `SELECT *` (#607).
        let len = document.text().len();
        document.selection = crate::editor::SelectionRange::new(0, len);
        document.cursor.set_offset(&document.buffer, len);
        document.connection_id = connection_id.or_else(|| self.active_connection_id.clone());
        document.schema = Some(self.active_schema.clone());
        self.query_session.add_document(document);
        self.activate_query_surface();
    }


    pub(crate) fn rename_document_inline(&mut self, index: usize) {
        let Some(document) = self.query_session.documents.get_mut(index) else {
            return;
        };
        if document.title.starts_with("Scratch ") {
            let number = document
                .title
                .trim_start_matches("Scratch ")
                .parse::<u32>()
                .unwrap_or(1);
            document.title = format!("Scratch {}", number + 1);
        } else if let Some(rest) = document.title.strip_prefix("Query ") {
            let number = rest.parse::<u32>().unwrap_or(1);
            document.title = format!("Query {}", number + 1);
        } else {
            document.title = format!("{} (renamed)", document.title);
        }
        self.feedback
            .set_runtime_message(format!("Renamed tab to {}", document.title));
    }

    pub(crate) fn switch_document(&mut self, index: usize) {
        if index == self.query_session.active_document_index || !self.query_session.select_document(index) {
            return;
        }
        self.sync_active_cursor_and_selection();
        let title = self
            .query_session
            .active_document()
            .map(|document| document.title.clone())
            .unwrap_or_default();
        self.feedback.set_runtime_message(format!("Opened {title}"));
    }

    pub(crate) fn set_active_text(&mut self, text: impl Into<String>) {
        let index = self.query_session.active_document_index;
        self.cancel_prediction(index);
        self.query_session.set_active_text(text);
    }

    pub(crate) fn append_active_text(&mut self, text: &str) {
        let index = self.query_session.active_document_index;
        self.cancel_prediction(index);
        self.query_session.append_active_text(text);
    }

    pub(crate) fn set_document_connection(&mut self, index: usize, connection_id: Option<String>) {
        self.cancel_prediction(index);
        self.query_session.set_document_connection(index, connection_id);
    }

    pub(crate) fn set_document_schema(&mut self, index: usize, schema: Option<String>) {
        self.cancel_prediction(index);
        self.query_session.set_document_schema(index, schema);
    }

    pub(crate) fn cancel_prediction_for_document(&mut self, index: usize) {
        self.cancel_prediction(index);
    }

    pub(crate) fn close_document(&mut self, index: usize) {
        if index >= self.query_session.documents.len() {
            return;
        }
        if self.execution.document_is_busy(&self.query_session.documents[index].id) {
            self.feedback
                .set_runtime_message("Commit or rollback the open transaction before closing this query");
            return;
        }

        self.cancel_prediction(index);
        let closed = self.query_session.documents[index].clone();
        // A running request outlives the tab if we just forget it: the
        // server keeps executing and the result lands on a dead document.
        // Cancel in-flight work before dropping the document.
        self.cancel_document_requests(&closed.id);
        self.execution.forget_document(&closed.id);
        self.cancel_agent_run(&closed.id);
        self.query_session.remove_document(index);
        self.query_output.tabs_by_document.remove(&closed.id);

        if self.query_session.documents.is_empty() {
            self.query_session.active_document_index = 0;
            self.reset_cursor();
            if self.workspace.active_tab == WorkspaceTab::Query {
                self.activate_fallback_surface();
            }
            self.feedback.set_runtime_message(format!("Closed {}", closed.title));
            return;
        }

        self.sync_active_cursor_and_selection();
        let active_title = self
            .query_session
            .active_document()
            .map(|document| document.title.as_str())
            .unwrap_or(closed.title.as_str());
        self.feedback.set_runtime_message(format!("Closed {active_title}"));
    }

    pub(crate) fn request_close_document(&mut self, index: usize) {
        if self
            .query_session
            .documents
            .get(index)
            .is_some_and(QueryDocument::is_dirty)
        {
            self.query_session.pending_dirty_close = Some(index);
        } else {
            self.close_document(index);
        }
    }

    pub(crate) fn duplicate_document(&mut self, index: usize) {
        if !self.evict_oldest_document_if_capped() {
            return;
        }
        let Some(source) = self.query_session.documents.get(index).cloned() else {
            return;
        };
        let (document_id, _) = self.next_document_identity();
        let mut document = QueryDocument::new(document_id, format!("{} (Copy)", source.title), source.text());
        document.connection_id = source.connection_id.or_else(|| self.active_connection_id.clone());
        document.schema = source.schema.or_else(|| Some(self.active_schema.clone()));
        self.query_session.add_document(document);
        self.query_editor.query_focus_editor_on_open = true;
        self.workspace.active_tab = WorkspaceTab::Query;
        self.feedback
            .set_runtime_message(format!("Duplicated {}", source.title));
    }

    pub(crate) fn close_other_documents(&mut self, keep_index: usize) {
        if keep_index >= self.query_session.documents.len() {
            return;
        }
        if self.query_session.documents.iter().enumerate().any(|(index, document)| {
            index != keep_index && self.execution.document_is_busy(&document.id)
        }) {
            self.feedback
                .set_runtime_message("Commit or rollback the open transaction before closing this query");
            return;
        }
        // Dirty tabs in the close set keep their SQL via the bulk-confirm
        // dialog; only a clean bulk close proceeds immediately.
        // Replay after the bulk dirty confirm skips the guard; the pending
        // marker is cleared before re-entering so the check below only fires
        // once.
        if self.query_session.pending_bulk_close == Some(PendingBulkClose::Others(keep_index)) {
            self.query_session.pending_bulk_close = None;
        } else if self.bulk_close_has_dirty(|index| index != keep_index) {
            self.query_session.pending_bulk_close = Some(PendingBulkClose::Others(keep_index));
            return;
        }
        let removed: Vec<String> = self
            .query_session
            .documents
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != keep_index)
            .map(|(_, document)| document.id.clone())
            .collect();
        for index in 0..self.query_session.documents.len() {
            if index != keep_index {
                self.cancel_prediction(index);
                let id = self.query_session.documents[index].id.clone();
                self.cancel_document_requests(&id);
            }
        }
        self.query_session.keep_document(keep_index);
        for document_id in removed {
            self.execution.forget_document(&document_id);
        }
        self.feedback.set_runtime_message("Closed other queries");
    }

    pub(crate) fn close_documents_to_right(&mut self, index: usize) {
        if index >= self.query_session.documents.len() {
            return;
        }
        if self
            .query_session
            .documents
            .iter()
            .skip(index + 1)
            .any(|document| self.execution.document_is_busy(&document.id))
        {
            self.feedback
                .set_runtime_message("Commit or rollback the open transaction before closing this query");
            return;
        }
        if self.query_session.pending_bulk_close == Some(PendingBulkClose::ToRight(index)) {
            self.query_session.pending_bulk_close = None;
        } else if self.bulk_close_has_dirty(|query_index| query_index > index) {
            self.query_session.pending_bulk_close = Some(PendingBulkClose::ToRight(index));
            return;
        }
        let removed: Vec<String> = self
            .query_session
            .documents
            .iter()
            .skip(index + 1)
            .map(|document| document.id.clone())
            .collect();
        for query_index in index + 1..self.query_session.documents.len() {
            self.cancel_prediction(query_index);
            let id = self.query_session.documents[query_index].id.clone();
            self.cancel_document_requests(&id);
        }
        self.query_session.close_documents_to_right(index);
        for document_id in removed {
            self.execution.forget_document(&document_id);
        }
        self.feedback.set_runtime_message("Closed queries to the right");
    }

    pub(crate) fn close_all_tabs(&mut self) {
        if self.execution.any_transaction_busy() {
            self.feedback
                .set_runtime_message("Commit or rollback the open transaction before closing this query");
            return;
        }
        if self.query_session.pending_bulk_close == Some(PendingBulkClose::All) {
            self.query_session.pending_bulk_close = None;
        } else if self.bulk_close_has_dirty(|_| true) {
            self.query_session.pending_bulk_close = Some(PendingBulkClose::All);
            return;
        }
        let removed: Vec<String> = self
            .query_session
            .documents
            .iter()
            .map(|document| document.id.clone())
            .collect();
        for index in 0..self.query_session.documents.len() {
            self.cancel_prediction(index);
            let id = self.query_session.documents[index].id.clone();
            self.cancel_document_requests(&id);
        }
        self.workspace.welcome_open = true;
        self.workspace.diagram_open = false;
        self.workspace.results_open = false;
        self.query_session
            .replace_with_document(QueryDocument::new("query-1", "Query 1", String::new()));
        self.schema_explorer.selected_table = None;
        self.schema_explorer.selected_schema_object = None;
        self.workspace.active_tab = WorkspaceTab::Welcome;
        for document_id in removed {
            self.execution.forget_document(&document_id);
        }
        self.feedback.set_runtime_message("Closed all tabs");
    }

    pub(crate) fn close_welcome_tab(&mut self) {
        self.workspace.welcome_open = false;
        if self.workspace.active_tab == WorkspaceTab::Welcome {
            self.activate_fallback_surface();
        }
        self.feedback.set_runtime_message("Closed Welcome");
    }

    pub(crate) fn activate_welcome_tab(&mut self) {
        self.workspace.welcome_open = true;
        self.workspace.active_tab = WorkspaceTab::Welcome;
    }

    /// Free one slot before a new document is pushed (#401). The oldest clean,
    /// idle tab is closed through the normal teardown path; when every tab is
    /// dirty or running something, the open is refused with feedback instead
    /// of silently dropping a buffer.
    fn evict_oldest_document_if_capped(&mut self) -> bool {
        if self.query_session.documents.len() < query_state::MAX_QUERY_DOCUMENTS {
            return true;
        }
        let Some(index) = self
            .query_session
            .documents
            .iter()
            .position(|document| !document.is_dirty() && !self.execution.document_is_busy(&document.id))
        else {
            self.feedback.set_runtime_message(format!(
                "Too many query tabs open ({}) — close or save a tab before opening another",
                query_state::MAX_QUERY_DOCUMENTS
            ));
            return false;
        };
        self.close_document(index);
        true
    }

    /// Push a document assembled by the caller (e.g. a workspace `.sql` file),
    /// subject to the same cap as other open paths (#401).
    pub(crate) fn push_document_capped(&mut self, document: QueryDocument) -> bool {
        if !self.evict_oldest_document_if_capped() {
            return false;
        }
        self.query_session.add_document(document);
        true
    }

    fn next_document_identity(&self) -> (String, usize) {
        let mut number = self.query_session.documents.len().saturating_add(1);
        loop {
            let id = format!("query-{number}");
            if !self.query_session.documents.iter().any(|document| document.id == id) {
                return (id, number);
            }
            number = number.saturating_add(1);
        }
    }

    fn activate_query_surface(&mut self) {
        self.query_editor.query_focus_editor_on_open = true;
        self.reset_cursor();
        self.workspace.activity = Activity::Queries;
        self.workspace.sidebar_open = true;
        self.workspace.active_tab = WorkspaceTab::Query;
    }

    pub(crate) fn reset_cursor(&mut self) {
        self.query_editor.query_cursor_line = 1;
        self.query_editor.query_cursor_column = 1;
    }

    fn sync_active_cursor_and_selection(&mut self) {
        let Some(document) = self.query_session.active_document() else {
            self.reset_cursor();
            return;
        };
        self.query_editor.query_cursor_line = document.cursor.line + 1;
        self.query_editor.query_cursor_column = document.cursor.col + 1;
        if !document.selection.is_empty() {
            let (start, end) = document.selection.normalized();
            self.query_session.selected_text = document.buffer.slice(start, end).to_owned();
        } else {
            self.query_session.selected_text.clear();
        }
    }

    fn cancel_prediction(&mut self, index: usize) {
        let request_id = self.query_session.invalidate_prediction(index);
        if let Some(request_id) = request_id {
            let _ = self
                .command_dispatcher
                .send_best_effort(UiCommand::CancelSqlPrediction { request_id });
        }
    }

    fn cancel_agent_run(&mut self, document_id: &str) {
        let Some(run_id) = self
            .agent
            .sessions
            .get(document_id)
            .and_then(|session| session.active_run_id)
        else {
            self.agent.sessions.remove(document_id);
            return;
        };
        let request_id = self.command_dispatcher.next_request_id();
        let _ = self
            .command_dispatcher
            .send_best_effort(UiCommand::CancelAgentRun { request_id, run_id });
        self.agent.sessions.remove(document_id);
    }

    fn activate_fallback_surface(&mut self) {
        if !self.query_session.documents.is_empty() {
            self.workspace.active_tab = WorkspaceTab::Query;
        } else if self.schema_explorer.selected_table.is_some() {
            self.workspace.active_tab = WorkspaceTab::Table;
        } else if self.schema_explorer.selected_schema_object.is_some() {
            self.workspace.active_tab = WorkspaceTab::SchemaObject;
        } else {
            self.activate_welcome_tab();
        }
    }

    /// Cancels every in-flight runtime request owned by `document_id` (query
    /// execution or explain) before the document is dropped or disconnected.
    fn cancel_document_requests(&mut self, document_id: &str) {
        if let Some(document) = self
            .query_session
            .documents
            .iter()
            .find(|document| document.id == document_id)
        {
            if let QueryExecutionState::Running(request_id) = document.execution_state {
                let _ = self
                    .command_dispatcher
                    .send_best_effort(UiCommand::CancelQuery { request_id });
            }
            if let Some(request_id) = document.explain_request {
                let _ = self
                    .command_dispatcher
                    .send_best_effort(UiCommand::CancelQuery { request_id });
            }
        }
    }

    /// True when any document matching `target` has unsaved text — the bulk
    /// closes gate on this so they can't drop SQL the dirty-close dialog
    /// would have protected for a single tab.
    fn bulk_close_has_dirty(&self, target: impl Fn(usize) -> bool) -> bool {
        self.query_session
            .documents
            .iter()
            .enumerate()
            .any(|(index, document)| target(index) && document.is_dirty())
    }
}

impl DbProApp {
    fn query_document_context(&mut self) -> QueryDocumentContext<'_, '_> {
        let active_connection_id = self.connection.lifecycle.active_connection_id().map(str::to_owned);
        let active_schema = self.active_schema().to_owned();
        QueryDocumentContext {
            query_session: &mut self.query.session,
            query_editor: &mut self.query.editor,
            workspace: &mut self.workspace,
            agent: &mut self.agent,
            query_output: &mut self.query.output,
            schema_explorer: &mut self.schema.explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge),
            feedback: &mut self.feedback,
            execution: &mut self.query.execution,
            active_connection_id,
            active_schema,
        }
    }

    pub(crate) fn new_query_document(&mut self) {
        self.query_document_context().new_document(false);
    }

    /// Disposable scratch tab for throwaway SQL (#211).
    pub(crate) fn new_scratch_query_document(&mut self) {
        self.query_document_context().new_document(true);
    }

    /// Cycle a simple numbered rename for the open query tab (#211).
    pub(crate) fn rename_query_document_inline(&mut self, index: usize) {
        self.query_document_context().rename_document_inline(index);
    }

    pub(crate) fn switch_query_document(&mut self, index: usize) {
        self.query_document_context().switch_document(index);
    }

    pub(crate) fn set_active_query_text(&mut self, text: impl Into<String>) {
        self.query_document_context().set_active_text(text);
    }

    pub(crate) fn append_to_active_query(&mut self, text: &str) {
        self.query_document_context().append_active_text(text);
    }

    pub(crate) fn set_document_connection(&mut self, index: usize, connection_id: Option<String>) {
        self.query_document_context()
            .set_document_connection(index, connection_id);
    }

    pub(crate) fn set_document_schema(&mut self, index: usize, schema: Option<String>) {
        self.query_document_context().set_document_schema(index, schema);
    }

    /// Open `text` in a new query document bound to `connection_id` (or the
    /// active connection when `None`). Use this for every "Open in SQL editor"
    /// surface — never overwrite the active document's unsaved SQL (#419/#420).
    pub(crate) fn open_query_document_with(&mut self, text: impl Into<String>, connection_id: Option<String>) {
        self.query_document_context().open_text_document(text, connection_id);
    }

    /// Push a caller-assembled document (workspace file, etc.) through the
    /// same cap eviction as the other open paths (#401).
    pub(crate) fn push_query_document(&mut self, document: QueryDocument) -> bool {
        self.query_document_context().push_document_capped(document)
    }


    pub(crate) fn cancel_prediction_for_document(&mut self, index: usize) {
        self.query_document_context().cancel_prediction_for_document(index);
    }

    pub(crate) fn open_history_entry(&mut self, entry: &UiQueryHistoryEntry, run: bool) {
        {
            self.query_document_context().open_history_entry(entry);
        }
        if run {
            self.dispatch_query();
        }
    }

    pub(crate) fn close_query_document(&mut self, index: usize) {
        self.query_document_context().close_document(index);
    }

    pub(crate) fn request_close_query_document(&mut self, index: usize) {
        self.query_document_context().request_close_document(index);
    }

    pub(crate) fn duplicate_query_document(&mut self, index: usize) {
        self.query_document_context().duplicate_document(index);
    }

    pub(crate) fn close_other_query_documents(&mut self, keep_index: usize) {
        self.query_document_context().close_other_documents(keep_index);
    }

    pub(crate) fn close_query_documents_to_right(&mut self, index: usize) {
        self.query_document_context().close_documents_to_right(index);
    }

    pub(crate) fn close_all_tabs(&mut self) {
        self.query_document_context().close_all_tabs();
    }

    pub(crate) fn close_welcome_tab(&mut self) {
        self.query_document_context().close_welcome_tab();
    }

    pub(crate) fn activate_welcome_tab(&mut self) {
        self.query_document_context().activate_welcome_tab();
    }

    pub(super) fn reset_query_cursor(&mut self) {
        self.query_document_context().reset_cursor();
    }

    pub(super) fn execute_pending_navigation(&mut self, action: PendingNavigationAction) {
        match action {
            PendingNavigationAction::OpenTable { schema, table } => self.open_table_in_schema(schema, table),
            PendingNavigationAction::ChangeSchema(schema) => {
                super::schema_explorer_state::SchemaActivationContext::new(
                    &mut self.schema.explorer,
                    &mut self.table,
                    &mut self.workspace,
                    &mut self.feedback,
                )
                .activate(&schema);
            }
            PendingNavigationAction::ChangeConnection(connection_id) => {
                let connection = self.connection.catalog.find(&connection_id).cloned();
                if let Some(connection) = connection {
                    self.connect_to_connection(&connection);
                }
            }
            PendingNavigationAction::Disconnect(connection_id) => {
                let connection = self.connection.catalog.find(&connection_id).cloned();
                if let Some(connection) = connection {
                    self.disconnect_from_connection(&connection);
                }
            }
            PendingNavigationAction::OpenReferencedRow {
                connection_id,
                schema,
                table,
                filters,
            } => {
                self.schema.explorer.selected_schema = Some(schema.clone());
                self.schema.explorer.selected_table = Some(table.clone());
                self.workspace.active_tab = WorkspaceTab::Table;
                self.table.state.table_view = TableView::Data;
                let request_id = self.next_request_id();
                let _ = self.dispatch_command(table_data_view::build_load_row_command(
                    request_id,
                    table_data_view::TableDataTarget {
                        connection_id,
                        schema,
                        table,
                    },
                    filters,
                ));
            }
            PendingNavigationAction::CloseWorkspace(tab) => self.request_close_workspace_tab(tab),
        }
    }
}

#[cfg(test)]
#[path = "tests/query_documents_tests.rs"]
mod tests;

