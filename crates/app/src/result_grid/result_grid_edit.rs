//! In-cell editor + advanced value inspector for the result grid (#228).
use super::*;
use egui::Rect;

#[path = "result_grid_cell_editor_surface_view.rs"]
mod result_grid_cell_editor_surface_view;
#[path = "result_grid_date_picker_view.rs"]
mod result_grid_date_picker_view;
#[path = "result_grid_inspector_surface_view.rs"]
mod result_grid_inspector_surface_view;
#[path = "result_grid_record_surface_view.rs"]
mod result_grid_record_surface_view;

impl DbProApp {
    pub(super) fn draw_grid_cell_editor(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
        cell_rect: Rect,
    ) {
        let kind = self.classify_cell_editor(result, column_index);
        let is_expanded = self.table.editing.expanded_data_editor == Some((row_index, column_index));
        if !is_expanded {
            let action = {
                let mut context = result_grid_cell_editor_surface_view::CellEditorContext {
                    theme: self.theme,
                    kind,
                    value: &mut self.table.editing.data_edit_value,
                    error: &mut self.table.editing.data_edit_error,
                };
                result_grid_cell_editor_surface_view::draw(&mut context, ui, cell_rect)
            };
            match action {
                Some(result_grid_cell_editor_surface_view::CellEditorAction::Commit) => {
                    self.submit_data_cell_edit(result, row_index, column_index);
                }
                Some(result_grid_cell_editor_surface_view::CellEditorAction::Cancel) => {
                    self.close_data_inspector();
                }
                None => {}
            }
            // Clicking anywhere outside the cell commits — the same end state
            // as clicking another cell, which commits via its own handler.
            // While a select/calendar popup is open, clicks inside it land
            // outside the cell rect but must not commit; `any_popup_open`
            // covers both (a committed pick closes its popup first).
            let clicked_outside = ui.ctx().input(|input| {
                input.pointer.primary_clicked()
                    && input.pointer.interact_pos().is_some_and(|pos| !cell_rect.contains(pos))
            });
            if clicked_outside && !egui::Popup::is_any_open(ui.ctx()) {
                self.commit_active_data_edit(result);
            }
            return;
        }

        self.draw_advanced_cell_inspector(ui, result, row_index, column_index);
    }

    fn classify_cell_editor(
        &self,
        result: &UiQueryResult,
        column_index: usize,
    ) -> result_grid_cell_editor_surface_view::CellEditorKind {
        use result_grid_cell_editor_surface_view::CellEditorKind;
        let Some(column) = result.columns.get(column_index) else {
            return CellEditorKind::Text;
        };
        if self.table.editing.fk_lookup_column.as_deref() == Some(column.name.as_str())
            && !self.table.editing.fk_lookup_options.is_empty()
        {
            return CellEditorKind::Select(self.table.editing.fk_lookup_options.clone());
        }
        let data_type = column.data_type.to_ascii_lowercase();
        if data_type.contains("bool") {
            return CellEditorKind::Boolean {
                nullable: column.nullable,
            };
        }
        if let Some(labels) = self
            .table
            .state
            .table_info
            .as_ref()
            .and_then(|info| info.columns.iter().find(|item| item.name == column.name))
            .filter(|item| !item.enum_labels.is_empty())
            .map(|item| item.enum_labels.clone())
        {
            let mut values = labels;
            if column.nullable {
                values.push("NULL".to_owned());
            }
            return CellEditorKind::Select(values);
        }
        if data_type.contains("enum") {
            let mut seen = std::collections::BTreeSet::new();
            for row in &result.rows {
                if let Some(UiCell::Text(value)) = row.get(column_index) {
                    seen.insert(value.clone());
                }
                if seen.len() >= 200 {
                    break;
                }
            }
            let mut values: Vec<String> = seen.into_iter().collect();
            if column.nullable {
                values.push("NULL".to_owned());
            }
            return CellEditorKind::Select(values);
        }
        if data_type.starts_with("date") || data_type.contains("timestamp") || data_type.contains("datetime") {
            return CellEditorKind::Temporal {
                date_only: data_type.starts_with("date") && !data_type.contains("time"),
            };
        }
        CellEditorKind::Text
    }

