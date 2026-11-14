//! Shared UI enums and small value types for the native shell.
use crate::UiTableSummary;
use lucide_icons::Icon;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingNavigationAction {
    /// `schema` is `None` when the caller means the currently selected schema.
    OpenTable { schema: Option<String>, table: String },
    ChangeSchema(String),
    ChangeConnection(String),
    /// Routed through the staged-changes confirmation: disconnecting wipes
    /// `staged_changes`, so it must pass `discard_changes_confirmation` first.
    Disconnect(String),
    /// "Open referenced row" navigates to another table — same staged-changes
    /// hazard as `OpenTable`, so it goes through the confirmation too.
    OpenReferencedRow {
        connection_id: String,
        schema: String,
        table: String,
        filters: Vec<crate::UiTableDataFilter>,
    },
    CloseWorkspace(WorkspaceTab),
}

/// Which tab set a pending bulk close targets; stored while the dirty-close
/// confirmation is open so `Discard` can replay the exact close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingBulkClose {
    Others(usize),
    ToRight(usize),
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedGridColumnLayout {
    pub(crate) column_name: String,
    pub(crate) width: f32,
    pub(crate) order: usize,
    pub(crate) hidden: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PersistedGridLayout {
    /// Stable layout entries. The legacy fields below are read only for
    /// migration and are intentionally not written after the next save.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) columns: Vec<PersistedGridColumnLayout>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) widths: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) order: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hidden_columns: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activity {
    Explorer,
    Queries,
    Tools,
    Files,
    Data,
    History,
    Transfers,
    Monitor,
    Security,
    Settings,
    Diagram,
    Schema,
    Compare,
    Problems,
    Tasks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FilesPanelTab {
    #[default]
    Tree,
    Search,
    Migrations,
    Tasks,
    Graph,
    Git,
}

/// Cap for MRU recent-table entries persisted for Data Activity (#212).
pub(crate) const RECENT_TABLES_MAX: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ProblemsSeverityFilter {
    #[default]
    All,
    Errors,
    Warnings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HistoryOutcomeFilter {
    #[default]
    All,
    Success,
    Failed,
    Cancelled,
}

impl HistoryOutcomeFilter {
    pub(crate) const ALL: [Self; 4] = [Self::All, Self::Success, Self::Failed, Self::Cancelled];

    /// Locale key for the filter's display label; views render it via `t!()`.
    /// Domain types never own display strings — this is a key, not a value.
    pub(crate) fn locale_key(self) -> &'static str {
        match self {
            Self::All => "sidebar.filter_all",
            Self::Success => "sidebar.filter_success",
            Self::Failed => "sidebar.filter_failed",
            Self::Cancelled => "sidebar.filter_cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ProblemsSourceFilter {
    #[default]
    All,
    Parser,
    Lint,
    Delimiter,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProblemEntry {
    pub(crate) document_index: usize,
    /// Buffer version captured when the entry was collected (#425): a fix that
    /// applies a byte range captured at diagnostic time must refuse to run
    /// against a buffer that changed since.
    pub(crate) document_version: u64,
    pub(crate) document_id: String,
    pub(crate) document_title: String,
    pub(crate) diagnostic_index: usize,
    pub(crate) severity: crate::editor::DiagnosticSeverity,
    pub(crate) source: crate::editor::DiagnosticSource,
    pub(crate) message: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) range: (usize, usize),
    pub(crate) has_fix: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteMode {
    QuickOpen,
    Commands,
}

/// Scope chips for global search / command palette (#201).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SearchScope {
    #[default]
    All,
    Connections,
    Schema,
    Queries,
    Commands,
    Agent,
}

impl SearchScope {
    pub(crate) fn all() -> &'static [SearchScope] {
        &[
            SearchScope::All,
            SearchScope::Connections,
            SearchScope::Schema,
            SearchScope::Queries,
            SearchScope::Commands,
            SearchScope::Agent,
        ]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            SearchScope::All => "All",
            SearchScope::Connections => "Connections",
            SearchScope::Schema => "Schema",
            SearchScope::Queries => "Queries",
            SearchScope::Commands => "Commands",
            SearchScope::Agent => "Agent",
        }
    }
}

/// Kind tags used by SearchService ranking (#201).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchKind {
    Connection,
    Table,
    View,
    Column,
    Function,
    SavedQuery,
    History,
    Command,
    Agent,
    WorkspaceFile,
    Navigation,
}

impl SearchKind {
    pub(crate) fn matches_scope(self, scope: SearchScope) -> bool {
        match scope {
            SearchScope::All => true,
            SearchScope::Connections => matches!(self, SearchKind::Connection),
            SearchScope::Schema => matches!(
                self,
                SearchKind::Table | SearchKind::View | SearchKind::Column | SearchKind::Function
            ),
            SearchScope::Queries => matches!(self, SearchKind::SavedQuery | SearchKind::History),
            SearchScope::Commands => matches!(self, SearchKind::Command | SearchKind::Navigation),
            SearchScope::Agent => matches!(self, SearchKind::Agent),
        }
    }

    /// Small boost so tables outrank history noise on equal match class.
    pub(crate) fn rank_boost(self) -> u32 {
        match self {
            SearchKind::Command | SearchKind::Navigation => 80,
            SearchKind::Table | SearchKind::View | SearchKind::Function => 70,
            SearchKind::Column | SearchKind::Connection => 60,
            SearchKind::SavedQuery | SearchKind::Agent => 50,
            SearchKind::WorkspaceFile => 40,
            SearchKind::History => 30,
        }
    }

    /// Exact-title-match priority: concrete objects the user literally named
    /// (tables, views, connections, queries) beat navigation commands that
    /// merely share the same label ("Files" the command vs the `files` table).
    pub(crate) fn exact_match_boost(self) -> u32 {
        match self {
            SearchKind::Table | SearchKind::View | SearchKind::Function => 100,
            SearchKind::Column | SearchKind::Connection => 95,
            SearchKind::SavedQuery => 90,
            SearchKind::WorkspaceFile => 85,
            SearchKind::Agent => 80,
            SearchKind::Command | SearchKind::Navigation => 70,
            SearchKind::History => 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PaletteAction {
    Welcome,
    Query,
    History,
    Data,
    Files,
    Settings,
    Diagram,
    Agent,
    Problems,
    Diagnostics,
    SchemaWorkbench,
    SchemaCompare,
    Transfers,
    Tasks,
    Monitor,
    NewQuery,
    NewConnection,
    RefreshSchema,
    ToggleExplorer,
    OpenTable(String),
    OpenView(String),
    OpenFunction { name: String, identity_arguments: String },
    OpenSavedQuery(String),
    OpenHistoryEntry(String),
    OpenWorkspaceFile(String),
    OpenWorkspaceFolder,
    CloseWorkspaceFolder,
    InsertColumn(String),
    InsertSnippet(usize),
    ExplainQuery,
    ExportResults,
    RunQuery,
    FormatSql,
    SwitchConnection(String),
    TogglePinTable(String),
    ComponentGallery,
}

#[derive(Debug, Clone)]
pub(crate) struct PaletteItem {
    pub(crate) icon: Icon,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) shortcut: Option<String>,
    pub(crate) action: PaletteAction,
}

pub(crate) const AGENT_SIDEBAR_COLLAPSE_WIDTH: f32 = 1180.0;
pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 220.0;
pub(crate) const SIDEBAR_MAX_WIDTH: f32 = 380.0;
pub(crate) const AGENT_MIN_WIDTH: f32 = 300.0;
pub(crate) const AGENT_MAX_WIDTH: f32 = 480.0;

fn agent_panel_rect_key() -> egui::Id {
    egui::Id::new("agent_panel_screen_rect")
}

/// Records the agent side panel's screen rect for this pass. Panels share the
/// central `Order::Middle` layer with the grid, so `layer_id_at` cannot tell a
/// grid row from the panel covering it — the stored rect is the occlusion test
/// row hover uses instead (#338).
pub(crate) fn store_agent_panel_rect(ctx: &egui::Context, rect: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(agent_panel_rect_key(), rect));
}

/// Whether `pos` lands inside the agent panel painted this or last pass.
pub(crate) fn agent_panel_contains_pointer(ctx: &egui::Context, pos: egui::Pos2) -> bool {
    ctx.data(|d| d.get_temp::<egui::Rect>(agent_panel_rect_key()))
        .is_some_and(|rect| rect.contains(pos))
}
/// Absolute floor for the output dock — four result rows plus the tab strip.
pub(crate) const OUTPUT_MIN_HEIGHT: f32 = 120.0;
/// Upper bound when the window is tall enough; see [`output_dock_max_height`].
pub(crate) const OUTPUT_MAX_HEIGHT: f32 = 420.0;
/// Share of the pane the dock may claim when the user drags it open (#576).
pub(crate) const OUTPUT_MAX_FRACTION: f32 = 0.72;
/// Editor floor the dock yields to on a short window (#576).
pub(crate) const EDITOR_MIN_HEIGHT: f32 = 120.0;
/// Hard floor for the dock, below which it stops yielding to the editor.
pub(crate) const OUTPUT_ABSOLUTE_MIN_HEIGHT: f32 = 48.0;

/// Height ceiling for the output dock given the space actually available.
///
/// #576: the limit was a flat 420 px, so a 4K window gave the same ~13 result
/// rows as a 768 px laptop, while a short window could be overdrawn because
/// `min_height` (120) + editor + status exceeded the available height. The floor
/// now yields to the pane: `min` is never larger than the space left for the
/// editor.
pub(crate) fn output_dock_max_height(available_height: f32) -> f32 {
    let proportional = (available_height * OUTPUT_MAX_FRACTION).max(OUTPUT_MIN_HEIGHT);
    proportional.max(OUTPUT_MIN_HEIGHT)
}

/// Floor for the output dock given the space actually available.
///
/// On a very short pane the dock shrinks below [`OUTPUT_MIN_HEIGHT`] rather than
/// pushing the editor past the bottom edge.
pub(crate) fn output_dock_min_height(available_height: f32) -> f32 {
    (available_height - EDITOR_MIN_HEIGHT).clamp(OUTPUT_ABSOLUTE_MIN_HEIGHT, OUTPUT_MIN_HEIGHT)
}
pub(crate) const TABLE_PAGE_SIZE: u64 = 100;
pub(crate) const GRID_ROW_NUMBER_WIDTH: f32 = 48.0;
/// Height of the header row (one grid row) — the body starts below it.
pub(crate) const GRID_HEADER_HEIGHT: f32 = 28.0;
/// Height of one body row; virtualization and hit-testing share it.
pub(crate) const GRID_ROW_HEIGHT: f32 = 28.0;
pub use crate::diagram::{ErGraph, ErSpatialIndex};

/// Safety valve for pathological schemas, not a product limit.
///
/// #572: the explorer stopped at 100 tables per schema and told the user to
/// "refine filter", so a 300-table ERP database was unreachable except by
/// typing a name. The list is culled per viewport while painting, so the cap
/// only needs to bound the `Vec<String>` built per keystroke.
pub(crate) const EXPLORER_MAX_TABLES: usize = 20_000;
/// Row height used by the Codex navigator for viewport culling.
pub(crate) const EXPLORER_ROW_HEIGHT: f32 = 26.0;

/// True when a row of `height` starting at `row_top` cannot intersect `clip`.
///
/// Shared by the Tables folder and the Views/Functions/Triggers folders so
/// viewport culling uses one culling rule (#583).
pub(crate) fn explorer_row_is_offscreen(row_top: f32, height: f32, clip: egui::Rect) -> bool {
    row_top + height < clip.top() || row_top > clip.bottom()
}

/// Schemas the explorer should show — and the destructive DROP SCHEMA guard's
/// system-schema boundary (#348/#402). `driver` is the provider label
/// (`"PostgreSQL"`, `"MySQL"`, `"SQL Server"`, `"SQLite"`); unknown drivers get
/// the conservative union so a catalog name never passes as "user schema".
pub(crate) fn is_user_visible_schema(driver: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let postgres_system = lower == "pg_catalog"
        || lower == "information_schema"
        || lower == "pg_toast"
        || lower.starts_with("pg_temp")
        || lower.starts_with("pg_toast_temp");
    let mysql_system = lower == "mysql"
        || lower == "performance_schema"
        || lower == "sys"
        || lower == "information_schema";
    // `db_*` schemas are fixed SQL Server roles, not user schema space.
    let sqlserver_system = lower == "sys"
        || lower == "information_schema"
        || lower == "guest"
        || lower.starts_with("db_");
    match driver.to_ascii_lowercase().as_str() {
        d if d.contains("mysql") => !mysql_system,
        d if d.contains("sql server") || d.contains("sqlserver") || d.contains("mssql") => {
            !sqlserver_system
        }
        // SQLite has no schema drops; `main`/`temp` are the user's data space.
        d if d.contains("sqlite") => true,
        d if d.contains("postgres") || d.is_empty() => !postgres_system,
        _ => !(postgres_system || mysql_system || sqlserver_system),
    }
}

/// Cached explorer table listing for one `(connection, schema, search)` key.
/// Rebuilt only when the key changes — avoids reallocating 1k+ name strings every frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExplorerNavCache {
    pub connection_id: String,
    pub schema: String,
    pub search: String,
    pub mode: super::schema_explorer_state::ExplorerMatchMode,
    pub total_count: usize,
    pub matching_count: usize,
    pub visible: Vec<String>,
}

/// ER search predicate for one table (#544). `query` must already be
/// trimmed + ASCII-lowercased (the draw path normalizes once per frame).
/// Matching is byte-wise ASCII case folding with zero allocations:
/// `to_ascii_lowercase().contains(query)` allocated a fresh `String` for the
/// table name, the schema, and every column on each call — 39k allocations per
/// redraw on a 1 300 × 30 catalog, re-running every repaint while search was
/// open. Semantics are unchanged, including for non-ASCII bytes
/// (`to_ascii_lowercase` folds only ASCII, exactly like the windowed compare).
pub(crate) fn matches_diagram_search(table: &UiTableSummary, query: &str) -> bool {
    ascii_contains_ignore_case(&table.name, query)
        || ascii_contains_ignore_case(&table.schema, query)
        || table
            .columns
            .iter()
            .any(|column| ascii_contains_ignore_case(&column.name, query))
}

pub(crate) fn ddl_impact_summary(sql: &str, target: &str) -> String {
    let verb = sql.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    match verb.as_str() {
        "drop" => format!("Drop {target}: removes the database object; recovery requires a backup."),
        "truncate" => format!("Truncate {target}: removes its rows; recovery requires a backup."),
        "alter" => format!("Alter {target}: changes its structure and may invalidate dependent queries."),
        "create" => format!("Create {target}: adds or rebuilds a database object."),
        _ => format!("This SQL changes {target}; review the preview and keep a backup before applying."),
    }
}

pub(crate) fn matches_explorer_table(table: &str, query: &str) -> bool {
    query.is_empty() || ascii_contains_ignore_case(table, query)
}

pub(crate) fn matches_explorer_table_with_mode(
    table: &str,
    query: &str,
    mode: super::schema_explorer_state::ExplorerMatchMode,
) -> bool {
    match mode {
        super::schema_explorer_state::ExplorerMatchMode::Contains => matches_explorer_table(table, query),
        super::schema_explorer_state::ExplorerMatchMode::Prefix => {
            query.is_empty() || ascii_starts_with_ignore_case(table, query)
        }
    }
}

/// Case-fold ASCII only. SQL names are ASCII, so the search does not allocate
/// a lowercase copy of every name on each keystroke.
pub(crate) fn ascii_contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let needle = needle_lower.as_bytes();
    let bytes = haystack.as_bytes();
    if needle.len() > bytes.len() {
        return false;
    }
    bytes.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.to_ascii_lowercase() == *right)
    })
}

