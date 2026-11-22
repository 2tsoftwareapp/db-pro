// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::*;

pub(crate) struct StagedApplyFailure<'a> {
    pub(crate) statement_index: usize,
    pub(crate) code: &'a str,
    pub(crate) message: &'a str,
    pub(crate) rolled_back: bool,
}

impl DbProApp {
    pub(crate) fn can_mutate_active_connection(&self) -> bool {
        table_editor_context::can_mutate_active_connection(&self.connection.catalog, &self.connection.lifecycle)
    }

    pub(crate) fn edit_surface(&self) -> table_editor_context::RowEditSurface {
        if self.workspace.active_tab == WorkspaceTab::SchemaObject
            && matches!(
                self.schema.explorer.selected_schema_object,
                Some(SchemaObjectSelection::View(_))
            )
        {
            return table_editor_context::RowEditSurface::View;
        }
        if self.workspace.active_tab == WorkspaceTab::Table && self.table.state.table_view == TableView::Data {
            if self.table.data_query.inline_query_result || self.table.data_query.inline_query_request.is_some() {
                return table_editor_context::RowEditSurface::InlineSql;
            }
            return table_editor_context::RowEditSurface::Table;
        }
        table_editor_context::RowEditSurface::Query
    }

    pub(crate) fn result_edit_reason(&self, result: &UiQueryResult) -> Option<String> {
        let info = self.table.state.table_info.as_ref();
        let identity = info.and_then(|info| {
            TableDataState::identity_key(info, self.table.data.manual_row_identity.as_deref())
                .map(|key| key.columns)
        });
        table_editor_context::result_edit_reason(
            self.edit_surface(),
            self.can_mutate_active_connection(),
            info.is_some(),
            identity.as_deref(),
            &result.columns,
        )
    }

    pub(crate) fn has_row_identity(&self) -> bool {
        self.table.state.table_info.as_ref().is_some_and(|info| {
            TableDataState::identity_key(info, self.table.data.manual_row_identity.as_deref()).is_some()
        })
    }

    /// SQL for the destination-value dropdown of a single-column foreign key, or `None` when
    /// the column is not an FK or the key is composite — a joined "7, 42" label cannot be
    /// written back into one cell, so those columns stay plain text edits (#496).
    pub(crate) fn foreign_key_lookup_sql(&self, column: &str) -> Option<String> {
        let info = self.table.state.table_info.as_ref()?;
        let fk = table_fk_lookup::foreign_key_for_column(info, column)?;
        if !table_fk_lookup::dropdown_applies(fk) {
            return None;
        }
        let driver = self
            .connection
            .lifecycle
            .active_connection_id()
            .and_then(|id| self.connection.catalog.find(id))
            .map(|connection| connection.driver.clone())
            .unwrap_or_else(|| "PostgreSQL".to_owned());
        let dialect = db_pro_core::application::DriverDialect::from_name(&driver);
        Some(table_fk_lookup::lookup_sql(&dialect, fk))
    }

    pub(crate) fn apply_foreign_key_lookup(&mut self, column: &str, result: &UiQueryResult) {
        self.table.editing.fk_lookup_column = Some(column.to_owned());
        self.table.editing.fk_lookup_options = table_fk_lookup::options_from_result(result);
        // The lookup is capped at `FK_LOOKUP_LIMIT`; when it lands exactly on the cap, flag
        // that the dropdown may be truncated instead of implying the list is complete (#497).
        if result.rows.len() >= table_fk_lookup::FK_LOOKUP_LIMIT {
            self.feedback.set_runtime_message(format!(
                "Foreign key values truncated to the first {} rows",
                table_fk_lookup::FK_LOOKUP_LIMIT
            ));
        }
    }

