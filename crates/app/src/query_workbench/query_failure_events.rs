//! Query-local failure reducer after cross-feature request routing.

use super::events::QueryHistoryRecord;
use super::*;
use crate::RequestId;

pub(super) struct QueryFailureContext<'a> {
    pub(super) query_session: &'a mut QuerySessionState,
    pub(super) query_editor: &'a mut QueryEditorState,
    pub(super) query_output: &'a mut QueryOutputState,
    pub(super) workspace: &'a mut WorkspaceShellState,
    pub(super) feedback: &'a mut FeedbackState,
}

pub(super) fn on_query_failed(
    context: &mut QueryFailureContext<'_>,
    request_id: RequestId,
    message: String,
    position: Option<usize>,
    code: Option<String>,
) {
    let QueryFailureContext {
        query_session,
        query_editor,
        query_output,
        workspace,
        feedback,
    } = context;
    // #603: never surface a raw connection UUID to the user.
    let message = super::query_view::friendly_connection_error(&message);
    if let Some((document_id, _)) = query_session.save_requests.remove(&request_id) {
        if query_session.pending_close_after_save.is_some_and(|index| {
            query_session
                .documents
                .get(index)
                .is_some_and(|document| document.id == document_id)
        }) {
            query_session.pending_close_after_save = None;
        }
        feedback.set_runtime_message(format!("Save failed · {message}"));
    } else if query_session.document_requests.contains_key(&request_id) {
        let target_doc_id = query_session.document_requests.remove(&request_id);
        let mut history = None;
        if let Some(doc_id) = &target_doc_id {
            if let Some(doc) = query_session.documents.iter_mut().find(|doc| &doc.id == doc_id) {
                let execution_range = doc.executing_range.take();
                let execution_sql = doc.executing_sql.take();
                let execution_version = doc.executing_version.take();
                doc.executing_row_limit = None;
                let (started_at, duration_ms) = doc.take_execution_timing(0);
                history = Some((
                    execution_sql.clone().unwrap_or_else(|| doc.text().to_owned()),
                    doc.connection_id.clone(),
                    doc.schema.clone(),
                    started_at,
                    duration_ms,
                ));
                doc.execution_state = QueryExecutionState::Failed;
                doc.execution_diagnostic = if execution_version == Some(doc.buffer.version()) {
                    execution_sql.as_deref().and_then(|sql| {
                        execution_range.and_then(|range| {
                            super::query_view::database_error_diagnostic(
                                &message,
                                sql,
                                range,
                                position,
                                code.as_deref(),
                            )
                        })
                    })
                } else {
                    None
                };
                doc.query_messages.push(format!("Query failed · {message}"));
            }
        }
        if let Some((sql, connection_id, schema, started_at, duration_ms)) = history {
            super::query_history_events::record_query_history(
                query_editor,
                QueryHistoryRecord {
                    sql,
                    connection_id,
                    schema,
                    started_at,
                    status: UiQueryHistoryStatus::Failed,
                    duration_ms,
                    row_count: None,
                    affected_rows: None,
                    error_code: code,
                    error_summary: Some(message.clone()),
                },
            );
        }
        let is_active_doc = query_session
            .documents
            .get(query_session.active_document_index)
            .is_some_and(|doc| target_doc_id.as_ref() == Some(&doc.id));

        if is_active_doc {
            feedback.set_runtime_message(format!("Query failed · {message}"));
            workspace.bottom_panel_open = true;
            query_editor.query_output_dock_maximized = false;
            if let Some(doc_id) = target_doc_id {
                query_output.tabs_by_document.insert(doc_id, OutputTab::Messages);
                query_output.active_tab = OutputTab::Messages;
            }
        }
    } else if let Some(doc_index) = query_session
        .documents
        .iter()
        .position(|doc| doc.explain_request == Some(request_id))
    {
        let doc_id = query_session.documents[doc_index].id.clone();
        query_output.tabs_by_document.insert(doc_id, OutputTab::Messages);
        query_output.active_tab = OutputTab::Messages;
        workspace.bottom_panel_open = true;
        query_editor.query_output_dock_maximized = false;
        let message = format!("Explain failed · {message}");
        feedback.set_runtime_message(message);
        if let Some(doc) = query_session.documents.get_mut(doc_index) {
            doc.explain_request = None;
            doc.explain_plan = None;
            doc.query_messages.push(feedback.runtime_message.clone());
        }
    } else {
        feedback.set_runtime_message(format!("Operation failed · {message}"));
    }
}
