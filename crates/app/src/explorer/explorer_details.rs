//! Table rows and the nested detail folders (Columns / Foreign keys / Indexes)
//! of the Codex / DBeaver navigator tree.

use super::explorer_table_row_view::TableRowAction;
use super::schema_workbench::SchemaWorkbenchMode;
use super::*;
use db_pro_core::domain::object_mutation::ObjectAction;

fn build_insert_query(schema: &str, table: &str, info: Option<&UiTableInfo>) -> String {
    let columns = info
        .map(|table_info| {
            table_info
                .columns
                .iter()
                .map(|column| result_grid_export::quote_sql_identifier(&column.name))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|columns| !columns.is_empty())
        .unwrap_or_else(|| "column1, column2".to_owned());
    let values = info
        .map(|table_info| {
            table_info
                .columns
                .iter()
                .map(|_| "DEFAULT")
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| "'value1', 'value2'".to_owned());
    let target = result_grid_export::qualified_sql_name(schema, table);
    format!("INSERT INTO {target} ({columns})\nVALUES ({values});")
}

fn build_update_query(schema: &str, table: &str, info: Option<&UiTableInfo>) -> String {
    let set_clause = info
        .map(|table_info| {
            table_info
                .columns
                .iter()
                .filter(|column| !column.is_primary_key)
                .map(|column| format!("    {} = DEFAULT", result_grid_export::quote_sql_identifier(&column.name)))
                .collect::<Vec<_>>()
                .join(",\n")
        })
        .filter(|clause| !clause.is_empty())
        .unwrap_or_else(|| "    column_name = DEFAULT".to_owned());
    // No PK → no fabricated WHERE: a template the user can run must never
    // silently target `id = 1` (#398).
    let where_clause = pk_where_clause(info).unwrap_or_else(|| "/* TODO: add WHERE conditions */ 1 = 0".to_owned());
    let target = result_grid_export::qualified_sql_name(schema, table);
    format!("UPDATE {target}\nSET\n{set_clause}\nWHERE {where_clause};")
}

/// WHERE clause over the table's primary-key columns (#398). Placeholder
/// values respect the column type: text/uuid/date columns get a quoted literal
/// the user must replace, numeric types get `1`.
fn pk_where_clause(info: Option<&UiTableInfo>) -> Option<String> {
    let clauses: Vec<String> = info?
        .columns
        .iter()
        .filter(|column| column.is_primary_key)
        .map(|column| {
            let quoted = result_grid_export::quote_sql_identifier(&column.name);
            let numeric = {
                let ty = column.data_type.to_ascii_lowercase();
                ty.contains("int") || ty.contains("numeric") || ty.contains("decimal") || ty.contains("float") || ty.contains("real") || ty.contains("serial") || ty.contains("money")
            };
            if numeric { format!("{quoted} = 1") } else { format!("{quoted} = '<value>'") }
        })
        .collect();
    (!clauses.is_empty()).then(|| clauses.join(" AND "))
}

fn build_delete_query(schema: &str, table: &str, info: Option<&UiTableInfo>) -> String {
    let where_clause = pk_where_clause(info).unwrap_or_else(|| "/* TODO: add WHERE conditions */ 1 = 0".to_owned());
    let target = result_grid_export::qualified_sql_name(schema, table);
    format!("DELETE FROM {target}\nWHERE {where_clause};")
}

impl DbProApp {
    pub(super) fn apply_table_row_action(
        &mut self,
        action: TableRowAction,
        table: &str,
        schema: &str,
        ui: &mut egui::Ui,
    ) {
        match action {
            TableRowAction::OpenData => self.open_table_view(TableView::Data),
            TableRowAction::OpenStructure => self.open_table_view(TableView::Structure),
            TableRowAction::OpenModifyTable => {
                self.schema.workbench.schema = schema.to_owned();
                self.schema.workbench.name = table.to_owned();
                self.schema.workbench.parent_table = table.to_owned();
                self.schema.workbench.mode = SchemaWorkbenchMode::Table;
                if let Some(details) = self
                    .schema
                    .explorer
                    .schema
                    .table_details
                    .iter()
                    .find(|t| t.schema == schema && t.name == table)
                    .cloned()
                {
                    self.schema.workbench.load_from_table_details(&details);
                }
                self.workspace.activity = Activity::Schema;
                self.workspace.active_tab = WorkspaceTab::SchemaWorkbench;
                self.workspace.sidebar_open = true;
            }
            TableRowAction::DropTable => {
                self.schema.workbench.schema = schema.to_owned();
                self.schema.workbench.name = table.to_owned();
                self.schema.workbench.parent_table = table.to_owned();
                self.schema.workbench.mode = SchemaWorkbenchMode::Table;
                self.plan_workbench_action(ObjectAction::Drop);
                self.schema.workbench.apply_confirmation = true;
                self.workspace.activity = Activity::Schema;
                self.workspace.active_tab = WorkspaceTab::SchemaWorkbench;
                self.workspace.sidebar_open = true;
            }
            TableRowAction::OpenDdl => self.open_table_view(TableView::Ddl),
            TableRowAction::OpenQuery => {
                let from = result_grid_export::qualified_sql_name(schema, table);
                // Dialect-correct row limit: SQL Server rejects `LIMIT` (#260).
                let query = result_grid_export::select_top_snippet(self.active_driver(), &from, 100);
                self.open_query_document(query)
            }
            TableRowAction::GenerateInsert => {
                let query = build_insert_query(schema, table, self.table.state.table_info.as_ref());
                self.open_query_document(query);
            }
            TableRowAction::GenerateUpdate => {
                let query = build_update_query(schema, table, self.table.state.table_info.as_ref());
                self.open_query_document(query);
            }
            TableRowAction::GenerateDelete => {
                let query = build_delete_query(schema, table, self.table.state.table_info.as_ref());
                self.open_query_document(query);
            }
            TableRowAction::CopyQualifiedName => {
                let qualified_name = result_grid_export::qualified_sql_name(schema, table);
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(qualified_name.clone())));
                self.feedback.runtime_message = t!("explorer.copy_to_clipboard", name = qualified_name).to_string();
            }
            TableRowAction::CopyName => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(table.to_owned())));
                self.feedback.runtime_message = t!("explorer.copy_to_clipboard", name = table).to_string();
            }
            TableRowAction::AskAgent => self.open_agent_prompt(
                format!("Explain the `{schema}.{table}` table structure and suggest useful queries"),
                ui.ctx(),
            ),
            TableRowAction::RefreshSchema => {
                if let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) {
                    self.refresh_explorer_node(&connection_id, Some(schema), Some(table));
                }
            }
        }
    }

    fn open_table_view(&mut self, view: TableView) {
        self.table.state.table_view = view;
        self.workspace.active_tab = WorkspaceTab::Table;
    }

    fn open_query_document(&mut self, query: String) {
        // New document per action (#420): never overwrite the active buffer.
        self.open_query_document_with(query, None);
    }
}
