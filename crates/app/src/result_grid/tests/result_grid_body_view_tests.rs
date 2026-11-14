//! Tests for `../result_grid_body_view.rs`.
//!
//! #574: the body used a vertical scroll area nested inside a horizontal one, so
//! the vertical scrollbar sat at the right edge of the *content* — off-screen for
//! a wide table. The body is now a single two-axis area whose rows are virtualized
//! by these two helpers, the same window `show_rows` used to compute.

use super::{grid_first_visible_row, grid_last_visible_row};

const ROW_HEIGHT: f32 = 28.0;

#[test]
fn first_visible_row_tracks_the_vertical_offset() {
    assert_eq!(grid_first_visible_row(0.0, ROW_HEIGHT, 10_000), 0);
    assert_eq!(grid_first_visible_row(27.9, ROW_HEIGHT, 10_000), 0);
    assert_eq!(grid_first_visible_row(28.0, ROW_HEIGHT, 10_000), 1);
    assert_eq!(grid_first_visible_row(280.0, ROW_HEIGHT, 10_000), 10);
    // Scrolling past the end clamps instead of overflowing.
    assert_eq!(grid_first_visible_row(1_000_000.0, ROW_HEIGHT, 100), 100);
}

#[test]
fn visible_window_covers_offscreen_rows_and_stops_after_the_last() {
    // A viewport 10 rows tall starting at row 10 must include row 20 partially.
    let first = grid_first_visible_row(280.0, ROW_HEIGHT, 10_000);
    let last = grid_last_visible_row(280.0 + 10.0 * ROW_HEIGHT, ROW_HEIGHT, 10_000);
    assert_eq!(first, 10);
    assert_eq!(last, 21);
    assert_eq!(grid_last_visible_row(0.0, ROW_HEIGHT, 3), 3);
}

#[test]
fn degenerate_row_height_never_divides_by_zero() {
    assert_eq!(grid_first_visible_row(100.0, 0.0, 50), 0);
    assert_eq!(grid_last_visible_row(100.0, 0.0, 50), 50);
}
