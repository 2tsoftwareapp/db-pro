use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedApplyFailureTransition {
    has_target: bool,
    mutation_failure: MutationFailure,
    display_message: String,
    is_conflict: bool,
    reload_identity: Option<RowIdentity>,
}

impl StagedApplyFailureTransition {
    pub(crate) fn has_target(&self) -> bool {
        self.has_target
    }

    pub(crate) fn target(&self) -> Option<&MutationTarget> {
        self.mutation_failure.target.as_ref()
    }

    pub(crate) fn is_conflict(&self) -> bool {
        self.is_conflict
    }

    pub(crate) fn reload_identity(&self) -> Option<&RowIdentity> {
        self.reload_identity.as_ref()
    }

    pub(crate) fn is_rolled_back(&self) -> bool {
        self.mutation_failure.rolled_back
    }

    pub(crate) fn statement_index(&self) -> usize {
        self.mutation_failure.statement_index
    }

    pub(crate) fn display_message(&self) -> &str {
        &self.display_message
    }
}

/// Owns the local table-edit transaction and its asynchronous apply lifecycle.
#[derive(Debug)]
pub(crate) struct TableMutationState {
    pub(super) table_mutation_request: Option<crate::RequestId>,
    pub(super) staged_changes: ChangeSet,
    pub(super) pending_changes_open: bool,
    /// Delete-containing apply awaiting the "+N ~N -N" confirmation gate.
    pub(super) staged_apply_confirm: bool,
    pub(super) staged_apply_request: Option<crate::RequestId>,
    pub(super) staged_apply_targets: Vec<MutationTarget>,
    pub(super) table_mutation_retry_after_reload: bool,
    pub(super) table_mutation_retry_target: Option<MutationTarget>,
    pub(super) table_mutation_error: Option<MutationFailure>,
    pub(super) conflict_dialog_open: bool,
}

impl Default for TableMutationState {
    fn default() -> Self {
        Self {
            table_mutation_request: None,
            staged_changes: ChangeSet::new(),
            pending_changes_open: false,
            staged_apply_confirm: false,
            staged_apply_request: None,
            staged_apply_targets: Vec::new(),
            table_mutation_retry_after_reload: false,
            table_mutation_retry_target: None,
            table_mutation_error: None,
            conflict_dialog_open: false,
        }
    }
}

impl TableMutationState {
    pub(crate) fn reset_apply_lifecycle(&mut self) {
        self.staged_apply_request = None;
        self.table_mutation_request = None;
        self.staged_apply_confirm = false;
        self.table_mutation_retry_after_reload = false;
        self.table_mutation_retry_target = None;
    }

    pub(crate) fn record_apply_failure(
        &mut self,
        failure: super::table_mutation_actions::StagedApplyFailure<'_>,
    ) -> StagedApplyFailureTransition {
        let target = self.staged_apply_targets.get(failure.statement_index).cloned();
        let normalized_code = normalize_failure_code(failure.code);
        let display_message = failure_message(normalized_code, failure.message);
        let reload_identity = conflict_identity(normalized_code, target.as_ref());
        let mutation_failure = MutationFailure {
            statement_index: failure.statement_index,
            target: target.clone(),
            code: normalized_code.to_owned(),
            message: display_message.clone(),
            rolled_back: failure.rolled_back,
        };
        self.table_mutation_error = Some(mutation_failure.clone());
        StagedApplyFailureTransition {
            has_target: target.is_some(),
            mutation_failure,
            display_message,
            is_conflict: normalized_code == "CONFLICT",
            reload_identity,
        }
    }

    pub(crate) fn row_reload_filters(
        info: &UiTableInfo,
        identity: &RowIdentity,
    ) -> Result<Vec<UiTableDataFilter>, String> {
        identity
            .original_pk_columns
            .iter()
            .zip(&identity.original_pk_values)
            .map(|(column, value)| {
                let data_type = info
                    .columns
                    .iter()
                    .find(|candidate| candidate.name == *column)
                    .map(|candidate| candidate.data_type.clone())
                    .ok_or_else(|| format!("Primary-key metadata is missing for {column}"))?;
                Ok(UiTableDataFilter {
                    column: column.clone(),
                    data_type,
                    operator: UiTableFilterOperator::Equals,
                    value: crate::cell_text(value),
                })
            })
            .collect()
    }

    pub(crate) fn build_apply_plan(&mut self) -> TableApplyPlan {
        let retry_target = self.table_mutation_retry_target.take();
        let mut builder = ApplyPlanBuilder::default();
        for change in self.staged_changes.iter() {
            if retry_target
                .as_ref()
                .is_some_and(|target| !change_matches_target(change, target))
            {
                continue;
            }
            builder.add_change(change);
        }
        builder.finish()
    }

    pub(crate) fn mutation_error_for_identity(&self, identity: &RowIdentity) -> bool {
        let Some(failure) = self.table_mutation_error.as_ref() else {
            return false;
        };
        matches!(
            failure.target.as_ref(),
            Some(MutationTarget::Update { identity: target, .. })
                | Some(MutationTarget::Delete { identity: target, .. }) if target == identity
        )
    }

    pub(crate) fn mutation_error_for_cell(&self, identity: &RowIdentity, column_index: usize) -> bool {
        let Some(failure) = self.table_mutation_error.as_ref() else {
            return false;
        };
        match failure.target.as_ref() {
            Some(MutationTarget::Update {
                identity: target,
                columns,
                ..
            }) => target == identity && columns.contains(&column_index),
            Some(MutationTarget::Delete { identity: target, .. }) => target == identity,
            Some(MutationTarget::Insert) | None => false,
        }
    }

