//! Connection lifecycle reducers over explicit feature state.

use super::*;
use super::connection::PendingConnectionOperation;
use crate::RequestId;

pub(super) fn handle_connection_request_failure(
    lifecycle: &mut ConnectionLifecycleState,
    dialog: &mut ConnectionDialogState,
    schema_explorer: &mut SchemaExplorerState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    message: &str,
) -> bool {
    if lifecycle.pending_request() != Some(request_id) {
        return false;
    }

    let operation = lifecycle.pending_operation();
    lifecycle.clear_pending_request();
    let is_dialog_operation = matches!(
        operation,
        Some(PendingConnectionOperation::Test) | Some(PendingConnectionOperation::Save)
    );
    // Test/Save failures belong to the draft being edited in the dialog, not to
    // whatever connection is currently active — never fall back to the active
    // connection for those operations (#602).
    let pending_id = lifecycle.take_pending_connection_id();
    let connection_id = if is_dialog_operation {
        pending_id
    } else {
        pending_id.or_else(|| lifecycle.active_connection_id().map(str::to_owned))
    };
    let is_delete = operation == Some(PendingConnectionOperation::Delete);
    if is_delete {
        let formatted = format!("Delete failed · {message}");
        feedback.set_runtime_message(formatted.clone());
        feedback.show_error_toast(formatted);
    } else {
        if let Some(connection_id) = connection_id {
            lifecycle.record_connection_failure(connection_id, message.to_owned());
        }
        if !is_dialog_operation && !dialog.is_open() {
            lifecycle.set_connected(false);
            schema_explorer.schema_request = None;
            schema_explorer.schema_error = None;
        }
        if operation == Some(PendingConnectionOperation::Test) {
            // Rewrite the placeholder auth stage with the runtime's real
            // reason so the FAIL line explains the failure (#609).
            let mut report = dialog.diagnostics.take().unwrap_or_else(|| {
                super::logic::probe_draft_diagnostics(&dialog.draft, false, message)
            });
            super::logic::mark_diagnostics_auth_failed(&mut report, message);
            dialog.diagnostics = Some(report);
        }
        dialog.set_error(message);
        feedback.set_runtime_message(match operation {
            Some(PendingConnectionOperation::Test) => format!("Connection test failed · {message}"),
            Some(PendingConnectionOperation::Save) => format!("Save failed · {message}"),
            _ => format!("Connection failed · {message}"),
        });
    }
    true
}

/// Replace the connection read model and return the connection that should be
/// auto-connected by the composition root, if any.
pub(super) fn on_connections_loaded(
    lifecycle: &mut ConnectionLifecycleState,
    catalog: &mut ConnectionCatalogState,
    feedback: &mut FeedbackState,
    connections: Vec<UiConnectionSummary>,
) -> Option<UiConnectionSummary> {
    lifecycle.set_connections_request_pending(false);
    lifecycle.clear_connections_error();
    catalog.replace(connections);
    if lifecycle.active_connection_id().is_none() {
        lifecycle.set_active_connection_id(catalog.get(0).map(|connection| connection.id.clone()));
    }
    let should_connect = !lifecycle.is_connected() && lifecycle.pending_request().is_none();
    let active = should_connect
        .then(|| lifecycle.active_connection_id().map(str::to_owned))
        .flatten()
        .and_then(|connection_id| catalog.find(&connection_id).cloned());
    feedback.set_runtime_message(format!("Loaded {} connections", catalog.len()));
    active
}

/// Apply the authoritative connected transition and return the active id for
/// the follow-up schema/query refreshes owned by the composition root.
pub(super) fn on_connected(
    lifecycle: &mut ConnectionLifecycleState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    connection_id: String,
) -> Option<String> {
    if lifecycle
        .pending_request()
        .is_some_and(|expected_request| expected_request != request_id)
    {
        return None;
    }
    lifecycle.clear_pending_request();
    lifecycle.set_pending_connection_id(None);
    lifecycle.set_active_connection_id(Some(connection_id.clone()));
    lifecycle.set_connected(true);
    lifecycle.clear_connection_error(&connection_id);
    lifecycle.note_session_opened(&connection_id);
    feedback.set_runtime_message("Connection established");
    Some(connection_id)
}

#[cfg(test)]
#[path = "tests/connection_events_tests.rs"]
mod tests;

