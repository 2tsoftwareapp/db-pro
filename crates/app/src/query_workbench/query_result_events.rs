//! Single-statement query result reducer over explicit query feature state.

use super::events::QueryHistoryRecord;
use super::*;
use crate::RequestId;

pub(super) struct QueryResultContext<'a> {
    pub(super) query_session: &'a mut QuerySessionState,
    pub(super) query_editor: &'a mut QueryEditorState,
    pub(super) query_output: &'a mut QueryOutputState,
    pub(super) table_data: &'a mut TableDataState,
    pub(super) table_data_query: &'a mut TableDataQueryState,
    pub(super) workspace: &'a mut WorkspaceShellState,
    pub(super) feedback: &'a mut FeedbackState,
}

pub(super) fn on_query_completed(context: &mut QueryResultContext<'_>, request_id: RequestId, result: UiQueryResult) {
    let QueryResultContext {
        query_session,
        query_editor,
        query_output,
        table_data,
        table_data_query,
        workspace,
        feedback,
    } = context;
    if table_data_query.complete_inline_query(request_id) {
        table_data_query.set_result(result.clone());
        table_data_query.total_rows = None;
        table_data_query.offset = 0;
        table_data_query.error = None;
        table_data.grid_sort_column = None;
        table_data.grid_column_widths = vec![180.0; result.columns.len()];
        table_data.selected_cell = None;
        table_data.selected_row = None;
        table_data.selected_rows.clear();
        table_data.selection_anchor_row = None;
        table_data.selection_anchor_cell = None;
        // UAT-4: report the applied cap without claiming the result was
        // truncated; providers do not probe for an extra row.
        let limit_note = query_row_limit::row_limit_note(table_data_query.last_result_row_limit);
        feedback.set_runtime_message(format!("Query completed · {} rows{limit_note}", result.row_count));
    }
    let target_doc_id = query_session.document_requests.remove(&request_id);
    // A new result set replaces the rows behind the grid, so nothing the projection cache holds
    // may survive it.
    table_data.invalidate_grid_projection();
    let completed_row_count = result.row_count;
    let completed_column_count = result.columns.len();
    // Capture the cap before `executing_row_limit` is cleared. The result
    // header retains it and completion messages report it (UAT-4).
    let mut limit_note = String::new();
    let mut history = None;
    if let Some(doc_id) = &target_doc_id {
        if let Some(doc) = query_session.documents.iter_mut().find(|doc| &doc.id == doc_id) {
            let (started_at, duration_ms) = doc.take_execution_timing(result.duration_ms);
            history = Some((
                doc.executing_sql.clone().unwrap_or_else(|| doc.text().to_owned()),
                doc.connection_id.clone(),
                doc.schema.clone(),
                started_at,
                duration_ms,
            ));
            // One owned copy. `query_result` and `execution_output.result_set`
            // used to each hold another full grid.
            doc.query_results = vec![std::rc::Rc::new(result)];
            doc.query_result_row_limit = doc.executing_row_limit.take();
            limit_note = query_row_limit::row_limit_note(doc.query_result_row_limit);
            doc.query_result = None;
            doc.active_result_index = 0;
            doc.execution_output = Some(UiQueryExecutionOutput {
                statements: vec![UiStatementOutput {
                    statement_index: 0,
                    result_set: None,
                    affected_rows: None,
                    duration_ms,
                    message: None,
                    error: None,
                }],
                total_duration_ms: duration_ms,
            });
            doc.execution_state = QueryExecutionState::Idle;
            doc.executing_range = None;
            doc.executing_sql = None;
            doc.executing_row_limit = None;
            doc.executing_version = None;
            doc.execution_diagnostic = None;
            doc.query_messages
                .push(format!("Query completed · {completed_row_count} rows{limit_note}"));
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
                status: UiQueryHistoryStatus::Success,
                duration_ms,
                row_count: Some(completed_row_count),
                affected_rows: None,
                error_code: None,
                error_summary: None,
            },
        );
    }
    let is_active_doc = query_session
        .documents
        .get(query_session.active_document_index)
        .is_some_and(|doc| target_doc_id.as_ref() == Some(&doc.id));

    if is_active_doc {
        feedback.set_runtime_message(format!("Query completed · {completed_row_count} rows{limit_note}"));
        table_data.grid_sort_column = None;
        table_data.grid_column_widths = vec![180.0; completed_column_count];
        table_data.selected_cell = None;
        table_data.selected_row = None;
        table_data.selected_rows.clear();
        table_data.selection_anchor_row = None;
        table_data.selection_anchor_cell = None;
        feedback.copy_status.clear();
        if let Some(doc_id) = target_doc_id {
            query_output.tabs_by_document.insert(doc_id.clone(), OutputTab::Results);
            if query_session
                .documents
                .get(query_session.active_document_index)
                .is_some_and(|doc| doc.id == doc_id)
            {
                query_output.active_tab = OutputTab::Results;
            }
        }
        workspace.bottom_panel_open = true;
        query_editor.query_output_dock_maximized = false;
    }
}
