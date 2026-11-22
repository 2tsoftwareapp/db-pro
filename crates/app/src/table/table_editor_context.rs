//! Explicit table-editor state transitions.

use super::table_mutation_state::TableApplyPlan;
use super::{
    ColumnWritePolicy, ConnectionCatalogState, ConnectionLifecycleState, FeedbackState, MutationTarget, RowIdentity,
    StagedChange, TableDataQueryState, TableDataState, TableEditingState, TableMutationState, TableState, UiCell,
    UiQueryResult,
};

pub(crate) fn can_mutate_active_connection(
    catalog: &ConnectionCatalogState,
    lifecycle: &ConnectionLifecycleState,
) -> bool {
    lifecycle.is_connected()
        && lifecycle
            .active_connection_id()
            .and_then(|id| catalog.find(id))
            .is_some_and(|connection| !connection.readonly)
}

pub(crate) const VIEW_ROWS_READ_ONLY: &str = "Views are read-only. Open the base table to edit rows.";
pub(crate) const INLINE_SQL_READ_ONLY: &str = "Inline SQL results are read-only.";

/// Which grid is on screen. Foreign keys are not part of this decision.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowEditSurface {
    Table,
    Query,
    View,
    InlineSql,
}

pub(crate) fn surface_read_only_notice(surface: RowEditSurface) -> Option<&'static str> {
    match surface {
        RowEditSurface::View => Some(VIEW_ROWS_READ_ONLY),
        RowEditSurface::InlineSql => Some(INLINE_SQL_READ_ONLY),
        RowEditSurface::Table | RowEditSurface::Query => None,
    }
}

/// `None` when this result can be edited. A NULL primary-key value is refused per row.
pub(crate) fn result_edit_reason(
    surface: RowEditSurface,
    writable: bool,
    table_known: bool,
    primary_key: Option<&[String]>,
    result_columns: &[crate::UiColumn],
) -> Option<String> {
    if let Some(notice) = surface_read_only_notice(surface) {
        return Some(notice.to_owned());
    }
    if !writable {
        return Some("Connect with write access to edit rows".to_owned());
    }
    if !table_known {
        return Some(if surface == RowEditSurface::Query {
            "Open the table so its primary key can identify these rows.".to_owned()
        } else {
            "Table structure is still loading".to_owned()
        });
    }
    let Some(identity_columns) = primary_key.filter(|columns| !columns.is_empty()) else {
        return Some("Table has no primary key; safe row editing is unavailable.".to_owned());
    };
    for column in identity_columns {
        if !result_columns.iter().any(|item| item.name == *column) {
            return Some(format!("The key column {column} is not present in this result"));
        }
    }
    None
}

pub(crate) struct TableMutationContext<'a> {
    table_state: &'a mut TableState,
    table_data_query: &'a mut TableDataQueryState,
    table_data: &'a mut TableDataState,
    table_editing: &'a mut TableEditingState,
    table_mutation: &'a mut TableMutationState,
    feedback: &'a mut FeedbackState,
    selected_table: Option<&'a str>,
}

pub(crate) fn staged_cell_value(
    table_data: &TableDataState,
    table_state: &TableState,
    table_mutation: &TableMutationState,
    result: &UiQueryResult,
    row_index: usize,
    column_index: usize,
) -> Option<UiCell> {
    let identity = table_data.row_identity_for_result(result, table_state.table_info.as_ref(), row_index)?;
    table_mutation.staged_changes.cell_value(&identity, column_index)
}

pub(crate) fn staged_row_deleted(
    table_data: &TableDataState,
    table_state: &TableState,
    table_mutation: &TableMutationState,
    result: &UiQueryResult,
    row_index: usize,
) -> bool {
    let Some(identity) = table_data.row_identity_for_result(result, table_state.table_info.as_ref(), row_index) else {
        return false;
    };
    table_mutation.staged_changes.row_deleted(&identity)
}

impl<'a> TableMutationContext<'a> {
    pub(crate) fn new(
        table_state: &'a mut TableState,
        table_data_query: &'a mut TableDataQueryState,
        table_data: &'a mut TableDataState,
        table_editing: &'a mut TableEditingState,
        table_mutation: &'a mut TableMutationState,
        feedback: &'a mut FeedbackState,
        selected_table: Option<&'a str>,
    ) -> Self {
        Self {
            table_state,
            table_data_query,
            table_data,
            table_editing,
            table_mutation,
            feedback,
            selected_table,
        }
    }

