//! IDE workspace model + operations for waves W01–W24 (#261–#284).
//!
//! Multi-root folders, file ops, search/replace, migrations, trust, env
//! profiles, diagnostics scan, rename refs, benchmarks, and sandbox helpers.

// allow: IDE workspace module is integrated incrementally into shell (state/search/replace
// are used); remaining parts of persisted/migration contract don't have callers yet — scoped
// at module level rather than sprinkling #[allow(dead_code)] across individual items.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

pub const WORKSPACE_MAX_ENTRIES: usize = 2_000;
pub const WORKSPACE_MAX_DEPTH: usize = 8;
pub const WORKSPACE_RECENT_MAX: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WorkspaceTrust {
    #[default]
    Untrusted,
    Trusted,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileNode {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<WorkspaceFileNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceIndexEntry {
    pub root_id: String,
    pub relative_path: String,
    pub is_sql: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEnvironment {
    pub name: String,
    pub connection_id: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEntry {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub version: String,
    pub status: MigrationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// allow: migration status of workspace root — not yet read by current shell but part of
// module's persisted state contract.
#[allow(dead_code)]
pub enum MigrationStatus {
    Pending,
    Unknown,
}

impl MigrationStatus {
    /// Display label; the UI must not print the variant name with `{:?}` (#569).
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Unknown => "Unknown",
        }
    }
}

impl std::fmt::Display for MigrationStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub root_id: String,
    pub relative_path: String,
    pub line: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePreview {
    pub root_id: String,
    pub relative_path: String,
    pub replacements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceHit {
    pub root_id: String,
    pub relative_path: String,
    pub line: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDiagnostic {
    pub root_id: String,
    pub relative_path: String,
    pub line: usize,
    pub message: String,
    pub severity: WorkspaceDiagnosticSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRunResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkCase {
    pub name: String,
    pub sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkResult {
    pub name: String,
    pub runs: Vec<u64>,
    pub avg_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from_file: String,
    pub object_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotebookCell {
    pub kind: NotebookCellKind,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotebookCellKind {
    Markdown,
    Sql,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookDocument {
    pub version: u32,
    pub cells: Vec<NotebookCellSerde>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookCellSerde {
    pub kind: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
    pub id: String,
    pub path: PathBuf,
    pub tree: Vec<WorkspaceFileNode>,
    pub index: Vec<WorkspaceIndexEntry>,
}

#[derive(Debug, Clone, Default)]
// allow: several fields (migration, environment) are not yet read by shell; struct is
// initial state contract in app_state so full payload is preserved.
#[allow(dead_code)]
pub struct IdeWorkspaceState {
    pub roots: Vec<WorkspaceRoot>,
    pub active_root: usize,
    pub trust: WorkspaceTrust,
    pub recent_roots: Vec<PathBuf>,
    pub last_error: Option<String>,
    pub expanded: std::collections::BTreeSet<String>,
    pub environments: Vec<WorkspaceEnvironment>,
    pub active_environment: usize,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub last_task: Option<TaskRunResult>,
    pub schema_fingerprint: Option<u64>,
    pub schema_drift_message: Option<String>,
    pub last_drift_check: Option<Instant>,
    /// Cache for [`IdeWorkspaceState::dependency_edges`] (#578).
    ///
    /// The Graph tab used to re-read every `.sql` file of every workspace root
    /// from disk on **every frame** (caret blink, hover, 50 ms poll) just to show
    /// the first 60 dependency edges. The edges are now computed once per
    /// `index_epoch` and reused until the index (or a scanned root) changes.
    pub dependency_edge_cache: Option<DependencyEdgeCache>,
    /// Bumped on every index mutation; keys [`Self::dependency_edge_cache`].
    pub index_epoch: u64,
}

/// Cached dependency edges plus the epoch that produced them (#578).
#[derive(Debug, Clone)]
pub struct DependencyEdgeCache {
    pub index_epoch: u64,
    pub edges: Vec<DependencyEdge>,
}
