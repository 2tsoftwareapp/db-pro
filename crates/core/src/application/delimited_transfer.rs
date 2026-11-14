//! CSV/TSV streaming adapters for TransferService (#194).

use crate::domain::transfer::{TransferError, TransferRow};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use super::transfer_service::{TransferSource, TransferTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimitedFormat {
    Csv,
    Tsv,
}

impl DelimitedFormat {
    pub fn delimiter(self) -> u8 {
        match self {
            Self::Csv => b',',
            Self::Tsv => b'\t',
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
        }
    }
}

/// Streaming delimited file source (does not load the whole file).
pub struct DelimitedFileSource {
    reader: csv::Reader<BufReader<File>>,
    headers: Vec<String>,
}

impl DelimitedFileSource {
    pub fn open(path: impl AsRef<Path>, format: DelimitedFormat, has_headers: bool) -> Result<Self, TransferError> {
        let file = File::open(path.as_ref()).map_err(|e| TransferError::Source(e.to_string()))?;
        let mut builder = csv::ReaderBuilder::new();
        builder.delimiter(format.delimiter()).has_headers(has_headers);
        let mut reader = builder.from_reader(BufReader::new(file));
        let mut headers = if has_headers {
            reader
                .headers()
                .map_err(|e| TransferError::Source(e.to_string()))?
                .iter()
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        };
        // A UTF-8 BOM (`\u{feff}`) precedes the first column — Excel exports it
        // routinely and it would otherwise poison header mapping (#486).
        if let Some(first) = headers.first_mut() {
            if let Some(stripped) = first.strip_prefix('\u{feff}') {
                *first = stripped.to_owned();
            }
        }
        Ok(Self { reader, headers })
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    /// Peek first `limit` rows for import preview (rewinds by re-open is caller's job;
    /// this consumes from the current position).
    pub fn preview_rows(&mut self, limit: usize) -> Result<Vec<TransferRow>, TransferError> {
        let mut rows = Vec::new();
        for result in self.reader.records().take(limit) {
            let record = result.map_err(|e| TransferError::Source(e.to_string()))?;
            rows.push(TransferRow {
                cells: record.iter().map(str::to_owned).collect(),
            });
        }
        Ok(rows)
    }
}

impl TransferSource for DelimitedFileSource {
    fn next_batch(&mut self, max_rows: usize) -> Result<Option<Vec<TransferRow>>, TransferError> {
        let mut batch = Vec::with_capacity(max_rows);
        for _ in 0..max_rows {
            match self.reader.records().next() {
                Some(Ok(record)) => batch.push(TransferRow {
                    cells: record.iter().map(str::to_owned).collect(),
                }),
                Some(Err(err)) => return Err(TransferError::Source(err.to_string())),
                None => break,
            }
        }
        if batch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(batch))
        }
    }
}

/// Streaming delimited writer that publishes atomically (temp → rename).
pub struct DelimitedFileTarget {
    writer: Option<csv::Writer<BufWriter<File>>>,
    final_path: PathBuf,
    temp_path: PathBuf,
    wrote_header: bool,
    headers: Vec<String>,
}

impl DelimitedFileTarget {
    pub fn create(
        path: impl AsRef<Path>,
        format: DelimitedFormat,
        headers: Vec<String>,
    ) -> Result<Self, TransferError> {
        let final_path = path.as_ref().to_path_buf();
        let temp_path = final_path.with_extension(format!("{}.partial", format.extension()));
        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent).map_err(|e| TransferError::Target(e.to_string()))?;
        }
        let file = File::create(&temp_path).map_err(|e| TransferError::Target(e.to_string()))?;
        let mut builder = csv::WriterBuilder::new();
        builder.delimiter(format.delimiter());
        let writer = builder.from_writer(BufWriter::new(file));
        Ok(Self {
            writer: Some(writer),
            final_path,
            temp_path,
            wrote_header: false,
            headers,
        })
    }
}

