//! Table runtime event reducers.

use super::*;
use crate::RequestId;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TableEventTransition {
    pub(crate) invalidate_grid_caches: bool,
    pub(crate) apply_staged_changes: bool,
    /// Discard the current grid result and re-issue a full table-data request.
    pub(crate) reload_grid: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TableFailureTransition {
    Handled,
    StagedApplyFailed,
    /// The failed request was a workbench/table ExecuteDdl run.
    DdlFailed,
}

/// Routes a table-related failure to the matching request slot.
pub(crate) fn handle_table_request_failure(
    table_state: &mut TableState,
    data_query: &mut TableDataQueryState,
    table_mutation: &mut TableMutationState,
    table_editing: &mut TableEditingState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    message: &str,
) -> Option<TableFailureTransition> {
    if table_mutation.staged_apply_request == Some(request_id) {
        // Older runtimes can still report the generic failure event. Keep
        // the staged changes and let the root apply the unmapped mutation result.
        return Some(TableFailureTransition::StagedApplyFailed);
    }
    if table_mutation.table_mutation_request == Some(request_id) {
        table_mutation.table_mutation_request = None;
        table_editing.data_editing_cell = None;
        table_editing.data_edit_value.clear();
        table_editing.data_edit_error = None;
        table_editing.data_delete_confirmation = false;
        let formatted = format!("Row mutation failed · {message}");
        feedback.set_runtime_message(formatted.clone());
        feedback.show_error_toast(formatted);
        return Some(TableFailureTransition::Handled);
    }
    if table_state.table_info_request == Some(request_id) {
        table_state.table_info_request = None;
        table_state.table_info_error = Some(message.to_owned());
        feedback.set_runtime_message(format!("Table structure failed · {message}"));
        return Some(TableFailureTransition::Handled);
    }
    if table_state.table_ddl_request == Some(request_id) {
        table_state.table_ddl_request = None;
        table_state.table_ddl_error = Some(message.to_owned());
        feedback.set_runtime_message(format!("Table DDL failed · {message}"));
        return Some(TableFailureTransition::Handled);
    }
    if data_query.row_reload_request == Some(request_id) {
        data_query.row_reload_request = None;
        data_query.row_reload_identity = None;
        table_mutation.table_mutation_retry_after_reload = false;
        table_mutation.table_mutation_retry_target = None;
        feedback.set_runtime_message(format!("Could not reload row: {message}"));
        return Some(TableFailureTransition::Handled);
    }
    if data_query.request == Some(request_id) {
        data_query.request = None;
        table_mutation.table_mutation_retry_after_reload = false;
        table_mutation.table_mutation_retry_target = None;
        data_query.error = Some(message.to_owned());
        let formatted = format!("Table data failed · {message}");
        feedback.set_runtime_message(formatted.clone());
        feedback.show_error_toast(formatted);
        return Some(TableFailureTransition::Handled);
    }
    if table_state.ddl_execution_request == Some(request_id) {
        table_state.ddl_execution_request = None;
        table_state.ddl_execute_confirmation = false;
        table_state.table_ddl_error = Some(message.to_owned());
        let formatted = format!("DDL execution failed · {message}");
        feedback.set_runtime_message(formatted.clone());
        feedback.show_error_toast(formatted);
        return Some(TableFailureTransition::DdlFailed);
    }
    None
}

/// Applies table metadata and seeds the first filter/sort choices.
pub(crate) fn on_table_info_loaded(
    table_state: &mut TableState,
    data_query: &mut TableDataQueryState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    table_info: UiTableInfo,
) -> Option<TableEventTransition> {
    if table_state.table_info_request != Some(request_id) {
        return None;
    }
    if data_query.filter_column.is_empty() {
        data_query.filter_column = table_info
            .columns
            .first()
            .map(|column| column.name.clone())
            .unwrap_or_default();
    }
    if data_query.sorts.is_empty() {
        if let Some(column) = table_info
            .primary_key
            .as_ref()
            .and_then(|columns| columns.first().cloned())
            .or_else(|| table_info.columns.first().map(|column| column.name.clone()))
        {
            data_query.sorts.push(UiTableDataSort {
                column,
                descending: false,
            });
        }
    }
    table_state.table_info = Some(table_info);
    table_state.table_info_error = None;
    table_state.table_info_request = None;
    feedback.set_runtime_message("Table structure loaded");
    Some(TableEventTransition {
        invalidate_grid_caches: true,
        ..Default::default()
    })
}

pub(crate) fn on_table_ddl_loaded(
    table_state: &mut TableState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    sql: String,
) {
    if table_state.table_ddl_request != Some(request_id) {
        return;
    }
    table_state.table_ddl = Some(sql.clone());
    table_state.table_ddl_baseline = Some(sql);
    table_state.ddl_execute_confirmation = false;
    table_state.table_ddl_discard_confirm = false;
    table_state.table_ddl_error = None;
    table_state.table_ddl_request = None;
    feedback.set_runtime_message("Table DDL loaded");
}

/// Applies table data and returns the side effects that need root orchestration.
pub(crate) fn on_table_data_loaded(
    data_query: &mut TableDataQueryState,
    table_mutation: &mut TableMutationState,
    table_data: &mut TableDataState,
    feedback: &mut FeedbackState,
    request_id: RequestId,
    result: UiQueryResult,
    total_rows: u64,
) -> Option<TableEventTransition> {
    if data_query.row_reload_request == Some(request_id) {
        return Some(on_table_row_reloaded(data_query, table_mutation, feedback, result));
    }
    if data_query.request != Some(request_id) {
        return None;
    }
    if data_query.filter_column.is_empty() {
        data_query.filter_column = result
            .columns
            .first()
            .map(|column| column.name.clone())
            .unwrap_or_default();
    }
    if data_query.sorts.is_empty() {
        if let Some(column) = result.columns.first().map(|column| column.name.clone()) {
            data_query.sorts.push(UiTableDataSort {
                column,
                descending: false,
            });
        }
    }
    data_query.set_result(result);
    data_query.total_rows = Some(total_rows);
    data_query.inline_query_result = false;
    data_query.last_result_row_limit = None;
    let clear_selection = table_mutation.staged_changes.is_empty();
    if clear_selection {
        table_data.selected_cell = None;
        table_data.selected_row = None;
        table_data.selected_rows.clear();
        table_data.selection_anchor_row = None;
        table_data.selection_anchor_cell = None;
    }
    data_query.error = None;
    data_query.request = None;
    feedback.set_runtime_message(format!("Table data loaded · {total_rows} rows"));
    Some(TableEventTransition {
        invalidate_grid_caches: true,
        apply_staged_changes: table_mutation.table_mutation_retry_after_reload,
        ..Default::default()
    })
}

pub(crate) fn on_table_row_reloaded(
    data_query: &mut TableDataQueryState,
    table_mutation: &mut TableMutationState,
    feedback: &mut FeedbackState,
    result: UiQueryResult,
) -> TableEventTransition {
    data_query.row_reload_request = None;
    let Some(identity) = data_query.row_reload_identity.take() else {
        return TableEventTransition::default();
    };
    let UiQueryResult { columns: server_columns, rows: server_rows, .. } = result;
    if server_rows.is_empty() {
        feedback.set_runtime_message("Row was deleted");
        if table_mutation.table_mutation_retry_after_reload {
            table_mutation.table_mutation_retry_after_reload = false;
            table_mutation.table_mutation_retry_target = None;
        }
        return TableEventTransition::default();
    }

    // A reload that returns columns in a different order than the visible grid would write cells
    // under the wrong headers. Discard the result and reload the page instead of patching.
    let column_layout_matches = data_query
        .result
        .as_ref()
        .is_some_and(|table_result| table_result.columns == server_columns);
    if !column_layout_matches {
        feedback.set_runtime_message("Table columns changed — reloading table data");
        return TableEventTransition { reload_grid: true, ..Default::default() };
    }

    if server_rows.len() > 1 {
        // The reload WHERE is expected to match one row; more rows means the identity matched
        // multiple rows, so a positional patch could target the wrong one.
        feedback.set_runtime_message("Row reload matched multiple rows — reloading table data");
        return TableEventTransition { reload_grid: true, ..Default::default() };
    }
    let server_row = server_rows.into_iter().next().expect("checked non-empty");

    let mut replaced_row = false;
    if let Some(table_result) = data_query.result_mut() {
        let column_indexes: std::collections::HashMap<&str, usize> = table_result
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.name.as_str(), index))
            .collect();
        if let Some(row_index) = table_result
            .rows
            .iter()
            .position(|row| row_matches_identity(row, &column_indexes, &identity))
        {
            table_result.rows[row_index] = server_row;
            replaced_row = true;
        }
    }
    data_query.error = None;
    feedback.set_runtime_message("Row reloaded from database");
    TableEventTransition {
        invalidate_grid_caches: replaced_row,
        apply_staged_changes: table_mutation.table_mutation_retry_after_reload,
        reload_grid: false,
    }
}

pub(crate) fn row_matches_identity(
    row: &[UiCell],
    column_indexes: &std::collections::HashMap<&str, usize>,
    identity: &RowIdentity,
) -> bool {
    identity
        .original_pk_columns
        .iter()
        .zip(&identity.original_pk_values)
        .all(|(column, value)| {
            column_indexes
                .get(column.as_str())
                .and_then(|index| row.get(*index))
                .is_some_and(|candidate| candidate == value)
        })
}
