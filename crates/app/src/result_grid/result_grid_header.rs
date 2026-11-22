use super::result_grid_header_surface_view::{GridHeaderAction, GridHeaderInput, GridHeaderViewContext};
use super::*;

impl DbProApp {
    pub(super) fn draw_grid_header(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        widths: &[f32],
        order: &[usize],
    ) {
        let actions = GridHeaderViewContext {
            theme: self.theme,
            workspace: &self.workspace,
            table: &self.table,
        }
        .draw_header(ui, GridHeaderInput { result, widths, order });
        for action in actions {
            self.apply_grid_header_action(ui, result, indexes, order, action);
        }
    }

    fn apply_grid_header_action(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        order: &[usize],
        action: GridHeaderAction,
    ) {
        match action {
            GridHeaderAction::Sort {
                column_index,
                descending,
            } => {
                self.set_table_or_grid_sort(result, column_index, descending);
            }
            GridHeaderAction::CycleTableSort { column_index, shift } => {
                self.cycle_table_data_sort(result, column_index, shift);
            }
            GridHeaderAction::StartResize { widths } => {
                self.table.data.grid_column_widths = widths;
                self.table.data.grid_columns_user_resized = true;
            }
            GridHeaderAction::Resize { column_index, delta } => {
                if let Some(width) = self.table.data.grid_column_widths.get_mut(column_index) {
                    *width = (*width + delta).clamp(60.0, 1000.0);
                }
            }
            GridHeaderAction::CopyColumnName(column_index) => {
                self.copy_column_name(ui, result, column_index);
            }
            GridHeaderAction::CopyColumnValues(column_index) => {
                self.copy_column_values(ui, result, column_index, indexes);
            }
            GridHeaderAction::MoveLeft(index) if index > 0 => {
                self.table.data.move_column(index, index - 1, order.len());
            }
            GridHeaderAction::MoveRight(index) if index + 1 < order.len() => {
                self.table.data.move_column(index, index + 1, order.len());
            }
            GridHeaderAction::ResetOrder => {
                self.table.data.grid_column_order = (0..result.columns.len()).collect();
            }
            GridHeaderAction::ResetWidths => {
                self.table.data.grid_columns_user_resized = false;
            }
            GridHeaderAction::HideColumn(column_index) => {
                self.table.data.hide_column(column_index, order.len());
            }
            GridHeaderAction::AddFilter(column_index) => {
                if let Some(column) = result.columns.get(column_index) {
                    self.table.data_query.filter_column = column.name.clone();
                    self.table.data_query.filter_operator = UiTableFilterOperator::Equals;
                    self.table.data_query.filter_value.clear();
                    self.table.data_query.filter_editing = None;
                    self.feedback.runtime_message = format!("Filter draft ready for {}", column.name);
                }
            }
            GridHeaderAction::ShowColumns => self.table.data.show_all_columns(),
            GridHeaderAction::ResetLayout => self.table.data.reset_grid_layout(result.columns.len()),
            GridHeaderAction::AutoSize(column_index) => {
                // #582: the grid is shared via `Rc`, so a header action that
                // mutates column state must be applied to the owned copy.
                self.table.data.auto_size_column(result, indexes, column_index);
            }
            GridHeaderAction::MoveLeft(_) | GridHeaderAction::MoveRight(_) => {}
        }
    }
}
