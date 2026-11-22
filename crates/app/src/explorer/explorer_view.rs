//! Entry point for the Database Navigator in Codex / DBeaver style: a unified
//! hierarchical tree where connections are the root nodes.
//!
//! Row painting lives in `explorer_tree`, table details in `explorer_details`,
//! and the Views / Functions / Triggers folders in `explorer_folders`.

use super::explorer_surface_view::{ExplorerSurfaceAction, ExplorerSurfaceContext};
use super::*;

impl DbProApp {
    /// Entry-point for Database Navigator in Codex / DBeaver style:
    /// Unified hierarchical tree where connections are root nodes.
    pub(crate) fn draw_explorer_sub_panes(&mut self, ui: &mut egui::Ui) {
        let active_schema = self.active_schema().to_owned();
        let functions_enabled = self.active_capabilities().allows(|c| c.schema.functions);
        let trigger_toggle_unavailable_reason = self
            .query_capabilities()
            .feature_limitation(db_pro_core::domain::capabilities::CapabilityFeature::TriggerEnableDisable);
        let actions = ExplorerSurfaceContext {
            theme: self.theme,
            catalog: &self.connection.catalog,
            lifecycle: &self.connection.lifecycle,
            explorer: &mut self.schema.explorer,
            active_schema,
            table_info: self.table.state.table_info.clone(),
            reduce_motion: self.preferences.reduce_motion,
            functions_enabled,
            trigger_toggle_unavailable_reason,
            modifier: Self::primary_modifier_label(),
        }
        .draw(ui);
        for action in actions {
            self.apply_explorer_surface_action(action, ui);
        }
    }

    fn apply_explorer_surface_action(&mut self, action: ExplorerSurfaceAction, ui: &mut egui::Ui) {
        match action {
            ExplorerSurfaceAction::NewConnection => self.connection.open_new(),
            ExplorerSurfaceAction::RetryConnections => self.retry_connections_load(),
            ExplorerSurfaceAction::RefreshSchema => {
                if let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) {
                    let schema = self.schema.explorer.selected_schema.clone();
                    let table = self.schema.explorer.selected_table.clone();
                    self.refresh_explorer_node(&connection_id, schema.as_deref(), table.as_deref());
                }
            }
            ExplorerSurfaceAction::Connection {
                connection,
                action,
                is_connected,
            } => self.apply_connection_row_action(action, &connection, is_connected, ui),
            ExplorerSurfaceAction::Schema { connection_id, action } => {
                self.apply_schema_tree_action(action, &connection_id, ui)
            }
        }
    }

    /// Refreshes the node the user has selected. A table loads that table; a schema reloads its connection.
    pub(super) fn refresh_explorer_node(&mut self, connection_id: &str, schema: Option<&str>, table: Option<&str>) {
        if self.connection.lifecycle.active_connection_id() != Some(connection_id) {
            self.connection
                .lifecycle
                .set_active_connection_id(Some(connection_id.to_owned()));
        }
        if let Some(schema) = schema {
            self.schema.explorer.selected_schema = Some(schema.to_owned());
        }
        if let Some(table) = table {
            self.schema.explorer.selected_table = Some(table.to_owned());
            self.request_table_info();
            return;
        }
        self.request_schema_introspection(connection_id.to_owned(), true);
    }

    /// Clears the connected state and closes the runtime handle.
    pub(crate) fn disconnect_from_connection(&mut self, connection: &UiConnectionSummary) {
        let closed = explorer_navigation::ExplorerConnectionContext::new(
            &mut self.connection.lifecycle,
            &mut self.schema.explorer,
            &mut self.table,
            &mut self.workspace,
            &mut self.agent,
            &mut self.query.execution,
            &mut self.feedback,
        )
        .disconnect(connection);
        if !closed {
            return;
        }
        self.connection.lifecycle.note_session_closed(&connection.id);
        self.management.audit.reset_connection_scope();
        self.management.monitoring.reset_connection_scope();
        // In-flight requests bound to this connection outlive the disconnect
        // otherwise — the server keeps running them and results land on
        // documents whose connection is gone.
        let orphaned: Vec<crate::RequestId> = self
            .query
            .session
            .documents
            .iter()
            .filter(|document| document.connection_id.as_deref() == Some(connection.id.as_str()))
            .flat_map(|document| {
                let running = match document.execution_state {
                    crate::query::query_document::QueryExecutionState::Running(id) => Some(id),
                    _ => None,
                };
                running.into_iter().chain(document.explain_request)
            })
            .collect();
        for request_id in orphaned {
            let _ = self.dispatch_command(UiCommand::CancelQuery { request_id });
        }
        let request_id = self.next_request_id();
        let command = UiCommand::Disconnect {
            request_id,
            connection_id: connection.id.clone(),
        };
        let _ = self.dispatch_command(command);
    }

    /// Helper to initiate connection logic.
    pub(crate) fn connect_to_connection(&mut self, connection: &UiConnectionSummary) {
        if self.connection.lifecycle.active_connection_id() == Some(connection.id.as_str())
            && self.connection.lifecycle.is_connected()
        {
            return;
        }
        let request_id = self.next_request_id();
        let command = explorer_navigation::ExplorerConnectionContext::new(
            &mut self.connection.lifecycle,
            &mut self.schema.explorer,
            &mut self.table,
            &mut self.workspace,
            &mut self.agent,
            &mut self.query.execution,
            &mut self.feedback,
        )
        .connect(connection);
        if let Some(request) = command {
            let command = connection::logic::build_connect_command(request_id, request.connection_id);
            if self.dispatch_command(command) {
                explorer_navigation::ExplorerConnectionContext::new(
                    &mut self.connection.lifecycle,
                    &mut self.schema.explorer,
                    &mut self.table,
                    &mut self.workspace,
                    &mut self.agent,
                    &mut self.query.execution,
                    &mut self.feedback,
                )
                .commit_connect(connection, request_id);
                self.management.audit.reset_connection_scope();
                self.management.monitoring.reset_connection_scope();
            }
        }
    }
}
