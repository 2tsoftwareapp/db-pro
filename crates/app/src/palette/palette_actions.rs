// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::*;

impl DbProApp {
    pub(crate) fn execute_palette_action(&mut self, action: PaletteAction, ctx: &egui::Context) {
        self.palette.mode = None;
        match action {
            PaletteAction::Welcome => self.activate_welcome_tab(),
            PaletteAction::Query => self.workspace.active_tab = WorkspaceTab::Query,
            PaletteAction::History => self.open_palette_activity(Activity::History),
            PaletteAction::Data => self.open_palette_activity(Activity::Data),
            PaletteAction::Files => self.open_palette_activity(Activity::Files),
            PaletteAction::Diagram => self.open_diagram_tab(),
            PaletteAction::SchemaWorkbench => self.open_schema_workbench(),
            PaletteAction::SchemaCompare => {
                self.workspace.activity = Activity::Compare;
                self.workspace.active_tab = WorkspaceTab::SchemaCompare;
                self.workspace.sidebar_open = true;
            }
            PaletteAction::Transfers => self.open_palette_activity(Activity::Transfers),
            PaletteAction::Tasks => self.open_palette_activity(Activity::Tasks),
            PaletteAction::Monitor => self.open_palette_activity(Activity::Monitor),
            PaletteAction::Settings => self.open_palette_activity(Activity::Settings),
            PaletteAction::Agent => self.set_agent_open(true, ctx),
            PaletteAction::Problems => self.open_palette_activity(Activity::Problems),
            PaletteAction::Diagnostics => {
                self.open_palette_activity(Activity::Settings);
                self.feedback.runtime_message = "Opened Settings → Diagnostics".to_owned();
            }
            PaletteAction::NewQuery => {
                self.workspace.active_tab = WorkspaceTab::Query;
                self.new_query_document();
                self.feedback.runtime_message = "New query ready".to_owned();
            }
            PaletteAction::NewConnection => self.connection.open_new(),
            PaletteAction::RefreshSchema => self.refresh_schema_palette(),
            PaletteAction::ToggleExplorer => self.workspace.sidebar_open = !self.workspace.sidebar_open,
            PaletteAction::OpenTable(table) => self.open_table_from_palette(table),
            PaletteAction::OpenView(name) => {
                let schema = self.active_schema().to_owned();
                self.open_schema_object(SchemaObjectSelection::View(name.clone()), &schema, &name, "view");
            }
            PaletteAction::OpenFunction {
                name,
                identity_arguments,
            } => {
                let schema = self.active_schema().to_owned();
                self.open_schema_object(
                    SchemaObjectSelection::Function {
                        name: name.clone(),
                        identity_arguments,
                    },
                    &schema,
                    &name,
                    "function",
                );
            }
            PaletteAction::OpenWorkspaceFile(path) => self.open_workspace_sql_file(path),
            PaletteAction::OpenWorkspaceFolder => self.request_open_workspace_folder(),
            PaletteAction::CloseWorkspaceFolder => self
                .workspace
                .files
                .close(&mut self.workspace.shell, &mut self.feedback),
            PaletteAction::OpenSavedQuery(query_id) => self.open_saved_query_from_palette(query_id),
            PaletteAction::OpenHistoryEntry(entry_id) => {
                let entry = self
                    .query
                    .editor
                    .query_history_entries
                    .iter()
                    .find(|entry| entry.id == entry_id)
                    .cloned();
                if let Some(entry) = entry {
                    self.open_history_entry(&entry, false);
                } else {
                    self.feedback.runtime_message = "History entry is no longer available".to_owned();
                }
            }
            PaletteAction::InsertColumn(column) => {
                self.workspace.active_tab = WorkspaceTab::Query;
                self.append_to_active_query(&column);
                self.feedback.runtime_message = format!("Inserted column {column}");
            }
            PaletteAction::InsertSnippet(index) => {
                if let Some((_, _, snippet)) = query_snippets::builtin_sql_snippets().get(index) {
                    self.insert_snippet(snippet);
                }
            }
            PaletteAction::ExplainQuery => self.explain_query(),
            PaletteAction::ExportResults => self.export_results_from_palette(),
            PaletteAction::RunQuery => {
                self.workspace.active_tab = WorkspaceTab::Query;
                self.dispatch_query();
            }
            PaletteAction::FormatSql => {
                self.workspace.active_tab = WorkspaceTab::Query;
                let capabilities = self.query_capabilities();
                if let Some(request_id) = query_diagnostics_view::format_active_query(
                    &mut self.query,
                    capabilities,
                    &mut self.feedback,
                ) {
                    self.send_command_best_effort(UiCommand::CancelSqlPrediction { request_id });
                }
            }
            PaletteAction::SwitchConnection(connection_id) => self.switch_connection_from_palette(connection_id),
            PaletteAction::TogglePinTable(table) => self.toggle_pinned_table(table),
            PaletteAction::ComponentGallery => self.workspace.active_tab = WorkspaceTab::ComponentGallery,
        }
    }

    fn open_palette_activity(&mut self, activity: Activity) {
        self.workspace.activity = activity;
        self.workspace.sidebar_open = true;
    }
}
