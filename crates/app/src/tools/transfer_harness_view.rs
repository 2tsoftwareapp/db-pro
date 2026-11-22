// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::*;

pub(super) struct TransferHarnessContext<'a> {
    pub(super) management: &'a mut DatabaseManagementState,
    pub(super) feedback: &'a mut FeedbackState,
    pub(super) table_details: &'a [UiTableSummary],
}

/// Result of a transfer job finished on a worker thread (#421). Applying it to
/// `transfer_jobs`/`feedback` happens in `poll`, never on the UI click path.
#[derive(Debug)]
pub(super) struct TransferHarnessOutcome {
    /// Jobs in display order; applied newest-first into `transfer_jobs`.
    pub(super) jobs: Vec<db_pro_core::domain::transfer::TransferJob>,
    pub(super) message: String,
    /// Detail-pane body produced by a DB→DB copy.
    pub(super) copy_body: Option<String>,
    /// Masking surface error produced by the masked-export harness.
    pub(super) masking_error: Option<String>,
    /// Demo files written into temp_dir; `ClearJobs` deletes them (#422).
    pub(super) temp_files: Vec<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// Off-UI-thread execution (#421)
//
// `TransferService::run`, `copy_tables` and the file exporters block on I/O;
// run_* fns below only build the request and spawn a `std::thread` whose
// outcome is applied by `poll` — same shape as `git_workspace::spawn_git_task`.
// ---------------------------------------------------------------------------

fn spawn_transfer_task(
    job: impl FnOnce() -> TransferHarnessOutcome + Send + 'static,
) -> std::sync::mpsc::Receiver<TransferHarnessOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(job());
    });
    receiver
}
fn apply_outcome(
    transfer: &mut super::transfer_state::TransferState,
    feedback: &mut FeedbackState,
    mut outcome: TransferHarnessOutcome,
) {
    // Copy workers can't know the final job numbers at spawn time; renumber
    // their placeholders in display order before inserting newest-first.
    let base = transfer.transfer_jobs.len();
    for (index, job) in outcome.jobs.iter_mut().enumerate() {
        if job.id.starts_with("copy-outcome-") {
            job.id = format!("copy-{}", base + index + 1);
        }
    }
    for job in outcome.jobs.into_iter().rev() {
        transfer.transfer_jobs.insert(0, job);
    }
    if transfer.transfer_jobs.len() > 40 {
        transfer.transfer_jobs.truncate(40);
    }
    if let Some(body) = outcome.copy_body {
        transfer.last_copy_body = body;
    }
    transfer.harness_temp_files.extend(outcome.temp_files);
    if !outcome.message.is_empty() {
        feedback.runtime_message = outcome.message;
    }
}

fn synthetic_outcome(next_index: usize, cancel_midway: bool) -> TransferHarnessOutcome {
    use db_pro_core::application::TransferService;
    use db_pro_core::domain::transfer::{TransferCancellation, TransferJob, TransferStatus};

    let id = format!("xfer-{next_index}");
    let mut job = TransferJob::new_synthetic(id, if cancel_midway { 20_000 } else { 5_000 }, 128);
    let cancel = TransferCancellation::new();
    if cancel_midway {
        use db_pro_core::application::{CountingTarget, SyntheticSource, TransferSource, TransferTarget};
        let mut source = SyntheticSource::new(20_000);
        let mut target = CountingTarget::default();
        job.status = TransferStatus::Running;
        if let Ok(Some(batch)) = source.next_batch(job.batch_size) {
            job.progress.rows_read += batch.len() as u64;
            if let Ok(written) = target.write_batch(&batch) {
                job.progress.rows_written += written;
                job.progress.bytes_written = target.bytes_written;
            }
        }
        cancel.cancel();
        let _ = TransferService::run(&mut job, &mut source, &mut target, &cancel);
        job.progress.bytes_written = target.bytes_written;
    } else {
        let _ = TransferService::run_synthetic(&mut job, &cancel);
    }
    TransferHarnessOutcome {
        message: format!(
            "Transfer {} · {} · wrote {}",
            job.id, job.status, job.progress.rows_written
        ),
        jobs: vec![job],
        copy_body: None,
        masking_error: None,
        temp_files: Vec::new(),
    }
}

