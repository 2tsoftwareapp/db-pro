use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

use agent_workflow_state::AgentUiSession;
use change_set::{ChangeSet, MutationFailure, MutationTarget, RowIdentity, StagedChange};
use eframe::egui::{self, Align, FontId, Layout, RichText, Sense};
use lucide_icons::Icon;

use self::connection::ConnectionFeatureState;
pub(crate) use self::connection::{ConnectionCatalogState, ConnectionDialogState, ConnectionLifecycleState};
use crate::{
    agent::{AgentContext, AgentMessage, AgentProvider, AgentRole, OfflineAgentProvider},
    policy::{ColumnWriteBlock, ColumnWritePolicy},
    result_grid::{GridProjectionCache, GridProjectionKey},
};
use db_pro_ui::{
    components::*, editor::PredictionMode, editor_frame, grid_frame, icon_text, query::SchemaSymbolIndex,
    section_label, sidebar_item, tokens::*, toolbar_frame, DbProTheme, RequestId, TaskBridge, UiCell, UiCommand,
    UiConnectionDraft, UiConnectionSummary, UiDriver, UiEvent, UiFunctionSummary, UiQueryExecutionOutput,
    UiQueryFolderSummary, UiQueryHistoryEntry, UiQueryHistoryStatus, UiQueryResult, UiSavedQuerySummary,
    UiSchemaForeignKey, UiSchemaSummary, UiStatementOutput, UiTableDataFilter, UiTableDataSort, UiTableFilterOperator,
    UiTableInfo, UiTableMutation, UiTableSummary, UiTriggerSummary, UiViewSummary,
};

include!("app_modules.rs");
pub struct DbProApp {
    theme: DbProTheme,
    preferences: PreferencesState,
    workspace: WorkspaceFeatureState,
    query: QueryFeatureState,
    palette: PaletteState,
    agent: AgentState,
    task_bridge: TaskBridge,
    feedback: FeedbackState,
    table: TableEditorState,
    overlay: OverlayState,
    connection: ConnectionFeatureState,
    schema: SchemaWorkspaceState,
    management: DatabaseManagementState,
    saved_tasks: SavedTaskState,
    /// Installed by the native shell. Copy refuses to run when this is absent.
    table_copy: Option<std::sync::Arc<dyn db_pro_core::application::TableCopyConnector>>,
    /// Frames rendered so far; drives first-frame-only shell setup.
    ///
    /// #573: this used to re-send `Maximized(true)` for the first three frames,
    /// which overrode the window frame eframe had just restored.
    initial_frames_count: u8,
    gallery_state: component_gallery_view::ComponentGalleryState,
    #[cfg(debug_assertions)]
    dev_tools: crate::dev_tools::DevToolsState,
    /// Set by `mark_capture_provenance` when a capture fixture opener ran —
    /// a field, not an env var, so the review gate reads real provenance.
    capture_evidence_seeded: bool,
}

// CapabilityLookup lives in `capability_lookup.rs`.

impl DbProApp {
    /// True when a live database session exists — used by the visual-review
    /// privacy gate to keep screenshots of real data from leaving the app.
    pub fn has_live_connection(&self) -> bool {
        self.connection.lifecycle.is_connected()
    }

    /// Provenance for the visual-review privacy gate: true when the visible
    /// surface was seeded by a capture fixture (`*for_capture` openers run
    /// here) or a dev-tools fixture painted — not merely when an env var
    /// claims it. Debug-only: the review pipeline is a dev-tools feature.
    #[cfg(debug_assertions)]
    pub fn fixture_provenance(&self) -> bool {
        self.dev_tools.fixture_provenance() || self.capture_evidence_seeded
    }

    /// Mark that a `DB_PRO_CAPTURE_*` surface opener ran — the capture
    /// driver's provenance stamp, set by code path not by env string.
    pub fn mark_capture_provenance(&mut self) {
        self.capture_evidence_seeded = true;
    }

    /// Apply a driver choice from the connection dialog.
    pub fn select_connection_driver(&mut self, driver: UiDriver) {
        connection::select_connection_driver(self.connection.dialog.draft_mut(), driver);
    }

    /// Open connection edit dialog from a saved summary.
    pub fn open_edit_connection(&mut self, connection: &UiConnectionSummary) {
        connection::open_edit_connection(&mut self.connection.dialog, &mut self.connection.lifecycle, connection);
    }

    /// Open a duplicate connection draft from a saved summary.
    pub fn open_duplicate_connection(&mut self, connection: &UiConnectionSummary) {
        connection::open_duplicate_connection(&mut self.connection.dialog, &mut self.connection.lifecycle, connection);
    }

