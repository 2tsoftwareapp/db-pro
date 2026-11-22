//! Query destructive-run, export, and SQL diagnostics helpers.
use super::query_view::write_file_atomically;
use super::*;
use crate::app::overlay_state::ExportFormat;

/// SQL exports need a target table; an empty dialog field degrades to a clearly-named placeholder
/// instead of an invalid `""` identifier (#426).
fn export_table_name_or_placeholder(name: &str) -> &str {
    if name.is_empty() { "exported_rows" } else { name }
}

/// `~/Downloads/<name>_<timestamp>.<ext>` — the target a dialog that opens
/// without a path starts with (#612), instead of an empty field.
fn default_export_path(table_name: &str, format: ExportFormat) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("{}_{stamp}.{}", export_stem(table_name), format.extension());
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return file_name;
    };
    let downloads = home.join("Downloads");
    let dir = if downloads.is_dir() { downloads } else { home };
    dir.join(file_name).to_string_lossy().into_owned()
}

/// Filesystem-safe stem that still reads like the table name.
fn export_stem(table_name: &str) -> String {
    let cleaned: String = table_name
        .chars()
        .map(|ch| if ch.is_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() { "results".to_owned() } else { trimmed.to_owned() }
}

impl DbProApp {
    /// Held destructive statements render as a ctx-level overlay so the
    /// confirmation can't be stranded inside a closed Results dock or a
    /// Messages tab — `hold_destructive_run` parks the statement until the
    /// dialog answers.
    pub(super) fn draw_destructive_run_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.query.execution.pending_destructive_run().cloned() else {
            return;
        };
        match (query_dialog_surface_view::DestructiveDialogContext {
            theme: self.theme,
            pending: &pending,
        })
        .draw_ctx(ctx)
        {
            Some(query_dialog_surface_view::DestructiveDialogAction::Confirm) => {
                self.confirm_pending_destructive_run()
            }
            Some(query_dialog_surface_view::DestructiveDialogAction::Cancel) => {
                self.cancel_pending_destructive_run()
            }
            None => {}
        }
    }

    /// Bulk tab closes (Other/Right/All) with dirty documents wait on this
    /// ctx-level confirmation; `Discard` replays the parked close scope.
    pub(super) fn draw_bulk_dirty_close_dialog(&mut self, ctx: &egui::Context) {
        let Some(scope) = self.query.session.pending_bulk_close else {
            return;
        };
        let dirty_count = self
            .query
            .session
            .documents
            .iter()
            .enumerate()
            .filter(|(index, document)| {
                let in_scope = match scope {
                    PendingBulkClose::Others(keep) => *index != keep,
                    PendingBulkClose::ToRight(boundary) => *index > boundary,
                    PendingBulkClose::All => true,
                };
                in_scope && document.is_dirty()
            })
            .count();
        let mut open = true;
        let action = crate::components::alert::AlertDialog::new(
            "Close unsaved queries?",
            format!("{dirty_count} tab(s) have unsaved SQL. Closing discards it permanently."),
            self.theme,
        )
        .confirm_label("Discard && Close")
        .cancel_label("Keep Tabs")
        .destructive(true)
        .id_salt("bulk_dirty_close_dialog")
        .show(ctx, &mut open);
        if !open && action.is_none() {
            self.query.session.pending_bulk_close = None;
            return;
        }
        use crate::components::alert::AlertDialogAction;
        match action {
            Some(AlertDialogAction::Confirm) => {
                match scope {
                    PendingBulkClose::Others(keep) => self.close_other_query_documents(keep),
                    PendingBulkClose::ToRight(index) => self.close_query_documents_to_right(index),
                    PendingBulkClose::All => self.close_all_tabs(),
                }
            }
            Some(AlertDialogAction::Cancel) => {
                self.query.session.pending_bulk_close = None;
            }
            None => {}
        }
    }

    pub(super) fn draw_export_dialog(&mut self, ui: &mut egui::Ui, result: Option<&UiQueryResult>) {
        if !self.overlay.export_open {
            return;
        }
        // Freeze the exported result the first frame the dialog is open (#404):
        // a query that finishes between open and Export must not rewrite what
        // the user previewed.
        if self.overlay.export_snapshot.is_none() {
            self.overlay.export_snapshot = result.cloned();
        }
        // #396: pre-fill the export target only from the source the result actually shows —
        // a browsed table (or view) in the Table/SchemaObject data view. A query result has no
        // derivable source table, so the field stays empty and the user types the target rather
        // than silently getting whatever happens to be selected in the explorer.
        if self.overlay.export_table_name.trim().is_empty() {
            self.overlay.export_table_name = self
                .export_source_table_name()
                .unwrap_or_default();
        }
        // #612: a dialog that opens with an empty Output file makes the user
        // invent an absolute path. Pre-fill `~/Downloads/<name>_<timestamp>` and
        // let them overwrite it; the extension follows the chosen format.
        if self.overlay.export_path.trim().is_empty() {
            self.overlay.export_path = default_export_path(
                self.overlay.export_table_name.trim(),
                self.overlay.export_format,
            );
        }
        let result = self.overlay.export_snapshot.clone();
        // COPY … FROM stdin is a PostgreSQL script; hide it for other drivers and
        // fall back so a format remembered from another connection cannot be exported.
        let copy_supported = matches!(
            crate::app::connection::mapper::ui_driver_from_str(self.active_driver()),
            crate::UiDriver::Postgres
        );
        if !copy_supported && self.overlay.export_format == ExportFormat::SqlCopy {
            self.overlay.export_format = ExportFormat::Csv;
            self.overlay.export_overwrite_pending = None;
        }
        let action = query_dialog_surface_view::ExportDialogContext {
            theme: self.theme,
            overlay: &mut self.overlay,
            copy_supported,
        }
        .draw(ui.ctx());
        match action {
            Some(query_dialog_surface_view::ExportDialogAction::Export) => {
                if let Some(result) = &result {
                    self.export_result(result);
                }
            }
            Some(query_dialog_surface_view::ExportDialogAction::Cancel) => {
                self.overlay.export_open = false;
                self.overlay.export_snapshot = None;
                self.overlay.export_overwrite_pending = None;
                self.overlay.export_table_name.clear();
            }
            Some(query_dialog_surface_view::ExportDialogAction::Overwrite) => {
                if let Some(result) = &result {
                    self.export_result_confirming_overwrite(result);
                }
            }
            Some(query_dialog_surface_view::ExportDialogAction::KeepExisting) => {
                self.overlay.export_overwrite_pending = None;
            }
            None => {}
            Some(query_dialog_surface_view::ExportDialogAction::Browse) => {
                // A newly picked path is not the path the prompt named (#539).
                self.overlay.export_overwrite_pending = None;
                let request_id = self.next_request_id();
                self.dispatch_command(UiCommand::PickExportFile { request_id });
            }
        }
    }

    /// The table the export dialog's data actually comes from, when it is a browsed table/view:
    /// `None` for arbitrary query results (#396).
    fn export_source_table_name(&self) -> Option<String> {
        match self.workspace.active_tab {
            WorkspaceTab::Table if self.table.state.table_view == TableView::Data => {
                self.schema.explorer.selected_table.clone()
            }
            WorkspaceTab::SchemaObject
                if self.schema.explorer.schema_object_view == SchemaObjectView::Data =>
            {
                match &self.schema.explorer.selected_schema_object {
                    Some(SchemaObjectSelection::View(name)) => Some(name.clone()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn explain_query(&mut self) {
        self.dispatch_explain_query(false);
    }

    pub(crate) fn explain_query_analyze(&mut self) {
        self.dispatch_explain_query(true);
    }

    fn dispatch_explain_query(&mut self, analyze: bool) {
        let capabilities = self.query_capabilities();
        let connection_id = self.active_query_connection_id().map(str::to_owned);
        let driver = self.active_query_driver().to_owned();
        let Some(request) = self
            .query_explain_context()
            .prepare(connection_id, &capabilities, analyze, &driver)
        else {
            return;
        };
        // EXPLAIN ANALYZE executes its inner statement: a destructive one must go
        // through the same confirmation gate as a direct run, not just a checkbox.
        let held_destructive = request.analyze
            && request.analyze_executes
            && db_pro_core::domain::safety::classify_script_safety(&request.sql)
                == Some(db_pro_core::domain::safety::StatementSafety::Destructive);
        if held_destructive {
            let options = super::events_query_dispatch::QueryRunOptions {
                execution_range: (0, request.sql.len()),
                version: self.query.session.active_buffer_version(),
                all_statements: false,
                bound_params: None,
                applied_row_limit: None,
            };
            self.hold_destructive_run(&request.sql, PendingDestructiveRunOrigin::ExplainAnalyze, &options);
            return;
        }
        let request_id = self.next_request_id();
        let command = UiCommand::ExplainQuery {
            request_id,
            connection_id: request.connection_id.clone(),
            sql: request.sql.clone(),
            params: request.params.clone(),
            analyze: request.analyze,
            schema: self.query.session.active_schema().map(str::to_owned),
            session_id: self.query.session.active_document().map(|document| document.id.clone()),
        };
        if self.dispatch_command(command) {
            self.query_explain_context().commit_dispatched(request_id, &request);
        }
    }

    fn query_explain_context(&mut self) -> query_explain_actions::QueryExplainContext<'_> {
        query_explain_actions::QueryExplainContext::new(
            &mut self.query.session,
            &mut self.query.execution,
            &mut self.query.output,
            &mut self.feedback,
        )
    }

    pub(crate) fn export_result(&mut self, result: &UiQueryResult) {
        self.export_result_to_disk(result, false);
    }

    /// Write the export after the user confirmed overwriting an existing file (#244, E-1).
    pub(crate) fn export_result_confirming_overwrite(&mut self, result: &UiQueryResult) {
        self.export_result_to_disk(result, true);
    }

    /// Export the visible result to `export_path`.
    ///
    /// #244 (E-1): the file is published atomically and an existing path is never replaced without
    /// asking, so a failed export cannot leave a truncated file that looks complete and a typo in the
    /// path cannot destroy an unrelated file.
    ///
    /// #539: confirmation is bound to the effective path the prompt named. Changing the Format or
    /// the path while the prompt is open must not let `Overwrite` replace a file the user never saw.
    pub(super) fn export_result_to_disk(&mut self, result: &UiQueryResult, overwrite: bool) {
        let path_text = self.overlay.export_path.trim().to_owned();
        if path_text.is_empty() {
            self.feedback.runtime_message = t!("query.export_path_missing").into_owned();
            return;
        }
        // #396: the written file's extension must match the chosen format — a path with a missing
        // or wrong extension gets the format's extension appended (checked for existence after
        // normalization so the overwrite prompt covers the real destination).
        let path = self.overlay.effective_export_path();
        let path_text = path.to_string_lossy().into_owned();
        let overwrite_confirmed = overwrite
            && self.overlay.export_overwrite_pending.as_deref() == Some(path.as_path());
        if path.exists() && !overwrite_confirmed {
            self.overlay.export_overwrite_pending = Some(path);
            return;
        }

        let output = if self.overlay.export_format == ExportFormat::Excel {
            match result_grid_export::format_result_excel(result) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.feedback.runtime_message = t!("query.export_failed", error = error.to_string().as_str()).into_owned();
                    self.overlay.export_overwrite_pending = None;
                    self.overlay.export_open = false;
                    self.overlay.export_snapshot = None;
                    self.overlay.export_table_name.clear();
                    return;
                }
            }
        } else {
            let driver = self.active_driver().to_owned();
            match self.overlay.export_format {
                // Empty field falls back to a visible placeholder rather than `INSERT INTO ""` (#426).
                ExportFormat::SqlInsert => result_grid_export::format_result_sql_insert(
                    result,
                    export_table_name_or_placeholder(self.overlay.export_table_name.trim()),
                    &driver,
                ),
                ExportFormat::SqlCopy => result_grid_export::format_result_copy(
                    result,
                    export_table_name_or_placeholder(self.overlay.export_table_name.trim()),
                    &driver,
                ),
                ExportFormat::Json => result_grid_export::format_result_json(result),
                ExportFormat::Markdown => result_grid_export::format_result_markdown(result),
                ExportFormat::Csv | ExportFormat::Tsv => result_grid_export::format_result_delimited_with_null(
                    result,
                    self.overlay.export_format.delimiter().unwrap_or(","),
                    self.overlay.export_null.text(),
                ),
                ExportFormat::Excel => unreachable!("excel handled above"),
            }
            .into_bytes()
        };
        // #612: Excel on a vi-VN locale reads a BOM-less CSV as the system
        // codepage, so Vietnamese text arrives as mojibake. TSV/JSON/SQL stay
        // BOM-less — they are consumed by tools, not by a spreadsheet.
        let output = if self.overlay.export_format == ExportFormat::Csv {
            let mut with_bom = Vec::with_capacity(output.len() + 3);
            with_bom.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            with_bom.extend_from_slice(&output);
            with_bom
        } else {
            output
        };
        let exported_rows = result.rows.len();
        match write_file_atomically(&path, &output) {
            Ok(()) => {
                // Truthful row count: a capped result exports the rows it holds, not the rows the
                // query matched, and the message has to say which one it is.
                self.feedback.runtime_message = if result.row_count > exported_rows as u64 {
                    t!(
                        "query.export_done_capped",
                        rows = exported_rows,
                        path = path_text.as_str(),
                        total = result.row_count
                    )
                    .into_owned()
                } else {
                    t!("query.export_done", rows = exported_rows, path = path_text.as_str()).into_owned()
                };
            }
            Err(error) => {
                self.feedback.runtime_message = t!("query.export_failed", error = error.to_string().as_str()).into_owned();
            }
        }
        self.overlay.export_overwrite_pending = None;
        self.overlay.export_open = false;
        self.overlay.export_snapshot = None;
        self.overlay.export_table_name.clear();
    }

    pub(super) fn save_query_document(&mut self) {
        self.save_query_document_at(self.query.session.active_document_index);
    }

    pub(crate) fn save_query_document_at(&mut self, document_index: usize) {
        if self
            .query
            .session
            .documents
            .get(document_index)
            .and_then(|document| document.file_path.as_ref())
            .is_some()
            && document_index == self.query.session.active_document_index
            && self.save_active_workspace_file()
        {
            return;
        }
        let Some(connection_id) = self
            .query
            .session
            .documents
            .get(document_index)
            .and_then(|document| document.connection_id.clone())
            .or_else(|| self.connection.lifecycle.active_connection_id().map(str::to_owned))
        else {
            self.feedback.runtime_message = "Create or select a connection first".to_owned();
            return;
        };
        let request_id = self.next_request_id();
        let Some(prepared) = self
            .query_save_context()
            .prepare_save(request_id, document_index, Some(connection_id))
        else {
            return;
        };
        let command = query_save_commands::save_query_command(&prepared);
        if self.dispatch_command(command) {
            self.query_save_context().commit_dispatched(request_id, document_index);
        }
    }

    fn query_save_context(&mut self) -> query_save_actions::QuerySaveContext<'_> {
        query_save_actions::QuerySaveContext::new(&mut self.query.session, &self.query.library, &mut self.feedback)
    }

    pub(crate) fn open_save_as_dialog(&mut self) {
        self.query.session.save_as_name = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map_or_else(|| "Saved query".to_owned(), |document| document.title.clone());
        self.query.session.save_as_name_error = false;
        self.query.session.save_as_open = true;
    }

    pub(super) fn draw_save_as_dialog(&mut self, ctx: &egui::Context) {
        if !self.query.session.save_as_open {
            return;
        }
        let action = query_save_dialog_surface_view::SaveAsDialogContext {
            theme: self.theme,
            name: &mut self.query.session.save_as_name,
            show_name_error: &mut self.query.session.save_as_name_error,
        }
        .draw(ctx);
        match action {
            Some(query_save_dialog_surface_view::SaveAsDialogAction::Cancel) => {
                self.query.session.save_as_name_error = false;
                self.query.session.save_as_open = false;
            }
            Some(query_save_dialog_surface_view::SaveAsDialogAction::Save) => {
                let name = self.query.session.save_as_name.trim().to_owned();
                if name.is_empty() {
                    self.query.session.save_as_name_error = true;
                    return;
                }
                let document_index = self.query.session.active_document_index;
                if let Some(document) = self.query.session.documents.get_mut(document_index) {
                    document.saved_query_id = None;
                    document.title = name;
                }
                self.query.session.save_as_name_error = false;
                self.query.session.save_as_open = false;
                self.save_query_document_at(document_index);
            }
            None => {}
        }
    }


    pub(super) fn draw_dirty_close_dialog(&mut self, ctx: &egui::Context) {
        let Some(document_index) = self.query.session.pending_dirty_close else {
            return;
        };
        let Some(title) = self
            .query
            .session
            .documents
            .get(document_index)
            .map(|document| document.title.clone())
        else {
            self.query.session.pending_dirty_close = None;
            return;
        };
        let action = query_save_dialog_surface_view::DirtyCloseDialogContext {
            theme: self.theme,
            title: &title,
        }
        .draw(ctx);
        match action {
            Some(query_save_dialog_surface_view::DirtyCloseDialogAction::Cancel) => {
                self.query.session.pending_dirty_close = None;
            }
            Some(query_save_dialog_surface_view::DirtyCloseDialogAction::Discard) => {
                self.query.session.pending_dirty_close = None;
                self.close_query_document(document_index);
            }
            Some(query_save_dialog_surface_view::DirtyCloseDialogAction::Save) => {
                self.query.session.pending_dirty_close = None;
                self.query.session.pending_close_after_save = Some(document_index);
                self.save_query_document_at(document_index);
            }
            None => {}
        }
    }
}