fn csv_export_outcome(next_index: usize) -> TransferHarnessOutcome {
    use db_pro_core::application::{DelimitedFileTarget, DelimitedFormat, SyntheticSource, TransferService};
    use db_pro_core::domain::transfer::{
        TransferCancellation, TransferJob, TransferSourceKind, TransferStatus, TransferTargetKind,
    };

    let path = harness_temp_path("csv");
    let mut job = TransferJob {
        id: format!("csv-{next_index}"),
        label: format!("CSV export → {}", path.display()),
        source: TransferSourceKind::Synthetic { rows: 1_000 },
        target: TransferTargetKind::File {
            path: path.to_string_lossy().into_owned(),
            format: "csv".into(),
        },
        mapping: Default::default(),
        batch_size: 100,
        options: Default::default(),
        status: TransferStatus::Pending,
        progress: Default::default(),
        error: None,
    };
    let mut source = SyntheticSource::new(1_000);
    match DelimitedFileTarget::create(&path, DelimitedFormat::Csv, vec!["id".into(), "name".into()]) {
        Ok(mut target) => {
            let cancel = TransferCancellation::new();
            let _ = TransferService::run(&mut job, &mut source, &mut target, &cancel);
        }
        Err(err) => {
            job.status = TransferStatus::Failed;
            job.error = Some(err.to_string());
        }
    }
    TransferHarnessOutcome {
        message: format!("CSV transfer {} · {} · {}", job.id, job.status, path.display()),
        jobs: vec![job],
        copy_body: None,
        masking_error: None,
        temp_files: vec![path],
    }
}

fn csv_import_preview_outcome(next_index: usize) -> TransferHarnessOutcome {
    use db_pro_core::application::{DelimitedFileSource, DelimitedFileTarget, DelimitedFormat};
    use db_pro_core::domain::transfer::{TransferJob, TransferStatus, TransferTargetKind};

    let path = harness_temp_path("csv");
    let _ = DelimitedFileTarget::create(&path, DelimitedFormat::Csv, vec!["id".into(), "name".into()]).and_then(
        |mut target| {
            use db_pro_core::application::TransferTarget;
            use db_pro_core::domain::transfer::TransferRow;
            target.write_batch(&[
                TransferRow {
                    cells: vec!["1".into(), "alpha".into()],
                },
                TransferRow {
                    cells: vec!["2".into(), "beta".into()],
                },
            ])?;
            target.finish()
        },
    );

    let mut job = TransferJob::new_synthetic(format!("csv-import-{next_index}"), 0, 50);
    job.label = format!("CSV import preview ← {}", path.display());
    job.target = TransferTargetKind::File {
        path: path.to_string_lossy().into_owned(),
        format: "csv".into(),
    };
    match DelimitedFileSource::open(&path, DelimitedFormat::Csv, true) {
        Ok(mut source) => match source.preview_rows(20) {
            Ok(rows) => {
                job.status = TransferStatus::Succeeded;
                job.progress.rows_read = rows.len() as u64;
                job.progress.message = format!(
                    "headers={}; preview {} row(s)",
                    source.headers().join(", "),
                    rows.len()
                );
            }
            Err(err) => {
                job.status = TransferStatus::Failed;
                job.error = Some(err.to_string());
            }
        },
        Err(err) => {
            job.status = TransferStatus::Failed;
            job.error = Some(err.to_string());
        }
    }
    TransferHarnessOutcome {
        message: job.progress.message.clone(),
        jobs: vec![job],
        copy_body: None,
        masking_error: None,
        temp_files: vec![path],
    }
}