    pub(crate) fn stage_data_cell_edit(
        &mut self,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
    ) -> bool {
        let Some(info) = self.table_state.table_info.clone() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return false;
        };
        let Some(column) = result.columns.get(column_index).map(|column| column.name.clone()) else {
            self.table_editing.data_editing_cell = None;
            return false;
        };
        // #537: write-boundary guard — the display prefix must never overwrite the full value,
        // no matter which UI path staged this edit.
        if result
            .rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .is_some_and(UiCell::is_truncated)
        {
            let error = t!("query.cell_truncated_for_display").into_owned();
            self.feedback.runtime_message = error.clone();
            self.table_editing.data_edit_error = Some(error);
            return false;
        }
        let Some(column_info) = info.columns.iter().find(|item| item.name == column) else {
            self.feedback.runtime_message = "The selected column is not present in the table metadata".to_owned();
            self.table_editing.data_editing_cell = None;
            return false;
        };
        if let Some(block) = ColumnWritePolicy::read(column_info).write_block() {
            let error = block.reason().to_owned();
            self.table_editing.data_edit_error = Some(error.clone());
            self.feedback.runtime_message = format!("{}: {error}", column_info.name);
            return false;
        }
        let value = match super::table_editor_values::parse_update_value(
            &self.table_editing.data_edit_value,
            &column_info.data_type,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.feedback.runtime_message = format!("{}: {error}", column_info.name);
                self.table_editing.data_edit_error = Some(error);
                return false;
            }
        };
        if matches!(value, UiCell::Null) && !column_info.nullable {
            let error = format!("{} is NOT NULL; enter a value instead", column_info.name);
            self.feedback.runtime_message = error.clone();
            self.table_editing.data_edit_error = Some(error);
            return false;
        }
        let manual = self.table_data.manual_row_identity.clone();
        let identity = match TableDataState::row_identity(result, &info, manual.as_deref(), row_index) {
            Ok(identity) => identity,
            Err(error) => {
                self.feedback.runtime_message = error;
                self.table_editing.data_edit_error = Some(self.feedback.runtime_message.clone());
                return false;
            }
        };
        let Some(original) = result
            .rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .cloned()
        else {
            self.table_editing.data_editing_cell = None;
            self.feedback.runtime_message = "The selected cell is no longer available".to_owned();
            self.table_editing.data_edit_error = Some(self.feedback.runtime_message.clone());
            return false;
        };
        if let Some(table) = self.selected_table {
            self.table_mutation.staged_changes.ensure_target(table);
        }
        self.table_mutation.staged_changes.stage_update(StagedChange::Update {
            identity,
            current_row_index: Some(row_index),
            column_index,
            column,
            data_type: column_info.data_type.clone(),
            original,
            value,
        });
        self.table_mutation.table_mutation_error = None;
        self.table_mutation.staged_apply_targets.clear();
        self.table_editing.data_editing_cell = None;
        self.table_editing.expanded_data_editor = None;
        self.table_editing.data_edit_error = None;
        let counts = self.table_mutation.staged_changes.counts();
        self.feedback.runtime_message = format!(
            "Staged edit · {} pending (+{} ~{} -{})",
            counts.total(),
            counts.inserts,
            counts.updates,
            counts.deletes
        );
        true
    }

    pub(crate) fn prepare_apply(&mut self, table: &str) -> Option<TableApplyPlan> {
        if self.table_editing.data_edit_error.is_some() {
            self.feedback.runtime_message = "Fix the validation error before applying changes".to_owned();
            return None;
        }
        if let Some(target) = self.table_mutation.staged_changes.target_table() {
            if target != table {
                self.feedback.runtime_message = format!("Staged changes belong to table `{target}`, not `{table}`");
                return None;
            }
        }
        let plan = self.table_mutation.build_apply_plan();
        if plan.changes.is_empty() {
            self.table_mutation.table_mutation_retry_after_reload = false;
            self.feedback.runtime_message = "The related staged change is no longer available".to_owned();
            return None;
        }
        Some(plan)
    }

    pub(crate) fn select_failed_target(&mut self, target: &MutationTarget) {
        match target {
            MutationTarget::Update {
                identity,
                current_row_index,
                columns,
            } => {
                let row_index = self.current_row_index_for_identity(identity, *current_row_index);
                self.select_failed_row(row_index);
                if let (Some(row_index), Some(column_index)) = (row_index, columns.first().copied()) {
                    self.table_data.selected_cell = Some((row_index, column_index));
                    self.table_data.selection_anchor_cell = Some((row_index, column_index));
                }
            }
            MutationTarget::Delete {
                identity,
                current_row_index,
            } => {
                let row_index = self.current_row_index_for_identity(identity, *current_row_index);
                self.select_failed_row(row_index);
                self.table_data.selected_cell = None;
                self.table_data.selection_anchor_cell = None;
            }
            MutationTarget::Insert => {}
        }
    }

    fn select_failed_row(&mut self, row_index: Option<usize>) {
        self.table_data.selected_row = row_index;
        self.table_data.selected_rows.clear();
        if let Some(row_index) = row_index {
            self.table_data.selected_rows.insert(row_index);
        }
    }

    pub(crate) fn current_row_index_for_identity(
        &self,
        identity: &RowIdentity,
        fallback: Option<usize>,
    ) -> Option<usize> {
        self.table_data_query
            .result
            .as_ref()
            .and_then(|result| {
                result.rows.iter().enumerate().find_map(|(row_index, _)| {
                    (self
                        .table_data
                        .row_identity_for_result(result, self.table_state.table_info.as_ref(), row_index)
                        .as_ref()
                        == Some(identity))
                    .then_some(row_index)
                })
            })
            .or(fallback)
    }

    pub(crate) fn stage_delete_selected_rows(&mut self, result: &UiQueryResult) -> bool {
        let row_indexes = self.selected_row_indexes();
        if row_indexes.is_empty() {
            self.feedback.runtime_message = "Select a row before deleting".to_owned();
            return false;
        }
        let Some(info) = self.table_state.table_info.clone() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return false;
        };
        if let Some(table) = self.selected_table {
            self.table_mutation.staged_changes.ensure_target(table);
        }
        self.table_editing.data_editing_cell = None;
        self.table_editing.expanded_data_editor = None;
        self.table_editing.data_edit_value.clear();
        self.table_editing.data_edit_error = None;
        self.table_editing.data_delete_confirmation = false;
        // Validate every row identity before staging anything: aborting mid-loop would otherwise
        // leave a partial delete set in `staged_changes` that the user never approved.
        let manual = self.table_data.manual_row_identity.clone();
        let mut identities = Vec::with_capacity(row_indexes.len());
        for row_index in row_indexes {
            if staged_row_deleted(
                self.table_data,
                self.table_state,
                self.table_mutation,
                result,
                row_index,
            ) {
                continue;
            }
            match TableDataState::row_identity(result, &info, manual.as_deref(), row_index) {
                Ok(identity) => identities.push((row_index, identity)),
                Err(error) => {
                    self.feedback.runtime_message = error;
                    return false;
                }
            }
        }
        for (row_index, identity) in identities {
            self.table_mutation.staged_changes.stage_delete(StagedChange::Delete {
                identity,
                current_row_index: Some(row_index),
            });
        }
        self.table_mutation.table_mutation_error = None;
        self.table_mutation.staged_apply_targets.clear();
        let deleted_count = self.table_mutation.staged_changes.counts().deletes;
        self.feedback.runtime_message = format!(
            "{} row(s) marked for deletion · {} staged change(s)",
            deleted_count,
            self.table_mutation.staged_changes.counts().total()
        );
        true
    }

    fn selected_row_indexes(&self) -> Vec<usize> {
        if self.table_data.selected_rows.is_empty() {
            self.table_data.selected_row.into_iter().collect()
        } else {
            self.table_data.selected_rows.iter().copied().collect()
        }
    }

    pub(crate) fn revert_staged_cell(&mut self, result: &UiQueryResult, row_index: usize, column_index: usize) {
        let Some(identity) =
            self.table_data
                .row_identity_for_result(result, self.table_state.table_info.as_ref(), row_index)
        else {
            return;
        };
        if self.table_mutation.staged_changes.revert_cell(&identity, column_index) {
            self.table_mutation
                .clear_error_for_identity(&identity, Some(column_index));
            self.feedback.set_runtime_message("Cell change reverted");
        }
    }

    pub(crate) fn revert_staged_row(&mut self, result: &UiQueryResult, row_index: usize) {
        let Some(identity) =
            self.table_data
                .row_identity_for_result(result, self.table_state.table_info.as_ref(), row_index)
        else {
            return;
        };
        if self.table_mutation.staged_changes.revert_row(&identity) {
            self.table_mutation.clear_error_for_identity(&identity, None);
            self.feedback.set_runtime_message("Row changes reverted");
        }
    }

    /// Clears staged UI state and reports whether a data reload is required.
    pub(crate) fn discard_staged_changes(&mut self) -> bool {
        if self.table_mutation.staged_apply_request.is_some() {
            self.feedback
                .set_runtime_message("Wait for the current database write before discarding");
            return false;
        }
        self.table_mutation.staged_changes.clear();
        self.table_mutation.staged_apply_targets.clear();
        self.table_mutation.table_mutation_error = None;
        self.table_editing.data_editing_cell = None;
        self.table_editing.expanded_data_editor = None;
        self.table_editing.data_edit_error = None;
        self.table_editing.data_delete_confirmation = false;
        self.table_editing.discard_changes_confirmation = false;
        self.table_mutation.pending_changes_open = false;
        self.table_mutation.staged_apply_confirm = false;
        self.table_editing.data_edit_value.clear();
        self.table_data_query.clear_result();
        self.table_data_query.error = None;
        self.feedback.set_runtime_message("Staged changes discarded");
        true
    }
}
