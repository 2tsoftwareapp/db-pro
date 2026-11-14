use super::UiCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowIdentity {
    pub(crate) original_pk_columns: Vec<String>,
    pub(crate) original_pk_values: Vec<UiCell>,
    /// Column types parallel to `original_pk_columns` (#587).
    ///
    /// Without them the translator sniffed PK cells by content, so a TEXT
    /// primary key holding a UUID was sent as a UUID and never matched a row.
    pub(crate) original_pk_data_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StagedChange {
    Update {
        identity: RowIdentity,
        current_row_index: Option<usize>,
        column_index: usize,
        column: String,
        data_type: String,
        original: UiCell,
        value: UiCell,
    },
    Delete {
        identity: RowIdentity,
        current_row_index: Option<usize>,
    },
    Insert {
        local_id: u64,
        columns: Vec<String>,
        values: Vec<UiCell>,
        /// Column types parallel to `columns` (#587).
        data_types: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MutationTarget {
    Update {
        identity: RowIdentity,
        current_row_index: Option<usize>,
        columns: Vec<usize>,
    },
    Delete {
        identity: RowIdentity,
        current_row_index: Option<usize>,
    },
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MutationFailure {
    pub statement_index: usize,
    pub target: Option<MutationTarget>,
    pub code: String,
    pub message: String,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChangeCounts {
    pub inserts: usize,
    pub updates: usize,
    pub deletes: usize,
}

impl ChangeCounts {
    pub fn total(self) -> usize {
        self.inserts + self.updates + self.deletes
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ChangeSet {
    target_table: Option<String>,
    entries: Vec<StagedChange>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn target_table(&self) -> Option<&str> {
        self.target_table.as_deref()
    }

    pub fn ensure_target(&mut self, table: &str) -> bool {
        if let Some(ref current) = self.target_table {
            if current != table {
                return false;
            }
        } else {
            self.target_table = Some(table.to_owned());
        }
        true
    }

    pub fn clear(&mut self) {
        self.target_table = None;
        self.entries.clear();
    }

    /// Remove staged entries that were part of a successful apply/retry batch.
    pub fn remove_applied_targets(&mut self, targets: &[MutationTarget]) {
        if targets.is_empty() {
            return;
        }
        let insert_slots = targets
            .iter()
            .filter(|target| matches!(target, MutationTarget::Insert))
            .count();
        let mut insert_slots_left = insert_slots;
        self.entries.retain(|change| {
            if matches!(change, StagedChange::Insert { .. }) {
                if insert_slots_left > 0 {
                    insert_slots_left -= 1;
                    return false;
                }
                return true;
            }
            !targets.iter().any(|target| Self::change_matches_target(change, target))
        });
        if self.entries.is_empty() {
            self.target_table = None;
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, StagedChange> {
        self.entries.iter()
    }

    /// Keep one final value per edited cell and drop a no-op edit.
    // cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
    pub fn stage_update(&mut self, change: StagedChange) {
        let StagedChange::Update {
            column_index,
            value,
            original,
            identity,
            current_row_index,
            ..
        } = &change
        else {
            self.entries.push(change);
            return;
        };

        if let Some(existing) = self.entries.iter_mut().find(|entry| {
            matches!(entry, StagedChange::Update {
                column_index: column,
                identity: existing_identity,
                ..
            } if *column == *column_index && existing_identity == identity)
        }) {
            let existing_matches_original = match existing {
                StagedChange::Update {
                    original: existing_original,
                    ..
                } => value == existing_original,
                _ => false,
            };
            if existing_matches_original {
                self.entries.retain(|entry| {
                    !matches!(entry, StagedChange::Update {
                        column_index: column,
                        identity: existing_identity,
                        ..
                    } if *column == *column_index && existing_identity == identity)
                });
                return;
            }

            if let StagedChange::Update {
                value: staged_value,
                current_row_index: staged_row_index,
                ..
            } = existing
            {
                *staged_value = value.clone();
                *staged_row_index = *current_row_index;
            }
        } else {
            if value != original {
                self.entries.push(change);
            }
        }
    }

    /// Deleting a server row supersedes all updates for that stable PK identity.
    pub fn stage_delete(&mut self, change: StagedChange) {
        let StagedChange::Delete { identity, .. } = &change else {
            self.entries.push(change);
            return;
        };
        self.entries.retain(|entry| {
            !matches!(entry, StagedChange::Update {
                identity: existing_identity,
                ..
            } if existing_identity == identity)
                && !matches!(entry, StagedChange::Delete {
                    identity: existing_identity,
                    ..
                } if existing_identity == identity)
        });
        self.entries.push(change);
    }

    pub fn stage_insert(&mut self, columns: Vec<String>, values: Vec<UiCell>) -> u64 {
        self.stage_insert_typed(columns, Vec::new(), values)
    }

    /// Stage an insert with the column types the grid knows about (#587).
    pub fn stage_insert_typed(
        &mut self,
        columns: Vec<String>,
        data_types: Vec<String>,
        values: Vec<UiCell>,
    ) -> u64 {
        let local_id = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StagedChange::Insert { local_id, .. } => Some(*local_id),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.entries.push(StagedChange::Insert {
            local_id,
            columns,
            values,
            data_types,
        });
        local_id
    }

    /// Removing a staged insert row cancels it locally without emitting SQL.
    pub fn remove_insert(&mut self, local_id: u64) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !matches!(entry, StagedChange::Insert { local_id: id, .. } if *id == local_id));
        before != self.entries.len()
    }

    pub fn revert_cell(&mut self, identity: &RowIdentity, column_index: usize) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            !matches!(entry, StagedChange::Update {
                identity: entry_identity,
                column_index: column,
                ..
            } if entry_identity == identity && *column == column_index)
        });
        before != self.entries.len()
    }

    pub fn revert_row(&mut self, identity: &RowIdentity) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            !matches!(entry, StagedChange::Update { identity: entry_identity, .. } if entry_identity == identity)
                && !matches!(entry, StagedChange::Delete { identity: entry_identity, .. } if entry_identity == identity)
        });
        before != self.entries.len()
    }

    pub fn update_original_baseline(&mut self, identity: &RowIdentity, column_index: usize, new_original: UiCell) {
        for entry in self.entries.iter_mut() {
            if let StagedChange::Update {
                identity: entry_identity,
                column_index: changed_column,
                original,
                ..
            } = entry
            {
                if entry_identity == identity && *changed_column == column_index {
                    *original = new_original.clone();
                }
            }
        }
    }

    pub fn cell_value(&self, identity: &RowIdentity, column_index: usize) -> Option<UiCell> {
        self.entries.iter().rev().find_map(|entry| match entry {
            StagedChange::Update {
                identity: entry_identity,
                column_index: changed_column,
                value,
                ..
            } if entry_identity == identity && *changed_column == column_index => Some(value.clone()),
            _ => None,
        })
    }

    pub fn row_deleted(&self, identity: &RowIdentity) -> bool {
        self.entries.iter().any(|entry| {
            matches!(entry, StagedChange::Delete { identity: entry_identity, .. } if entry_identity == identity)
        })
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

    pub fn row_has_changes(&self, identity: &RowIdentity) -> bool {
        self.entries.iter().any(|entry| match entry {
            StagedChange::Update {
                identity: entry_identity,
                ..
            }
            | StagedChange::Delete {
                identity: entry_identity,
                ..
            } => entry_identity == identity,
            StagedChange::Insert { .. } => false,
        })
    }

    pub fn counts(&self) -> ChangeCounts {
        let mut counts = ChangeCounts::default();
        let mut updated_rows = Vec::<RowIdentity>::new();
        for entry in &self.entries {
            match entry {
                StagedChange::Insert { .. } => counts.inserts += 1,
                StagedChange::Update { identity, .. } => {
                    if !updated_rows.iter().any(|existing| existing == identity) {
                        updated_rows.push(identity.clone());
                    }
                }
                StagedChange::Delete { .. } => counts.deletes += 1,
            }
        }
        counts.updates = updated_rows.len();
        counts
    }
}

impl From<Vec<StagedChange>> for ChangeSet {
    fn from(entries: Vec<StagedChange>) -> Self {
        Self {
            target_table: None,
            entries,
        }
    }
}

#[cfg(test)]
#[path = "tests/change_set_tests.rs"]
mod tests;