fn jsonl_export_outcome(next_index: usize) -> TransferHarnessOutcome {
    use db_pro_core::application::{JsonlFileTarget, SyntheticSource, TransferService};
    use db_pro_core::domain::transfer::{TransferCancellation, TransferJob, TransferStatus, TransferTargetKind};

    let path = harness_temp_path("jsonl");
    let mut job = TransferJob::new_synthetic(format!("jsonl-{next_index}"), 400, 50);
    job.label = format!("JSONL export → {}", path.display());
    job.target = TransferTargetKind::File {
        path: path.to_string_lossy().into_owned(),
        format: "jsonl".into(),
    };
    match JsonlFileTarget::create(&path, vec!["id".into(), "name".into()]) {
        Ok(mut target) => {
            let mut source = SyntheticSource::new(400);
            let cancel = TransferCancellation::new();
            let _ = TransferService::run(&mut job, &mut source, &mut target, &cancel);
        }
        Err(err) => {
            job.status = TransferStatus::Failed;
            job.error = Some(err.to_string());
        }
    }
    TransferHarnessOutcome {
        message: format!("JSONL {} · {}", job.id, job.status),
        jobs: vec![job],
        copy_body: None,
        masking_error: None,
        temp_files: vec![path],
    }
}

fn excel_export_outcome(next_index: usize) -> TransferHarnessOutcome {
    use db_pro_core::application::{ExcelFileTarget, SyntheticSource, TransferService};
    use db_pro_core::domain::transfer::{TransferCancellation, TransferJob, TransferStatus, TransferTargetKind};

    let path = harness_temp_path("xlsx");
    let mut job = TransferJob::new_synthetic(format!("xlsx-{next_index}"), 80, 20);
    job.label = format!("Excel export → {}", path.display());
    job.target = TransferTargetKind::File {
        path: path.to_string_lossy().into_owned(),
        format: "xlsx".into(),
    };
    match ExcelFileTarget::create(&path, "Sheet1", vec!["id".into(), "name".into()]) {
        Ok(mut target) => {
            let mut source = SyntheticSource::new(80);
            let cancel = TransferCancellation::new();
            let _ = TransferService::run(&mut job, &mut source, &mut target, &cancel);
        }
        Err(err) => {
            job.status = TransferStatus::Failed;
            job.error = Some(err.to_string());
        }
    }
    TransferHarnessOutcome {
        message: format!("Excel {} · {}", job.id, job.status),
        jobs: vec![job],
        copy_body: None,
        masking_error: None,
        temp_files: vec![path],
    }
}

struct MaskedExportSpec {
    next_index: usize,
    columns_csv: String,
    rule: db_pro_core::domain::masking::MaskRule,
    keyed: bool,
}

