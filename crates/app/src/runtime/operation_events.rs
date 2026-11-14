//! Runtime orchestration for database-management and mutation events.

use super::*;
use crate::RequestId;

fn take_current_connection_request(
    pending: &mut Option<(u64, String)>,
    request_id: RequestId,
    response_connection_id: &str,
    active_connection_id: Option<&str>,
) -> bool {
    if pending.as_ref().is_none_or(|(pending_id, _)| *pending_id != request_id.0) {
        return false;
    }
    let Some((_, pending_connection_id)) = pending.take() else {
        return false;
    };
    pending_connection_id == response_connection_id && active_connection_id == Some(response_connection_id)
}

fn is_authoritative_connection_completion(operation: &str, pending: bool) -> bool {
    !matches!(
        operation,
        "connection.created" | "connection.updated" | "connection.deleted" | "connection.tested" | "connection.disconnected"
    ) || pending
}

impl DbProApp {
    pub(super) fn active_database_connection_id(&self) -> Option<String> {
        if !self.connection.lifecycle.is_connected() {
            return None;
        }
        self.connection.lifecycle.active_connection_id().map(str::to_owned)
    }

    pub(super) fn on_operation_progress(&mut self, operation: String, status: String) {
        management_events::on_operation_progress(&mut self.feedback, operation, status);
    }

    pub(super) fn on_backup_completed(&mut self, request_id: RequestId, output_path: String, size_bytes: u64) {
        management_events::on_backup_completed(&mut self.feedback, output_path.clone(), size_bytes);
        self.finish_saved_task(
            request_id.0,
            Ok(format!("Backup finished · {output_path} ({size_bytes} bytes)")),
        );
    }

