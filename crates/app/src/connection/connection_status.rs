//! Active connection, schema names, and statusbar helpers.
use super::*;
use egui::Color32;
use lucide_icons::Icon;

pub(super) fn active_connection<'a>(
    catalog: &'a ConnectionCatalogState,
    lifecycle: &ConnectionLifecycleState,
) -> Option<&'a UiConnectionSummary> {
    catalog
        .iter()
        .find(|connection| Some(connection.id.as_str()) == lifecycle.active_connection_id())
}

pub(super) fn active_connection_name<'a>(
    catalog: &'a ConnectionCatalogState,
    lifecycle: &'a ConnectionLifecycleState,
) -> &'a str {
    active_connection(catalog, lifecycle)
        .map(|connection| connection.name.as_str())
        .unwrap_or(lifecycle.fallback_name())
}

pub(super) fn active_driver<'a>(
    catalog: &'a ConnectionCatalogState,
    lifecycle: &'a ConnectionLifecycleState,
) -> &'a str {
    active_connection(catalog, lifecycle)
        .map(|connection| connection.driver.as_str())
        .unwrap_or("PostgreSQL")
}

pub(crate) fn active_capabilities(
    catalog: &ConnectionCatalogState,
    lifecycle: &ConnectionLifecycleState,
) -> CapabilityLookup {
    match active_connection(catalog, lifecycle) {
        Some(connection) => CapabilityLookup::for_driver_label(&connection.driver),
        None => CapabilityLookup::NoActiveConnection,
    }
}

pub(super) fn active_schema<'a>(
    schema_explorer: &'a SchemaExplorerState,
    catalog: &'a ConnectionCatalogState,
    lifecycle: &'a ConnectionLifecycleState,
) -> &'a str {
    schema_explorer
        .selected_schema
        .as_deref()
        .or_else(|| schema_explorer.schema.schemas.first().map(String::as_str))
        .unwrap_or_else(|| {
            if active_driver(catalog, lifecycle).eq_ignore_ascii_case("sqlite") {
                "main"
            } else {
                "public"
            }
        })
}

pub(super) fn active_schema_table_names(
    schema_explorer: &SchemaExplorerState,
    catalog: &ConnectionCatalogState,
    lifecycle: &ConnectionLifecycleState,
) -> Vec<String> {
    schema_table_names(schema_explorer, active_schema(schema_explorer, catalog, lifecycle))
}

/// Table names belonging to `schema`. Empty `schema` (SQLite flat tree) returns all tables.
pub(super) fn schema_table_names(schema_explorer: &SchemaExplorerState, schema: &str) -> Vec<String> {
    if schema_explorer.schema.schemas.is_empty() || schema_explorer.schema.table_details.is_empty() {
        return schema_explorer.schema.tables.clone();
    }
    if schema.is_empty() {
        return schema_explorer
            .schema
            .table_details
            .iter()
            .map(|table| table.name.clone())
            .collect();
    }
    schema_explorer
        .schema
        .table_details
        .iter()
        .filter(|table| table.schema == schema)
        .map(|table| table.name.clone())
        .collect()
}

pub(super) fn schema_table_count(schema_explorer: &SchemaExplorerState, schema: &str) -> usize {
    if schema_explorer.schema.schemas.is_empty() || schema_explorer.schema.table_details.is_empty() {
        return schema_explorer.schema.tables.len();
    }
    if schema.is_empty() {
        return schema_explorer.schema.table_details.len();
    }
    schema_explorer
        .schema
        .table_details
        .iter()
        .filter(|table| table.schema == schema)
        .count()
}

/// Count matching table names without allocating a name list.
pub(super) fn schema_matching_table_count(schema_explorer: &SchemaExplorerState, schema: &str, query: &str) -> usize {
    let mode = schema_explorer.explorer_filter.mode;
    if query.is_empty() {
        return schema_table_count(schema_explorer, schema);
    }
    if schema_explorer.schema.schemas.is_empty() || schema_explorer.schema.table_details.is_empty() {
        return schema_explorer
            .schema
            .tables
            .iter()
            .filter(|table| matches_explorer_table_with_mode(table, query, mode))
            .count();
    }
    schema_explorer
        .schema
        .table_details
        .iter()
        .filter(|table| schema.is_empty() || table.schema == schema)
        .filter(|table| matches_explorer_table_with_mode(&table.name, query, mode))
        .count()
}