impl TransferTarget for DelimitedFileTarget {
    fn write_batch(&mut self, rows: &[TransferRow]) -> Result<u64, TransferError> {
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| TransferError::Target("writer already finished".into()))?;
        if !self.wrote_header && !self.headers.is_empty() {
            let headers: Vec<String> = self
                .headers
                .iter()
                .map(|h| guard_csv_field(h.as_str()).into_owned())
                .collect();
            writer
                .write_record(&headers)
                .map_err(|e| TransferError::Target(e.to_string()))?;
            self.wrote_header = true;
        }
        for row in rows {
            let cells: Vec<String> = row
                .cells
                .iter()
                .map(|cell| guard_csv_field(cell.as_str()).into_owned())
                .collect();
            writer
                .write_record(&cells)
                .map_err(|e| TransferError::Target(e.to_string()))?;
        }
        writer.flush().map_err(|e| TransferError::Target(e.to_string()))?;
        Ok(rows.len() as u64)
    }

    fn finish(&mut self) -> Result<(), TransferError> {
        if let Some(mut writer) = self.writer.take() {
            if !self.wrote_header && !self.headers.is_empty() {
                let headers: Vec<String> = self
                    .headers
                    .iter()
                    .map(|header| guard_csv_field(header).into_owned())
                    .collect();
                writer
                    .write_record(&headers)
                    .map_err(|error| TransferError::Target(error.to_string()))?;
                self.wrote_header = true;
            }
            writer.flush().map_err(|e| TransferError::Target(e.to_string()))?;
            drop(writer);
        }
        fs::rename(&self.temp_path, &self.final_path).map_err(|e| TransferError::Target(e.to_string()))?;
        Ok(())
    }

    fn cleanup_partial(&mut self) -> Result<(), TransferError> {
        self.writer.take();
        let _ = fs::remove_file(&self.temp_path);
        Ok(())
    }
}

/// Serialize provider cell text for CSV (quotes handled by csv crate).
pub fn csv_cell_from_display(value: &str, treat_empty_as_null_token: bool) -> String {
    if treat_empty_as_null_token && value.is_empty() {
        String::new()
    } else {
        value.to_owned()
    }
}

/// Leading characters spreadsheet apps treat as formula starts (#429).
///
/// CSV/TSV cells that begin with one of these are prefixed with `'` (the Excel-standard literal
/// escape) before they hit a delimited writer, so attacker-controlled database text like
/// `=HYPERLINK(...)` or `=cmd|'/c calc'!A1` survives an export as inert text instead of executing
/// when the file is opened in Excel or LibreOffice.
///
/// A leading space is deliberately **not** in this list (#593): " value" is ordinary data, and
/// prefixing it corrupted exported values and clipboard copies.
const CSV_FORMULA_PREFIXES: &[char] = &['=', '+', '-', '@', '\t', '\r', '\n'];

/// Whether a delimited write should neutralise spreadsheet formulas (#593).
///
/// `Raw` is RFC 4180 only — clipboard copies, TSV pastes and DB→file transfers that are meant to
/// be re-imported (or pasted into psql/an editor) must not gain characters. `ExcelSafe` adds the
/// formula guard for files a spreadsheet will open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelimitedSafety {
    /// No formula guard: values round-trip byte-for-byte.
    Raw,
    /// #429 guard: values a spreadsheet could evaluate are prefixed with `'`.
    #[default]
    ExcelSafe,
}

/// Returns `value` prefixed with `'` when a spreadsheet would evaluate it as a formula.
///
/// Exempt values: anything that parses as an `f64` (`-42`, `+1.5` — spreadsheets read those as
/// numbers) and phone-like strings (`+84 912 345 678`, `-`, `(555) 010-9999`) so a `+`/`-` lead
/// character alone is not treated as a formula start (#593).
/// Shared by every delimited write path (`DelimitedFileTarget`, service-level CSV renders, the
/// result-grid CSV/TSV formatters) so the guard cannot drift between call sites.
pub fn guard_csv_field(value: &str) -> std::borrow::Cow<'_, str> {
    guard_csv_field_with(value, DelimitedSafety::ExcelSafe)
}

/// [`guard_csv_field`] with an explicit safety mode (#593).
pub fn guard_csv_field_with(value: &str, safety: DelimitedSafety) -> std::borrow::Cow<'_, str> {
    if safety == DelimitedSafety::Raw || !is_formula_like(value) {
        return std::borrow::Cow::Borrowed(value);
    }
    std::borrow::Cow::Owned(format!("'{value}"))
}

/// True when the value starts the way a spreadsheet formula does (#429/#593).
fn is_formula_like(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    if !CSV_FORMULA_PREFIXES.contains(&first) {
        return false;
    }
    if value.parse::<f64>().is_ok() || is_phone_like(value) {
        return false;
    }
    // `=` and `@` always open an expression; tab/CR/LF can shift a cell into
    // formula position. `+`/`-` are only interesting when followed by more.
    matches!(first, '=' | '@' | '\t' | '\r' | '\n')
        || value.chars().nth(1).is_some_and(|next| !next.is_whitespace())
}

/// Digits, spaces and the punctuation a human writes in a phone number (#593).
fn is_phone_like(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | ' ' | '(' | ')' | '.'))
}

#[cfg(test)]
#[path = "tests/delimited_transfer_tests.rs"]
mod tests;
