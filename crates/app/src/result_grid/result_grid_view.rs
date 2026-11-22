// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
pub use super::result_grid_projection::GridSelectionLookup;
use super::*;
use crate::GridProjectionKey;

#[path = "result_grid_body_view.rs"]
mod result_grid_body_view;

#[cfg(test)]
#[path = "tests/result_grid_record_tests.rs"]
mod record_tests;

#[cfg(test)]
#[path = "tests/result_grid_view_tests.rs"]
mod view_tests;

/// Per-cell render context for the result grid.
pub(crate) struct GridCell<'a> {
    pub(crate) visible_indexes: &'a [usize],
    pub(crate) selection_lookup: &'a GridSelectionLookup,
    pub(crate) row_index: usize,
    pub(crate) column_index: usize,
    pub(crate) row_hovered: bool,
    pub(crate) row_selected: bool,
    pub(crate) row_dirty: bool,
    pub(crate) row_mutation_error: bool,
    pub(crate) cell_mutation_error: bool,
    pub(crate) editable: bool,
    pub(crate) width: f32,
    pub(crate) cell: &'a UiCell,
}

/// Context shared by every visible grid row.
pub(crate) struct GridRows<'a> {
    pub(crate) indexes: &'a [usize],
    pub(crate) widths: &'a [f32],
    pub(crate) order: &'a [usize],
    pub(crate) editable: bool,
    pub(crate) row_offset: u64,
    pub(crate) selection_lookup: &'a GridSelectionLookup,
}

struct GridInteractionApplicationContext<'a> {
    ui: &'a mut egui::Ui,
    result: &'a UiQueryResult,
    indexes: &'a [usize],
    order: &'a [usize],
    editable: bool,
    selection_lookup: &'a GridSelectionLookup,
}