fn masked_csv_export_outcome(spec: MaskedExportSpec) -> TransferHarnessOutcome {
    use db_pro_core::application::{DelimitedFileTarget, DelimitedFormat, TransferService};
    use db_pro_core::domain::masking::{mask_transfer_batch, ColumnMask, MaskingProfile};
    use db_pro_core::domain::transfer::{
        TransferCancellation, TransferJob, TransferSourceKind, TransferStatus, TransferTargetKind,
    };

    let path = harness_temp_path("csv");
    let headers = vec!["id".into(), "email".into(), "phone".into()];
    let profile = MaskingProfile {
        name: "export".into(),
        schema: String::new(),
        table: String::new(),
        columns: spec
            .columns_csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|column| ColumnMask {
                column: column.to_owned(),
                rule: spec.rule,
                replacement: "[masked]".into(),
                keep_prefix: 2,
                keep_suffix: 2,
            })
            .collect(),
        keyed: spec.keyed,
        key_id: "local-install".into(),
    };
    // Keyed masking needs secret key material: the per-install key from
    // `<data dir>/masking.key`, never a compile-time literal (#395).
    let key_material = match masking::local_masking_key_material() {
        Ok(key) => key,
        Err(err) => {
            let mut job = TransferJob {
                id: format!("masked-{}", spec.next_index),
                label: format!("Demo masked CSV → {}", path.display()),
                source: TransferSourceKind::Synthetic { rows: 0 },
                target: TransferTargetKind::File {
                    path: path.display().to_string(),
                    format: "csv".into(),
                },
                mapping: Default::default(),
                batch_size: 50,
                options: Default::default(),
                status: TransferStatus::Failed,
                progress: Default::default(),
                error: Some(err.clone()),
            };
            job.status = TransferStatus::Failed;
            return TransferHarnessOutcome {
                jobs: vec![job],
                message: String::new(),
                copy_body: None,
                masking_error: Some(err),
                temp_files: Vec::new(),
            };
        }
    };
    let raw = vec![
        db_pro_core::domain::transfer::TransferRow {
            cells: vec!["1".into(), "ada@example.com".into(), "1234567890".into()],
        },
        db_pro_core::domain::transfer::TransferRow {
            cells: vec!["2".into(), "grace@example.com".into(), "0987654321".into()],
        },
    ];
    let masked = mask_transfer_batch(&headers, &raw, &profile, &key_material);
    let mut job = TransferJob {
        id: format!("masked-{}", spec.next_index),
        label: format!("Demo masked CSV → {}", path.display()),
        source: TransferSourceKind::Synthetic {
            rows: masked.len() as u64,
        },
        target: TransferTargetKind::File {
            path: path.display().to_string(),
            format: "csv".into(),
        },
        mapping: Default::default(),
        batch_size: 50,
        options: Default::default(),
        status: TransferStatus::Pending,
        progress: Default::default(),
        error: None,
    };
    let mut outcome = TransferHarnessOutcome {
        jobs: Vec::new(),
        message: String::new(),
        copy_body: None,
        masking_error: None,
        // Tracked so `Clear jobs` removes the file — it still holds masked-but-real data (#422).
        temp_files: vec![path.clone()],
    };
    match DelimitedFileTarget::create(&path, DelimitedFormat::Csv, headers.clone()) {
        Ok(mut target) => {
            let cancel = TransferCancellation::new();
            struct MaskedSource {
                rows: Vec<db_pro_core::domain::transfer::TransferRow>,
                idx: usize,
            }
            impl db_pro_core::application::TransferSource for MaskedSource {
                fn next_batch(
                    &mut self,
                    max_rows: usize,
                ) -> Result<
                    Option<Vec<db_pro_core::domain::transfer::TransferRow>>,
                    db_pro_core::domain::transfer::TransferError,
                > {
                    if self.idx >= self.rows.len() {
                        return Ok(None);
                    }
                    let end = (self.idx + max_rows).min(self.rows.len());
                    let batch = self.rows[self.idx..end].to_vec();
                    self.idx = end;
                    Ok(Some(batch))
                }
            }
            let mut source = MaskedSource { rows: masked, idx: 0 };
            let _ = TransferService::run(&mut job, &mut source, &mut target, &cancel);
            restrict_owner_only(&path);
            outcome.message =
                format!("Demo masked CSV written to {} (synthetic rows)", path.display());
        }
        Err(err) => {
            job.status = TransferStatus::Failed;
            job.error = Some(err.to_string());
            outcome.masking_error = Some(err.to_string());
        }
    }
    outcome.jobs.push(job);
    outcome
}

/// Apply the DB→DB copy result: one job per copied table, matching the old
/// inline bookkeeping.
fn copy_output_outcome(
    request: db_pro_core::application::TableCopyRequest,
    connector: std::sync::Arc<dyn db_pro_core::application::TableCopyConnector>,
) -> TransferHarnessOutcome {
    use db_pro_core::application::{copy_tables, CopyFormat};

    match copy_tables(connector.as_ref(), &request) {
        Ok(output) => {
            let mut jobs = Vec::with_capacity(output.tables.len());
            for (index, qualified) in output.tables.iter().enumerate() {
                let (schema, table) = split_schema_table(qualified);
                let mut job = db_pro_core::domain::transfer::TransferJob::new_db_table_copy(
                    format!("copy-outcome-{index}"),
                    db_pro_core::domain::transfer::DbTableEndpoint {
                        connection_id: output.source_connection_id.clone(),
                        schema: schema.clone(),
                        table: table.clone(),
                    },
                    db_pro_core::domain::transfer::DbTableEndpoint {
                        connection_id: output.target_connection_id.clone(),
                        schema,
                        table,
                    },
                    100,
                    db_pro_core::domain::transfer::DbTransferOptions {
                        conflict: output.conflict,
                        transaction: db_pro_core::domain::transfer::TransferTransactionPolicy::PerBatch,
                        create_target_if_missing: false,
                    },
                );
                job.status = db_pro_core::domain::transfer::TransferStatus::Succeeded;
                job.progress.rows_read = output.rows_written + output.rows_skipped;
                job.progress.rows_written = output.rows_written;
                job.progress.error_rows = output.rows_skipped;
                jobs.push(job);
            }
            let format_name = match output.format {
                CopyFormat::Csv => "csv",
                CopyFormat::Json => "json",
            };
            TransferHarnessOutcome {
                jobs,
                message: format!(
                    "Copied {} → {} · {format_name} · wrote {} skipped {}",
                    output.source_connection_id,
                    output.target_connection_id,
                    output.rows_written,
                    output.rows_skipped
                ),
                copy_body: Some(output.body),
                masking_error: None,
                temp_files: Vec::new(),
            }
        }
        Err(error) => TransferHarnessOutcome {
            jobs: Vec::new(),
            message: error,
            copy_body: None,
            masking_error: None,
            temp_files: Vec::new(),
        },
    }
}

