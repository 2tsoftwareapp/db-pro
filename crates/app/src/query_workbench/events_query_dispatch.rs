// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Keyboard shortcuts and query dispatch / destructive-run gate.
use super::query_editor_support::close_completion;
use super::*;

/// Parameters shared by query dispatch and the destructive-confirm hold path.
/// Keeping the executed range/version, binding, and injected cap together
/// prevents confirmation from dropping query metadata.
pub(super) struct QueryRunOptions {
    pub(super) execution_range: (usize, usize),
    pub(super) version: u64,
    pub(super) all_statements: bool,
    pub(super) bound_params: Option<Vec<db_pro_core::domain::query::QueryParam>>,
    pub(super) applied_row_limit: Option<u64>,
}

impl DbProApp {
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.shortcuts_blocked(ctx) {
            return;
        }
        // `text_edit_focused` (not `egui_wants_keyboard_input`) — the latter is
        // true for *any* focused widget, which killed palette/sidebar chords
        // after clicking a button or grid cell (#340). Shortcuts that target
        // the query editor must not fire while an egui text field elsewhere
        // (WHERE filter, agent composer, dialogs) is typing (#379); the custom
        // SqlEditor is not a TextEdit, so editor chords still fire there.
        let egui_text_focused = ctx.text_edit_focused();
        if !egui_text_focused {
            if self.handle_document_shortcuts(ctx) {
                return;
            }
            // Run-all is checked before run so a shifted chord is not also a
            // plain run.
            if self.handle_catalog_shortcuts(ctx) {
                return;
            }
        }
        // Only an egui TextEdit (WHERE filter, agent composer, dialog) typing
        // text blocks global chords. The custom SqlEditor is our own surface:
        // Focus it holds must not deaden palette/sidebar/new-connection
        // shortcuts, otherwise every chord stops working once the user clicks
        // into the editor (#340 covers the reverse direction).
        let text_input_has_focus = egui_text_focused;
        if self.handle_unfocused_shortcuts(ctx, text_input_has_focus) {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.handle_escape(ctx);
        }
    }

    fn shortcuts_blocked(&self, ctx: &egui::Context) -> bool {
        // Overlay flags that exist outside the modal registry still gate
        // here; any *rendered* Dialog/Sheet is detected via modal_guard so
        // global chords and Esc can't reach the surface behind a modal.
        // A live cell editor (or its calendar/FK popup) owns Esc/locals
        // first — letting the global handler see the same keypress cancels
        // running queries behind the user's back (#372). The WHERE
        // completion popup consumes Esc itself before its editor draws, so
        // it must also preempt the global fallback.
        self.palette.mode.is_some()
            || self.connection.dialog.is_open()
            || self.overlay.connection_delete_confirmation_id.is_some()
            || self.overlay.folder_delete_confirmation.is_some()
            || self.table.editing.insert_row_open
            || self.table.editing.data_editing_cell.is_some()
            || self.table.data_query.sql_condition_suggestions_open
            || egui::Popup::is_any_open(ctx)
            || crate::components::dialog::modal_guard::any_open(ctx)
    }

    fn handle_document_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if ctx.input(|input| self.shortcut_pressed(input, "query.save_as")) {
            self.open_save_as_dialog();
            return true;
        }
        if ctx.input(|input| self.shortcut_pressed(input, "query.save")) {
            self.save_query_document_at(self.query.session.active_document_index);
            return true;
        }
        false
    }

    fn handle_unfocused_shortcuts(&mut self, ctx: &egui::Context, text_input_has_focus: bool) -> bool {
        // Debug-build developer tools toggle; must work even while a text field
        // holds focus so the inspector can be closed from anywhere.
        #[cfg(debug_assertions)]
        if ctx.input(|i| settings_model::match_shortcut_token(i, "mod+shift+i")) {
            self.dev_tools.open = !self.dev_tools.open;
            return true;
        }
        if text_input_has_focus {
            return false;
        }
        if ctx.input(|i| self.shortcut_pressed(i, "palette.commands")) {
            self.palette.open(PaletteMode::Commands);
            return true;
        }
        if ctx.input(|i| {
            self.shortcut_pressed(i, "palette.quick_open_alt")
                || self.shortcut_pressed(i, "palette.quick_open")
        }) {
            self.palette.open(PaletteMode::QuickOpen);
            return true;
        }
        if ctx.input(|i| self.shortcut_pressed(i, "view.toggle_sidebar")) {
            self.workspace.sidebar_open = !self.workspace.sidebar_open;
        }
        if ctx.input(|i| self.shortcut_pressed(i, "connection.new")) {
            self.connection.open_new();
            return true;
        }
        false
    }

    /// Commands the catalog owns. They run while the editor is focused, and a
    /// rebinding replaces the old chord instead of leaving a hardcoded one behind.
    fn handle_catalog_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.press_catalog(ctx, "query.run_all") {
            self.dispatch_query_all();
            return true;
        }
        if self.press_catalog(ctx, "query.format") {
            self.format_focused_query();
            return true;
        }
        if self.press_catalog(ctx, "query.run") || f5_pressed(ctx) {
            // F5 is the conventional Run key and stays as a hardcoded alias of
            // the `query.run` command (whose catalog default is `mod+enter`).
            self.dispatch_query();
            return true;
        }
        if self.press_catalog(ctx, "query.new") {
            self.new_query_document();
            self.workspace.active_tab = WorkspaceTab::Query;
            return true;
        }
        if self.press_catalog(ctx, "editor.find") {
            self.query.editor.editor_search_open = true;
            return true;
        }
        if self.press_catalog(ctx, "table.open") {
            self.open_catalog_table();
            return true;
        }
        if self.press_catalog(ctx, "result.focus") {
            self.focus_result_grid(ctx);
            return true;
        }
        if self.press_catalog(ctx, "editor.comment") {
            self.toggle_focused_comment();
            return true;
        }
        false
    }

    /// SQL dialect for editor features. `numbered_parameters` is a placeholder-style
    /// capability, not a dialect — MySQL/SQL Server would be mis-formatted as SQLite.
    pub(crate) fn editor_dialect(&self) -> crate::editor::SqlDialect {
        let driver = self.active_driver();
        if driver.eq_ignore_ascii_case("postgresql") || driver.eq_ignore_ascii_case("postgres") {
            crate::editor::SqlDialect::Postgres
        } else if driver.eq_ignore_ascii_case("sqlite") {
            crate::editor::SqlDialect::SQLite
        } else {
            crate::editor::SqlDialect::Generic
        }
    }

    fn toggle_focused_comment(&mut self) {
        let dialect = self.editor_dialect();
        let index = self.query.session.active_document_index;
        if let Some(document) = self.query.session.documents.get_mut(index) {
            query_view::toggle_query_comment(document, dialect);
        }
    }

    /// Opens the query output dock and moves keyboard focus onto the result grid.
    fn focus_result_grid(&mut self, ctx: &egui::Context) {
        self.workspace.bottom_panel_open = true;
        self.workspace.active_tab = WorkspaceTab::Query;
        let document_id = self
            .query
            .session
            .active_document()
            .map(|document| document.id.clone());
        self.query
            .output
            .set_active_for_optional_document(document_id.as_deref(), OutputTab::Results);
        self.query.editor.query_editor_focused = false;
        if let Some(id) = result_grid_keyboard_view::query_result_grid_focus_id(ctx) {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }

    /// Opens the explorer's selected table through the palette Open Table path.
    /// With nothing selected, quick open is limited to schema objects so the
    /// chosen row is still that path.
    fn open_catalog_table(&mut self) {
        if let Some(table) = self.schema.explorer.selected_table.clone() {
            self.open_table_from_palette(table);
            return;
        }
        self.palette
            .open_with_scope(PaletteMode::QuickOpen, SearchScope::Schema);
    }

    fn press_catalog(&self, ctx: &egui::Context, command_id: &str) -> bool {
        let token = self.preferences.settings.keybindings.resolved(command_id);
        let pressed = ctx.input(|input| settings_model::match_shortcut_token(input, &token));
        if !pressed {
            return false;
        }
        ctx.input_mut(|input| {
            input.events.retain(|event| !settings_model::event_matches_shortcut(event, &token));
        });
        true
    }

    fn format_focused_query(&mut self) {
        let dialect = self.editor_dialect();
        let index = self.query.session.active_document_index;
        let changed = self
            .query
            .session
            .documents
            .get_mut(index)
            .map(|document| query_view::format_query_document(document, dialect));
        if let Some(changed) = changed {
            self.feedback.set_runtime_message(if changed {
                "SQL formatted"
            } else {
                "Already well formatted"
            });
        }
    }

    fn handle_escape(&mut self, ctx: &egui::Context) {
        if let Some(request_id) = self.query.session.active_running_request() {
            if self.query_capabilities().allows(|c| c.query.cancel) {
                self.cancel_query(request_id);
            } else {
                self.feedback.runtime_message = "Query cancellation is not supported for this provider".to_owned();
            }
        } else if let Some(request_id) = self.table.data_query.inline_query_request {
            if self.query_capabilities().allows(|c| c.query.cancel) {
                self.cancel_query(request_id);
            } else {
                self.feedback.runtime_message = "Query cancellation is not supported for this provider".to_owned();
            }
        } else if self.query.editor.query_tools_open {
            self.query.editor.query_tools_open = false;
        } else if self.query.editor.editor_search_open {
            self.query.editor.editor_search_open = false;
        } else if self.workspace.agent_open && ctx.memory(|m| m.focused().is_none()) {
            // Esc only dismisses the agent panel when no widget holds the
            // keyboard — Esc while typing in the editor or composer is a local
            // concern and must not close the panel (#349).
            self.set_agent_open(false, ctx);
        }
    }

    fn shortcut_pressed(&self, input: &egui::InputState, command_id: &str) -> bool {
        let token = self.preferences.settings.keybindings.resolved(command_id);
        settings_model::match_shortcut_token(input, &token)
    }

    pub(super) fn cancel_query(&mut self, request_id: crate::RequestId) {
        if self.dispatch_command(UiCommand::CancelQuery { request_id }) {
            self.feedback.runtime_message = "Cancelling query…".to_owned();
        }
    }

    pub(super) fn dispatch_query(&mut self) -> bool {
        self.dispatch_query_inner(None)
    }

    /// Dispatch the active document with pre-bound typed arguments (#252).
    /// The routine workbench generates placeholder-only SQL; its typed
    /// `QueryParam` vector rides the same `RunQuery` command the editor uses,
    /// bypassing editor parameter discovery.
    pub(super) fn dispatch_bound_query(&mut self, params: Vec<db_pro_core::domain::query::QueryParam>) -> bool {
        self.dispatch_query_inner(Some(params))
    }

    fn dispatch_query_inner(&mut self, bound_params: Option<Vec<db_pro_core::domain::query::QueryParam>>) -> bool {
        if let Some(doc) = self.query.session.documents.get_mut(self.query.session.active_document_index) {
            close_completion(doc);
        }
        if self.query.session.active_running_request().is_some() {
            return false;
        }
        let Some(connection_id) = self
            .active_query_connection_id()
            .map(String::from)
            .or_else(|| self.active_connection().map(|connection| connection.id.clone()))
        else {
            self.feedback.runtime_message = "Create or select a connection first".to_owned();
            return false;
        };
        let (sql, execution_range) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|doc| doc.resolve_executable_range())
            .unwrap_or_else(|| {
                (
                    self.query.session.active_text().trim().to_owned(),
                    (0, self.query.session.active_text().len()),
                )
            });
        if sql.trim().is_empty() {
            self.feedback.runtime_message = "Query is empty".to_owned();
            return false;
        }
        // The toolbar row cap only guards interactive editor runs — saved
        // tasks and other send_query_run callers keep their exact text.
        let applied_limit =
            query_row_limit::effective_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let sql = query_row_limit::apply_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let options = QueryRunOptions {
            execution_range,
            version: self.query.session.active_buffer_version(),
            all_statements: false,
            bound_params,
            applied_row_limit: applied_limit,
        };
        if self.hold_destructive_run_bound(&sql, PendingDestructiveRunOrigin::QueryEditor, &options) {
            return false;
        }
        if self.cross_tab_transaction_blocked(&connection_id) {
            return false;
        }
        self.send_query_run(connection_id, sql, options).is_some()
    }

    /// A transaction is per-connection on the server: refuse a run from a
    /// *different* tab while another document on this connection holds one
    /// open (#423). The owning tab still runs statements inside its txn.
    fn cross_tab_transaction_blocked(&mut self, connection_id: &str) -> bool {
        let active_doc_id = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|doc| doc.id.clone());
        let Some(owner) = self.query.execution.transaction_owner() else {
            return false;
        };
        if Some(owner) == active_doc_id.as_deref() {
            return false;
        }
        let owner_conn = self
            .query
            .session
            .documents
            .iter()
            .find(|doc| doc.id == owner)
            .and_then(|doc| doc.connection_id.clone());
        if owner_conn.as_deref() == Some(connection_id) || owner_conn.is_none() {
            self.feedback.runtime_message =
                "Another query tab holds an open transaction on this connection".to_owned();
            return true;
        }
        false
    }

    pub(super) fn dispatch_query_all(&mut self) {
        if self.query.session.active_running_request().is_some() {
            return;
        }
        let Some(connection_id) = self
            .active_query_connection_id()
            .map(String::from)
            .or_else(|| self.active_connection().map(|connection| connection.id.clone()))
        else {
            self.feedback.runtime_message = "Create or select a connection first".to_owned();
            return;
        };
        let (sql, execution_range) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|doc| {
                let text = doc.text().trim().to_owned();
                let leading = doc.text().len().saturating_sub(doc.text().trim_start().len());
                (text.clone(), (leading, leading + text.len()))
            })
            .unwrap_or_else(|| {
                (
                    self.query.session.active_text().trim().to_owned(),
                    (0, self.query.session.active_text().len()),
                )
            });
        if sql.is_empty() {
            self.feedback.runtime_message = "Query is empty".to_owned();
            return;
        }
        if let Some(doc) = self
            .query
            .session
            .documents
            .get_mut(self.query.session.active_document_index)
        {
            close_completion(doc);
        }
        let applied_limit =
            query_row_limit::effective_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let sql = query_row_limit::apply_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let options = QueryRunOptions {
            execution_range,
            version: self.query.session.active_buffer_version(),
            all_statements: true,
            bound_params: None,
            applied_row_limit: applied_limit,
        };
        if self.hold_destructive_run(&sql, PendingDestructiveRunOrigin::QueryEditor, &options) {
            return;
        }
        if self.cross_tab_transaction_blocked(&connection_id) {
            return;
        }
        self.send_query_run(connection_id, sql, options);
    }

    /// Hold a statement or script the classifier rates `Destructive` until the user
    /// confirms the exact text, and report whether the run was held.
    ///
    /// The query editor is the documented path for arbitrary SQL (including DDL), so
    /// nothing is refused here: the text is kept in `pending_destructive_run` and sent by
    /// `confirm_pending_destructive_run`. Reads, writes and plain DDL dispatch exactly as
    /// before, and the backend policy still runs on whatever is dispatched.
    pub(super) fn hold_destructive_run(
        &mut self,
        sql: &str,
        origin: PendingDestructiveRunOrigin,
        options: &QueryRunOptions,
    ) -> bool {
        self.hold_destructive_run_bound(sql, origin, options)
    }

    /// `hold_destructive_run` with pre-bound typed arguments (#252): a confirmed
    /// routine call must execute exactly the typed arguments the preview showed.
    pub(super) fn hold_destructive_run_bound(
        &mut self,
        sql: &str,
        origin: PendingDestructiveRunOrigin,
        options: &QueryRunOptions,
    ) -> bool {
        let QueryRunOptions {
            execution_range,
            version,
            all_statements,
            bound_params,
            applied_row_limit,
        } = options;
        // Resolve the connection the prompt names at hold time: confirming later may look
        // at a different active connection, and the dialog must show what will execute.
        let target_label = match origin {
            PendingDestructiveRunOrigin::InlineQuery => self.active_connection_name().to_owned(),
            PendingDestructiveRunOrigin::QueryEditor | PendingDestructiveRunOrigin::ExplainAnalyze => {
                let schema = self.active_schema();
                if schema.is_empty() {
                    self.active_connection_name().to_owned()
                } else {
                    format!("{} · {schema}", self.active_connection_name())
                }
            }
        };
        self.query_execution_context().hold_destructive_run(
            sql,
            origin,
            target_label,
            super::query_execution_state::PendingDestructiveRunMetadata::new(
                *execution_range,
                *version,
                *all_statements,
            )
            .with_row_limit(*applied_row_limit),
            bound_params.clone(),
        )
    }

    /// Send the statement the user confirmed. The text and the buffer version are the ones
    /// the prompt displayed, so a confirmation can never execute something the user did
    /// not see.
    pub(super) fn confirm_pending_destructive_run(&mut self) {
        // Peek before taking: without a connection the pending run must stay held so the
        // statement is not silently lost.
        let Some(pending) = self.query.execution.pending_destructive_run() else {
            return;
        };
        let origin = pending.origin();
        let sql = pending.sql().to_owned();
        let options = QueryRunOptions {
            execution_range: pending.execution_range(),
            version: pending.version(),
            all_statements: pending.all_statements(),
            bound_params: pending.bound_params().map(<[db_pro_core::domain::query::QueryParam]>::to_vec),
            applied_row_limit: pending.row_limit(),
        };
        let connection_id = match origin {
            PendingDestructiveRunOrigin::InlineQuery => self
                .active_connection()
                .map(|connection| connection.id.clone()),
            PendingDestructiveRunOrigin::QueryEditor | PendingDestructiveRunOrigin::ExplainAnalyze => self
                .active_query_connection_id()
                .map(String::from)
                .or_else(|| self.active_connection().map(|connection| connection.id.clone())),
        };
        let Some(connection_id) = connection_id else {
            self.feedback.runtime_message = "Create or select a connection first".to_owned();
            return;
        };
        if self.query_execution_context().take_pending_destructive_run().is_none() {
            return;
        }
        self.table.data_query.pending_inline_query_confirmation = false;
        // Route by the origin the hold was created with — the confirm target cannot drift
        // to whichever flag happened to be set at confirm time.
        match origin {
            PendingDestructiveRunOrigin::InlineQuery => {
                self.send_table_data_query_run(connection_id, sql);
            }
            PendingDestructiveRunOrigin::ExplainAnalyze => {
                self.send_explain_run(connection_id, sql, true);
            }
            PendingDestructiveRunOrigin::QueryEditor => {
                self.send_query_run(connection_id, sql, options);
            }
        }
    }

    /// Drop a held destructive statement without executing it.
    pub(super) fn cancel_pending_destructive_run(&mut self) {
        self.table.data_query.pending_inline_query_confirmation = false;
        self.query_execution_context().cancel_pending_destructive_run();
    }

    pub(super) fn send_query_run(
        &mut self,
        connection_id: String,
        sql: String,
        options: QueryRunOptions,
    ) -> Option<crate::RequestId> {
        let QueryRunOptions {
            execution_range,
            version,
            all_statements,
            bound_params,
            applied_row_limit,
        } = options;
        let request_id = self.next_request_id();
        let mut command = match bound_params {
            // Pre-bound typed arguments (routine workbench): the SQL already holds
            // provider placeholders, so editor discovery must not re-scan it (#252).
            Some(params) => self
                .query_execution_context()
                .prepare_bound_run(request_id, connection_id, sql, params, all_statements),
            None => self
                .query_execution_context()
                .prepare_query_run(request_id, connection_id, sql, all_statements)?,
        };
        command.session_id = self.query.session.active_document().map(|document| document.id.clone());
        let runtime_command = query_run_command(&command);
        if self.dispatch_command(runtime_command) {
            self.query_execution_context()
                .commit_dispatched(&command, execution_range, version, applied_row_limit);
            if let Some(document_id) = command.session_id.clone() {
                self.query
                    .execution
                    .note_transaction_dispatch(&document_id, request_id, &command.sql);
            }
            Some(request_id)
        } else {
            None
        }
    }

    /// Dispatch `ExplainQuery` for an EXPLAIN ANALYZE the user just confirmed through the
    /// destructive-run dialog. Skips `prepare` — the pending run already proved the gate.
    pub(crate) fn send_explain_run(&mut self, connection_id: String, sql: String, analyze: bool) -> bool {
        let request = query_explain_actions::PreparedExplainRequest {
            connection_id,
            sql,
            params: Vec::new(),
            analyze,
            analyze_executes: true,
            document_index: self.query.session.active_document_index,
        };
        let request_id = self.next_request_id();
        let command = UiCommand::ExplainQuery {
            request_id,
            connection_id: request.connection_id.clone(),
            sql: request.sql.clone(),
            params: request.params.clone(),
            analyze: request.analyze,
            schema: self.query.session.active_schema().map(str::to_owned),
            session_id: self.query.session.active_document().map(|document| document.id.clone()),
        };
        if !self.dispatch_command(command) {
            return false;
        }
        if let Some(document) = self.query.session.documents.get_mut(request.document_index) {
            document.explain_request = Some(request_id);
            document.explain_plan = None;
        }
        self.query.execution.pending_explain_analyze = false;
        self.query.execution.explain_analyze_confirmed = false;
        self.query.output.set_active_for_optional_document(
            self.query.session.active_document().map(|document| document.id.as_str()),
            OutputTab::Explain,
        );
        self.feedback
            .set_runtime_message("EXPLAIN ANALYZE running (query executes)…");
        true
    }

    pub(crate) fn send_table_data_query_run(&mut self, connection_id: String, sql: String) -> bool {
        let applied_limit =
            query_row_limit::effective_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let repeat_sql = sql.clone();
        let sql = query_row_limit::apply_row_limit(&sql, self.query.execution.query_row_limit, self.active_driver());
        let request_id = self.next_request_id();
        let Some(command) = self
            .query_execution_context()
            .prepare_query_run(request_id, connection_id, sql.clone(), false)
        else {
            return false;
        };
        if self.dispatch_command(query_run_command(&command)) {
            self.query_execution_context().commit_inline_dispatched(&command);
            self.table.data_query.begin_inline_query(request_id, repeat_sql, applied_limit);
            self.table.data_query.error = None;
            true
        } else {
            // dispatch already toasted; keep the failure visible inside the
            // inline-query pane the button press belongs to (#407).
            self.table.data_query.error = Some("Could not send the query — runtime worker unavailable".to_owned());
            false
        }
    }

    fn query_execution_context(&mut self) -> query_execution_actions::QueryExecutionContext<'_> {
        let driver = self.active_driver().to_owned();
        query_execution_actions::QueryExecutionContext::new(
            &mut self.query.session,
            &mut self.query.editor,
            &mut self.query.execution,
            &mut self.feedback,
            driver,
        )
    }
}

fn query_run_command(prepared: &query_execution_actions::PreparedQueryRun) -> UiCommand {
    if prepared.all_statements {
        UiCommand::RunQueryMulti {
            request_id: prepared.request_id,
            connection_id: prepared.connection_id.clone(),
            sql: prepared.sql.clone(),
            schema: prepared.schema.clone(),
            session_id: prepared.session_id.clone(),
        }
    } else {
        UiCommand::RunQuery {
            request_id: prepared.request_id,
            connection_id: prepared.connection_id.clone(),
            sql: prepared.sql.clone(),
            params: prepared.params.clone(),
            schema: prepared.schema.clone(),
            session_id: prepared.session_id.clone(),
        }
    }
}

/// F5 fires `query.run` regardless of the catalog binding — it is the
/// platform-conventional Run chord and carries no modifiers.
fn f5_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::F5) && i.modifiers.is_none())
}
