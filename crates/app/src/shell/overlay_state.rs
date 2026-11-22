/// State for transient shell overlays that are not owned by a single data view.
#[derive(Debug, Default)]
pub(crate) struct OverlayState {
    pub(super) export_open: bool,
    /// Result frozen at the moment the export dialog opens (#404) — a newer
    /// result must never leak into a dialog opened against older data.
    pub(super) export_snapshot: Option<crate::UiQueryResult>,
    pub(super) export_format: ExportFormat,
    pub(super) export_path: String,
    /// How a NULL cell is spelled in CSV/TSV exports (#612).
    pub(super) export_null: ExportNullMode,
    /// `true` while `export_path` came from the dialog's own pre-fill. Such a
    /// path carries a timestamp, so it is regenerated on every open instead of
    /// reusing the previous file and raising a pointless overwrite prompt (#621).
    pub(super) export_path_is_default: bool,
    /// Target table name used by SqlInsert/SqlCopy exports (#426).
    pub(super) export_table_name: String,
    /// Effective path the overwrite prompt was raised for (#539). A `bool`
    /// would stay armed across a Format/path change and let `Overwrite` replace
    /// a different file than the one named in the prompt.
    pub(super) export_overwrite_pending: Option<std::path::PathBuf>,
    pub(super) backup_output_path: String,
    pub(super) restore_input_path: String,
    pub(super) restore_confirmation: bool,
    pub(super) connection_delete_confirmation_id: Option<String>,
    /// Saved-query delete confirmation lives inside the queries sidebar
    /// surface; it must NOT share `connection_delete_confirmation_id`, which
    /// renders the connection delete modal in `draw_overlays`.
    pub(super) saved_query_delete_id: Option<String>,
    pub(super) folder_delete_confirmation: Option<String>,
}

/// NULL spelling for delimited exports (#612).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ExportNullMode {
    /// An empty field — what a spreadsheet expects from a CSV.
    #[default]
    Empty,
    /// The word `NULL`.
    Literal,
    /// The Postgres `COPY` sentinel, for round-trips (#487).
    Sentinel,
}

impl ExportNullMode {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Literal => "NULL",
            Self::Sentinel => "\\N",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Literal => "NULL",
            Self::Sentinel => "\\N",
        }
    }
}

impl OverlayState {
    /// The path the export will actually write (#396).
    ///
    /// The format's extension is appended when the typed path has none or a different one
    /// (`out.csv` + INSERT format → `out.csv.sql`) so the file on disk always matches its contents;
    /// an already-correct extension is kept as typed.
    pub(crate) fn effective_export_path(&self) -> std::path::PathBuf {
        let raw = self.export_path.trim();
        if raw.is_empty() {
            return std::path::PathBuf::new();
        }
        let path = std::path::PathBuf::from(raw);
        let expected = self.export_format.extension();
        match path.extension().and_then(|ext| ext.to_str()) {
            Some(existing) if existing.eq_ignore_ascii_case(expected) => path,
            _ => {
                let mut name = path.into_os_string();
                name.push(format!(".{expected}"));
                std::path::PathBuf::from(name)
            }
        }
    }
}

/// File format the result-grid export dialog writes. Owned by the overlay
/// state so the format list, labels and the writer dispatch in
/// `query_dialogs_view.rs` cannot drift out of sync — a variant either exists
/// here or it cannot be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExportFormat {
    #[default]
    Csv,
    Tsv,
    Json,
    Markdown,
    SqlInsert,
    SqlCopy,
    Excel,
}

impl ExportFormat {
    /// Pickable order in the dialog — every variant, once.
    pub(crate) const ALL: [Self; 7] = [
        Self::Csv,
        Self::Tsv,
        Self::Json,
        Self::Markdown,
        Self::SqlInsert,
        Self::SqlCopy,
        Self::Excel,
    ];

    /// Formats offered for the active connection. `COPY … FROM stdin` is a
    /// PostgreSQL script, so another provider must not be able to pick it.
    pub(crate) fn offered(postgres: bool) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |format| postgres || *format != Self::SqlCopy)
    }

    /// Short button label. These are file-format names, not UI copy, so they
    /// stay literal across locales.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Tsv => "TSV",
            Self::Json => "JSON",
            Self::Markdown => "Markdown",
            Self::SqlInsert => "INSERT",
            Self::SqlCopy => "COPY",
            Self::Excel => "Excel",
        }
    }

    /// `Some(delimiter)` for text-delimited formats; `None` means the writer
    /// owns the layout entirely (JSON, Markdown, SQL statements, Excel).
    pub(crate) fn delimiter(self) -> Option<&'static str> {
        match self {
            Self::Csv => Some(","),
            Self::Tsv => Some("\t"),
            _ => None,
        }
    }

    /// File extension matching what the writer emits (#396).
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Json => "json",
            Self::Markdown => "md",
            Self::SqlInsert | Self::SqlCopy => "sql",
            Self::Excel => "xlsx",
        }
    }
}

#[cfg(test)]
#[path = "tests/overlay_state_tests.rs"]
mod tests;
