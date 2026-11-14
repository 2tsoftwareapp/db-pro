// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::*;

impl DbProApp {
    pub(crate) fn draw_discard_changes_confirmation(&mut self, ui: &mut egui::Ui) {
        if !self.table.editing.discard_changes_confirmation {
            return;
        }
        let action = table_mutation_dialog_surface::DiscardChangesContext {
            theme: self.theme,
            counts: self.table.mutation.staged_changes.counts(),
        }
        .draw(ui);
        match action {
            Some(table_mutation_dialog_surface::DiscardChangesAction::Apply) => {
                self.table.editing.discard_changes_confirmation = false;
                self.apply_staged_changes();
            }
            Some(table_mutation_dialog_surface::DiscardChangesAction::Discard) => {
                self.table.editing.discard_changes_confirmation = false;
                let pending = self.workspace.pending_navigation_action.take();
                self.discard_staged_changes();
                if let Some(action) = pending {
                    self.execute_pending_navigation(action);
                }
            }
            Some(table_mutation_dialog_surface::DiscardChangesAction::Cancel) => {
                self.table.editing.discard_changes_confirmation = false;
                self.workspace.pending_navigation_action = None;
            }
            None => {}
        }
    }

    pub(crate) fn draw_pending_changes_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.table.mutation.pending_changes_open {
            return;
        }
        let entries: Vec<StagedChange> = self.table.mutation.staged_changes.iter().cloned().collect();
        let actions = table_mutation_dialog_surface::PendingChangesContext {
            theme: self.theme,
            changes: &entries,
        }
        .draw(ui);
        for action in actions {
            match action {
                table_mutation_dialog_surface::PendingChangesAction::RevertCell(identity, column_index) => {
                    self.table.mutation.staged_changes.revert_cell(&identity, column_index);
                    self.table
                        .mutation
                        .clear_error_for_identity(&identity, Some(column_index));
                }
                table_mutation_dialog_surface::PendingChangesAction::RevertRow(identity) => {
                    self.table.mutation.staged_changes.revert_row(&identity);
                    self.table.mutation.clear_error_for_identity(&identity, None);
                }
                table_mutation_dialog_surface::PendingChangesAction::RemoveInsert(local_id) => {
                    self.table.mutation.staged_changes.remove_insert(local_id);
                    self.table.mutation.table_mutation_error = None;
                }
                table_mutation_dialog_surface::PendingChangesAction::RequestDiscardAll => {
                    self.table.editing.discard_changes_confirmation = true;
                }
                table_mutation_dialog_surface::PendingChangesAction::Close => {
                    self.table.mutation.pending_changes_open = false;
                }
            }
        }
    }

    /// Mandatory apply gate for staged changes that include deletes.
    pub(crate) fn draw_staged_apply_confirmation(&mut self, ui: &mut egui::Ui) {
        if !self.table.mutation.staged_apply_confirm {
            return;
        }
        let counts = self.table.mutation.staged_changes.counts();
        let table = self.schema.explorer.selected_table.clone().unwrap_or_default();
        let action = table_mutation_dialog_surface::StagedApplyConfirmContext {
            theme: self.theme,
            counts,
            table,
        }
        .draw(ui);
        match action {
            Some(table_mutation_dialog_surface::StagedApplyConfirmAction::Apply) => {
                // Keep the flag set so `apply_staged_changes` knows this call
                // passed the gate; dispatch clears it.
                self.apply_staged_changes();
            }
            Some(table_mutation_dialog_surface::StagedApplyConfirmAction::Cancel) => {
                self.table.mutation.staged_apply_confirm = false;
            }
            None => {}
        }
    }

    pub(crate) fn conflict_keep_mine(&mut self) {
        let Some(failure) = self.table.mutation.table_mutation_error.as_ref() else {
            return;
        };
        let Some(target) = failure.target.clone() else {
            return;
        };
        match &target {
            MutationTarget::Update { identity, .. } => {
                if let Some(result) = self.table.data_query.result_ref() {
                    let col_map: std::collections::HashMap<&str, usize> = result
                        .columns
                        .iter()
                        .enumerate()
                        .map(|(i, col)| (col.name.as_str(), i))
                        .collect();
                    if let Some(row_index) = result
                        .rows
                        .iter()
                        .position(|row| table_events::row_matches_identity(row, &col_map, identity))
                    {
                        let current_db_row = &result.rows[row_index];
                        for (col_idx, value) in current_db_row.iter().enumerate() {
                            self.table.mutation.staged_changes.update_original_baseline(
                                identity,
                                col_idx,
                                value.clone(),
                            );
                        }
                    }
                }
                self.table.mutation.table_mutation_retry_target = Some(target);
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
                self.apply_staged_changes();
            }
            MutationTarget::Delete { .. } => {
                self.table.mutation.table_mutation_retry_target = Some(target);
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
                self.apply_staged_changes();
            }
            MutationTarget::Insert => {
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
                self.apply_staged_changes();
            }
        }
    }

    pub(crate) fn conflict_use_database(&mut self) {
        let Some(failure) = self.table.mutation.table_mutation_error.as_ref() else {
            return;
        };
        let Some(target) = failure.target.clone() else {
            return;
        };
        match &target {
            MutationTarget::Update { identity, .. } => {
                self.table.mutation.staged_changes.revert_row(identity);
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
                self.feedback
                    .show_info_toast("Reverted local changes; adopted database values");
            }
            MutationTarget::Delete { identity, .. } => {
                self.table.mutation.staged_changes.revert_row(identity);
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
                self.feedback.show_info_toast("Reverted staged delete");
            }
            MutationTarget::Insert => {
                self.table.mutation.table_mutation_error = None;
                self.table.mutation.conflict_dialog_open = false;
            }
        }
    }

    pub(crate) fn draw_conflict_dialog(&mut self, ui: &mut egui::Ui, result: &UiQueryResult) {
        if !self.table.mutation.conflict_dialog_open {
            return;
        }
        let Some(target) = self
            .table
            .mutation
            .table_mutation_error
            .as_ref()
            .and_then(|failure| failure.target.clone())
        else {
            self.table.mutation.conflict_dialog_open = false;
            return;
        };
        let current_row = match &target {
            MutationTarget::Update {
                identity,
                current_row_index,
                ..
            } => self
                .table_mutation_context()
                .current_row_index_for_identity(identity, *current_row_index)
                .and_then(|index| result.rows.get(index)),
            _ => None,
        };
        let action = table_conflict_dialog_surface::ConflictDialogContext {
            theme: self.theme,
            result,
            target: &target,
            staged_changes: &self.table.mutation.staged_changes,
            current_row,
        }
        .draw(ui);
        if let Some(action) = action {
            self.apply_conflict_dialog_action(action);
        }
    }

    fn apply_conflict_dialog_action(&mut self, action: table_conflict_dialog_surface::ConflictDialogAction) {
        match action {
            table_conflict_dialog_surface::ConflictDialogAction::Close => {
                self.table.mutation.conflict_dialog_open = false;
            }
            table_conflict_dialog_surface::ConflictDialogAction::KeepMine => {
                self.conflict_keep_mine();
            }
            table_conflict_dialog_surface::ConflictDialogAction::UseDatabase => {
                self.conflict_use_database();
            }
            table_conflict_dialog_surface::ConflictDialogAction::Retry => {
                self.table.mutation.conflict_dialog_open = false;
                self.retry_failed_mutation_after_reload();
            }
            table_conflict_dialog_surface::ConflictDialogAction::Discard => {
                self.table.mutation.conflict_dialog_open = false;
                self.discard_failed_mutation(false);
            }
        }
    }
}
