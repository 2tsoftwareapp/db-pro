//! Multi-statement query result reducer over explicit query feature state.

use super::events::QueryHistoryRecord;
use super::*;
use crate::RequestId;

pub(super) struct QueryMultiResultContext<'a> {
    pub(super) query_session: &'a mut QuerySessionState,
    pub(super) query_editor: &'a mut QueryEditorState,
    pub(super) query_output: &'a mut QueryOutputState,
    pub(super) table_data: &'a mut TableDataState,
    pub(super) workspace: &'a mut WorkspaceShellState,
    pub(super) feedback: &'a mut FeedbackState,
}

pub(super) fn on_query_multi_completed(
    context: &mut QueryMultiResultContext<'_>,
    request_id: RequestId,
    mut output: UiQueryExecutionOutput,
) {
    let QueryMultiResultContext {
        query_session,
        query_editor,
        query_output,
        table_data,
        workspace,
        feedback,
    } = context;
    let target_doc_id = query_session.document_requests.remove(&request_id);
    let Some(doc_id) = target_doc_id else {
        return;
    };
    let result_row_count: u64 = output
        .statements
        .iter()
        .filter_map(|statement| statement.result_set.as_ref().map(|result| result.row_count))
        .sum();
    // Same as the single-statement path: the result sets behind the grid are being replaced.
    table_data.invalidate_grid_projection();
    let mut history = None;
    if let Some(doc) = query_session.documents.iter_mut().find(|doc| doc.id == doc_id) {
        let (started_at, duration_ms) = doc.take_execution_timing(output.total_duration_ms);
        history = Some((
            doc.executing_sql.clone().unwrap_or_else(|| doc.text().to_owned()),
            doc.connection_id.clone(),
            doc.schema.clone(),
            started_at,
            duration_ms,
        ));
        doc.query_results = output
            .statements
            .iter_mut()
            .filter_map(|statement| statement.result_set.take())
            .map(std::rc::Rc::new)
            .collect();
        doc.results_stale = false;
        doc.query_result = None;
        doc.query_result_row_limit = doc.executing_row_limit;
        doc.active_result_index = 0;
        doc.execution_output = Some(output.clone());
        doc.execution_state = if output.statements.iter().any(|statement| statement.error.is_some()) {
            QueryExecutionState::Failed
        } else {
            QueryExecutionState::Idle
        };
        let execution_range = doc.executing_range;
        let execution_sql = doc.executing_sql.clone();
        let execution_version = doc.executing_version;
        doc.executing_range = None;
        doc.executing_sql = None;
        doc.executing_row_limit = None;
        doc.executing_version = None;
        doc.execution_diagnostic = output.statements.iter().find_map(|statement| {
            let error = statement.error.as_ref()?;
            if execution_version != Some(doc.buffer.version()) {
                return None;
            }
            let fallback_range = execution_range.unwrap_or((0, doc.buffer.len_bytes()));
            let (statement_sql, statement_range) = doc.analysis.statements.get(statement.statement_index).map_or_else(
                || (execution_sql.as_deref().unwrap_or_default(), fallback_range),
                |parsed| (parsed.text.as_str(), parsed.range),
            );
            super::query_view::database_error_diagnostic(
                &error.message,
                statement_sql,
                statement_range,
                error.position,
                Some(error.code.as_str()),
            )
        });
        for statement in &output.statements {
            if let Some(message) = &statement.message {
                doc.query_messages.push(message.clone());
            }
            if let Some(error) = &statement.error {
                doc.query_messages.push(format!(
                    "Statement {} failed · {}",
                    statement.statement_index + 1,
                    error.message
                ));
            }
        }
    }
    if let Some((sql, connection_id, schema, started_at, duration_ms)) = history {
        let failed = output.statements.iter().any(|statement| statement.error.is_some());
        super::query_history_events::record_query_history(
            query_editor,
            QueryHistoryRecord {
                sql,
                connection_id,
                schema,
                started_at,
                status: if failed {
                    UiQueryHistoryStatus::Failed
                } else {
                    UiQueryHistoryStatus::Success
                },
                duration_ms,
                row_count: Some(result_row_count),
                affected_rows: Some(
                    output
                        .statements
                        .iter()
                        .filter_map(|statement| statement.affected_rows)
                        .sum(),
                ),
                error_code: None,
                error_summary: output
                    .statements
                    .iter()
                    .find_map(|statement| statement.error.as_ref().map(|error| error.message.clone())),
            },
        );
    }
    let is_active_doc = query_session
        .documents
        .get(query_session.active_document_index)
        .is_some_and(|document| document.id == doc_id);
    if is_active_doc {
        let result_count = query_session
            .documents
            .get(query_session.active_document_index)
            .map_or(0, |document| document.query_results.len());
        feedback.set_runtime_message(format!(
            "Script completed · {} result{} · {} ms",
            result_count,
            if result_count == 1 { "" } else { "s" },
            output.total_duration_ms,
        ));
        query_output.tabs_by_document.insert(doc_id, OutputTab::Results);
        query_output.active_tab = OutputTab::Results;
        table_data.grid_sort_column = None;
        table_data.selected_cell = None;
        table_data.selected_row = None;
        table_data.selected_rows.clear();
        workspace.bottom_panel_open = true;
        query_editor.query_output_dock_maximized = false;
    }
}