impl DbProApp {
    pub(super) fn draw_result_grid(&mut self, ui: &mut egui::Ui, result: &UiQueryResult) {
        if result.columns.is_empty() {
            ui.centered_and_justified(|ui| {
                EmptyState::new(
                    Icon::CircleCheck,
                    &t!("result_grid.statement_completed"),
                    &t!("result_grid.statement_completed_desc"),
                    self.theme,
                )
                .show(ui);
            });
            return;
        }

        let is_table_data =
            self.workspace.active_tab == WorkspaceTab::Table && self.table.state.table_view == TableView::Data;
        self.claim_result_grid_focus(ui, is_table_data);
        if is_table_data {
            self.apply_capture_cell_edit_env();
        }
        let edit_reason = self.result_edit_reason(result);
        let editable = edit_reason.is_none();
        let read_only_notice = self.result_read_only_notice(edit_reason.as_deref());
        let (projection_key, indexes, order, selection_lookup) = self.prepare_grid_cache(result, is_table_data);

        if is_table_data && self.table.editing.record_view_open {
            if let Some(row_index) = self.table.data.selected_row.filter(|row| {
                self.table.data.selected_rows.len() == 1 && selection_lookup.row_positions.contains_key(row)
            }) {
                self.draw_table_record(ui, result, row_index);
                self.restore_grid_cache(projection_key, indexes, order, selection_lookup);
                return;
            }
            self.table.editing.record_view_open = false;
        }

        self.handle_grid_keyboard(ui, result, &indexes, &order, editable, &selection_lookup);

        if !is_table_data {
            let query_notice = (self.edit_surface() == table_editor_context::RowEditSurface::Query)
                .then_some(read_only_notice.as_deref())
                .flatten();
            self.draw_result_grid_toolbar(ui, result, &indexes, editable, query_notice);
        }

        let inspector_open = self.table.editing.record_inspector_open;
        let row_offset = if is_table_data { self.table.data_query.offset } else { 0 };
        if inspector_open {
            let inspector_width = 320.0f32.min(ui.available_width() * 0.45);
            let main_grid_width = (ui.available_width() - inspector_width - 8.0).max(180.0);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(main_grid_width, ui.available_height()),
                    Layout::top_down(Align::Min),
                    |ui| {
                        let widths = self.table.data.column_widths(result.columns.len(), main_grid_width);
                        self.draw_grid_body(
                            ui,
                            result_grid_body_view::ResultGridBodyContext {
                                result,
                                indexes: &indexes,
                                widths: &widths,
                                order: &order,
                                editable,
                                row_offset,
                                selection_lookup: &selection_lookup,
                                theme: self.theme,
                            },
                        );
                    },
                );
                Separator::vertical(self.theme).show(ui);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.available_height()),
                    Layout::top_down(Align::Min),
                    |ui| {
                        self.draw_record_inspector_panel(ui, result);
                    },
                );
            });
        } else {
            let grid_width = ui.available_width().max(0.0);
            let widths = self.table.data.column_widths(result.columns.len(), grid_width);
            self.draw_grid_body(
                ui,
                result_grid_body_view::ResultGridBodyContext {
                    result,
                    indexes: &indexes,
                    widths: &widths,
                    order: &order,
                    editable,
                    row_offset,
                    selection_lookup: &selection_lookup,
                    theme: self.theme,
                },
            );
        }

        self.restore_grid_cache(projection_key, indexes, order, selection_lookup);
    }

    fn draw_result_grid_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        editable: bool,
        read_only_notice: Option<&str>,
    ) {
        let action = {
            let mut context = result_grid_toolbar_view::ResultGridToolbarContext {
                theme: self.theme,
                data: &mut self.table.data,
                editing: &mut self.table.editing,
                feedback: &self.feedback,
                editable,
                read_only_notice,
                matching_rows: indexes.len(),
            };
            result_grid_toolbar_view::draw_toolbar(&mut context, ui)
        };
        if let Some(action) = action {
            match action {
                result_grid_toolbar_view::ResultGridToolbarAction::CopySelectedCell => {
                    self.copy_selected_cell(ui, result);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::CopySelectedRow => {
                    self.copy_selected_row(ui, result);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::CopyVisibleCsv => {
                    self.copy_all_as_csv(ui, result, indexes);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::CopyVisibleJson => {
                    self.copy_all_as_json(ui, result, indexes);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::CopyVisibleMarkdown => {
                    self.copy_all_as_markdown(ui, result, indexes);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::CopyVisibleInsert => {
                    self.copy_all_as_insert(ui, result, indexes);
                }
                result_grid_toolbar_view::ResultGridToolbarAction::InspectSelectedCell {
                    row_index,
                    column_index,
                } => self.open_cell_inspector(result, row_index, column_index),
            }
        }
    }

    fn prepare_grid_cache(
        &mut self,
        result: &UiQueryResult,
        is_table_data: bool,
    ) -> (GridProjectionKey, Vec<usize>, Vec<usize>, GridSelectionLookup) {
        let order = self.table.data.column_order_for_columns(&result.columns);
        let projection_key = self.table.data.projection_key(result);
        let indexes = self
            .table
            .data
            .grid_projection_cache
            .take(projection_key.clone(), result);
        let selection_lookup = self
            .table
            .data
            .grid_selection_cache
            .take(&projection_key, &order)
            .unwrap_or_else(|| GridSelectionLookup::new(&indexes, &order));

        if is_table_data {
            self.table
                .data
                .rebuild_row_identity_cache(result, self.table.state.table_info.as_ref());
        } else {
            self.table.data.grid_row_identity_cache.clear();
            self.table.data.grid_row_identity_cache_ready = false;
        }

        (projection_key, indexes, order, selection_lookup)
    }

    fn restore_grid_cache(
        &mut self,
        projection_key: GridProjectionKey,
        indexes: Vec<usize>,
        order: Vec<usize>,
        selection_lookup: GridSelectionLookup,
    ) {
        self.table
            .data
            .grid_projection_cache
            .restore(projection_key.clone(), indexes);
        self.table
            .data
            .grid_selection_cache
            .restore(projection_key, order, selection_lookup);
    }

    pub(crate) fn set_table_or_grid_sort(
        &mut self,
        result: &UiQueryResult,
        column_index: usize,
        descending: Option<bool>,
    ) {
        let server_side = self.workspace.active_tab == WorkspaceTab::Table
            && self.table.state.table_view == TableView::Data
            && !self.table.data_query.inline_query_result;
        if server_side {
            if !self.table.mutation.staged_changes.is_empty() {
                self.feedback.runtime_message = "Apply or discard staged changes before changing sort".to_owned();
                return;
            }
            let column = result.columns.get(column_index).map(|column| column.name.clone());
            self.table.data_query.set_sort(column, descending);
            self.table.data.grid_sort_column = None;
            self.table.data.grid_sort_desc = false;
            self.reload_table_data_from_start();
        } else {
            self.table.data.grid_sort_column = descending.map(|_| column_index);
            self.table.data.grid_sort_desc = descending.unwrap_or(false);
        }
    }

    /// Cycle a table-data sort clause. Shift-click keeps other clauses and
    /// makes the clicked column the next priority; plain click selects one
    /// clause and cycles ASC -> DESC -> none.\
    pub(crate) fn cycle_table_data_sort(&mut self, result: &UiQueryResult, column_index: usize, additive: bool) {
        if !self.table.mutation.staged_changes.is_empty() {
            self.feedback.runtime_message = "Apply or discard staged changes before changing sort".to_owned();
            return;
        }
        let Some(column) = result.columns.get(column_index).map(|column| column.name.clone()) else {
            return;
        };
        self.table.data_query.cycle_sort(column, additive);
        self.table.data.grid_sort_column = None;
        self.table.data.grid_sort_desc = false;
        self.reload_table_data_from_start();
    }

    /// Registers the grid's focus id so a focus command survives the next frame.
    fn claim_result_grid_focus(&self, ui: &mut egui::Ui, is_table_data: bool) {
        let id = result_grid_keyboard_view::result_grid_focus_id(ui);
        // A zero-size interactable is unreachable — audit flags it as a dead
        // focus target; give the claim the grid's visible bounds (#344).
        let rect = ui.min_rect().intersect(ui.clip_rect());
        let _ = ui.interact(rect, id, egui::Sense::focusable_noninteractive());
        // `handle_shortcuts` requests focus without a `Ui` in hand, so the
        // output-dock grid records its (ui-scoped) focus id once per frame.
        if !is_table_data {
            result_grid_keyboard_view::record_query_result_grid_focus_id(ui);
        }
    }

    /// Copy, paste-to-edit, staged-changes, and keyboard navigation for the result grid.
    pub(crate) fn handle_grid_keyboard(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        order: &[usize],
        editable: bool,
        selection_lookup: &GridSelectionLookup,
    ) {
        let actions = {
            let mut context = result_grid_interaction_surface_view::ResultGridInteractionContext {
                result,
                indexes,
                order,
                editable,
                table_state: &self.table.state,
                table_data: &mut self.table.data,
                table_editing: &mut self.table.editing,
                table_mutation: &self.table.mutation,
                feedback: &mut self.feedback,
            };
            context.handle_keyboard(ui, self.connection.dialog.is_open())
        };
        self.apply_grid_interaction_actions(
            GridInteractionApplicationContext {
                ui,
                result,
                indexes,
                order,
                editable,
                selection_lookup,
            },
            actions,
        );
    }

    fn apply_grid_interaction_actions(
        &mut self,
        context: GridInteractionApplicationContext<'_>,
        actions: Vec<result_grid_interaction_surface_view::ResultGridInteractionAction>,
    ) {
        use result_grid_interaction_surface_view::ResultGridInteractionAction as Action;

        for action in actions {
            match action {
                Action::CopySelectedRows => {
                    self.copy_selected_rows(context.ui, context.result, context.selection_lookup)
                }
                Action::CopySelectedCell => self.copy_selected_cell(context.ui, context.result),
                Action::ApplyStagedChanges => self.apply_staged_changes(),
                Action::DiscardStagedChanges => self.discard_staged_changes(),
                Action::DeleteSelectedRows => self.request_delete_selected_data_rows(context.result),
                Action::SubmitCellEdit {
                    row_index,
                    column_index,
                } => {
                    self.submit_data_cell_edit(context.result, row_index, column_index);
                }
                Action::BeginCellEdit {
                    row_index,
                    column_index,
                } => {
                    if let Some(cell) = context
                        .result
                        .rows
                        .get(row_index)
                        .and_then(|row| row.get(column_index))
                        .cloned()
                    {
                        self.begin_data_cell_edit(context.result, row_index, column_index, &cell);
                    }
                }
                Action::CommitEditAndNavigate => {
                    if self.commit_active_data_edit(context.result) {
                        self.navigate_grid(
                            context.ui,
                            context.indexes,
                            context.order,
                            context.editable,
                            context.selection_lookup,
                        );
                    }
                    return;
                }
                Action::Navigate => self.navigate_grid(
                    context.ui,
                    context.indexes,
                    context.order,
                    context.editable,
                    context.selection_lookup,
                ),
            }
        }
    }

    fn navigate_grid(
        &mut self,
        ui: &mut egui::Ui,
        indexes: &[usize],
        order: &[usize],
        editable: bool,
        selection_lookup: &GridSelectionLookup,
    ) {
        let mut context = result_grid_selection::GridNavigationContext {
            data: &mut self.table.data,
            editing: &mut self.table.editing,
            feedback: &mut self.feedback,
        };
        result_grid_selection::handle_grid_navigation(ui, indexes, order, editable, selection_lookup, &mut context);
    }

    /// The write policy for the column behind a grid cell, if it is blocked.
    pub(crate) fn blocked_write_for_cell(
        &self,
        result: &UiQueryResult,
        column_index: usize,
    ) -> Option<ColumnWriteBlock> {
        let column = result.columns.get(column_index)?;
        self.table.state.column_write_block(&column.name)
    }

    /// Scrollable grid: continuous spreadsheet header plus visible slice of rows.
    fn draw_grid_body(&mut self, ui: &mut egui::Ui, context: result_grid_body_view::ResultGridBodyContext<'_>) {
        let mut renderer = GridBodyRenderer { app: self };
        result_grid_body_view::draw_body(ui, context, &mut renderer);
    }

    /// One grid row: the row-number gutter plus every visible cell with continuous borders.
    pub(crate) fn draw_grid_row(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        rows: &GridRows<'_>,
        position: usize,
    ) {
        let row_index = rows.indexes[position];
        let row = &result.rows[row_index];
        let row_selected = self.table.data.selected_rows.contains(&row_index);
        let row_dirty = self.staged_row_deleted(result, row_index)
            || (0..result.columns.len())
                .any(|column_index| self.staged_cell_value(result, row_index, column_index).is_some());
        let row_identity =
            self.table
                .data
                .row_identity_for_result(result, self.table.state.table_info.as_ref(), row_index);
        let row_mutation_error = row_identity
            .as_ref()
            .is_some_and(|identity| self.table.mutation.mutation_error_for_identity(identity));
        let cell_mutation_errors = (0..result.columns.len())
            .map(|column_index| {
                row_identity
                    .as_ref()
                    .is_some_and(|identity| self.table.mutation.mutation_error_for_cell(identity, column_index))
            })
            .collect::<Vec<_>>();
        let theme = self.theme;
        let mut renderer = GridRowRenderer { app: self, result };
        result_grid_row_view::draw_row(
            ui,
            result_grid_row_view::GridRowSurfaceContext {
                row,
                rows,
                row_index,
                display_position: position,
                row_selected,
                row_dirty,
                row_mutation_error,
                cell_mutation_errors: &cell_mutation_errors,
                theme,
            },
            &mut renderer,
        );
    }
}

struct GridRowRenderer<'a> {
    app: &'a mut DbProApp,
    result: &'a UiQueryResult,
}

struct GridBodyRenderer<'a> {
    app: &'a mut DbProApp,
}

impl result_grid_body_view::ResultGridBodyRenderer for GridBodyRenderer<'_> {
    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        widths: &[f32],
        order: &[usize],
    ) {
        self.app.draw_grid_header(ui, result, indexes, widths, order);
    }

    fn draw_row(&mut self, ui: &mut egui::Ui, input: result_grid_body_view::ResultGridRowInput<'_>) {
        self.app.draw_grid_row(ui, input.result, input.rows, input.position);
    }
}

impl result_grid_row_view::GridRowSurfaceRenderer for GridRowRenderer<'_> {
    fn on_gutter_click(&mut self, ui: &mut egui::Ui, rows: &GridRows<'_>, position: usize) -> bool {
        if self.app.table.editing.data_editing_cell.is_some() && !self.app.commit_active_data_edit(self.result) {
            return false;
        }
        self.app.table.data.selected_cell = None;
        let modifiers = ui.input(|input| input.modifiers);
        self.app.table.data.select_visible_row(
            rows.indexes,
            &rows.selection_lookup.row_positions,
            position,
            modifiers.shift,
            modifiers.command || modifiers.ctrl,
        );
        self.app.table.data.selection_anchor_cell = None;
        self.app.table.editing.data_editing_cell = None;
        self.app.table.editing.data_edit_value.clear();
        self.app.feedback.copy_status.clear();
        true
    }

    fn draw_cell(&mut self, ui: &mut egui::Ui, cell: GridCell<'_>) {
        self.app.draw_grid_cell(ui, self.result, cell);
    }
}
