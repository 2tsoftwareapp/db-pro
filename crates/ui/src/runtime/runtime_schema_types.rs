//! Schema and table-facing UI runtime models.

use super::{runtime_connection_types::UiSslMode, runtime_query_types::UiCell};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiSchemaSummary {
    pub schemas: Vec<String>,
    pub tables: Vec<String>,
    pub columns: Vec<String>,
    pub table_details: Vec<UiTableSummary>,
    pub views: Vec<UiViewSummary>,
    pub triggers: Vec<UiTriggerSummary>,
    pub functions: Vec<UiFunctionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableSummary {
    pub schema: String,
    pub name: String,
    pub row_count: Option<u64>,
    pub columns: Vec<UiSchemaColumn>,
    pub foreign_keys: Vec<UiSchemaForeignKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSchemaColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSchemaForeignKey {
    pub name: String,
    pub from_columns: Vec<String>,
    pub to_schema: String,
    pub to_table: String,
    pub to_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiViewSummary {
    pub schema: String,
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTriggerSummary {
    pub schema: String,
    pub name: String,
    pub table_name: String,
    pub timing: String,
    pub event: String,
    pub definition: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiRoutineParameter {
    pub name: String,
    pub data_type: String,
    pub mode: String,
    pub has_default: bool,
    pub default_expr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiFunctionSummary {
    pub schema: String,
    pub name: String,
    pub routine_type: String,
    pub data_type: String,
    pub definition: String,
    pub identity_arguments: String,
    pub language: String,
    pub volatility: String,
    pub security_definer: bool,
    pub parameters: Vec<UiRoutineParameter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableInfo {
    pub schema: String,
    pub name: String,
    pub row_count: Option<u64>,
    pub columns: Vec<UiTableColumn>,
    pub primary_key: Option<Vec<String>>,
    pub indexes: Vec<UiTableIndex>,
    pub foreign_keys: Vec<UiTableForeignKey>,
    pub check_constraints: Vec<UiCheckConstraint>,
    pub dependencies: Vec<UiTableDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiCheckConstraint {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiDependencyDirection {
    DependsOn,
    DependedBy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiDependencyKind {
    Table,
    View,
    ForeignKey,
    Trigger,
    Function,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableDependency {
    pub name: String,
    pub schema: String,
    pub kind: UiDependencyKind,
    pub direction: UiDependencyDirection,
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UiTableFilterOperator {
    Equals,
    NotEquals,
    #[default]
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableDataFilter {
    pub column: String,
    pub data_type: String,
    pub operator: UiTableFilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableDataSort {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiTableMutation {
    Update {
        columns: Vec<String>,
        data_types: Vec<String>,
        values: Vec<UiCell>,
        pk_columns: Vec<String>,
        pk_values: Vec<UiCell>,
    },
    Delete {
        pk_columns: Vec<String>,
        pk_values: Vec<UiCell>,
        /// Column types parallel to `pk_columns` (#587). Empty when the caller
        /// has no metadata; the translator then falls back to content sniffing.
        pk_data_types: Vec<String>,
    },
    Insert {
        columns: Vec<String>,
        /// Column types parallel to `columns` (#587).
        data_types: Vec<String>,
        values: Vec<UiCell>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiTableColumn {
    pub name: String,
    pub data_type: String,
    pub ordinal: usize,
    pub nullable: bool,
    pub default: Option<String>,
    pub is_primary_key: bool,
    pub is_unique: bool,
    pub is_identity: bool,
    pub is_generated: bool,
    pub collation: Option<String>,
    /// Enum labels for enum-typed columns; empty for all other types.
    pub enum_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub method: String,
    pub primary: bool,
    pub include_columns: Vec<String>,
    pub predicate: Option<String>,
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiTableForeignKey {
    pub name: String,
    pub from_columns: Vec<String>,
    pub to_schema: String,
    pub to_table: String,
    pub to_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
    pub match_option: String,
    pub deferrable: bool,
    pub initially_deferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiQueryFolderSummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSavedQuerySummary {
    pub id: String,
    pub name: String,
    pub sql: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiConnectionSummary {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub driver: String,
    /// The TLS mode stored with the connection. Present so the edit/duplicate
    /// prefill can restore it instead of falling back to a default.
    pub ssl_mode: UiSslMode,
    pub readonly: bool,
    /// SQLite: the saved "create the database file if it is missing" choice (#597).
    pub create_if_missing: bool,
    pub tags: Vec<String>,
    pub group: Option<String>,
    pub favorite: bool,
    /// Workspace safety classification — an enum, not a display label, so a
    /// localized or custom string can never silently disable the production
    /// guard (#387).
    pub environment: db_pro_core::domain::connection::ConnectionEnvironment,
}
