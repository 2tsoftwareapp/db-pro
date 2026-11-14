// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::*;

pub(super) struct TableDataTarget {
    pub(super) connection_id: String,
    pub(super) schema: String,
    pub(super) table: String,
}

pub(super) fn build_load_data_command(
    state: &TableDataQueryState,
    request_id: RequestId,
    target: TableDataTarget,
) -> UiCommand {
    UiCommand::LoadTableData {
        request_id,
        connection_id: target.connection_id,
        schema: target.schema,
        table: target.table,
        limit: state.limit,
        offset: state.offset,
        filters: state.filters.clone(),
        sorts: state.sorts.clone(),
    }
}

pub(super) fn build_load_row_command(
    request_id: RequestId,
    target: TableDataTarget,
    filters: Vec<UiTableDataFilter>,
) -> UiCommand {
    UiCommand::LoadTableData {
        request_id,
        connection_id: target.connection_id,
        schema: target.schema,
        table: target.table,
        limit: 1,
        offset: 0,
        filters,
        sorts: Vec::new(),
    }
}

impl DbProApp {
    pub(crate) fn draw_table_data(&mut self, ui: &mut egui::Ui, table_name: &str) {
        // Temporarily move the result out while rendering. The grid mutates
        // selection/edit state, so borrowing it directly from `self` would
        // conflict with those updates; moving avoids cloning every frame.
        let Some(result) = self.table.data_query.result.take() else {
            self.draw_table_data_placeholder(ui, table_name);
            return;
        };

        let can_mutate = can_mutate_table_toolbar(
            self.can_mutate_active_connection(),
            self.table.data_query.inline_query_request.is_some(),
        );
        let total_known = self.table.data_query.total_rows.is_some();
        let total_rows = self.table.data_query.total_rows.unwrap_or(result.row_count);
        let sql_prefix = table_data_toolbar_surface_view::select_prefix(
            self.schema.explorer.selected_schema.as_deref(),
            table_name,
        );
        let paging = table_data_toolbar_surface_view::TableDataPaging {
            page_range: {
                // UAT-4: label the applied cap, without asserting that the
                // capped result was necessarily truncated.
                let limit_note =
                    query_row_limit::row_limit_note(self.table.data_query.last_result_row_limit);
                format!("{}{limit_note}", crate::components::common::format_page_range(
                    self.table.data_query.offset,
                    result.row_count,
                    self.table.data_query.total_rows,
                ))
            },
            total_rows,
            total_known,
            has_next: if total_known {
                self.table.data_query.offset.saturating_add(self.table.data_query.limit) < total_rows
            } else {
                result.row_count >= self.table.data_query.limit
            },
            has_previous: self.table.data_query.offset > 0,
            inline_query_result: self.table.data_query.inline_query_result,
        };

        let edit_reason = self.result_edit_reason(&result);
        let read_only_notice = self.result_read_only_notice(edit_reason.as_deref());
        self.draw_table_data_unified_toolbar(ui, table_name, &sql_prefix, can_mutate, &paging, read_only_notice.as_deref());
        let context = table_data_surface_view::TableDataSurfaceContext {
            theme: self.theme,
            record_view_open: self.table.editing.record_view_open,
            record_available: self.table.data.selected_rows.len() == 1
                && self.table.data.selected_row.is_some_and(|row| row < result.rows.len()),
        };
        let (_, presentation) = table_data_surface_view::draw_surface(&context, ui, |ui| {
            if result.row_count == 0 && !self.table.data_query.inline_query_result {
                let context = table_data_placeholder_view::TableDataPlaceholderContext {
                    theme: self.theme,
                    error: None,
                    empty: true,
                };
                table_data_placeholder_view::draw_placeholder(&context, ui, table_name);
            } else {
                self.draw_result_grid(ui, &result);
            }
        });
        if let Some(presentation) = presentation {
            self.set_table_data_presentation(presentation, &result);
        }
        self.draw_table_data_unified_footer(ui, table_name, &sql_prefix, can_mutate, &paging);
        self.draw_pending_changes_dialog(ui);
        self.draw_staged_apply_confirmation(ui);
        self.draw_conflict_dialog(ui, &result);
        self.draw_export_dialog(ui, Some(&result));
        if self.table.data_query.result.is_none() {
            self.table.data_query.set_result(result);
        }
    }