    pub(crate) fn clear_error_for_identity(&mut self, identity: &RowIdentity, column_index: Option<usize>) -> bool {
        let clear =
            self.table_mutation_error
                .as_ref()
                .is_some_and(|failure| match (failure.target.as_ref(), column_index) {
                    (
                        Some(MutationTarget::Update {
                            identity: target,
                            columns,
                            ..
                        }),
                        Some(column),
                    ) => target == identity && columns.contains(&column),
                    (Some(MutationTarget::Update { identity: target, .. }), None)
                    | (Some(MutationTarget::Delete { identity: target, .. }), None) => target == identity,
                    (Some(MutationTarget::Delete { identity: target, .. }), Some(_)) => target == identity,
                    _ => false,
                });
        if clear {
            self.table_mutation_error = None;
        }
        clear
    }
}

#[derive(Default)]
struct ApplyPlanBuilder {
    deletes: Vec<(UiTableMutation, MutationTarget)>,
    updates: Vec<PendingUpdate>,
    inserts: Vec<(UiTableMutation, MutationTarget)>,
}

struct PendingUpdate {
    identity: RowIdentity,
    current_row_index: Option<usize>,
    columns: Vec<String>,
    data_types: Vec<String>,
    values: Vec<UiCell>,
    column_indexes: Vec<usize>,
}

impl ApplyPlanBuilder {
    fn add_change(&mut self, change: &StagedChange) {
        match change {
            StagedChange::Update {
                identity,
                current_row_index,
                column_index,
                column,
                data_type,
                value,
                ..
            } => self.add_update(identity, *current_row_index, *column_index, column, data_type, value),
            StagedChange::Delete {
                identity,
                current_row_index,
            } => self.deletes.push((
                UiTableMutation::Delete {
                    pk_columns: identity.original_pk_columns.clone(),
                    pk_values: identity.original_pk_values.clone(),
                    pk_data_types: identity.original_pk_data_types.clone(),
                },
                MutationTarget::Delete {
                    identity: identity.clone(),
                    current_row_index: *current_row_index,
                },
            )),
            StagedChange::Insert {
                columns,
                values,
                data_types,
                ..
            } => self.inserts.push((
                UiTableMutation::Insert {
                    columns: columns.clone(),
                    data_types: data_types.clone(),
                    values: values.clone(),
                },
                MutationTarget::Insert,
            )),
        }
    }

    fn add_update(
        &mut self,
        identity: &RowIdentity,
        current_row_index: Option<usize>,
        column_index: usize,
        column: &str,
        data_type: &str,
        value: &UiCell,
    ) {
        if let Some(entry) = self.updates.iter_mut().find(|entry| entry.identity == *identity) {
            entry.columns.push(column.to_owned());
            entry.data_types.push(data_type.to_owned());
            entry.values.push(value.clone());
            entry.column_indexes.push(column_index);
            return;
        }
        self.updates.push(PendingUpdate {
            identity: identity.clone(),
            current_row_index,
            columns: vec![column.to_owned()],
            data_types: vec![data_type.to_owned()],
            values: vec![value.clone()],
            column_indexes: vec![column_index],
        });
    }

    fn finish(self) -> TableApplyPlan {
        let mut plan = TableApplyPlan::default();
        for (change, target) in self.deletes {
            plan.push(change, target);
        }
        for update in self.updates {
            let target = MutationTarget::Update {
                identity: update.identity.clone(),
                current_row_index: update.current_row_index,
                columns: update.column_indexes,
            };
            let change = UiTableMutation::Update {
                columns: update.columns,
                data_types: update.data_types,
                values: update.values,
                pk_columns: update.identity.original_pk_columns,
                pk_values: update.identity.original_pk_values,
                pk_data_types: update.identity.original_pk_data_types,
            };
            plan.push(change, target);
        }
        for (change, target) in self.inserts {
            plan.push(change, target);
        }
        plan
    }
}

fn normalize_failure_code(code: &str) -> &str {
    match code {
        "CONSTRAINT_VIOLATION" => "CONSTRAINT_VIOLATION",
        "VALIDATION_ERROR" => "VALIDATION_ERROR",
        "CONFLICT" => "CONFLICT",
        "INTERNAL_ERROR" => "INTERNAL",
        _ => "INTERNAL",
    }
}

fn failure_message(code: &str, message: &str) -> String {
    if code == "CONFLICT" {
        format!("This row changed or was deleted in the database. Database: {message}")
    } else {
        message.to_owned()
    }
}

fn conflict_identity(code: &str, target: Option<&MutationTarget>) -> Option<RowIdentity> {
    if code != "CONFLICT" {
        return None;
    }
    match target {
        Some(MutationTarget::Update { identity, .. }) | Some(MutationTarget::Delete { identity, .. }) => {
            Some(identity.clone())
        }
        Some(MutationTarget::Insert) | None => None,
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct TableApplyPlan {
    pub(super) changes: Vec<UiTableMutation>,
    pub(super) targets: Vec<MutationTarget>,
}

impl TableApplyPlan {
    fn push(&mut self, change: UiTableMutation, target: MutationTarget) {
        self.changes.push(change);
        self.targets.push(target);
    }
}

fn change_matches_target(change: &StagedChange, target: &MutationTarget) -> bool {
    match (change, target) {
        (
            StagedChange::Update { identity, .. },
            MutationTarget::Update {
                identity: target_identity,
                ..
            },
        )
        | (
            StagedChange::Delete { identity, .. },
            MutationTarget::Delete {
                identity: target_identity,
                ..
            },
        ) => identity == target_identity,
        (StagedChange::Insert { .. }, MutationTarget::Insert) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "tests/table_mutation_state_tests.rs"]
mod tests;

