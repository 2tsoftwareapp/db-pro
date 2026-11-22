use super::*;

impl DbProApp {
    pub(crate) fn open_duplicate_row(&mut self, result: &UiQueryResult, row_index: usize) {
        if let Some(notice) = table_editor_context::surface_read_only_notice(self.edit_surface()) {
            self.feedback.runtime_message = notice.to_owned();
            return;
        }
        if !self.can_mutate_active_connection() {
            self.feedback.runtime_message = "Connect with write access to insert rows".to_owned();
            return;
        }
        let Some(info) = self.table.state.table_info.clone() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return;
        };
        let Some(row) = result.rows.get(row_index) else {
            return;
        };
        self.table.editing.insert_row_values = table_editor_values::duplicate_row_values(&info, row);
        self.table.editing.insert_row_columns = info.columns.iter().map(|c| c.name.clone()).collect();
        self.table.editing.insert_row_error.clear();
        self.table.editing.insert_row_open = true;
    }

    pub(crate) fn open_insert_row(&mut self) {
        if let Some(notice) = table_editor_context::surface_read_only_notice(self.edit_surface()) {
            self.feedback.runtime_message = notice.to_owned();
            return;
        }
        if !self.can_mutate_active_connection() {
            self.feedback.runtime_message = "Connect with write access to insert rows".to_owned();
            return;
        }
        let Some(info) = self.table.state.table_info.clone() else {
            self.feedback.runtime_message = "Table structure is still loading".to_owned();
            return;
        };
        self.table.editing.insert_row_values = vec![String::new(); info.columns.len()];
        self.table.editing.insert_row_columns = info.columns.iter().map(|c| c.name.clone()).collect();
        self.table.editing.insert_row_error.clear();
    }

    pub(crate) fn submit_insert_row(&mut self) {
        let (table, info) = match self.insert_row_targets() {
            Ok(targets) => targets,
            Err(error) => return self.reject_insert_row(error),
        };
        let (columns, values) =
            match table_editor_values::parse_insert_row_values(&info, &self.table.editing.insert_row_values) {
                Ok(parsed) => parsed,
                Err(error) => return self.reject_insert_row(error),
            };
        self.table.mutation.staged_changes.ensure_target(&table);
        // #587: carry the column types so INSERT values are typed, not sniffed.
        let data_types = insert_column_types(&info, &columns);
        self.table
            .mutation
            .staged_changes
            .stage_insert_typed(columns, data_types, values);
        self.table.editing.insert_row_open = false;
        self.table.editing.insert_row_error.clear();
        self.feedback.runtime_message = format!("Row staged for {}", table);
    }

    pub(super) fn draw_insert_row_dialog(&mut self, ctx: &egui::Context) {
        let Some(info) = self.table.state.table_info.clone() else {
            self.table.editing.insert_row_open = false;
            return;
        };
        let column_names: Vec<&str> = info.columns.iter().map(|c| c.name.as_str()).collect();
        if self.table.editing.insert_row_columns != column_names {
            // Schema refreshed under the open dialog: keep entered values by
            // column name; new columns start empty, dropped ones are lost.
            let old = std::mem::take(&mut self.table.editing.insert_row_values);
            let old_names = std::mem::take(&mut self.table.editing.insert_row_columns);
            self.table.editing.insert_row_values = info
                .columns
                .iter()
                .map(|col| {
                    old_names
                        .iter()
                        .position(|name| name == &col.name)
                        .and_then(|idx| old.get(idx).cloned())
                        .unwrap_or_default()
                })
                .collect();
            self.table.editing.insert_row_columns = info.columns.iter().map(|c| c.name.clone()).collect();
            self.table.editing.insert_row_error =
                "Table structure changed; values were kept by column name".to_owned();
        }
        let error = self.table.editing.insert_row_error.clone();
        let action = {
            let mut context = table_insert_row_surface_view::InsertRowDialogContext {
                theme: self.theme,
                info: &info,
                values: &mut self.table.editing.insert_row_values,
                error: &error,
            };
            table_insert_row_surface_view::draw_dialog(&mut context, ctx)
        };
        match action {
            Some(table_insert_row_surface_view::InsertRowDialogAction::Submit) => self.submit_insert_row(),
            Some(table_insert_row_surface_view::InsertRowDialogAction::Cancel) => {
                // Partially-filled values are expensive to retype — confirm
                // before clearing the form.
                if self
                    .table
                    .editing
                    .insert_row_values
                    .iter()
                    .any(|value| !value.trim().is_empty())
                {
                    self.table.editing.insert_row_discard_confirmation = true;
                } else {
                    self.table.editing.insert_row_open = false;
                    self.table.editing.insert_row_error.clear();
                }
            }
            None => {}
        }

        if self.table.editing.insert_row_discard_confirmation {
            let mut confirm_open = true;
            let confirm_action = crate::components::alert::AlertDialog::new(
                "Discard row values?",
                "The insert form has filled values. Closing clears them.",
                self.theme,
            )
            .confirm_label("Discard")
            .cancel_label("Keep Editing")
            .destructive(true)
            .id_salt("insert_row_discard")
            .show(ctx, &mut confirm_open);
            match confirm_action {
                Some(crate::components::alert::AlertDialogAction::Confirm) => {
                    self.table.editing.insert_row_discard_confirmation = false;
                    self.table.editing.insert_row_open = false;
                    self.table.editing.insert_row_values.iter_mut().for_each(String::clear);
                    self.table.editing.insert_row_error.clear();
                }
                Some(crate::components::alert::AlertDialogAction::Cancel) => {
                    self.table.editing.insert_row_discard_confirmation = false;
                }
                None => {}
            }
            if !confirm_open {
                self.table.editing.insert_row_discard_confirmation = false;
            }
        }
    }
}

/// Column types for a staged insert, in `columns` order (#587).
fn insert_column_types(info: &crate::UiTableInfo, columns: &[String]) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            info.columns
                .iter()
                .find(|candidate| candidate.name == *column)
                .map_or_else(String::new, |candidate| candidate.data_type.clone())
        })
        .collect()
}

impl DbProApp {
    /// Report a rejected insert and keep the dialog open (#587 refactor).
    fn reject_insert_row(&mut self, error: String) {
        self.table.editing.insert_row_error = error;
    }

    /// Resolve the table/info pair `submit_insert_row` needs (#587).
    fn insert_row_targets(&self) -> Result<(String, crate::UiTableInfo), String> {
        let table = self
            .schema
            .explorer
            .selected_table
            .clone()
            .ok_or_else(|| "Select a table before inserting a row".to_owned())?;
        let info = self
            .table
            .state
            .table_info
            .clone()
            .ok_or_else(|| "Table structure is still loading".to_owned())?;
        Ok((table, info))
    }
}