    fn draw_table_data_placeholder(&mut self, ui: &mut egui::Ui, table_name: &str) {
        self.table.editing.record_view_open = false;
        let surface = table_data_surface_view::TableDataSurfaceContext {
            theme: self.theme,
            record_view_open: false,
            record_available: false,
        };
        let context = table_data_placeholder_view::TableDataPlaceholderContext {
            theme: self.theme,
            error: self.table.data_query.error.as_deref(),
            empty: false,
        };
        let (action, _) = table_data_surface_view::draw_surface(&surface, ui, |ui| {
            table_data_placeholder_view::draw_placeholder(&context, ui, table_name)
        });
        if matches!(
            action,
            Some(table_data_placeholder_view::TableDataPlaceholderAction::Retry)
        ) {
            self.table.data_query.error = None;
            self.request_table_data();
        }
    }

    pub(super) fn set_table_data_presentation(
        &mut self,
        presentation: table_data_surface_view::TableDataPresentation,
        result: &UiQueryResult,
    ) {
        if presentation == table_data_surface_view::TableDataPresentation::Grid {
            self.table.editing.record_view_open = false;
        } else if self.table.data.selected_rows.len() == 1
            && self.table.data.selected_row.is_some_and(|row| row < result.rows.len())
            && self.commit_active_data_edit(result)
        {
            self.feedback.copy_status.clear();
            self.table.editing.record_view_open = true;
        }
    }