    /// Save the active draft's SSH parameters as a reusable profile.
    pub fn save_draft_as_ssh_profile(&mut self) {
        match connection::save_draft_as_ssh_profile(&mut self.connection.dialog) {
            Ok(id) => {
                self.connection.dialog.draft_mut().ssh_profile_id = id;
                self.feedback
                    .set_runtime_message("SSH profile saved — reusable by other connections");
            }
            Err(err) => self.feedback.set_runtime_message(err),
        }
    }

    /// Apply an existing SSH profile to the active draft.
    pub fn apply_ssh_profile(&mut self, profile_id: &str) {
        connection::apply_ssh_profile(&mut self.connection.dialog, profile_id);
    }

    /// Apply the selected cloud preset to the active draft.
    pub fn apply_cloud_preset(&mut self) {
        if let Err(err) = connection::apply_cloud_preset(&mut self.connection.dialog) {
            self.connection.dialog.set_error(err);
        }
    }

    /// Dispatch connection test or save command to the runtime worker.
    pub fn dispatch_connection_command(&mut self, save: bool) {
        let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
        connection::view::ConnectionDialogView {
            dialog: &mut self.connection.dialog,
            lifecycle: &mut self.connection.lifecycle,
            command_dispatcher: &mut command_dispatcher,
            feedback: &mut self.feedback,
            theme: self.theme,
        }
        .dispatch_connection_command(save);
    }

    /// Refresh the diagnostic report for the active draft.
    pub fn refresh_connection_diagnostics(&mut self, auth_ok: bool, auth_message: &str) {
        connection::refresh_connection_diagnostics(&mut self.connection.dialog, auth_ok, auth_message);
    }

    pub(super) fn handle_connection_request_failure(&mut self, request_id: RequestId, message: &str) -> bool {
        connection_events::handle_connection_request_failure(
            &mut self.connection.lifecycle,
            &mut self.connection.dialog,
            &mut self.schema.explorer,
            &mut self.feedback,
            request_id,
            message,
        )
    }

    pub(super) fn on_connections_loaded(&mut self, connections: Vec<UiConnectionSummary>) {
        let active = connection_events::on_connections_loaded(
            &mut self.connection.lifecycle,
            &mut self.connection.catalog,
            &mut self.feedback,
            connections,
        );
        if let Some(active) = active {
            self.connect_to_connection(&active);
        }
    }

    pub(super) fn on_connected(&mut self, request_id: RequestId, connection_id: String) {
        let Some(connection_id) = connection_events::on_connected(
            &mut self.connection.lifecycle,
            &mut self.feedback,
            request_id,
            connection_id,
        ) else {
            return;
        };
        // A fresh session can carry different server-side debug state (#252): drop the
        // cached probe so the routine workbench re-reads the catalogs on next draw.
        self.management.routine.clear_debug_probe();
        self.request_schema_introspection(connection_id.clone(), false);
        let request_id = self.task_bridge.next_request_id();
        self.dispatch_command(query_save_commands::list_queries_command(
            request_id,
            connection_id.clone(),
        ));
        let request_id = self.task_bridge.next_request_id();
        self.dispatch_command(query_save_commands::list_folders_command(request_id, connection_id));
    }

    // Connection read models and shell status are implemented as explicit pure helpers
    // in `connection_status.rs`; these root methods preserve the app's internal API while
    // keeping that feature module independent from the composition root.
    pub(super) fn active_connection(&self) -> Option<&UiConnectionSummary> {
        connection_status::active_connection(&self.connection.catalog, &self.connection.lifecycle)
    }

    pub(super) fn active_connection_name(&self) -> &str {
        connection_status::active_connection_name(&self.connection.catalog, &self.connection.lifecycle)
    }

    pub(super) fn active_driver(&self) -> &str {
        connection_status::active_driver(&self.connection.catalog, &self.connection.lifecycle)
    }

    pub(crate) fn active_capabilities(&self) -> CapabilityLookup {
        connection_status::active_capabilities(&self.connection.catalog, &self.connection.lifecycle)
    }

    pub(super) fn active_schema(&self) -> &str {
        connection_status::active_schema(
            &self.schema.explorer,
            &self.connection.catalog,
            &self.connection.lifecycle,
        )
    }

    pub(super) fn active_schema_table_names(&self) -> Vec<String> {
        connection_status::active_schema_table_names(
            &self.schema.explorer,
            &self.connection.catalog,
            &self.connection.lifecycle,
        )
    }

