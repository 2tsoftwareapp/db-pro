//! Tests for `../query_layout_surface_view.rs` (moved out of the source file).
use super::*;

#[test]
fn closed_dock_keeps_editor_above_minimum_height() {
    let layout = calculate(QueryPanelLayoutContext {
        available_width: 800.0,
        available_height: 400.0,
        bottom_panel_open: false,
        output_dock_maximized: false,
        bottom_panel_height: 500.0,
        right_dock_width: 400.0,
        dock_position: OutputDockPosition::Bottom,
    });
    assert!(!layout.dock_open);
    assert_eq!(layout.dock_height, 0.0);
    assert!(layout.editor_height >= 120.0);
}

#[test]
fn maximized_dock_reserves_compact_editor_strip() {
    let layout = calculate(QueryPanelLayoutContext {
        available_width: 1000.0,
        available_height: 900.0,
        bottom_panel_open: true,
        output_dock_maximized: true,
        bottom_panel_height: 320.0,
        right_dock_width: 400.0,
        dock_position: OutputDockPosition::Bottom,
    });
    assert_eq!(layout.editor_height, 80.0);
    assert!(layout.dock_height >= OUTPUT_MIN_HEIGHT);
}

#[test]
fn right_dock_calculates_horizontal_widths() {
    let layout = calculate(QueryPanelLayoutContext {
        available_width: 1200.0,
        available_height: 800.0,
        bottom_panel_open: true,
        output_dock_maximized: false,
        bottom_panel_height: 300.0,
        right_dock_width: 500.0,
        dock_position: OutputDockPosition::Right,
    });
    assert!(layout.dock_open);
    assert_eq!(layout.dock_position, OutputDockPosition::Right);
    assert_eq!(layout.dock_width, 500.0);
    assert_eq!(layout.editor_width, 700.0);
}

#[test]
fn tall_window_grows_the_dock_beyond_the_old_flat_cap() {
    // #576: a 4K-height pane used to cap the dock at 420 px.
    let layout = calculate(QueryPanelLayoutContext {
        available_width: 2000.0,
        available_height: 1800.0,
        bottom_panel_open: true,
        output_dock_maximized: false,
        bottom_panel_height: 900.0,
        right_dock_width: 400.0,
        dock_position: OutputDockPosition::Bottom,
    });
    assert!(layout.dock_height > OUTPUT_MAX_HEIGHT);
    assert!(layout.editor_height > 0.0);
    assert!(layout.dock_height + layout.editor_height <= 1800.0);
}

#[test]
fn short_window_keeps_editor_and_dock_inside_the_pane() {
    // #576: 120 px dock + editor floor + status overflowed a short window.
    let layout = calculate(QueryPanelLayoutContext {
        available_width: 1000.0,
        available_height: 300.0,
        bottom_panel_open: true,
        output_dock_maximized: false,
        bottom_panel_height: 260.0,
        right_dock_width: 300.0,
        dock_position: OutputDockPosition::Bottom,
    });
    assert!(
        layout.dock_height + layout.editor_height <= 300.0,
        "dock {} + editor {} exceed the 300 px pane",
        layout.dock_height,
        layout.editor_height
    );
    assert!(layout.dock_height >= output_dock_min_height(300.0));
}