    fn draw_table_data_unified_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        table_name: &str,
        sql_prefix: &str,
        can_mutate: bool,
        paging: &table_data_toolbar_surface_view::TableDataPaging,
        read_only_notice: Option<&str>,
    ) {
        let action = {
            let mut context = table_data_toolbar_surface_view::TableDataToolbarContext {
                theme: self.theme,
                table_name,
                sql_prefix,
                column_suggestions: self
                    .table
                    .state
                    .table_info
                    .as_ref()
                    .map_or(&[], |info| info.columns.as_slice()),
                can_mutate,
                connected: self.connection.lifecycle.is_connected(),
                has_primary_key: self.has_row_identity(),
                read_only_notice,
                staged_changes: &self.table.mutation.staged_changes,
                staged_apply_pending: self.table.mutation.staged_apply_request.is_some(),
                has_data_edit_error: self.table.editing.data_edit_error.is_some(),
                failure: self.table.mutation.table_mutation_error.as_ref(),
                selected_rows: self.table.data.selected_rows.len(),
                data_query: &mut self.table.data_query,
                paging,
            };
            table_data_toolbar_surface_view::draw_toolbar(&mut context, ui)
        };
        if let Some(action) = action {
            self.apply_table_data_toolbar_action(action);
        }
    }

    fn draw_table_data_unified_footer(
        &mut self,
        ui: &mut egui::Ui,
        table_name: &str,
        sql_prefix: &str,
        can_mutate: bool,
        paging: &table_data_toolbar_surface_view::TableDataPaging,
    ) {
        let action = {
            let mut context = table_data_toolbar_surface_view::TableDataToolbarContext {
                theme: self.theme,
                table_name,
                sql_prefix,
                column_suggestions: self
                    .table
                    .state
                    .table_info
                    .as_ref()
                    .map_or(&[], |info| info.columns.as_slice()),
                can_mutate,
                connected: self.connection.lifecycle.is_connected(),
                has_primary_key: self.has_row_identity(),
                read_only_notice: None,
                staged_changes: &self.table.mutation.staged_changes,
                staged_apply_pending: self.table.mutation.staged_apply_request.is_some(),
                has_data_edit_error: self.table.editing.data_edit_error.is_some(),
                failure: self.table.mutation.table_mutation_error.as_ref(),
                selected_rows: self.table.data.selected_rows.len(),
                data_query: &mut self.table.data_query,
                paging,
            };
            table_data_toolbar_surface_view::draw_footer(&mut context, ui)
        };
        if let Some(action) = action {
            self.apply_table_data_toolbar_action(action);
        }
    }

    fn run_table_data_sql(&mut self, sql: String) {
        if !self.table.mutation.staged_changes.is_empty() || self.table.mutation.staged_apply_request.is_some() {
            self.feedback.runtime_message = "Apply or discard staged changes before running SQL".to_owned();
            return;
        }
        if self.query.session.active_running_request().is_some()
            || self.table.data_query.inline_query_request.is_some()
            || self.table.data_query.request.is_some()
        {
            self.feedback.runtime_message =
                "Wait for the current query to finish before running another query".to_owned();
            return;
        }
        if !crate::query::discover_sql_parameters_for_driver(&sql, self.active_driver()).is_empty() {
            self.feedback.runtime_message = "Inline SQL does not support named parameters".to_owned();
            return;
        }
        let execution_range = (0, sql.len());
        let options = crate::app::events_query_dispatch::QueryRunOptions {
            execution_range,
            version: 0,
            all_statements: false,
            bound_params: None,
            applied_row_limit: None,
        };
        if self.hold_destructive_run(&sql, PendingDestructiveRunOrigin::InlineQuery, &options) {
            return;
        }
        let Some(connection_id) = self.active_connection().map(|connection| connection.id.clone()) else {
            self.feedback.runtime_message = "Create or select a connection first".to_owned();
            return;
        };
        self.send_table_data_query_run(connection_id, sql);
    }

    fn apply_table_data_toolbar_action(&mut self, action: table_data_toolbar_surface_view::TableDataToolbarAction) {
        use table_data_toolbar_surface_view::TableDataToolbarAction as Action;
        match action {
            Action::RequestData => self.request_table_data(),
            Action::RefreshBlocked => {
                self.feedback.runtime_message = "Apply or discard staged changes before refreshing".to_owned();
            }
            Action::RunSql(sql) => self.run_table_data_sql(sql),
            Action::RunSqlBlocked => {
                self.feedback.runtime_message = "Apply or discard staged changes before running SQL".to_owned();
            }
            Action::AddRow => self.open_insert_row(),
            Action::OpenPendingChanges => self.table.mutation.pending_changes_open = true,
            Action::ApplyStagedChanges => self.apply_staged_changes(),
            Action::ConfirmDiscardChanges => self.table.editing.discard_changes_confirmation = true,
            Action::DiscardStagedChanges => self.discard_staged_changes(),
            Action::ReloadFailedMutation => self.reload_failed_mutation(),
            Action::DiscardFailedMutation => self.discard_failed_mutation(false),
            Action::ResolveConflict => self.table.mutation.conflict_dialog_open = true,
            Action::RetryFailedMutation => self.retry_failed_mutation_after_reload(),
            Action::RemoveFilter(index) => self.remove_table_filter(index),
            Action::ClearFilters => self.clear_table_filters(),
            Action::ResetPage => self.reset_table_data_page(),
            Action::Export => self.open_table_data_export(),
        }
    }

    pub(crate) fn open_table_data_export(&mut self) {
        if self.table.data_query.result.is_none() {
            self.feedback.runtime_message = "Load table data before exporting".to_owned();
            return;
        }
        self.overlay.export_open = true;
    }
}

fn can_mutate_table_toolbar(connection_writable: bool, query_in_flight: bool) -> bool {
    connection_writable && !query_in_flight
}

#[cfg(test)]
#[path = "tests/table_data_view_tests.rs"]
mod tests;

