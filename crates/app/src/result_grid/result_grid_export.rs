//! Pure result-grid export and value formatting functions.

use super::*;
use db_pro_core::application::driver_dialect::driver_type_from_name;
use db_pro_core::application::DelimitedSafety;
use db_pro_core::application::DriverDialect;
use db_pro_core::domain::connection::DriverType;
use db_pro_core::ports::SqlDialect;

pub(crate) fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// `schema.name` with every part ANSI-quoted for generated SQL (Postgres and SQLite share `"`
/// quoting). Quoting each part keeps dotted identifiers — e.g. an AdventureWorks-style table
/// literally named `HumanResources.Department` inside schema `main` — from being reparsed by
/// the engine as extra qualification parts.
pub(crate) fn qualified_sql_name(schema: &str, name: &str) -> String {
    let name = quote_sql_identifier(name);
    if schema.is_empty() {
        name
    } else {
        format!("{}.{}", quote_sql_identifier(schema), name)
    }
}

/// Row-limited `SELECT *` snippet in the syntax the driver actually accepts (#260).
/// SQL Server has no `LIMIT` clause and fails on it; T-SQL needs `SELECT TOP (n)`.
/// PostgreSQL, SQLite and MySQL all accept ANSI `LIMIT`.
///
/// Same per-driver dispatch shape as `tools::saved_task_sql::build_export_query`, which
/// already emits `TOP` for SQL Server.
pub(crate) fn select_top_snippet(driver: &str, from: &str, limit: u32) -> String {
    match driver_type_from_name(driver) {
        DriverType::SqlServer => format!("SELECT TOP ({limit}) *\nFROM {from};"),
        DriverType::Postgres | DriverType::SQLite | DriverType::Mysql => {
            format!("SELECT *\nFROM {from}\nLIMIT {limit};")
        }
    }
}

/// True when `value` is a bare-printable SQL numeric literal (#590).
fn is_finite_sql_number(value: &str) -> bool {
    value.parse::<f64>().is_ok_and(f64::is_finite)
}

pub(super) fn cell_sql_literal(cell: &UiCell) -> String {
    match cell {
        UiCell::Null => "NULL".to_owned(),
        UiCell::Boolean(value) => value.to_string().to_uppercase(),
        // #590: `NaN`/`inf`/`Infinity` parse as `f64` but are not valid bare SQL
        // numeric literals — they must be quoted or PostgreSQL reads them as
        // identifiers. Only finite numbers print unquoted.
        UiCell::Number(value) if is_finite_sql_number(value) => value.clone(),
        UiCell::Json(value) => format!("'{}'", value.replace('\'', "''")),
        UiCell::Bytes(value) => format!("'{}'", value.replace('\'', "''")),
        UiCell::Number(value) | UiCell::Text(value) => format!("'{}'", value.replace('\'', "''")),
    }
}

/// Escape one field for CSV/TSV output (file export and clipboard).
///
/// Beyond RFC-4180 quoting, a value a spreadsheet would evaluate — leading `=`, `@`, tab, … —
/// is prefixed with `'` so exported data stays inert in Excel/LibreOffice (#429), unless the
/// caller asked for raw output (#593: clipboard/TSV round-trips must not gain characters).
/// Numeric literals and phone-like strings are left alone.
pub(super) fn escape_delimited_field(value: &str, delimiter: char) -> String {
    escape_delimited_field_with(value, delimiter, DelimitedSafety::ExcelSafe)
}

/// [`escape_delimited_field`] with an explicit safety mode (#593).
pub(super) fn escape_delimited_field_with(
    value: &str,
    delimiter: char,
    safety: DelimitedSafety,
) -> String {
    let value = db_pro_core::application::guard_csv_field_with(value, safety);
    if value.contains(delimiter) || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Format one cell as a CSV/TSV field.
///
/// NULL uses the Postgres `\copy` sentinel `\N` so a paste/export round-trip keeps the NULL vs
/// empty-string distinction (`UiCell::Text("")` stays an empty field). A literal `\N` text cell
/// collides with the sentinel on re-import — Postgres `COPY` itself escapes the backslash in text
/// fields, but this formatter emits RFC-4180 quoting for spreadsheet compatibility instead, so the
/// ambiguity is accepted and documented here (#487).
pub(super) fn format_cell_delimited(cell: &UiCell, delimiter: char) -> String {
    format_cell_delimited_with(cell, delimiter, DelimitedSafety::ExcelSafe)
}

/// [`format_cell_delimited`] with an explicit safety mode (#593).
pub(super) fn format_cell_delimited_with(
    cell: &UiCell,
    delimiter: char,
    safety: DelimitedSafety,
) -> String {
    match cell {
        UiCell::Null => "\\N".to_owned(),
        UiCell::Boolean(value) => value.to_string(),
        UiCell::Number(value) => escape_delimited_field_with(value, delimiter, safety),
        UiCell::Text(value) => escape_delimited_field_with(value, delimiter, safety),
        UiCell::Json(value) => escape_delimited_field_with(value, delimiter, safety),
        UiCell::Bytes(value) => escape_delimited_field_with(value, delimiter, safety),
    }
}

/// Clipboard CSV: raw RFC 4180 only (#593).
///
/// The clipboard is pasted into editors, chat and `psql` as often as into a spreadsheet, and a
/// leading `'` there changes the value. File exports keep the Excel-safe guard.
pub(super) fn format_cell_csv(cell: &UiCell) -> String {
    format_cell_delimited_with(cell, ',', DelimitedSafety::Raw)
}


/// Workbook of the visible grid. Integers outside the 53-bit exact range stay text.
pub(super) fn format_result_excel(result: &UiQueryResult) -> Result<Vec<u8>, String> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();
    let header = rust_xlsxwriter::Format::new().set_bold();
    for (index, column) in result.columns.iter().enumerate() {
        let index = excel_column_index(index)?;
        worksheet
            .write_string_with_format(0, index, &column.name, &header)
            .map_err(|error| format!("excel header write failed: {error}"))?;
    }
    for (row_index, row) in result.rows.iter().enumerate() {
        let row_index = excel_row_index(row_index)?;
        for (column_index, cell) in row.iter().enumerate() {
            write_excel_cell(worksheet, row_index, excel_column_index(column_index)?, cell)?;
        }
    }
    workbook
        .save_to_buffer()
        .map_err(|error| format!("excel save failed: {error}"))
}

