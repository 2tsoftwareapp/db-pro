use super::*;
use crate::{UiCell, UiColumn, UiQueryResult};

fn grid_result() -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            UiColumn {
                name: "note".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
            UiColumn {
                name: "amount".to_owned(),
                data_type: "numeric".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![vec![
            UiCell::Text("Ada".to_owned()),
            UiCell::Number("9007199254740993".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 1,
    }
}

fn data_tab_app(result: Option<UiQueryResult>) -> DbProApp {
    let mut app = DbProApp::default();
    app.workspace.active_tab = WorkspaceTab::Table;
    app.table.state.table_view = TableView::Data;
    app.table.data_query.result = result.map(std::rc::Rc::new);
    app
}

fn workbook_xml(path: &std::path::Path) -> String {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(
            "import sys, zipfile\n\
             archive = zipfile.ZipFile(sys.argv[1])\n\
             chunks = [archive.read(name) for name in archive.namelist() if name.endswith('.xml')]\n\
             sys.stdout.buffer.write(b'\\n'.join(chunks))\n",
        )
        .arg(path)
        .output()
        .expect("python3 reads the xlsx");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("xlsx xml is utf-8")
}

/// The Data tab exports its own grid, including an Excel workbook of those rows.
#[test]
fn data_tab_exports_visible_rows_as_excel() {

    let mut empty = data_tab_app(None);
    empty.export_results_from_palette();
    assert!(!empty.overlay.export_open);
    assert_eq!(empty.feedback.runtime_message, "Run a query before exporting results");
    empty.open_table_data_export();
    assert_eq!(empty.feedback.runtime_message, "Load table data before exporting");

    let mut app = data_tab_app(Some(grid_result()));
    app.export_results_from_palette();
    assert!(app.overlay.export_open);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Table);
    assert_ne!(app.feedback.runtime_message, "Run a query before exporting results");
    app.overlay.export_open = false;
    app.open_table_data_export();
    assert!(app.overlay.export_open);

    let path = std::env::temp_dir().join(format!("db-pro-data-export-{}.xlsx", uuid::Uuid::new_v4()));
    let path_text = path.to_string_lossy().into_owned();
    app.overlay.export_format = crate::app::overlay_state::ExportFormat::Excel;
    app.overlay.export_path = path_text.clone();
    let result = app.table.data_query.result_ref().cloned().expect("data tab rows");
    app.export_result(&result);

    assert!(!app.overlay.export_open);
    assert_eq!(app.feedback.runtime_message, format!("Exported 1 rows to {path_text}"));
    let bytes = std::fs::read(&path).expect("xlsx");
    let xml = workbook_xml(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(xml.contains("note"), "{xml}");
    assert!(xml.contains("Ada"), "{xml}");
    assert!(xml.contains("9007199254740993"), "{xml}");
}
