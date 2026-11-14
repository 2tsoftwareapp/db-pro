//! Shared component support that is not a standalone widget.
//!
//! The common layer owns reusable layout calculations and display formatting. It is kept
//! separate from component folders so components depend on a small, named support surface
//! instead of a catch-all utility implementation.

mod format;
mod frames;
mod layout;
mod text;

pub use format::{
    fmt_opt, fmt_opt_duration_millis, format_bytes, format_count_with_suffix, format_duration_millis, format_page_range,
    format_percentage, format_shown_total, shown_total_hint, truncate_ellipsis, VALUE_ABSENT,
};
pub use frames::{
    activity_bar_frame, agent_message_frame, app_frame, chip_frame, editor_frame, grid_frame, list_row_frame,
    panel_frame, popup_content_frame, shell_surface_frame, sidebar_frame, tab_frame, toolbar_frame, ShellSurface,
};
pub use layout::{
    calculate_dialog_layout, calculate_sheet_layout, clamp_popup_to_screen, dialog_body_cap, DialogLayout,
};
pub(crate) use text::icon_layout;
pub use text::icon_text;
