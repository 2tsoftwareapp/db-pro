//! Overlay-local layout constants.

use egui::Margin;

use crate::tokens::{RADIUS_DROPDOWN, RADIUS_MD, RADIUS_POPOVER, RADIUS_SM, RADIUS_TOAST};

/// Dropdown surface radius — token role for dropdowns.
pub const DROPDOWN_RADIUS: f32 = RADIUS_DROPDOWN;
pub const DROPDOWN_ITEM_HEIGHT: f32 = 34.0;
/// Popover surface radius — token role for popovers.
pub const POPOVER_RADIUS: f32 = RADIUS_POPOVER;
/// Toast surface radius — token role for toasts.
pub const TOAST_RADIUS: f32 = RADIUS_TOAST;
pub const OPEN_TRANSLATE_PX: f32 = 4.0;
/// Default inner margin of popover surfaces. Off-scale on purpose: anchored
/// popovers keep their established 10 px inset rather than snapping to a token.
pub const POPOVER_MARGIN: Margin = Margin::same(10);
/// Inner margin of the dropdown menu surface (off-scale values kept).
pub const DROPDOWN_ITEM_MARGIN: Margin = Margin::symmetric(4, 6);
/// Smallest popup width a dropdown takes relative to its trigger.
pub const DROPDOWN_MIN_WIDTH: f32 = 180.0;
/// Widest a dropdown menu grows; longer labels truncate rather than stretch the
/// panel to a screen-wide band.
pub const DROPDOWN_MAX_WIDTH: f32 = 320.0;
/// Left inset from a dropdown row's edge to its icon/label.
pub const DROPDOWN_ITEM_HORIZONTAL_PADDING: f32 = 8.0;
/// Horizontal distance from a dropdown row's icon to where its label starts.
pub const DROPDOWN_ITEM_ICON_OFFSET: f32 = 22.0;
/// Trailing width reserved for a shortcut badge on a dropdown row.
pub const DROPDOWN_ITEM_TRAILING_RESERVE: f32 = 64.0;
/// Space between a shortcut badge and the dropdown row's right edge.
pub const DROPDOWN_SHORTCUT_TRAILING_GAP: f32 = 6.0;

pub const TOOLTIP_FONT_SIZE: f32 = 12.0;
pub const TOOLTIP_HORIZONTAL_PADDING: f32 = 18.0;
pub const TOOLTIP_VERTICAL_PADDING: f32 = 12.0;
pub const TOOLTIP_MIN_HEIGHT: f32 = 24.0;
pub const TOOLTIP_SHORTCUT_WIDTH: f32 = 38.0;
/// Text-wrap width tooltip content is laid out against.
pub const TOOLTIP_MAX_WIDTH: f32 = 260.0;
/// Gap between a widget and its tooltip; also the margin probed when checking
/// whether the preferred position still fits on screen.
pub const TOOLTIP_GAP: f32 = 6.0;
/// Inset that keeps tooltips off the screen boundary.
pub const TOOLTIP_SCREEN_INSET: f32 = 4.0;
/// Distance a tooltip travels while fading in.
pub const TOOLTIP_MOTION_OFFSET: f32 = 3.0;
/// Inner margin of the tooltip surface (off-scale vertical kept).
pub const TOOLTIP_MARGIN: Margin = Margin::symmetric(8, 5);
/// Horizontal gap between tooltip text and its shortcut badge.
pub const TOOLTIP_ITEM_GAP: f32 = 6.0;

/// Corner radius of dropdown rows on hover/selection — `RADIUS_MD` role.
pub const DROPDOWN_ITEM_RADIUS: f32 = RADIUS_MD;
pub const DROPDOWN_ICON_SIZE: f32 = 14.0;
pub const DROPDOWN_LABEL_SIZE: f32 = 13.0;
pub const TOAST_ICON_SIZE: f32 = 14.0;
pub const TOAST_MESSAGE_SIZE: f32 = 13.0;
pub const TOAST_ACTION_SIZE: f32 = 12.0;
pub const TOAST_CLOSE_SIZE: f32 = 11.0;
/// Gap between the toast message and the optional action button (off-scale, kept).
pub const TOAST_ACTION_GAP: f32 = 10.0;
/// Corner radius of the toast action button (off-scale, kept).
pub const TOAST_ACTION_RADIUS: f32 = 5.0;
/// Gap before the toast close button (off-scale, kept).
pub const TOAST_CLOSE_GAP: f32 = 6.0;
/// Auto-dismiss duration for toasts that carry an action button.
pub const TOAST_ACTION_DURATION_SECS: f32 = 8.0;
/// Label of the copy action attached to error toasts (#571).
pub const TOAST_COPY_ACTION_LABEL: &str = "Copy";
pub const TOAST_ACCENT_WIDTH: f32 = 3.0;
pub const TOAST_STACK_OFFSET: f32 = 54.0;
pub const TOAST_SCREEN_MARGIN: f32 = 24.0;
pub const TOAST_HORIZONTAL_PADDING: f32 = 14.0;
pub const TOAST_VERTICAL_PADDING: f32 = 10.0;

/// Context menus are popover surfaces.
pub const CONTEXT_MENU_RADIUS: f32 = RADIUS_POPOVER;
pub const CONTEXT_MENU_SCREEN_INSET: f32 = 4.0;
pub const CONTEXT_MENU_MAX_WIDTH: f32 = 230.0;
pub const CONTEXT_MENU_MAX_HEIGHT: f32 = 340.0;
pub const CONTEXT_MENU_MIN_WIDTH: f32 = 210.0;
pub const CONTEXT_MENU_HORIZONTAL_PADDING: f32 = 4.0;
pub const CONTEXT_MENU_VERTICAL_PADDING: f32 = 6.0;
pub const CONTEXT_MENU_ITEM_HEIGHT: f32 = 26.0;
/// Corner radius of context-menu rows on hover — `RADIUS_SM` role.
pub const CONTEXT_MENU_ITEM_RADIUS: f32 = RADIUS_SM;
pub const CONTEXT_MENU_ITEM_HORIZONTAL_PADDING: f32 = 8.0;
pub const CONTEXT_MENU_ICON_SIZE: f32 = 12.5;
pub const CONTEXT_MENU_LABEL_SIZE: f32 = 12.5;
pub const CONTEXT_MENU_SHORTCUT_SIZE: f32 = 10.0;
pub const CONTEXT_MENU_ICON_OFFSET: f32 = 22.0;
