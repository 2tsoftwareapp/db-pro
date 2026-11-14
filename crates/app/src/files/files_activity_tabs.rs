use super::files_agent_context_view::{ActiveQueryContext, FilesAgentContextAction, FilesAgentContextView};
use super::files_git_view::{FilesGitAction, FilesGitContext};
use super::files_search_view::{FilesSearchAction, FilesSearchContext};
use super::files_tasks_view::{FilesTasksAction, FilesTasksContext};
use super::files_tree_view::{FilesTreeAction, FilesTreeContext};
use super::*;

#[path = "files_secondary_tabs_view.rs"]
mod files_secondary_tabs_view;

impl DbProApp {
    /// Quiet agent-context strip: icon actions instead of a wrapped button soup.
    pub(super) fn draw_files_agent_context_strip(&mut self, ui: &mut egui::Ui) {
        let active_document = self.query.session.active_document().map(|document| ActiveQueryContext {
            path: document.file_path.clone(),
            title: document.title.clone(),
        });
        let actions = FilesAgentContextView {
            theme: self.theme,
            context_items: &self.workspace.files.workspace_context_items,
            active_document: active_document.as_ref(),
            selected_text: &self.query.session.selected_text,
            selected_table: self.schema.explorer.selected_table.as_deref(),
        }
        .draw(ui);
        for action in actions {
            self.apply_files_agent_context_action(action);
        }
    }

    fn apply_files_agent_context_action(&mut self, action: FilesAgentContextAction) {
        match action {
            FilesAgentContextAction::Clear => self.workspace.files.clear_context_items(),
            FilesAgentContextAction::RefreshSchemaDrift => {
                let names = self
                    .schema
                    .explorer
                    .schema
                    .table_details
                    .iter()
                    .map(|table| format!("{}.{}", table.schema, table.name))
                    .collect::<Vec<_>>();
                self.workspace
                    .files
                    .refresh_schema_drift_watch(&names, &mut self.feedback);
            }
            FilesAgentContextAction::ExportSchemaSnapshot => self
                .workspace
                .files
                .export_live_schema_snapshot(&self.schema.explorer.schema, &mut self.feedback),
            FilesAgentContextAction::ToggleSplitEditor => self.toggle_split_editor(),
            FilesAgentContextAction::AddItem(item) => self.workspace.files.add_context_item(item),
            FilesAgentContextAction::RemoveItem(item) => {
                self.workspace
                    .files
                    .workspace_context_items
                    .retain(|existing| existing != &item);
            }
        }
    }

    pub(super) fn draw_files_tree_tab(&mut self, ui: &mut egui::Ui) {
        let active_file_path = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .and_then(|doc| doc.file_path.clone())
            .map(|path| path.to_owned());
        let tree = self.workspace.files.ide_workspace.tree().to_vec();
        let actions = FilesTreeContext {
            theme: self.theme,
            file_count: self.workspace.files.ide_workspace.index().len(),
            active_file_path: active_file_path.as_deref(),
            tree: &tree,
            expanded: &self.workspace.files.ide_workspace.expanded,
        }
        .draw(ui);
        for action in actions {
            self.apply_files_tree_action(action);
        }
    }

