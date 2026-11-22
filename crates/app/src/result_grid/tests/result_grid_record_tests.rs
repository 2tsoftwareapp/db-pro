use super::*;

fn projection_test_result() -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![
            vec![UiCell::Number("2".to_owned()), UiCell::Text("Beta".to_owned())],
            vec![UiCell::Number("1".to_owned()), UiCell::Text("Alpha".to_owned())],
            vec![UiCell::Number("3".to_owned()), UiCell::Text("Gamma".to_owned())],
        ],
        row_count: 3,
        duration_ms: 1,
    }
}

fn record_frame(
    app: &mut DbProApp,
    ctx: &egui::Context,
    result: &UiQueryResult,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        events,
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| app.draw_result_grid(ui, result));
    })
}
/// Extracts the text egui asked the OS to copy this frame.
fn copied_text(output: &egui::FullOutput) -> String {
    output
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}


fn text_position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                Some(shape.pos + shape.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Text not rendered: {text}"))
}

fn click_record_text(app: &mut DbProApp, ctx: &egui::Context, result: &UiQueryResult, text: &str) -> egui::FullOutput {
    record_frame(app, ctx, result, Vec::new());
    let output = record_frame(app, ctx, result, Vec::new());
    let pos = text_position(&output, text);
    for pressed in [true, false] {
        let output = record_frame(
            app,
            ctx,
            result,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        if !pressed {
            return output;
        }
    }
    unreachable!()
}

#[test]
fn record_click_copies_label_and_full_staged_value() {
    let mut app = DbProApp::default();
    app.open_table_workspace_for_capture();
    app.table.data.select_single_cell((0, 1));
    let result = app.table.data_query.result.clone().expect("grid").as_ref().clone();
    let value = "long staged value ".repeat(12);
    app.table.editing.data_editing_cell = Some((0, 1));
    app.table.editing.data_edit_value = value.clone();
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);
    assert!(app.table.editing.record_view_open);
    assert!(app.table.editing.data_editing_cell.is_none());
    assert!(!app.table.mutation.staged_changes.is_empty());

    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    assert_eq!(
        copied_text(&click_record_text(&mut app, &ctx, &result, "email")),
        "email"
    );
    assert_eq!(
        copied_text(&click_record_text(&mut app, &ctx, &result, &value)),
        value
    );
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Grid, &result);
    assert_eq!(app.table.data.selected_cell, Some((0, 1)));
    assert_eq!(result.rows[0][1], UiCell::Text("alice@example.com".to_owned()));
}

#[test]
fn record_click_copies_null_and_empty_values() {
    // #487: `''` is a real value and must copy (as an empty clipboard write), while NULL
    // copies as the Postgres `\copy` sentinel `\N` — neither reports "nothing to copy".
    let mut app = DbProApp::default();
    app.open_table_workspace_for_capture();
    let mut result = app.table.data_query.result.clone().expect("grid").as_ref().clone();
    result.rows[0][1] = UiCell::Text(String::new());
    result.rows[0][3] = UiCell::Null;
    app.table.data.select_single_row(0);
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    assert_eq!(
        copied_text(&click_record_text(&mut app, &ctx, &result, "(empty)")),
        ""
    );
    assert_eq!(app.feedback.copy_status, "Cell copied");
    assert_eq!(
        copied_text(&click_record_text(&mut app, &ctx, &result, "NULL")),
        "\\N"
    );
    assert_eq!(app.feedback.copy_status, "Cell copied");
}

#[test]
fn record_mode_rejects_multi_selection_and_invalid_active_edit() {
    let mut app = DbProApp::default();
    app.open_table_workspace_for_capture();
    let result = app.table.data_query.result.clone().expect("grid").as_ref().clone();
    app.table.data.select_single_row(0);
    app.table.data.selected_rows.insert(1);
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);
    assert!(!app.table.editing.record_view_open);
    app.table.data.select_single_cell((0, 2));
    app.table.editing.data_editing_cell = Some((0, 2));
    app.table.editing.data_edit_value = "invalid boolean".to_owned();
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);
    assert!(!app.table.editing.record_view_open);
    assert!(app.table.editing.data_edit_error.is_some());
    assert!(app.table.mutation.staged_changes.is_empty());
}