pub(super) fn active_schema_column_names(
    schema_explorer: &SchemaExplorerState,
    catalog: &ConnectionCatalogState,
    lifecycle: &ConnectionLifecycleState,
) -> Vec<String> {
    if schema_explorer.schema.schemas.is_empty() || schema_explorer.schema.table_details.is_empty() {
        return schema_explorer.schema.columns.clone();
    }
    let schema = active_schema(schema_explorer, catalog, lifecycle);
    schema_explorer
        .schema
        .table_details
        .iter()
        .filter(|table| table.schema == schema)
        .flat_map(|table| table.columns.iter().map(|column| column.name.clone()))
        .collect()
}

pub(super) fn has_runtime_error(feedback: &FeedbackState) -> bool {
    feedback.runtime_message.contains("failed")
        || feedback.runtime_message.contains("Failed")
        || feedback.runtime_message.contains("error")
        || feedback.runtime_message.contains("Error")
}

/// The status-bar message and the colour it is rendered in.
pub(super) fn runtime_status(
    feedback: &FeedbackState,
    theme: DbProTheme,
    current_activity: super::app_types::Activity,
) -> Option<(String, Color32)> {
    let message = feedback.runtime_message_for_display(current_activity);
    if message.trim().is_empty() {
        None
    } else if has_runtime_error(feedback) {
        Some((message.to_owned(), theme.danger))
    } else {
        Some((message.to_owned(), theme.text_secondary))
    }
}

pub(super) fn statusbar_state(
    shown_connected: bool,
    feedback: &FeedbackState,
    theme: DbProTheme,
) -> (Icon, Color32, &'static str) {
    if shown_connected {
        return (Icon::CircleCheck, theme.success, "Connected");
    }
    if feedback.runtime_message.starts_with("Connecting") {
        return (Icon::Circle, theme.accent, "Connecting…");
    }
    if has_runtime_error(feedback) {
        return (Icon::TriangleAlert, theme.danger, "Runtime error");
    }
    (Icon::Circle, theme.warning, "Not connected")
}

pub(super) fn shows_editor_status() -> bool {
    // Query status strip owns Ln/Col — keep the shell statusbar free of duplicate chrome.
    false
}

pub(super) fn statusbar_context_label(workspace: &WorkspaceShellState, table_state: &TableState) -> &'static str {
    match workspace.active_tab {
        WorkspaceTab::Welcome => "Workspace",
        WorkspaceTab::Query => "SQL Editor",
        WorkspaceTab::Table => match table_state.table_view {
            TableView::Structure => "Table Structure",
            TableView::Data => "Data Editor",
            TableView::Profile => "Column Profile",
            TableView::Indexes => "Table Indexes",
            TableView::Relations => "Table Relations",
            TableView::Constraints => "Table Constraints",
            TableView::Dependencies => "Table Dependencies",
            TableView::Ddl => "Table DDL",
        },
        WorkspaceTab::SchemaObject => "Schema Object",
        WorkspaceTab::Results => "Query Results",
        WorkspaceTab::Diagram => "ER Diagram",
        WorkspaceTab::SchemaWorkbench => "Schema Workbench",
        WorkspaceTab::SchemaCompare => "Schema Compare",
        WorkspaceTab::ComponentGallery => "Component Gallery",
    }
}

pub(super) fn connection_indicator(
    lifecycle: &ConnectionLifecycleState,
    connection: &UiConnectionSummary,
    theme: DbProTheme,
) -> (Icon, Color32) {
    let is_active = lifecycle.active_connection_id() == Some(connection.id.as_str());
    // A session behind a bound query tab keeps working after the connection loses
    // the global active slot — it is not "disconnected" (#603).
    let is_connected = (is_active && lifecycle.is_connected()) || lifecycle.has_live_session(&connection.id);
    let is_failed = lifecycle.has_failed_connection(&connection.id);
    let icon = if is_connected {
        Icon::CircleCheck
    } else if is_failed {
        Icon::AlertCircle
    } else {
        Icon::Circle
    };
    let color = if is_connected && connection.readonly {
        theme.warning
    } else if is_connected {
        theme.success
    } else if is_failed {
        theme.danger
    } else if is_active {
        theme.accent
    } else {
        theme.text_muted
    };
    (icon, color)
}