    pub(super) fn on_monitoring_snapshot_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        snapshot: db_pro_core::domain::monitoring::MonitoringSnapshot,
    ) {
        let active_connection_id = self.active_database_connection_id();
        let monitoring = &mut self.management.monitoring;
        if !take_current_connection_request(
            &mut monitoring.monitoring_snapshot_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_monitoring_snapshot_loaded(monitoring, &mut self.feedback, snapshot);
    }

    pub(super) fn on_monitoring_workload_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        workload: db_pro_core::domain::monitoring::StatStatementsSnapshot,
    ) {
        let active_connection_id = self.active_database_connection_id();
        let monitoring = &mut self.management.monitoring;
        if !take_current_connection_request(
            &mut monitoring.monitoring_workload_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_monitoring_workload_loaded(monitoring, &mut self.feedback, workload);
    }

    pub(super) fn on_audit_page_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        page: db_pro_core::domain::audit::AuditPage,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.audit.audit_page_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_audit_page_loaded(
            &mut self.management.audit.audit_page,
            &mut self.management.audit.audit_error,
            &mut self.feedback,
            page,
        );
    }

    pub(super) fn on_pg_settings_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        snapshot: db_pro_core::domain::pg_settings::PgSettingsSnapshot,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.pg_settings.pg_settings_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_pg_settings_loaded(&mut self.management.pg_settings, &mut self.feedback, snapshot);
    }

    /// Stores one PL/pgSQL debug-capability probe reply (#252).
    ///
    /// The reply must answer the pending request for the connection the workbench still
    /// shows; a stale or foreign reply is dropped instead of painting another server's
    /// debug state onto the current connection.
    pub(super) fn on_routine_debug_probe_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        probe: db_pro_core::domain::routine_debug::RoutineDebugProbe,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if active_connection_id.as_deref() != Some(connection_id.as_str()) {
            return;
        }
        self.management.routine.complete_debug_probe(request_id.0, &connection_id, probe);
    }

    pub(super) fn on_pg_setting_action_completed(&mut self, action: String, name: String) {
        management_events::on_pg_setting_action_completed(&mut self.feedback, action, name);
        if let Some(connection_id) = self.active_database_connection_id() {
            let request_id = self.next_request_id();
            if self.dispatch_command(super::pg_settings_activity_view::list_pg_settings_command(
                request_id,
                connection_id.clone(),
            )) {
                self.management.pg_settings.pg_settings_request = Some((request_id.0, connection_id));
            }
        }
    }

    pub(super) fn on_fdw_inventory_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        inventory: db_pro_core::domain::fdw::FdwInventory,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.fdw.fdw_inventory_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_fdw_inventory_loaded(&mut self.management.fdw, &mut self.feedback, inventory);
    }

    pub(super) fn on_fdw_action_completed(&mut self, action: String, name: String) {
        management_events::on_fdw_action_completed(&mut self.management.fdw, &mut self.feedback, action, name);
        if let Some(connection_id) = self.active_database_connection_id() {
            let request_id = self.next_request_id();
            if self.dispatch_command(super::fdw_activity_view::list_fdw_inventory_command(
                request_id,
                connection_id.clone(),
            )) {
                self.management.fdw.fdw_inventory_request = Some((request_id.0, connection_id));
            }
        }
    }

    pub(super) fn on_replication_inventory_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        inventory: db_pro_core::domain::replication::ReplicationInventory,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.replication.replication_inventory_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_replication_inventory_loaded(
            &mut self.management.replication,
            &mut self.feedback,
            inventory,
        );
    }

    pub(super) fn on_replication_action_completed(&mut self, action: String, name: String) {
        management_events::on_replication_action_completed(
            &mut self.management.replication,
            &mut self.feedback,
            action,
            name,
        );
        if let Some(connection_id) = self.active_database_connection_id() {
            let request_id = self.next_request_id();
            if self.dispatch_command(super::replication_activity_view::list_replication_inventory_command(
                request_id,
                connection_id.clone(),
            )) {
                self.management.replication.replication_inventory_request = Some((request_id.0, connection_id));
            }
        }
    }

    pub(super) fn on_event_trigger_inventory_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        inventory: db_pro_core::domain::event_trigger::EventTriggerInventory,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.event_trigger.event_trigger_inventory_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_event_trigger_inventory_loaded(
            &mut self.management.event_trigger,
            &mut self.feedback,
            inventory,
        );
    }

    pub(super) fn on_event_trigger_action_completed(&mut self, action: String, name: String) {
        management_events::on_event_trigger_action_completed(
            &mut self.management.event_trigger,
            &mut self.feedback,
            action,
            name,
        );
        if let Some(connection_id) = self.active_database_connection_id() {
            let request_id = self.next_request_id();
            if self.dispatch_command(super::event_trigger_activity_view::list_event_triggers_command(
                request_id,
                connection_id.clone(),
            )) {
                self.management.event_trigger.event_trigger_inventory_request =
                    Some((request_id.0, connection_id));
            }
        }
    }

    pub(super) fn on_monitoring_action_completed(
        &mut self,
        request_id: RequestId,
        action: String,
        backend_id: i64,
        succeeded: bool,
    ) {
        let task_request = request_id.0;
        let task_outcome = if succeeded {
            Ok(format!("Maintenance finished · {action}"))
        } else {
            Err(format!("Maintenance did not finish · {action}"))
        };
        management_events::on_monitoring_action_completed(
            &mut self.management.monitoring,
            &mut self.feedback,
            action,
            backend_id,
            succeeded,
        );
        if let Some(connection_id) = self.active_database_connection_id() {
            let snapshot_request = self.next_request_id();
            let sort = self.management.monitoring.monitoring_stat_sort;
            if self.dispatch_command(super::monitoring_activity_view::snapshot_command(
                snapshot_request,
                connection_id.clone(),
                sort,
            )) {
                self.management.monitoring.monitoring_snapshot_request =
                    Some((snapshot_request.0, connection_id));
                self.management.monitoring.monitoring_last_poll = Some(std::time::Instant::now());
            }
        }
        self.finish_saved_task(task_request, task_outcome);
    }

    pub(super) fn on_users_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        users: Vec<db_pro_core::domain::user::DatabaseUser>,
    ) {
        let active_connection_id = self.active_database_connection_id();
        let security = &mut self.management.security;
        if !take_current_connection_request(
            &mut security.security_users_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_users_loaded(security, &mut self.feedback, users);
    }

    pub(super) fn on_privileges_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        role_name: String,
        privileges: Vec<db_pro_core::domain::user::Privilege>,
    ) {
        let active_connection_id = self.active_database_connection_id();
        let security = &mut self.management.security;
        if !take_current_connection_request(
            &mut security.security_privilege_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_privileges_loaded(security, role_name, privileges);
    }

    pub(super) fn on_memberships_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        member: String,
        memberships: Vec<db_pro_core::domain::user::RoleMembership>,
    ) {
        let active_connection_id = self.active_database_connection_id();
        let security = &mut self.management.security;
        if !take_current_connection_request(
            &mut security.security_membership_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_memberships_loaded(security, member, memberships);
    }

    pub(super) fn on_table_rls_loaded(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        state: db_pro_core::domain::rls::TableRlsState,
    ) {
        let active_connection_id = self.active_database_connection_id();
        if !take_current_connection_request(
            &mut self.management.security.security_rls_request,
            request_id,
            &connection_id,
            active_connection_id.as_deref(),
        ) {
            return;
        }
        management_events::on_table_rls_loaded(&mut self.management.security, &mut self.feedback, state);
    }

    pub(super) fn on_data_diff_loaded(
        &mut self,
        request_id: RequestId,
        source_id: String,
        target_id: String,
        diff: db_pro_core::domain::cross_connection::DataDiff,
    ) {
        let active_source_id = self.connection.lifecycle.active_connection_id();
        let Some((pending_id, pending_source_id, pending_target_id)) =
            self.schema.compare.data_diff_request.take()
        else {
            return;
        };
        if pending_id != request_id.0
            || pending_source_id != source_id
            || pending_target_id != target_id
            || active_source_id != Some(source_id.as_str())
            || !self.connection.lifecycle.is_connected()
        {
            return;
        }
        management_events::on_data_diff_loaded(&mut self.schema.compare, &mut self.feedback, diff);
    }

    /// A native file picker returned (or was cancelled).
    /// A native file picker returned (or was cancelled).
    pub(super) fn on_file_picked(&mut self, kind: &str, path: Option<String>) {
        if kind == "audit-export" {
            match path {
                Some(path) => self.write_audit_export(&path),
                None => self
                    .feedback
                    .set_runtime_message("Audit export cancelled — no file chosen"),
            }
            return;
        }
        if let Some(folder) = file_picker_events::on_file_picked(
            &mut self.connection.dialog,
            &mut self.overlay,
            &mut self.feedback,
            kind,
            path,
        ) {
            self.open_workspace_folder(folder);
        }
    }

    /// DDL applied; re-introspect so the tree and table view pick up the change.
    pub(super) fn on_ddl_completed(&mut self, request_id: RequestId, affected_rows: u64) {
        let design_apply = self.schema.diagram.design.apply_request == Some(request_id.0);
        // Patch the workbench history entry with the real outcome.
        self.schema.workbench.finalize_execution(request_id, true);
        if let Some(transition) = ddl_events::on_ddl_completed(
            &mut self.table.state,
            &mut self.management.security,
            &mut self.feedback,
            request_id,
            affected_rows,
            self.schema.explorer.selected_table.is_some(),
        ) {
            if design_apply {
                self.schema.diagram.design.discard();
            }
            if transition.refresh_schema {
                if let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) {
                    self.request_schema_introspection(connection_id, true);
                }
            }
            if transition.refresh_rls {
                self.request_security_rls();
            }
        }
    }

    fn request_security_users(&mut self) {
        let connection_id = self.active_database_connection_id();
        let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
        security_activity_view::request_security_users(
            &mut self.management.security,
            connection_id.as_deref(),
            &mut command_dispatcher,
            &mut self.feedback,
        );
    }

    fn request_security_role_details(&mut self, role_name: &str) {
        let connection_id = self.active_database_connection_id();
        let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
        security_activity_view::request_security_role_details(
            &mut self.management.security,
            connection_id.as_deref(),
            role_name,
            &mut command_dispatcher,
            &mut self.feedback,
        );
    }

    fn request_security_rls(&mut self) {
        let connection_id = self.active_database_connection_id();
        let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
        security_activity_view::request_security_rls(
            &mut self.management.security,
            connection_id.as_deref(),
            &mut command_dispatcher,
            &mut self.feedback,
        );
    }

    /// Generic completion for connection, table-row and query operations.
    pub(super) fn on_operation_completed(&mut self, request_id: RequestId, operation: String) {
        let pending_connection_request = self.connection.lifecycle.pending_request() == Some(request_id);
        if !is_authoritative_connection_completion(&operation, pending_connection_request) {
            return;
        }
        self.feedback.set_runtime_message(operation.clone());
        if pending_connection_request {
            self.connection.lifecycle.clear_pending_request();
        }
        self.apply_security_operation_completion(&operation);
        self.apply_connection_operation_completion(&operation);
        self.apply_connection_test_completion(&operation, pending_connection_request);
        if operation.starts_with("table-row.") || operation == "table-changes.applied" {
            self.on_table_row_operation_completed(request_id);
        }
        if operation == "connection.created" || operation == "connection.updated" {
            self.close_connection_dialog();
        }
        if operation.starts_with("query") || operation.starts_with("query-folder") {
            self.request_saved_queries_refresh();
        }
        if operation == "connection.deleted" {
            self.clear_deleted_connection();
        }
    }

    fn apply_security_operation_completion(&mut self, operation: &str) {
        if !matches!(
            operation,
            "create_role"
                | "drop_role"
                | "alter_role"
                | "update_role_password"
                | "grant_membership"
                | "revoke_membership"
                | "grant_privilege"
                | "revoke_privilege"
        ) {
            return;
        }
        self.management.security.security_drop_confirm = None;
        self.management.security.security_password.clear();
        self.request_security_users();
        if let Some(role) = self.management.security.security_selected_role.clone() {
            self.request_security_role_details(&role);
        }
    }

    fn apply_connection_operation_completion(&mut self, operation: &str) {
        if matches!(operation, "connection.created" | "connection.updated" | "connection.deleted") {
            self.connection.lifecycle.clear_connections_requested();
            self.request_connections_once();
        }
    }

    fn apply_connection_test_completion(&mut self, operation: &str, pending: bool) {
        if operation != "connection.tested" || !pending {
            return;
        }
        self.connection.dialog.clear_error();
        self.refresh_connection_diagnostics(true, "Authentication succeeded");
        if self.connection.dialog.test_draft() == Some(self.connection.dialog.draft()) {
            self.connection.dialog.set_test_valid(true);
            self.feedback.set_runtime_message(
                self.connection
                    .dialog
                    .diagnostics()
                    .map(|result| result.summary())
                    .unwrap_or_else(|| "Connection test succeeded".to_owned()),
            );
        } else {
            self.connection.dialog.set_test_valid(false);
            self.feedback
                .set_runtime_message("Connection changed · test again before saving");
        }
    }

    fn close_connection_dialog(&mut self) {
        self.connection
            .dialog
            .transition(super::connection::state::ConnectionDialogAction::Close);
        self.connection.dialog.set_editing_connection_id(None);
    }

    fn clear_deleted_connection(&mut self) {
        // `pending_connection_id` is the delete target (set by the confirm dialog).
        let deleted_id = self.connection.lifecycle.take_pending_connection_id();
        if let Some(ref id) = deleted_id {
            self.connection.lifecycle.clear_connection_error(id);
        }
        let deleted_was_active = deleted_id
            .as_ref()
            .is_some_and(|id| self.connection.lifecycle.active_connection_id() == Some(id.as_str()));
        // Only tear down the live session when the deleted connection was active.
        // Deleting a sibling must not force a reconnect / schema reload of the open one.
        if deleted_was_active {
            self.connection.lifecycle.set_active_connection_id(None);
            self.connection.lifecycle.set_connected(false);
        }
    }

    /// Row mutation follow-up: finalise a staged apply, or refresh the grid.
    fn on_table_row_operation_completed(&mut self, request_id: RequestId) {
        self.table.editing.data_editing_cell = None;
        self.table.editing.data_edit_value.clear();
        self.table.editing.data_edit_error = None;
        self.table.editing.data_delete_confirmation = false;
        if self.table.mutation.staged_apply_request == Some(request_id) {
            self.staged_apply_completed();
        } else if self.table.mutation.table_mutation_request == Some(request_id) {
            self.table.mutation.table_mutation_request = None;
            self.table.data_query.clear_result();
            self.table.data_query.total_rows = None;
            self.table.data_query.error = None;
            self.table.data_query.request = None;
            if self.workspace.active_tab == WorkspaceTab::Table {
                self.request_table_data();
            }
        }
    }

    /// Re-reads saved queries for the active connection.
    fn request_saved_queries_refresh(&mut self) {
        if let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) {
            let request_id = self.next_request_id();
            self.dispatch_command(query_save_commands::list_queries_command(request_id, connection_id));
        }
    }

    pub(super) fn on_table_changes_failed(
        &mut self,
        request_id: RequestId,
        code: String,
        message: String,
        statement_index: usize,
        rolled_back: bool,
    ) {
        if self.table.mutation.staged_apply_request == Some(request_id) {
            self.staged_apply_failed(StagedApplyFailure {
                statement_index,
                code: &code,
                message: &message,
                rolled_back,
            });
        }
    }
}

#[cfg(test)]
mod connection_request_tests {
    use super::take_current_connection_request;
    use crate::RequestId;

    #[test]
    fn accepts_only_latest_request_for_response_and_active_connection() {
        let mut pending = Some((7, "conn-a".to_owned()));
        assert!(take_current_connection_request(
            &mut pending,
            RequestId(7),
            "conn-a",
            Some("conn-a"),
        ));
        assert!(pending.is_none());

        pending = Some((7, "conn-a".to_owned()));
        assert!(!take_current_connection_request(
            &mut pending,
            RequestId(6),
            "conn-a",
            Some("conn-a"),
        ));
        assert_eq!(pending, Some((7, "conn-a".to_owned())));

        assert!(!take_current_connection_request(
            &mut pending,
            RequestId(7),
            "conn-b",
            Some("conn-b"),
        ));
        assert!(pending.is_none());

        pending = Some((7, "conn-a".to_owned()));
        assert!(!take_current_connection_request(
            &mut pending,
            RequestId(7),
            "conn-a",
            Some("conn-b"),
        ));
        assert!(pending.is_none());
    }
}