#[test]
fn record_uses_selected_row_and_exits_when_filtered_out() {
    let mut app = DbProApp::default();
    app.workspace.active_tab = WorkspaceTab::Table;
    app.table.state.table_view = TableView::Data;
    let result = projection_test_result();
    app.table.data.select_single_row(0);
    app.table.data.grid_sort_column = Some(0);
    app.table.data_query.offset = 100;
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    let output = record_frame(&mut app, &ctx, &result, Vec::new());
    text_position(&output, "Beta");
    app.table.data.grid_filter = "Gamma".to_owned();
    record_frame(&mut app, &ctx, &result, Vec::new());
    assert!(!app.table.editing.record_view_open);
}

#[test]
fn record_mode_resets_on_new_table_result() {
    let mut app = DbProApp::default();
    app.table.editing.record_view_open = true;
    app.table.data_query.request = Some(RequestId(9));
    app.on_table_data_loaded(RequestId(8), projection_test_result(), 3);
    assert!(
        app.table.editing.record_view_open,
        "stale requests must not change presentation"
    );
    app.on_table_data_loaded(RequestId(9), projection_test_result(), 3);
    assert!(!app.table.editing.record_view_open);
}

#[test]
fn record_view_exits_when_refreshed_result_no_longer_contains_the_row() {
    let mut app = DbProApp::default();
    app.workspace.active_tab = WorkspaceTab::Table;
    app.table.state.table_view = TableView::Data;
    app.table.data.select_single_row(4);
    app.table.editing.record_view_open = true;
    let result = projection_test_result();
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    record_frame(&mut app, &ctx, &result, Vec::new());
    assert!(!app.table.editing.record_view_open);
}

#[test]
fn draw_table_record_closes_instead_of_panicking_on_out_of_range_row() {
    // Regression for the `result.rows[row_index]` panic: a stale selection index must close the
    // record view rather than index past the refreshed row set.
    let mut app = DbProApp::default();
    app.table.editing.record_view_open = true;
    let result = projection_test_result();
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| app.draw_table_record(ui, &result, 99));
    });
    assert!(!app.table.editing.record_view_open);
}

#[test]
fn record_surface_renders_all_fields_inside_full_height_content_area() {
    let mut app = DbProApp::default();
    app.open_table_workspace_for_capture();
    app.prepare_table_record_for_capture("record");
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut output = None;
    for _ in 0..2 {
        output = Some(crate::test_frame::frame(&ctx, egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
            ..Default::default()
        }, |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_table_data(ui, "customers"));
        }));
    }
    let output = output.unwrap();
    for field in ["id", "email", "active", "created_at"] {
        let pos = text_position(&output, field);
        assert!(pos.y > 0.0 && pos.y < 600.0, "field {field} must remain visible");
    }
}

#[test]
fn record_long_unbroken_values_are_laid_out_within_the_value_column() {
    let mut app = DbProApp::default();
    app.workspace.active_tab = WorkspaceTab::Table;
    app.table.state.table_view = TableView::Data;
    let mut result = projection_test_result();
    let long_xml = format!("<Survey>{}</Survey>", "<Answer>long-value</Answer>".repeat(500));
    result.rows[0][1] = UiCell::Text(long_xml.clone());
    app.table.data.select_single_row(0);
    app.set_table_data_presentation(table_data_surface_view::TableDataPresentation::Record, &result);

    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    let output = record_frame(&mut app, &ctx, &result, Vec::new());
    let rendered_width = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(shape) if shape.galley.text() == long_xml => Some(shape.galley.rect.width()),
            _ => None,
        })
        .expect("record value should be rendered");

    assert!(rendered_width < 800.0, "long value expanded to {rendered_width}px");
}
