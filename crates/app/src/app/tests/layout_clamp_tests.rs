//! Tests for the output-dock clamp helpers in `../app_types.rs`.
//!
//! #576: the dock was clamped to a flat 120–420 px, so a 4K window showed the
//! same handful of result rows as a laptop, and a short window could push the
//! editor past the bottom edge.

use super::{output_dock_max_height, output_dock_min_height, OUTPUT_MAX_HEIGHT, OUTPUT_MIN_HEIGHT};

#[test]
fn tall_windows_may_show_more_result_rows() {
    // 1440 px pane: the old flat ceiling was 420 px.
    assert!(output_dock_max_height(1_440.0) > OUTPUT_MAX_HEIGHT);
    // 4K-ish pane.
    assert!(output_dock_max_height(2_000.0) >= 1_400.0);
    // The dock never claims the whole pane — the editor keeps a strip.
    assert!(output_dock_max_height(2_000.0) < 2_000.0);
}

#[test]
fn short_windows_shrink_the_floor_instead_of_overflowing() {
    // 240 px pane: a 120 px dock would leave nothing for the editor.
    let floor = output_dock_min_height(240.0);
    assert!(floor < OUTPUT_MIN_HEIGHT, "floor {floor} must yield to the pane");
    assert!(floor < 240.0);
    // A normal window keeps the readable floor.
    assert_eq!(output_dock_min_height(900.0), OUTPUT_MIN_HEIGHT);
}

#[test]
fn clamp_range_is_always_ordered() {
    for available in [0.0, 100.0, 240.0, 640.0, 900.0, 1_440.0, 3_000.0] {
        let min = output_dock_min_height(available);
        let max = output_dock_max_height(available);
        assert!(min <= max, "available {available}: min {min} > max {max}");
        assert!(min > 0.0);
    }
}

#[test]
fn maximized_dock_keeps_an_editor_strip() {
    let available = 800.0;
    let maximized = (available - 80.0).max(output_dock_min_height(available));
    assert!(maximized <= available - 80.0 + 0.001);
    assert!(available - maximized >= 80.0 - 0.001);
}