fn excel_column_index(index: usize) -> Result<u16, String> {
    u16::try_from(index).map_err(|_| "Excel export has too many columns".to_owned())
}

fn excel_row_index(index: usize) -> Result<u32, String> {
    index
        .checked_add(1)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| "Excel export has too many rows".to_owned())
}

fn write_excel_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    column: u16,
    cell: &UiCell,
) -> Result<(), String> {
    let written = match cell {
        UiCell::Null => return Ok(()),
        UiCell::Boolean(value) => worksheet.write_boolean(row, column, *value).map(|_| ()),
        UiCell::Number(value) => write_excel_number(worksheet, row, column, value),
        UiCell::Text(value) | UiCell::Json(value) | UiCell::Bytes(value) => {
            worksheet.write_string(row, column, value).map(|_| ())
        }
    };
    written.map_err(|error| format!("excel cell write failed: {error}"))
}

fn write_excel_number(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    column: u16,
    value: &str,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    const EXACT_LIMIT: i64 = 1_i64 << 53;
    if let Ok(integer) = value.parse::<i64>() {
        if (-EXACT_LIMIT..=EXACT_LIMIT).contains(&integer) {
            return worksheet.write_number(row, column, integer as f64).map(|_| ());
        }
        return worksheet.write_string(row, column, value).map(|_| ());
    }
    if let Ok(number) = value.parse::<f64>() {
        if number.is_finite() {
            return worksheet.write_number(row, column, number).map(|_| ());
        }
    }
    worksheet.write_string(row, column, value).map(|_| ())
}

pub(super) fn format_result_delimited(result: &UiQueryResult, delimiter: &str) -> String {
    let separator = delimiter.chars().next().unwrap_or(',');
    let mut output = result
        .columns
        .iter()
        .map(|column| escape_delimited_field(&column.name, separator))
        .collect::<Vec<_>>()
        .join(delimiter);
    output.push('\n');
    for row in &result.rows {
        output.push_str(
            &row.iter()
                .map(|cell| format_cell_delimited(cell, separator))
                .collect::<Vec<_>>()
                .join(delimiter),
        );
        output.push('\n');
    }
    output
}

