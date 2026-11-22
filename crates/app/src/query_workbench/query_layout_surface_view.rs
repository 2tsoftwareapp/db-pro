//! Query workspace panel geometry.
use super::super::*;

pub(super) struct QueryPanelLayoutContext {
    pub(super) available_width: f32,
    pub(super) available_height: f32,
    pub(super) bottom_panel_open: bool,
    pub(super) output_dock_maximized: bool,
    pub(super) bottom_panel_height: f32,
    pub(super) right_dock_width: f32,
    pub(super) dock_position: OutputDockPosition,
}

pub(super) struct QueryPanelLayout {
    pub(super) dock_open: bool,
    pub(super) dock_position: OutputDockPosition,
    pub(super) dock_height: f32,
    pub(super) dock_width: f32,
    pub(super) editor_height: f32,
    pub(super) editor_width: f32,
}

/// Height of the bottom output dock for this frame.
///
/// #576: the ceiling follows the pane instead of a flat 420 px, and the floor
/// shrinks on a short window so editor + dock + status never exceed the
/// available height.
fn bottom_dock_height(context: &QueryPanelLayoutContext, status_height: f32) -> f32 {
    if !context.bottom_panel_open {
        return 0.0;
    }
    let panel_budget = (context.available_height - status_height).max(OUTPUT_MIN_HEIGHT);
    if context.output_dock_maximized {
        return (panel_budget - 80.0).max(output_dock_min_height(panel_budget));
    }
    context.bottom_panel_height.clamp(
        output_dock_min_height(panel_budget),
        output_dock_max_height(panel_budget),
    )
}

/// Dock/editor split for the bottom dock position.
///
/// #576: when the pane is too short for the dock plus an editor floor, the dock
/// gives height back instead of pushing the editor past the bottom edge.
fn vertical_split(context: &QueryPanelLayoutContext, status_height: f32) -> (f32, f32) {
    let mut dock_height = bottom_dock_height(context, status_height);
    if context.output_dock_maximized && context.bottom_panel_open {
        return (dock_height, 80.0);
    }
    let leftover = context.available_height - status_height - dock_height;
    if leftover < EDITOR_MIN_HEIGHT {
        dock_height = (dock_height - (EDITOR_MIN_HEIGHT - leftover)).max(OUTPUT_ABSOLUTE_MIN_HEIGHT);
    }
    let editor_height = (context.available_height - status_height - dock_height).max(0.0);
    (dock_height, editor_height)
}

pub(super) fn calculate(context: QueryPanelLayoutContext) -> QueryPanelLayout {
    let status_height = query_status_bar_surface_view::QUERY_STATUS_HEIGHT;
    match context.dock_position {
        OutputDockPosition::Bottom => {
            let (dock_height, editor_height) = vertical_split(&context, status_height);
            QueryPanelLayout {
                dock_open: context.bottom_panel_open,
                dock_position: OutputDockPosition::Bottom,
                dock_height,
                dock_width: context.available_width,
                editor_height,
                editor_width: context.available_width,
            }
        }
        OutputDockPosition::Right => {
            let full_height = (context.available_height - status_height).max(120.0);
            let dock_width = if !context.bottom_panel_open {
                0.0
            } else if context.output_dock_maximized {
                (context.available_width - 120.0).max(280.0)
            } else {
                context.right_dock_width.clamp(240.0, (context.available_width - 160.0).max(240.0))
            };
            let editor_width = if context.output_dock_maximized && context.bottom_panel_open {
                120.0
            } else {
                (context.available_width - dock_width).max(160.0)
            };
            QueryPanelLayout {
                dock_open: context.bottom_panel_open,
                dock_position: OutputDockPosition::Right,
                dock_height: full_height,
                dock_width,
                editor_height: full_height,
                editor_width,
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/query_layout_surface_view_tests.rs"]
mod tests;