    fn apply_files_tree_action(&mut self, action: FilesTreeAction) {
        match action {
            FilesTreeAction::NewSql => {
                match self
                    .workspace
                    .files
                    .ide_workspace
                    .create_file("", "untitled.sql", "-- new query\nSELECT 1;\n")
                {
                    Ok(path) => {
                        let relative = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "untitled.sql".to_owned());
                        self.open_workspace_sql_file(relative);
                    }
                    Err(error) => self.feedback.runtime_message = error,
                }
            }
            FilesTreeAction::NewFolder => {
                if let Err(error) = self.workspace.files.ide_workspace.create_folder("", "new-folder") {
                    self.feedback.runtime_message = error;
                }
            }
            FilesTreeAction::ToggleDirectory(path) => {
                self.workspace.files.toggle_directory(path);
            }
            FilesTreeAction::CreateSql(path) => {
                let _ = self
                    .workspace
                    .files
                    .ide_workspace
                    .create_file(&path, "query.sql", "-- new query\nSELECT 1;\n");
            }
            FilesTreeAction::Delete(path) => {
                let _ = self.workspace.files.ide_workspace.delete_path(&path);
            }
            FilesTreeAction::OpenFile(path) => self.open_workspace_sql_file(path),
            FilesTreeAction::AddContext(path) => self.workspace.files.add_context_item(path),
            FilesTreeAction::FindReferences(stem) => {
                self.workspace.files.set_search_query(stem);
                self.workspace.files.select_panel_tab(FilesPanelTab::Search);
                self.workspace.files.run_search(&mut self.feedback);
            }
        }
    }

    pub(super) fn draw_files_search_tab(&mut self, ui: &mut egui::Ui) {
        let actions = {
            let mut context = FilesSearchContext {
                theme: self.theme,
                draft: self.workspace.files.search_draft(),
                replace_previews: &self.workspace.files.workspace_replace_previews,
                search_hits: &self.workspace.files.workspace_search_hits,
                pending_replace: self.workspace.files.pending_replace_all,
            };
            context.draw(ui)
        };
        for action in actions {
            match action {
                FilesSearchAction::UpdateDraft(draft) => self.workspace.files.apply_search_draft(draft),
                FilesSearchAction::Find => self.workspace.files.run_search(&mut self.feedback),
                FilesSearchAction::PreviewReplace => self.workspace.files.preview_replace(&mut self.feedback),
                FilesSearchAction::ReplaceAll => self.workspace.files.request_replace_all(&mut self.feedback),
                FilesSearchAction::ConfirmReplaceAll => self.workspace.files.confirm_replace_all(&mut self.feedback),
                FilesSearchAction::CancelReplaceAll => self.workspace.files.pending_replace_all = false,
                FilesSearchAction::Refactor => self.workspace.files.apply_refactor(&mut self.feedback),
                FilesSearchAction::OpenSql(path) => self.open_workspace_sql_file(path),
            }
        }
    }

    pub(super) fn draw_files_migrations_tab(&mut self, ui: &mut egui::Ui) {
        let migrations = self.workspace.files.ide_workspace.detect_migrations();
        for action in files_secondary_tabs_view::draw_migrations(ui, self.theme, &migrations) {
            let files_secondary_tabs_view::FilesSecondaryTabAction::OpenFile(path) = action;
            self.open_workspace_sql_file(path);
        }
    }

    pub(super) fn draw_files_tasks_tab(&mut self, ui: &mut egui::Ui) {
        let actions = {
            let mut context = FilesTasksContext {
                theme: self.theme,
                command: &mut self.workspace.files.workspace_task_command,
                last_task: self.workspace.files.ide_workspace.last_task.as_ref(),
                pending_shell: self.workspace.files.pending_shell,
                task_output: &self.workspace.files.task_output,
                task_running: self.workspace.files.task_in_flight(),
            };
            context.draw(ui)
        };
        for action in actions {
            match action {
                FilesTasksAction::RunTask => self.workspace.files.request_shell(&mut self.feedback),
                FilesTasksAction::CancelTask => self.workspace.files.cancel_task(&mut self.feedback),
                FilesTasksAction::ConfirmShell => self.workspace.files.confirm_shell(&mut self.feedback),
                FilesTasksAction::CancelShell => self.workspace.files.pending_shell = false,
                FilesTasksAction::RunBenchmark => {
                    let cases = vec![
                        ide_workspace::BenchmarkCase {
                            name: "select-1".to_owned(),
                            sql: "SELECT 1".to_owned(),
                        },
                        ide_workspace::BenchmarkCase {
                            name: "select-now".to_owned(),
                            sql: "SELECT CURRENT_TIMESTAMP".to_owned(),
                        },
                    ];
                    let results = ide_workspace::measure_local_benchmark(&cases, 5);
                    self.feedback.runtime_message = results
                        .into_iter()
                        .map(|result| format!("{}={}ms", result.name, result.avg_ms))
                        .collect::<Vec<_>>()
                        .join(", ");
                }
            }
        }
    }

    pub(super) fn draw_files_graph_tab(&mut self, ui: &mut egui::Ui) {
        // #578: cached per index epoch — no per-frame file reads.
        let edges = self.workspace.files.ide_workspace.dependency_edges().to_vec();
        for action in files_secondary_tabs_view::draw_graph(ui, self.theme, &edges) {
            let files_secondary_tabs_view::FilesSecondaryTabAction::OpenFile(path) = action;
            self.open_workspace_sql_file(path);
        }
    }

    pub(super) fn draw_files_git_tab(&mut self, ui: &mut egui::Ui) {
        // Borrowed doc views — the check is throttled internally (#405) and
        // cloning every buffer per frame churned on large SQL files.
        let documents: Vec<(&str, &str, bool)> = self
            .query
            .session
            .documents
            .iter()
            .filter_map(|doc| Some((doc.file_path.as_deref()?, doc.text(), doc.dirty)))
            .collect();
        self.workspace.files.check_external_file_changes(&documents);
        let git_busy = self.workspace.files.git_task_in_flight();
        let actions = {
            let mut context = FilesGitContext {
                theme: self.theme,
                status: self.workspace.files.git_status.as_ref(),
                last_error: self.workspace.files.git_last_error.as_deref(),
                external_changes: &self.workspace.files.workspace_external_changes,
                commit_message: &mut self.workspace.files.git_commit_message,
                diff: self.workspace.files.git_diff.as_ref(),
                busy: git_busy,
            };
            context.draw(ui)
        };
        for action in actions {
            match action {
                FilesGitAction::Refresh => self.workspace.files.refresh_git_status(&mut self.feedback),
                FilesGitAction::ReloadExternalFile(path) => self.reload_workspace_file_from_disk(&path),
                FilesGitAction::DismissExternalFile(path) => {
                    self.workspace.files.dismiss_external_change(&path);
                }
                FilesGitAction::CommitStaged => self.workspace.files.commit_git_staged(&mut self.feedback),
                FilesGitAction::Stage(path) => self.workspace.files.stage_git_path(&path, &mut self.feedback),
                FilesGitAction::Unstage(path) => self.workspace.files.unstage_git_path(&path, &mut self.feedback),
                FilesGitAction::Diff(path) => self.workspace.files.diff_git_path(&path, &mut self.feedback),
                FilesGitAction::Open(path) => self.open_workspace_sql_file(path),
            }
        }
    }
}
