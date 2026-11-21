//! Layout contract for the table workspace tab (#608).
//!
//! The shell owns full-window panels: however little data a table holds, the
//! central surface must stretch to the window and the status bar must stay
//! pinned to the bottom edge. A table with a handful of rows used to leave the
//! bottom of the window blank.

use super::*;
use crate::{UiCell, UiColumn, UiConnectionSummary, UiQueryResult, UiSslMode};

fn users_table_result() -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            UiColumn {
                name: "full_name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
        ],
        rows: (0..3)
            .map(|n| {
                vec![
                    UiCell::Number((n + 1).to_string()),
                    UiCell::Text(format!("User {n}")),
                ]
            })
            .collect(),
        row_count: 3,
        duration_ms: 1,
    }
}

fn sqlite_table_app() -> DbProApp {
    let mut app = DbProApp::default();
    *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
        id: "sqlite-1".to_owned(),
        name: "Local SQLite".to_owned(),
        host: String::new(),
        port: 0,
        database: "test.db".to_owned(),
        username: String::new(),
        driver: "SQLite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("sqlite-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.workspace.active_tab = WorkspaceTab::Table;
    app.schema.explorer.selected_table = Some("users".to_owned());
    app.table.state.table_view = TableView::Data;
    app.table.data_query.result = Some(std::rc::Rc::new(users_table_result()));
    app
}

fn screen_rect() -> egui::Rect {
    // `Rect::from_min_size` is not a `const fn`, so this cannot stay a `const`.
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 752.0))
}

fn render_shell(app: &mut DbProApp, ctx: &egui::Context) -> egui::FullOutput {
    crate::test_frame::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
        |ui| {
            app.prepare_frame(ui.ctx());
            app.draw_shell(ui);
        },
    )
}

fn statusbar_toggle_bottom(output: &egui::FullOutput) -> f64 {
    let update = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("shell paints an accesskit tree");
    update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Toggle output panel"))
        .and_then(|(_, node)| node.bounds())
        .map(|bounds| (bounds.y1 - f64::from(screen_rect().bottom())).abs())
        .expect("status bar output toggle is labelled")
}

#[test]
fn table_data_tab_pins_the_statusbar_to_the_bottom_of_the_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    DbProTheme::install_fonts(&ctx);
    let mut app = sqlite_table_app();

    // Warm up persisted panel state and fonts, then measure the settled frame.
    let _ = render_shell(&mut app, &ctx);
    let output = render_shell(&mut app, &ctx);
    let distance_from_bottom = statusbar_toggle_bottom(&output);
    assert!(
        distance_from_bottom < 1.0,
        "status bar must sit on the bottom edge of the window, off by {distance_from_bottom} points"
    );
}

#[test]
fn table_structure_tab_pins_the_statusbar_to_the_bottom_of_the_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    DbProTheme::install_fonts(&ctx);
    let mut app = sqlite_table_app();
    app.table.state.table_view = TableView::Structure;

    let _ = render_shell(&mut app, &ctx);
    let output = render_shell(&mut app, &ctx);
    let distance_from_bottom = statusbar_toggle_bottom(&output);
    assert!(
        distance_from_bottom < 1.0,
        "status bar must sit on the bottom edge of the window, off by {distance_from_bottom} points"
    );
}

#[test]
fn table_ddl_tab_pins_the_statusbar_to_the_bottom_of_the_window() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    DbProTheme::install_fonts(&ctx);
    let mut app = sqlite_table_app();
    app.table.state.table_view = TableView::Ddl;

    let _ = render_shell(&mut app, &ctx);
    let output = render_shell(&mut app, &ctx);
    let distance_from_bottom = statusbar_toggle_bottom(&output);
    assert!(
        distance_from_bottom < 1.0,
        "status bar must sit on the bottom edge of the window, off by {distance_from_bottom} points"
    );
}

#[test]
fn table_tab_pins_the_statusbar_to_the_bottom_with_the_output_panel_open() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    DbProTheme::install_fonts(&ctx);
    let mut app = sqlite_table_app();
    // A persisted output panel with a tall stored height used to push the
    // status bar up, leaving a background-colored gap below it (#608).
    app.workspace.bottom_panel_open = true;
    app.workspace.set_bottom_panel_height(220.0);

    let _ = render_shell(&mut app, &ctx);
    let output = render_shell(&mut app, &ctx);
    let distance_from_bottom = statusbar_toggle_bottom(&output);
    assert!(
        distance_from_bottom < 1.0,
        "status bar must stay on the bottom edge with the output panel open, off by {distance_from_bottom} points"
    );
}