    #[cfg(test)]
    pub(super) fn schema_table_count(&self, schema: &str) -> usize {
        connection_status::schema_table_count(&self.schema.explorer, schema)
    }

    pub(super) fn active_schema_column_names(&self) -> Vec<String> {
        connection_status::active_schema_column_names(
            &self.schema.explorer,
            &self.connection.catalog,
            &self.connection.lifecycle,
        )
    }

    pub(super) fn has_runtime_error(&self) -> bool {
        connection_status::has_runtime_error(&self.feedback)
    }

    pub(super) fn runtime_status(&self) -> Option<(String, egui::Color32)> {
        connection_status::runtime_status(&self.feedback, self.theme, self.workspace.activity)
    }

    pub(super) fn statusbar_state(&self) -> (Icon, egui::Color32, &'static str) {
        connection_status::statusbar_state(&self.connection.lifecycle, &self.feedback, self.theme)
    }

    pub(super) fn shows_editor_status(&self) -> bool {
        connection_status::shows_editor_status()
    }

    pub(super) fn statusbar_context_label(&self) -> &'static str {
        connection_status::statusbar_context_label(&self.workspace, &self.table.state)
    }

    pub(super) fn connection_indicator(&self, connection: &UiConnectionSummary) -> (Icon, egui::Color32) {
        connection_status::connection_indicator(&self.connection.lifecycle, connection, self.theme)
    }

    fn primary_modifier_label() -> &'static str {
        if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl"
        }
    }

    /// Platform-aware shortcut parts for kbd chips, e.g. `["Ctrl", "N"]` / `["⌘", "N"]`.
    fn shortcut_parts(keys: &[&str]) -> Vec<String> {
        let mut parts = Vec::with_capacity(keys.len() + 1);
        parts.push(Self::primary_modifier_label().to_owned());
        for key in keys {
            let mapped = match *key {
                "Shift" if cfg!(target_os = "macos") => "⇧",
                other => other,
            };
            parts.push(mapped.to_owned());
        }
        parts
    }

    /// Human-readable shortcut for tooltips: `Ctrl+N` on Linux/Windows, `⌘N` on macOS.
    fn format_shortcut(keys: &[&str]) -> String {
        let parts = Self::shortcut_parts(keys);
        if cfg!(target_os = "macos") {
            parts.concat()
        } else {
            parts.join("+")
        }
    }

    /// Queues a command and exposes a closed runtime boundary to the user.
    pub(crate) fn dispatch_command(&mut self, command: UiCommand) -> bool {
        command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge).dispatch(command, &mut self.feedback)
    }

    /// Sends a cancellation/background command without borrowing the whole app.
    ///
    /// These calls are intentionally best-effort because their authoritative
    /// guards are request IDs and document versions; a closed worker cannot
    /// execute the cancellation, but it also cannot mutate UI state anymore.
    pub(crate) fn send_command_best_effort(&self, command: UiCommand) -> bool {
        self.task_bridge.send_best_effort(command)
    }

    /// Allocates request identities at the composition boundary.
    ///
    /// Feature adapters may request an identity, but they do not reach into
    /// the runtime bridge directly. Keeping allocation here makes the
    /// request lifecycle auditable and prevents feature modules from coupling
    /// to the channel implementation.
    pub(crate) fn next_request_id(&mut self) -> RequestId {
        self.task_bridge.next_request_id()
    }

    // Connection/status: `connection_status.rs`.

    // Query session helpers: `query_session.rs`.

    // Schema snapshot / query result / txn: `query_session.rs`.
    // Close workspace tab: `workspace_actions.rs`.

    pub(crate) fn set_agent_open(&mut self, open: bool, ctx: &egui::Context) {
        if open == self.workspace.agent_open {
            return;
        }
        self.workspace.agent_open = open;
        if open {
            self.workspace.sidebar_open_before_agent = Some(self.workspace.sidebar_open);
            if ctx.content_rect().width() < AGENT_SIDEBAR_COLLAPSE_WIDTH {
                self.workspace.sidebar_open = false;
            }
        } else if let Some(sidebar_open) = self.workspace.sidebar_open_before_agent.take() {
            self.workspace.sidebar_open = sidebar_open;
        }
    }

    fn workspace_session_context(&mut self) -> workspace_session::WorkspaceSessionContext<'_> {
        workspace_session::WorkspaceSessionContext {
            workspace: &mut self.workspace,
            connection: &mut self.connection,
            schema_explorer: &mut self.schema.explorer,
            query_session_state: &mut self.query.session,
            feedback: &mut self.feedback,
            preferences: &self.preferences,
        }
    }

    pub(crate) fn save_named_workspace_session(&mut self) {
        self.workspace_session_context().save_named();
    }

    pub(crate) fn restore_named_workspace_session(&mut self, id: &str) {
        self.workspace_session_context().restore_named(id);
    }

    pub(crate) fn duplicate_named_workspace_session(&mut self, id: &str) {
        self.workspace_session_context().duplicate_named(id);
    }

    pub(crate) fn persist_workspace_sessions(&mut self, storage: &mut dyn eframe::Storage) {
        self.workspace_session_context().persist(storage);
    }

    pub(crate) fn load_named_sessions_from_storage(&mut self, storage: &dyn eframe::Storage) {
        self.workspace_session_context().load_named_sessions(storage);
    }

    pub(crate) fn restore_last_workspace_session_from_storage(&mut self, storage: &dyn eframe::Storage) {
        self.workspace_session_context().restore_last(storage);
    }

    pub(crate) fn open_agent_prompt(&mut self, prompt: impl Into<String>, ctx: &egui::Context) {
        self.agent.input = prompt.into();
        self.set_agent_open(true, ctx);
    }

    fn request_connections_once(&mut self) {
        if self.connection.lifecycle.connections_requested() {
            return;
        }
        self.connection.lifecycle.mark_connections_requested();
        let request_id = self.task_bridge.next_request_id();
        if self.dispatch_command(connection::logic::build_list_connections_command(request_id)) {
            self.connection.lifecycle.note_connections_list_started(request_id);
        }
    }

    /// Retries the connection-catalog load after a failure — clears the
    /// one-shot latch so `request_connections_once` fires again.
    pub(crate) fn retry_connections_load(&mut self) {
        self.connection.lifecycle.clear_connections_requested();
        self.request_connections_once();
    }

    // cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
    fn runtime_work_pending(&self) -> bool {
        self.connection.lifecycle.connections_request_pending()
            || self.connection.lifecycle.pending_request().is_some()
            || self.schema.explorer.schema_request.is_some()
            || self
                .query
                .session
                .documents
                .iter()
                .any(|d| d.pending_prediction_request.is_some() || d.prediction_debounce_deadline.is_some())
            || self.query.session.documents.iter().any(|d| d.explain_request.is_some())
            || self
                .agent
                .sessions
                .values()
                .any(|session| session.request_id.is_some() || session.active_run_id.is_some())
            || self.table.state.table_info_request.is_some()
            || self.table.state.table_ddl_request.is_some()
            || self.table.data_query.request.is_some()
            || self.table.mutation.table_mutation_request.is_some()
            || self.table.mutation.staged_apply_request.is_some()
            || self.table.state.ddl_execution_request.is_some()
    }

    fn request_schema_introspection(&mut self, connection_id: String, force_refresh: bool) {
        self.request_schema_introspection_with_cache(connection_id, force_refresh, false);
    }

    fn request_schema_introspection_with_cache(
        &mut self,
        connection_id: String,
        force_refresh: bool,
        invalidate_cache: bool,
    ) {
        let request_id = self.task_bridge.next_request_id();
        if !self.dispatch_command(schema_actions::introspect_schema_command(
            request_id,
            connection_id,
            force_refresh,
            invalidate_cache,
        )) {
            self.schema.explorer.schema_request = None;
            self.schema.explorer.schema_error = Some("Runtime worker unavailable".to_owned());
            self.schema.compare.cancel_diff_after_refresh();
            return;
        }
        self.schema.explorer.schema_request = Some(request_id);
        self.schema.explorer.schema_error = None;
        self.feedback.runtime_message = if invalidate_cache {
            "Refreshing schema cache…"
        } else if force_refresh {
            "Refreshing schema…"
        } else {
            "Loading schema…"
        }
        .to_owned();
    }

    /// Refreshes schema for the currently active connection from the View menu.
    pub fn refresh_active_schema_from_menu(&mut self, invalidate_cache: bool) {
        let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) else {
            self.feedback.runtime_message = "Connect a source database first".to_owned();
            return;
        };

        self.request_schema_introspection_with_cache(connection_id, true, invalidate_cache);
    }

    /// Opens the standard connection editor from the native File menu.
    pub fn open_new_connection_from_menu(&mut self) {
        self.connection.open_new();
    }

    /// Selects the Query workspace from the native File menu.
    pub fn open_query_from_menu(&mut self) {
        self.workspace.activity = Activity::Queries;
        self.workspace.active_tab = WorkspaceTab::Query;
        self.workspace.sidebar_open = true;
    }
}