impl TransferHarnessContext<'_> {
    fn build_synthetic_plan(&self) -> Result<db_pro_core::domain::synthetic_data::SyntheticPlan, String> {
        synthetic_data::build_plan(
            &self.management.synthetic_data.synthetic_table,
            &self.management.synthetic_data.synthetic_row_count,
            &self.management.synthetic_data.synthetic_seed,
            &self.management.synthetic_data.synthetic_null_pct,
            self.table_details,
        )
    }

    pub(super) fn preview_synthetic_seed(&mut self) {
        match self.build_synthetic_plan() {
            Ok(plan) => match db_pro_core::domain::synthetic_data::generate_preview(&plan, 20) {
                Ok(preview) => {
                    self.management.synthetic_data.synthetic_preview = Some(preview);
                    self.management.synthetic_data.synthetic_error = None;
                }
                Err(err) => {
                    self.management.synthetic_data.synthetic_error = Some(err);
                    self.management.synthetic_data.synthetic_preview = None;
                }
            },
            Err(err) => {
                self.management.synthetic_data.synthetic_error = Some(err);
                self.management.synthetic_data.synthetic_preview = None;
            }
        }
    }

    pub(super) fn build_synthetic_seed_sql(&self) -> Result<(String, usize), String> {
        let plan = self.build_synthetic_plan()?;
        let count = usize::try_from(plan.row_count.min(500)).map_err(|_| "invalid row count".to_owned())?;
        let rows = db_pro_core::domain::synthetic_data::generate_rows(&plan, count)?;
        let sql = db_pro_core::domain::synthetic_data::render_insert_sql(&plan, &rows)?;
        Ok((sql, count))
    }

    pub(super) fn preview_masking_sample(&mut self) {
        self.management.masking.masking_preview = Some(masking::build_preview(
            &self.management.masking.masking_columns_csv,
            self.management.masking.masking_rule,
            self.management.masking.masking_keyed,
        ));
        self.management.masking.masking_error = None;
    }

    /// Drain finished transfer workers into `transfer_jobs`/feedback (#421).
    /// Called from the per-frame tick so a slow copy never blocks drawing.
    pub(crate) fn poll(&mut self) {
        let outcomes = self.management.transfer.drain_pending();
        for mut outcome in outcomes {
            if let Some(error) = outcome.masking_error.take() {
                self.management.masking.masking_error = Some(error);
            }
            apply_outcome(&mut self.management.transfer, self.feedback, outcome);
        }
    }

    /// True while a transfer is still running on a worker thread.
    #[cfg(test)]
    pub(crate) fn has_pending(&self) -> bool {
        !self.management.transfer.pending_transfers.is_empty()
    }

    /// Block until pending transfers finish — tests only; the UI always polls.
    #[cfg(test)]
    pub(crate) fn wait_for_pending(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.has_pending() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.poll();
        }
    }

    fn next_index(&self) -> usize {
        self.management.transfer.transfer_jobs.len() + 1
    }

    fn spawn(&mut self, job: impl FnOnce() -> TransferHarnessOutcome + Send + 'static, running: &str) {
        self.management
            .transfer
            .pending_transfers
            .push(spawn_transfer_task(job));
        self.feedback.runtime_message = running.to_owned();
    }

    pub(crate) fn run_masked_csv_export_harness(&mut self) {
        let spec = MaskedExportSpec {
            next_index: self.next_index(),
            columns_csv: self.management.masking.masking_columns_csv.clone(),
            rule: self.management.masking.masking_rule,
            keyed: self.management.masking.masking_keyed,
        };
        self.spawn(move || masked_csv_export_outcome(spec), "Masked CSV export running…");
    }

    pub(crate) fn run_synthetic_transfer_harness(&mut self, cancel_midway: bool) {
        let next_index = self.next_index();
        self.spawn(move || synthetic_outcome(next_index, cancel_midway), "Synthetic transfer running…");
    }

    pub(crate) fn run_csv_export_harness(&mut self) {
        let next_index = self.next_index();
        self.spawn(move || csv_export_outcome(next_index), "CSV export running…");
    }

    pub(crate) fn run_csv_import_preview_harness(&mut self) {
        let next_index = self.next_index();
        self.spawn(move || csv_import_preview_outcome(next_index), "CSV import preview running…");
    }

    pub(crate) fn run_jsonl_export_harness(&mut self) {
        let next_index = self.next_index();
        self.spawn(move || jsonl_export_outcome(next_index), "JSONL export running…");
    }

    pub(crate) fn run_excel_export_harness(&mut self) {
        let next_index = self.next_index();
        self.spawn(move || excel_export_outcome(next_index), "Excel export running…");
    }

    pub(crate) fn run_chosen_copy(
        &mut self,
        connector: std::sync::Arc<dyn db_pro_core::application::TableCopyConnector>,
    ) {
        use db_pro_core::application::{CopyFormat, TableCopyRequest};

        let transfer = &self.management.transfer;
        let format = match CopyFormat::parse(&transfer.copy_format) {
            Ok(format) => format,
            Err(error) => {
                self.feedback.runtime_message = error;
                return;
            }
        };
        let conflict = match parse_copy_conflict(&transfer.copy_conflict) {
            Ok(conflict) => conflict,
            Err(error) => {
                self.feedback.runtime_message = error;
                return;
            }
        };
        let request = TableCopyRequest {
            source_connection_id: transfer.source_connection_id.clone(),
            target_connection_id: transfer.target_connection_id.clone(),
            tables: split_list(&transfer.copy_tables),
            format,
            conflict,
            columns: split_list(&transfer.copy_columns),
        };
        self.spawn(
            move || copy_output_outcome(request, connector),
            "Copy running on a worker thread…",
        );
    }
}