pub(crate) fn ascii_starts_with_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    let needle = needle_lower.as_bytes();
    let bytes = haystack.as_bytes();
    bytes.len() >= needle.len()
        && bytes
            .iter()
            .zip(needle)
            .all(|(left, right)| left.to_ascii_lowercase() == *right)
}

/// True when every query character appears in order. `custid` matches `customer_id`.
pub(crate) fn ascii_subsequence_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    let mut needle = needle_lower.bytes();
    let mut expected = needle.next();
    for byte in haystack.bytes() {
        let Some(want) = expected else {
            return true;
        };
        if byte.to_ascii_lowercase() == want {
            expected = needle.next();
        }
    }
    expected.is_none()
}

pub(crate) fn filtered_explorer_tables(
    tables: &[String],
    query: &str,
    mode: super::schema_explorer_state::ExplorerMatchMode,
) -> (usize, Vec<String>) {
    let matches = |table: &&String| matches_explorer_table_with_mode(table, query, mode);
    let mut matching_count = tables.iter().filter(matches).count();
    let mut visible_tables: Vec<String> = tables
        .iter()
        .filter(matches)
        .take(EXPLORER_MAX_TABLES)
        .cloned()
        .collect();
    // Contains found nothing: fall back to a subsequence so `custid` still
    // finds `customer_id`. Prefix mode stays strict.
    if matching_count == 0
        && !query.is_empty()
        && mode == super::schema_explorer_state::ExplorerMatchMode::Contains
    {
        let fuzzy = |table: &&String| ascii_subsequence_ignore_case(table, query);
        matching_count = tables.iter().filter(fuzzy).count();
        visible_tables = tables.iter().filter(fuzzy).take(EXPLORER_MAX_TABLES).cloned().collect();
    }
    (matching_count, visible_tables)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceTab {
    Welcome,
    Query,
    Table,
    SchemaObject,
    Results,
    Diagram,
    SchemaWorkbench,
    SchemaCompare,
    ComponentGallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableView {
    Structure,
    Data,
    Indexes,
    Relations,
    Constraints,
    Dependencies,
    Ddl,
    Profile,
}

#[derive(Debug, Clone)]
pub(crate) struct ColumnProfile {
    pub name: String,
    pub data_type: String,
    pub null_count: usize,
    pub null_rate: f64,
    pub distinct_count: usize,
    pub distinct_rate: f64,
    pub min: Option<String>,
    pub max: Option<String>,
    pub avg: Option<f64>,
    pub sum: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputTab {
    Results,
    Chart,
    Messages,
    Explain,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum OutputDockPosition {
    #[default]
    Bottom,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SchemaObjectSelection {
    View(String),
    Trigger(String),
    Function { name: String, identity_arguments: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaObjectView {
    Definition,
    Data,
}

#[cfg(test)]
#[path = "tests/layout_clamp_tests.rs"]
mod layout_clamp_tests;