    pub(super) fn draw_advanced_cell_inspector(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        row_index: usize,
        column_index: usize,
    ) {
        let ctx = ui.ctx().clone();
        let column_name = result
            .columns
            .get(column_index)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("col{column_index}"));
        let data_type = result
            .columns
            .get(column_index)
            .map(|c| c.data_type.clone())
            .unwrap_or_default();
        let write_block = self
            .table
            .state
            .column_write_policy(&column_name)
            .and_then(|policy| policy.write_block());
        let writable = write_block.is_none() && self.table.editing.data_editing_cell.is_some();
        let truncated = result
            .rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .is_some_and(UiCell::is_truncated);
        let cell = self
            .copy_cell_value(result, row_index, column_index)
            .unwrap_or(UiCell::Null);
        let kind = cell_inspector::classify_cell(&cell, &data_type);
        let action = {
            let mut context = result_grid_inspector_surface_view::ValueInspectorContext {
                theme: self.theme,
                row_index,
                column_index,
                column_name: &column_name,
                data_type: &data_type,
                cell,
                kind,
                truncated,
                write_block,
                writable,
                mode: &mut self.table.editing.cell_inspector_mode,
                value: &mut self.table.editing.data_edit_value,
                error: &mut self.table.editing.data_edit_error,
            };
            result_grid_inspector_surface_view::draw(&mut context, &ctx)
        };

