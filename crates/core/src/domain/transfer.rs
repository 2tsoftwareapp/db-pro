//! Streaming transfer job model (#193 / Phase C01).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// Completed with some rejected/error rows but not aborted.
    Partial,
}

impl TransferStatus {
    /// Human-readable label for progress messages.
    ///
    /// Views used to print the enum with `{:?}`, so the transfer panel showed
    /// `· Succeeded ·` with Rust's variant spelling (#569).
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Partial => "Partially succeeded",
        }
    }
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferSourceKind {
    Query {
        connection_id: String,
        sql: String,
    },
    Table {
        connection_id: String,
        schema: String,
        table: String,
    },
    Synthetic {
        rows: u64,
    },
    File {
        path: String,
        format: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferTargetKind {
    Table {
        connection_id: String,
        schema: String,
        table: String,
    },
    File {
        path: String,
        format: String,
    },
    /// Harness sink that counts rows without materializing them.
    Counting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransferConflictPolicy {
    /// Fail the job on the first conflicting primary/unique key.
    #[default]
    Abort,
    /// Skip conflicting rows and continue (counts as error_rows).
    Skip,
    /// Replace existing rows (DELETE+INSERT or upsert semantics of the writer).
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransferTransactionPolicy {
    /// Each batch is independent; cancel/fail reports committed batches truthfully.
    #[default]
    PerBatch,
    /// Hold one transaction for the whole job; cleanup rolls back everything.
    AllOrNothing,
    /// Autocommit every statement; no batch transaction wrapper.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TransferMapping {
    /// Source column index → target column name (empty = drop).
    pub columns: Vec<(usize, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DbTransferOptions {
    pub conflict: TransferConflictPolicy,
    pub transaction: TransferTransactionPolicy,
    /// Create target table only when Phase A / capability permits (caller gated).
    pub create_target_if_missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TransferProgress {
    pub rows_read: u64,
    pub rows_written: u64,
    pub bytes_written: u64,
    pub error_rows: u64,
    /// Batches successfully committed under the selected transaction policy.
    pub committed_batches: u64,
    /// Rows sitting in an open (not yet committed) transaction, if any.
    pub uncommitted_rows: u64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferResult {
    pub status: TransferStatus,
    pub progress: TransferProgress,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: String,
    pub label: String,
    pub source: TransferSourceKind,
    pub target: TransferTargetKind,
    pub mapping: TransferMapping,
    pub batch_size: usize,
    #[serde(default)]
    pub options: DbTransferOptions,
    pub status: TransferStatus,
    pub progress: TransferProgress,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbTableEndpoint {
    pub connection_id: String,
    pub schema: String,
    pub table: String,
}

impl TransferJob {
    pub fn new_synthetic(id: impl Into<String>, rows: u64, batch_size: usize) -> Self {
        Self {
            id: id.into(),
            label: format!("Synthetic transfer ({rows} rows)"),
            source: TransferSourceKind::Synthetic { rows },
            target: TransferTargetKind::Counting,
            mapping: TransferMapping::default(),
            batch_size: batch_size.max(1),
            options: DbTransferOptions::default(),
            status: TransferStatus::Pending,
            progress: TransferProgress::default(),
            error: None,
        }
    }

    pub fn new_db_table_copy(
        id: impl Into<String>,
        source: DbTableEndpoint,
        target: DbTableEndpoint,
        batch_size: usize,
        options: DbTransferOptions,
    ) -> Self {
        Self {
            id: id.into(),
            label: format!("DB copy {} → {}", source.table, target.table),
            source: TransferSourceKind::Table {
                connection_id: source.connection_id,
                schema: source.schema,
                table: source.table,
            },
            target: TransferTargetKind::Table {
                connection_id: target.connection_id,
                schema: target.schema,
                table: target.table,
            },
            mapping: TransferMapping::default(),
            batch_size: batch_size.max(1),
            options,
            status: TransferStatus::Pending,
            progress: TransferProgress::default(),
            error: None,
        }
    }
}

/// Shared cancellation flag for a running transfer.
#[derive(Debug, Clone, Default)]
pub struct TransferCancellation {
    flag: Arc<AtomicBool>,
}

impl TransferCancellation {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRow {
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferError {
    Cancelled,
    Source(String),
    Target(String),
    Mapping(String),
    /// Unsupported or refused conversion between providers/types.
    Conversion(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "transfer cancelled"),
            Self::Source(msg) => write!(f, "source: {msg}"),
            Self::Target(msg) => write!(f, "target: {msg}"),
            Self::Mapping(msg) => write!(f, "mapping: {msg}"),
            Self::Conversion(msg) => write!(f, "conversion: {msg}"),
        }
    }
}

impl std::error::Error for TransferError {}