    pub(crate) fn open_referenced_row(
        &mut self,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
    ) -> bool {
        let Some(info) = self.table.state.table_info.clone() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return false;
        };
        let Some(column_name) = result.columns.get(column_index).map(|column| column.name.clone()) else {
            return false;
        };
        let Some(fk) = table_fk_lookup::foreign_key_for_column(&info, &column_name).cloned() else {
            self.feedback.runtime_message = "This column is not a foreign key".to_owned();
            return false;
        };
        let Some(row) = result.rows.get(row_index) else {
            return false;
        };
        let Some(filters) = table_fk_lookup::referenced_row_filters(&info, &fk, result, row) else {
            self.feedback.runtime_message = "The referenced value is missing".to_owned();
            return false;
        };
        let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) else {
            self.feedback.runtime_message = "Connect to a database before opening the referenced row".to_owned();
            return false;
        };
        // Same staged-changes hazard as `open_table`: navigating switches the
        // selected table, stranding edits staged for the current target.
        // Park the navigation behind the discard-changes confirmation.
        if !self.table.mutation.staged_changes.is_empty() {
            self.workspace.pending_navigation_action =
                Some(PendingNavigationAction::OpenReferencedRow {
                    connection_id,
                    schema: fk.to_schema.clone(),
                    table: fk.to_table.clone(),
                    filters: filters.clone(),
                });
            self.table.editing.discard_changes_confirmation = true;
            self.feedback
                .set_runtime_message("Apply or discard staged changes before opening the referenced row");
            return false;
        }
        self.schema.explorer.selected_schema = Some(fk.to_schema.clone());
        self.schema.explorer.selected_table = Some(fk.to_table.clone());
        self.workspace.active_tab = WorkspaceTab::Table;
        self.table.state.table_view = TableView::Data;
        let request_id = self.next_request_id();
        let command = table_data_view::build_load_row_command(
            request_id,
            table_data_view::TableDataTarget {
                connection_id,
                schema: fk.to_schema,
                table: fk.to_table,
            },
            filters,
        );
        self.dispatch_command(command)
    }

    pub(crate) fn choose_row_identity_column(&mut self, column: String) {
        if self.table.state.has_primary_key() {
            self.feedback.runtime_message = "This table already uses its primary key as the row id.".to_owned();
            return;
        }
        let mut columns = self.table.data.manual_row_identity.clone().unwrap_or_default();
        if !columns.iter().any(|existing| existing == &column) {
            columns.push(column);
        }
        self.table.data.manual_row_identity = Some(columns);
        self.table.data.invalidate_grid_row_caches();
        let names = self.table.data.manual_row_identity.as_ref().map(|columns| columns.join(", "));
        self.feedback.runtime_message = format!("Row id columns: {}", names.unwrap_or_default());
    }

    /// Localized explanation when a read-only result has a specific user-facing cause.
    pub(crate) fn result_read_only_notice(&self, reason: Option<&str>) -> Option<String> {
        let surface = self.edit_surface();
        let surface_notice = match surface {
            table_editor_context::RowEditSurface::View => Some(t!("table.view_rows_read_only").into_owned()),
            table_editor_context::RowEditSurface::InlineSql => Some(t!("table.inline_sql_read_only").into_owned()),
            table_editor_context::RowEditSurface::Table | table_editor_context::RowEditSurface::Query => None,
        };
        surface_notice.or_else(|| {
            let column = reason?
                .strip_prefix("The key column ")?
                .strip_suffix(" is not present in this result")?;
            Some(t!("table.key_column_missing", column = column).into_owned())
        })
    }

    pub(crate) fn begin_data_cell_edit(
        &mut self,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
        cell: &UiCell,
    ) {
        if let Some(reason) = self.result_edit_reason(result) {
            self.feedback.runtime_message = reason;
            return;
        }
        if let Some(info) = self.table.state.table_info.as_ref() {
            let manual = self.table.data.manual_row_identity.clone();
            if let Err(error) = TableDataState::row_identity(result, info, manual.as_deref(), row_index) {
                self.feedback.runtime_message = error;
                return;
            }
        }
        if !self.can_mutate_active_connection() {
            self.feedback.runtime_message = "Connect with write access to edit rows".to_owned();
            return;
        }
        if self.staged_row_deleted(result, row_index) {
            self.feedback.runtime_message = "Discard the staged delete before editing this row".to_owned();
            return;
        }
        // #537: the grid only holds a display prefix of an oversized value; opening an
        // editor on it would stage that prefix over the full value.
        let truncated = result
            .rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .is_some_and(UiCell::is_truncated);
        if truncated {
            self.open_cell_inspector(result, row_index, column_index);
            self.feedback.runtime_message = t!("query.cell_truncated_for_display").into_owned();
            return;
        }
        if let Some(block) = result
            .columns
            .get(column_index)
            .and_then(|column| self.table.state.column_write_policy(&column.name))
            .and_then(|policy| policy.write_block())
        {
            // Still allow the advanced inspector for binary / blocked columns (#228).
            self.open_cell_inspector(result, row_index, column_index);
            self.feedback.runtime_message = block.reason().to_owned();
            return;
        }
        self.table.data.selected_cell = Some((row_index, column_index));
        self.table.data.selected_row = Some(row_index);
        self.table.data.selected_rows.clear();
        self.table.data.selected_rows.insert(row_index);
        self.table.data.selection_anchor_row = Some(row_index);
        self.table.data.selection_anchor_cell = Some((row_index, column_index));
        if let Some(identity) =
            self.table
                .data
                .row_identity_for_result(result, self.table.state.table_info.as_ref(), row_index)
        {
            self.table
                .mutation
                .clear_error_for_identity(&identity, Some(column_index));
        }
        if let Some(column_name) = result.columns.get(column_index).map(|column| column.name.clone()) {
            let lookup = self.foreign_key_lookup_sql(&column_name);
            if let Some(sql) = lookup {
                self.table.editing.fk_lookup_column = Some(column_name);
                self.table.editing.fk_lookup_options.clear();
                // Fetch the destination key list so the cell editor can offer
                // it as a select. Without an active connection there is nowhere
                // to run it — the column stays editable as plain text.
                self.table.editing.pending_fk_lookup_request = self
                    .connection
                    .lifecycle
                    .active_connection_id()
                    .map(str::to_owned)
                    .and_then(|connection_id| {
                        let request_id = self.next_request_id();
                        let dispatched = self.dispatch_command(UiCommand::RunQuery {
                            request_id,
                            connection_id,
                            sql,
                            params: Vec::new(),
                            schema: None,
                            session_id: None,
                        });
                        dispatched.then_some(request_id)
                    });
            } else if self.table.editing.fk_lookup_column.is_some() {
                self.table.editing.fk_lookup_column = None;
                self.table.editing.fk_lookup_options.clear();
                self.table.editing.pending_fk_lookup_request = None;
            }
        }
        self.table.editing.data_editing_cell = Some((row_index, column_index));
        let should_expand = result.columns.get(column_index).is_some_and(|column| {
            let data_type = column.data_type.to_ascii_lowercase();
            data_type.contains("json")
                || matches!(cell, UiCell::Json(_))
                || matches!(cell, UiCell::Bytes(_))
                || matches!(cell, UiCell::Text(value) if value.chars().count() > 120)
        });
        if should_expand {
            self.open_cell_inspector(result, row_index, column_index);
        } else {
            self.table.editing.expanded_data_editor = None;
            self.table.editing.data_edit_error = None;
            self.table.editing.data_edit_value = match cell {
                UiCell::Null => "NULL".to_owned(),
                _ => crate::cell_text(cell),
            };
        }
        self.feedback.copy_status.clear();
    }

    pub(crate) fn submit_data_cell_edit(
        &mut self,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
    ) -> bool {
        if let Some(reason) = self.result_edit_reason(result) {
            self.feedback.runtime_message = reason;
            return false;
        }
        self.table_mutation_context()
            .stage_data_cell_edit(result, row_index, column_index)
    }

    pub(crate) fn request_delete_selected_data_rows(&mut self, result: &UiQueryResult) {
        if let Some(reason) = self.result_edit_reason(result) {
            self.feedback.runtime_message = reason;
            return;
        }
        if !self.can_mutate_active_connection() {
            self.feedback.runtime_message = "Connect with write access to delete rows".to_owned();
            return;
        }
        self.table_mutation_context().stage_delete_selected_rows(result);
    }

    pub(crate) fn staged_cell_value(
        &self,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
    ) -> Option<UiCell> {
        table_editor_context::staged_cell_value(
            &self.table.data,
            &self.table.state,
            &self.table.mutation,
            result,
            row_index,
            column_index,
        )
    }

    pub(crate) fn staged_row_deleted(&self, result: &UiQueryResult, row_index: usize) -> bool {
        table_editor_context::staged_row_deleted(
            &self.table.data,
            &self.table.state,
            &self.table.mutation,
            result,
            row_index,
        )
    }

    pub(crate) fn revert_staged_cell(&mut self, result: &UiQueryResult, row_index: usize, column_index: usize) {
        self.table_mutation_context()
            .revert_staged_cell(result, row_index, column_index);
    }

    pub(crate) fn revert_staged_row(&mut self, result: &UiQueryResult, row_index: usize) {
        self.table_mutation_context().revert_staged_row(result, row_index);
    }

    pub(crate) fn discard_staged_changes(&mut self) {
        if self.table_mutation_context().discard_staged_changes() {
            self.request_table_data();
        }
    }

    pub(crate) fn table_mutation_context(&mut self) -> table_editor_context::TableMutationContext<'_> {
        table_editor_context::TableMutationContext::new(
            &mut self.table.state,
            &mut self.table.data_query,
            &mut self.table.data,
            &mut self.table.editing,
            &mut self.table.mutation,
            &mut self.feedback,
            self.schema.explorer.selected_table.as_deref(),
        )
    }

    pub(crate) fn discard_failed_mutation(&mut self, reload: bool) {
        let Some(target) = self
            .table
            .mutation
            .table_mutation_error
            .as_ref()
            .and_then(|failure| failure.target.clone())
        else {
            return;
        };
        match target {
            MutationTarget::Update { identity, columns, .. } => {
                for column_index in columns {
                    self.table.mutation.staged_changes.revert_cell(&identity, column_index);
                }
            }
            MutationTarget::Delete { identity, .. } => {
                self.table.mutation.staged_changes.revert_row(&identity);
            }
            MutationTarget::Insert => {}
        }
        self.table.mutation.table_mutation_error = None;
        self.table.mutation.table_mutation_retry_after_reload = false;
        self.table.mutation.table_mutation_retry_target = None;
        if reload {
            self.table.data_query.clear_result();
            self.table.data_query.total_rows = None;
            self.table.data_query.error = None;
            self.request_table_data();
        }
    }

    pub(super) fn reload_failed_mutation(&mut self) {
        let target = self
            .table
            .mutation
            .table_mutation_error
            .as_ref()
            .and_then(|failure| failure.target.clone());
        if let Some(MutationTarget::Update { identity, .. } | MutationTarget::Delete { identity, .. }) = target {
            self.request_table_row_reload(identity);
            return;
        }
        self.table.mutation.table_mutation_error = None;
        self.table.data_query.clear_result();
        self.table.data_query.total_rows = None;
        self.table.data_query.error = None;
        self.request_table_data();
    }

    pub(crate) fn retry_failed_mutation_after_reload(&mut self) {
        let Some(target) = self
            .table
            .mutation
            .table_mutation_error
            .as_ref()
            .and_then(|failure| failure.target.clone())
        else {
            self.feedback.runtime_message = "This failure has no retryable mutation target".to_owned();
            return;
        };
        self.table.mutation.table_mutation_retry_target = Some(target);
        self.table.mutation.table_mutation_retry_after_reload = true;
        self.reload_failed_mutation();
    }

    pub(crate) fn request_table_row_reload(&mut self, identity: RowIdentity) {
        let (Some(connection_id), Some(table)) = (
            self.connection.lifecycle.active_connection_id().map(str::to_owned),
            self.schema.explorer.selected_table.clone(),
        ) else {
            self.feedback.runtime_message = "Connect to a database before reloading the row".to_owned();
            return;
        };
        let Some(info) = self.table.state.table_info.as_ref() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return;
        };
        let filters = match TableMutationState::row_reload_filters(info, &identity) {
            Ok(filters) => filters,
            Err(error) => {
                self.feedback.runtime_message = error;
                return;
            }
        };
        let request_id = self.next_request_id();
        let command = table_data_view::build_load_row_command(
            request_id,
            table_data_view::TableDataTarget {
                connection_id,
                schema: self.active_schema().to_owned(),
                table,
            },
            filters,
        );
        if self.dispatch_command(command) {
            self.table.data_query.row_reload_request = Some(request_id);
            self.table.data_query.row_reload_identity = Some(identity);
            self.feedback.runtime_message = "Reloading row from database…".to_owned();
        }
    }

    pub(crate) fn apply_staged_changes(&mut self) {
        if self.table.mutation.staged_apply_request.is_some() {
            return;
        }
        if self.table.mutation.staged_changes.is_empty() {
            return;
        }
        if self.table.editing.data_edit_error.is_some() {
            self.feedback.runtime_message = "Fix the validation error before applying changes".to_owned();
            return;
        }
        // Deletes require the explicit "+N ~N -N" confirm gate (#408); inserts
        // and updates alone still apply directly.
        if self.table.mutation.staged_changes.counts().deletes > 0
            && !self.table.mutation.staged_apply_confirm
        {
            self.table.mutation.staged_apply_confirm = true;
            return;
        }
        let Some(connection) = self.active_connection().cloned() else {
            self.feedback.runtime_message = "Connect to a database before applying changes".to_owned();
            return;
        };
        let Some(table) = self.schema.explorer.selected_table.clone() else {
            self.feedback.runtime_message = "Select a table before applying changes".to_owned();
            return;
        };
        let Some(plan) = self.table_mutation_context().prepare_apply(&table) else {
            return;
        };
        let request_id = self.next_request_id();
        let command = UiCommand::ApplyTableChanges {
            request_id,
            connection_id: connection.id,
            schema: self.active_schema().to_owned(),
            table,
            changes: plan.changes,
        };
        if self.dispatch_command(command) {
            self.table.mutation.staged_apply_request = Some(request_id);
            self.table.mutation.table_mutation_request = Some(request_id);
            self.table.mutation.staged_apply_targets = plan.targets;
            self.table.mutation.staged_apply_confirm = false;
            self.table.mutation.table_mutation_error = None;
            let counts = self.table.mutation.staged_changes.counts();
            self.feedback.runtime_message = format!(
                "Applying {} changes in one transaction (+{} ~{} -{})…",
                counts.total(),
                counts.inserts,
                counts.updates,
                counts.deletes
            );
        } else {
            self.feedback.runtime_message = "Could not send staged change to runtime".to_owned();
        }
    }

    pub(crate) fn staged_apply_completed(&mut self) {
        let applied_targets = self.table.mutation.staged_apply_targets.clone();
        self.table.mutation.staged_apply_request = None;
        self.table.mutation.staged_apply_confirm = false;
        self.table.mutation.table_mutation_request = None;
        self.table.mutation.table_mutation_retry_after_reload = false;
        self.table.mutation.table_mutation_retry_target = None;
        if applied_targets.is_empty() {
            self.table.mutation.staged_changes.clear();
        } else {
            self.table
                .mutation
                .staged_changes
                .remove_applied_targets(&applied_targets);
        }
        self.table.mutation.staged_apply_targets.clear();
        self.table.mutation.table_mutation_error = None;
        let remaining = self.table.mutation.staged_changes.counts();
        if remaining.total() == 0 {
            self.feedback.runtime_message = "All staged changes applied".to_owned();
            self.feedback
                .show_success_toast("All staged changes applied successfully");
        } else {
            self.feedback.runtime_message = format!(
                "Applied {} change(s); {} staged edit(s) still pending (+{} ~{} -{})",
                applied_targets.len(),
                remaining.total(),
                remaining.inserts,
                remaining.updates,
                remaining.deletes
            );
            self.feedback.show_info_toast("Partial apply completed — remaining edits are still staged");
        }
        if let Some(action) = self.workspace.pending_navigation_action.take() {
            self.execute_pending_navigation(action);
            return;
        }
        self.table.data_query.clear_result();
        self.table.data_query.total_rows = None;
        self.table.data_query.error = None;
        self.request_table_data();
    }

    pub(crate) fn staged_apply_failed(&mut self, failure: StagedApplyFailure<'_>) {
        self.workspace.pending_navigation_action = None;
        self.table.mutation.reset_apply_lifecycle();
        let transition = self.table.mutation.record_apply_failure(failure);
        if let Some(target) = transition.target() {
            self.table_mutation_context().select_failed_target(target);
        }
        if transition.is_conflict() {
            self.table.mutation.conflict_dialog_open = true;
            if let Some(identity) = transition.reload_identity() {
                self.request_table_row_reload(identity.clone());
            }
        }
        let outcome = if transition.is_rolled_back() {
            "transaction rolled back"
        } else {
            "transaction outcome is unknown"
        };
        let formatted = if transition.has_target() {
            format!(
                "Staged change #{} failed · {outcome} · {}",
                transition.statement_index().saturating_add(1),
                transition.display_message()
            )
        } else {
            format!("Staged changes failed · {outcome} · {}", transition.display_message())
        };
        self.feedback.runtime_message = formatted.clone();
        self.feedback.show_error_toast(formatted);
    }
}