        match action {
            Some(result_grid_inspector_surface_view::ValueInspectorAction::CopyRaw) => {
                ctx.copy_text(self.table.editing.data_edit_value.clone());
                self.feedback.runtime_message = t!("explorer.copy_raw_value").to_string();
            }
            Some(result_grid_inspector_surface_view::ValueInspectorAction::ExportBytes) => {
                self.export_inspected_bytes();
            }
            Some(result_grid_inspector_surface_view::ValueInspectorAction::Apply) => {
                self.submit_data_cell_edit(result, row_index, column_index);
            }
            Some(result_grid_inspector_surface_view::ValueInspectorAction::Close) => {
                self.close_data_inspector();
            }
            None if ctx.input(|input| input.key_pressed(egui::Key::Escape)) => {
                self.close_data_inspector();
            }
            None => {}
        }
    }

    fn close_data_inspector(&mut self) {
        self.table.editing.data_editing_cell = None;
        self.table.editing.expanded_data_editor = None;
        self.table.editing.data_edit_value.clear();
        self.table.editing.data_edit_error = None;
    }

    pub(super) fn open_cell_inspector(&mut self, result: &UiQueryResult, row_index: usize, column_index: usize) {
        let Some(cell) = self.copy_cell_value(result, row_index, column_index) else {
            return;
        };
        // #537: a cell the runtime truncated holds only a display prefix. Staging an edit
        // would write that prefix back over the full value, so the inspector is read-only.
        let truncated = result
            .rows
            .get(row_index)
            .and_then(|row| row.get(column_index))
            .is_some_and(UiCell::is_truncated);
        self.table.data.selected_cell = Some((row_index, column_index));
        self.table.data.selected_row = Some(row_index);
        let write_block = result
            .columns
            .get(column_index)
            .and_then(|column| self.table.state.column_write_policy(&column.name))
            .and_then(|policy| policy.write_block());
        if write_block.is_none() && self.can_mutate_active_connection() && !truncated {
            self.table.editing.data_editing_cell = Some((row_index, column_index));
        } else {
            self.table.editing.data_editing_cell = None;
        }
        self.table.editing.expanded_data_editor = Some((row_index, column_index));
        self.table.editing.cell_inspector_mode = match cell_inspector::classify_cell(
            &cell,
            result
                .columns
                .get(column_index)
                .map(|c| c.data_type.as_str())
                .unwrap_or(""),
        ) {
            cell_inspector::CellInspectorKind::Json => cell_inspector::CellInspectorMode::Pretty,
            cell_inspector::CellInspectorKind::Bytes => cell_inspector::CellInspectorMode::Hex,
            _ => cell_inspector::CellInspectorMode::Raw,
        };
        self.table.editing.data_edit_error = None;
        self.table.editing.data_edit_value = cell_inspector::cell_raw_text(&cell);
        if let Some(reason) = write_block {
            self.feedback.runtime_message = reason.reason().to_owned();
        } else if truncated {
            self.feedback.runtime_message = t!("query.cell_truncated_for_display").into_owned();
        }
    }

    fn export_inspected_bytes(&mut self) {
        match cell_inspector::decode_bytes_payload(&self.table.editing.data_edit_value) {
            Ok(bytes) => {
                let path = std::env::temp_dir().join(format!(
                    "db-pro-cell-export-{}.bin",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                ));
                match std::fs::write(&path, &bytes) {
                    Ok(()) => {
                        self.feedback.runtime_message =
                            format!("Exported {} byte(s) → {}", bytes.len(), path.display());
                    }
                    Err(err) => self.feedback.runtime_message = format!("Export failed: {err}"),
                }
            }
            Err(err) => self.feedback.runtime_message = format!("Cannot export bytes: {err}"),
        }
    }

    pub(super) fn draw_record_inspector_panel(&mut self, ui: &mut egui::Ui, result: &UiQueryResult) {
        if !self.table.editing.record_inspector_open {
            return;
        }
        let Some(row_index) = self
            .table
            .data
            .selected_row
            .or_else(|| self.table.data.selected_cell.map(|(r, _)| r))
        else {
            ui.label(
                RichText::new("Select a row to inspect the full record.")
                    .small()
                    .color(self.theme.text_muted),
            );
            return;
        };
        let Some(row) = result.rows.get(row_index) else {
            return;
        };
        let context = result_grid_record_surface_view::RecordInspectorContext {
            theme: self.theme,
            row_index,
            columns: &result.columns,
            row,
        };
        match result_grid_record_surface_view::draw(&context, ui) {
            Some(result_grid_record_surface_view::RecordInspectorAction::Close) => {
                self.table.editing.record_inspector_open = false;
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::Inspect(column_index)) => {
                self.open_cell_inspector(result, row_index, column_index);
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::CopyLabel(column_index)) => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(result.columns[column_index].name.clone())));
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::CopyValue(column_index)) => {
                self.copy_cell_at(ui, result, row_index, column_index);
            }
            None => {}
        }
    }

    pub(super) fn draw_table_record(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        row_index: usize,
    ) {
        let Some(source_row) = result.rows.get(row_index) else {
            self.table.editing.record_view_open = false;
            return;
        };
        let row: Vec<_> = source_row.iter()
            .enumerate()
            .map(|(column_index, cell)| {
                self.staged_cell_value(result, row_index, column_index)
                    .unwrap_or_else(|| cell.clone())
            })
            .collect();
        let context = result_grid_record_surface_view::RecordInspectorContext {
            theme: self.theme,
            row_index,
            columns: &result.columns,
            row: &row,
        };
        match result_grid_record_surface_view::draw_record(&context, ui) {
            Some(result_grid_record_surface_view::RecordInspectorAction::Close) => {
                self.table.editing.record_view_open = false
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::CopyLabel(column_index)) => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(result.columns[column_index].name.clone())));
                self.feedback.copy_status = "Field name copied".to_owned();
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::CopyValue(column_index)) => {
                self.copy_cell_at(ui, result, row_index, column_index);
            }
            Some(result_grid_record_surface_view::RecordInspectorAction::Inspect(column_index)) => {
                self.open_cell_inspector(result, row_index, column_index);
                self.table.editing.data_editing_cell = None;
            }
            None => {}
        }
    }

    pub(super) fn commit_active_data_edit(&mut self, result: &UiQueryResult) -> bool {
        if let Some((row_index, column_index)) = self.table.editing.data_editing_cell {
            self.submit_data_cell_edit(result, row_index, column_index)
        } else {
            true
        }
    }
}

#[cfg(test)]
#[path = "tests/result_grid_edit_tests.rs"]
mod tests;