fn parse_copy_conflict(value: &str) -> Result<db_pro_core::domain::transfer::TransferConflictPolicy, String> {
    use db_pro_core::domain::transfer::TransferConflictPolicy;
    match value.trim().to_lowercase().as_str() {
        "abort" | "" => Ok(TransferConflictPolicy::Abort),
        "skip" => Ok(TransferConflictPolicy::Skip),
        "replace" => Ok(TransferConflictPolicy::Replace),
        other => Err(format!("Unknown conflict policy: {other}")),
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn split_schema_table(qualified: &str) -> (String, String) {
    match qualified.split_once('.') {
        Some((schema, table)) => (schema.trim().to_owned(), table.trim().to_owned()),
        None => ("public".to_owned(), qualified.trim().to_owned()),
    }
}

/// Unpredictable, per-run file name inside the shared temp dir for the demo harnesses (#422).
///
/// A deterministic `dbpro-export-N.csv` in `temp_dir` can be pre-created or read by another
/// process on a multi-user machine; the uuid makes that infeasible and the file flows back on
/// `TransferHarnessOutcome::temp_files` so `Clear jobs` deletes it.
fn harness_temp_path(extension: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("db-pro-harness-{}.{extension}", uuid::Uuid::new_v4()))
}

/// Best-effort 0600 on files that may hold masked-but-real data (#422). No-op off Unix.
fn restrict_owner_only(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Ok(metadata) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o700 | 0o600),
            );
        }
    }
}

#[cfg(test)]
#[path = "tests/transfer_harness_view_tests.rs"]
mod tests;