pub(super) fn format_result_sql_insert(result: &UiQueryResult, table: &str, driver: &str) -> String {
    if result.columns.is_empty() {
        return String::new();
    }
    // #538: identifiers and literals must match the connection's dialect — MySQL rejects ANSI
    // quotes, SQL Server rejects backticks, and MySQL treats an unescaped backslash as an
    // escape introducer.
    let dialect = DriverDialect::from_name(driver);
    let driver_type = driver_type_from_name(driver);
    let cols = result
        .columns
        .iter()
        .map(|column| dialect.quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = String::new();
    const BATCH: usize = 50;
    for chunk in result.rows.chunks(BATCH) {
        output.push_str(&format!(
            "INSERT INTO {} ({cols}) VALUES\n",
            dialect.quote_identifier(table)
        ));
        for (index, row) in chunk.iter().enumerate() {
            let values = row
                .iter()
                .map(|cell| format_cell_sql_literal(cell, driver_type))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str("  (");
            output.push_str(&values);
            output.push(')');
            if index + 1 < chunk.len() {
                output.push_str(",\n");
            } else {
                output.push_str(";\n");
            }
        }
        output.push('\n');
    }
    output
}

/// PostgreSQL `COPY … FROM stdin` script. The export dialog only offers this format for
/// PostgreSQL, so the ANSI quoting and COPY text escaping below are intentionally PG-specific.
pub(super) fn format_result_copy(result: &UiQueryResult, table: &str, driver: &str) -> String {
    let dialect = DriverDialect::from_name(driver);
    let cols = result
        .columns
        .iter()
        .map(|column| dialect.quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = format!("COPY {} ({cols}) FROM stdin;\n", dialect.quote_identifier(table));
    for row in &result.rows {
        output.push_str(&row.iter().map(format_cell_copy_field).collect::<Vec<_>>().join("\t"));
        output.push('\n');
    }
    output.push_str("\\.\n");
    output
}

pub(super) fn format_result_markdown(result: &UiQueryResult) -> String {
    if result.columns.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    output.push('|');
    for col in &result.columns {
        output.push_str(&format!(" {} |", col.name.replace('|', "\\|")));
    }
    output.push('\n');
    output.push('|');
    for _ in &result.columns {
        output.push_str(" --- |");
    }
    output.push('\n');
    for row in &result.rows {
        output.push('|');
        for cell in row {
            let text = match cell {
                UiCell::Null => "NULL".to_owned(),
                UiCell::Boolean(v) => v.to_string(),
                UiCell::Number(v) | UiCell::Text(v) | UiCell::Json(v) | UiCell::Bytes(v) => {
                    v.replace('|', "\\|").replace('\n', " ")
                }
            };
            output.push_str(&format!(" {text} |"));
        }
        output.push('\n');
    }
    output
}

/// Unique JSON object keys for `columns`: duplicates get `_2`, `_3`, … suffixes so a
/// `SELECT 1 AS x, 2 AS x` or self-join result does not silently drop values in a keyed map.
pub(super) fn unique_json_keys(columns: &[crate::UiColumn]) -> Vec<String> {
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    columns
        .iter()
        .map(|column| {
            let count = seen.entry(column.name.as_str()).or_insert(0);
            *count += 1;
            if *count == 1 { column.name.clone() } else { format!("{}_{}", column.name, count) }
        })
        .collect()
}

pub(super) fn format_result_json(result: &UiQueryResult) -> String {
    let keys = unique_json_keys(&result.columns);
    let array: Vec<serde_json::Map<String, serde_json::Value>> = result
        .rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (col_idx, cell) in row.iter().enumerate() {
                if let Some(key) = keys.get(col_idx) {
                    obj.insert(key.clone(), cell_to_json_value(cell));
                }
            }
            obj
        })
        .collect();

    serde_json::to_string_pretty(&array).unwrap_or_else(|_| "[]".to_owned())
}

pub(super) fn format_cell_sql_literal(cell: &UiCell, driver: DriverType) -> String {
    match cell {
        UiCell::Null => "NULL".into(),
        // SQL Server has no boolean literal; `bit` takes 1/0.
        UiCell::Boolean(value) => match driver {
            DriverType::SqlServer => if *value { "1" } else { "0" }.into(),
            _ => if *value { "TRUE" } else { "FALSE" }.into(),
        },
        UiCell::Number(value) => value.clone(),
        UiCell::Text(value) | UiCell::Json(value) | UiCell::Bytes(value) => {
            // MySQL treats `\` as an escape introducer inside single-quoted strings; doubling it
            // keeps `C:\new` from turning into a newline. Other drivers keep backslashes literal.
            let escaped = match driver {
                DriverType::Mysql => value.replace('\\', "\\\\").replace('\'', "''"),
                _ => value.replace('\'', "''"),
            };
            format!("'{escaped}'")
        }
    }
}

pub(super) fn format_cell_copy_field(cell: &UiCell) -> String {
    match cell {
        UiCell::Null => "\\N".into(),
        UiCell::Boolean(value) => if *value { "t" } else { "f" }.into(),
        UiCell::Number(value) => value.clone(),
        UiCell::Text(value) | UiCell::Json(value) | UiCell::Bytes(value) => value
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r"),
    }
}

pub(super) fn cell_to_json_value(cell: &UiCell) -> serde_json::Value {
    const JSON_EXACT_INTEGER_LIMIT: i64 = 1 << 53;
    match cell {
        UiCell::Null => serde_json::Value::Null,
        UiCell::Boolean(value) => serde_json::Value::Bool(*value),
        UiCell::Number(value) => match value.parse::<i64>() {
            Ok(integer) if (-JSON_EXACT_INTEGER_LIMIT..=JSON_EXACT_INTEGER_LIMIT).contains(&integer) => {
                serde_json::Value::Number(integer.into())
            }
            _ => match value.parse::<f64>() {
                Ok(number) if number.is_finite() && number.to_string() == *value => {
                    serde_json::Number::from_f64(number)
                        .map(serde_json::Value::Number)
                        .unwrap_or_else(|| serde_json::Value::String(value.clone()))
                }
                _ => serde_json::Value::String(value.clone()),
            },
        },
        UiCell::Text(value) => serde_json::Value::String(value.clone()),
        UiCell::Json(value) => serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.clone())),
        UiCell::Bytes(value) => serde_json::Value::String(value.clone()),
    }
}

#[cfg(test)]
#[path = "tests/result_grid_export_tests.rs"]
mod tests;

