//! Explorer connection/schema tree rendering.
use super::explorer_connection_row_view::ConnectionRowAction;
use super::explorer_schema_objects_view::ExplorerSchemaObjectsAction;
use super::explorer_schema_tree_view::ExplorerSchemaTreeAction;
use super::*;

/// Driver-aware URI suitable for "Copy Connection String" in the explorer.
fn connection_display_uri(connection: &UiConnectionSummary) -> String {
    let driver = connection.driver.to_ascii_lowercase();
    if driver.contains("sqlite") {
        return connection.database.clone();
    }
    let scheme = if driver.contains("mysql") {
        "mysql"
    } else if driver.contains("sqlserver") || driver.contains("mssql") {
        "sqlserver"
    } else {
        "postgresql"
    };
    format!(
        "{scheme}://{}@{}:{}/{}",
        connection.username, connection.host, connection.port, connection.database
    )
}

/// Sample CREATE TABLE for the connection driver. This is not a table designer.
fn sample_create_table_sql(driver: &str, database: &str) -> String {
    let driver_key = driver.to_ascii_lowercase();
    let (label, id_column, created_column) = if driver_key.contains("sqlite") {
        (
            "SQLite",
            "id INTEGER PRIMARY KEY",
            "created_at TEXT DEFAULT CURRENT_TIMESTAMP",
        )
    } else if driver_key.contains("mysql") {
        (
            "MySQL",
            "id INT PRIMARY KEY AUTO_INCREMENT",
            "created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP",
        )
    } else if driver_key.contains("sqlserver") || driver_key.contains("mssql") || driver_key.contains("sql server") {
        (
            "SQL Server",
            "id INT IDENTITY(1,1) PRIMARY KEY",
            "created_at DATETIME2 DEFAULT SYSUTCDATETIME()",
        )
    } else if driver_key.contains("postgres") {
        (
            "PostgreSQL",
            "id SERIAL PRIMARY KEY",
            "created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP",
        )
    } else {
        (driver, "id INTEGER PRIMARY KEY", "created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP")
    };
    format!(
        "-- Sample SQL for {label} on database `{database}`\nCREATE TABLE new_table (\n    {id_column},\n    {created_column}\n);\n"
    )
}

impl DbProApp {
    pub(super) fn apply_connection_row_action(
        &mut self,
        action: ConnectionRowAction,
        connection: &UiConnectionSummary,
        is_connected: bool,
        ui: &mut egui::Ui,
    ) {
        match action {
            ConnectionRowAction::Connect => self.connect_to_connection(connection),
            ConnectionRowAction::Disconnect => self.disconnect_from_connection(connection),
            ConnectionRowAction::Reconnect => {
                self.disconnect_from_connection(connection);
                self.connect_to_connection(connection);
            }
            ConnectionRowAction::RefreshSchema => self.request_schema_introspection(connection.id.clone(), true),
            ConnectionRowAction::NewScript => {
                self.open_query_on_connection(&connection.id);
                self.workspace.active_tab = WorkspaceTab::Query;
            }
            ConnectionRowAction::OpenErDiagram => {
                self.open_diagram_tab();
                if !is_connected {
                    self.connect_to_connection(connection);
                }
            }
            ConnectionRowAction::AskAgent => self.open_agent_prompt(
                format!(
                    "Analyze the database `{}` on connection `{}` ({}) and describe the schema architecture.",
                    connection.database, connection.name, connection.driver
                ),
                ui.ctx(),
            ),
            ConnectionRowAction::CreateTable => {
                self.open_query_document_with(
                    sample_create_table_sql(&connection.driver, &connection.database),
                    Some(connection.id.clone()),
                );
            }
            ConnectionRowAction::CopyName => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(connection.name.clone())));
                self.feedback.runtime_message = t!("explorer.copy_to_clipboard", name = connection.name).to_string();
            }
            ConnectionRowAction::CopyConnectionString => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(connection_display_uri(connection))));
                self.feedback.runtime_message = t!("explorer.copy_connection_string").to_string();
            }
            ConnectionRowAction::Edit => self.open_edit_connection(connection),
            ConnectionRowAction::Duplicate => self.open_duplicate_connection(connection),
            ConnectionRowAction::Delete => self.overlay.connection_delete_confirmation_id = Some(connection.id.clone()),
        }
    }

    pub(super) fn open_query_on_connection(&mut self, connection_id: &str) {
        self.new_query_document();
        let index = self.query.session.active_document_index;
        self.set_document_connection(index, Some(connection_id.to_owned()));
    }

    pub(super) fn apply_schema_tree_action(
        &mut self,
        action: ExplorerSchemaTreeAction,
        connection_id: &str,
        ui: &mut egui::Ui,
    ) {
        match action {
            ExplorerSchemaTreeAction::RefreshSchema { schema } => {
                self.refresh_explorer_node(connection_id, schema.as_deref(), None);
            }
            ExplorerSchemaTreeAction::ActivateSchema(schema) => {
                super::schema_explorer_state::SchemaActivationContext::new(
                    &mut self.schema.explorer,
                    &mut self.table,
                    &mut self.workspace,
                    &mut self.feedback,
                )
                .activate(&schema);
            }
            ExplorerSchemaTreeAction::OpenErDiagram => {
                self.open_diagram_tab();
            }
            ExplorerSchemaTreeAction::CopySchemaName(schema) => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(schema.clone())));
                self.feedback.runtime_message = t!("explorer.copy_to_clipboard", name = schema).to_string();
            }
            ExplorerSchemaTreeAction::DropSchema(schema) => self.plan_drop_schema(&schema),
            ExplorerSchemaTreeAction::SchemaObjects(action) => {
                self.apply_schema_objects_action(action, ui);
            }
        }
    }

    fn apply_schema_objects_action(&mut self, action: ExplorerSchemaObjectsAction, ui: &mut egui::Ui) {
        match action {
            ExplorerSchemaObjectsAction::SelectTable(table) => self.open_table(table),
            ExplorerSchemaObjectsAction::TableRow { table, action } => {
                let schema = self.active_schema().to_owned();
                self.apply_table_row_action(action, &table, &schema, ui);
            }
            ExplorerSchemaObjectsAction::SchemaObject(action) => {
                self.apply_schema_object_folder_action(action, ui);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/explorer_connections_tests.rs"]
mod tests;

