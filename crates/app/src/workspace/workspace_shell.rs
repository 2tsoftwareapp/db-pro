//! Feature-owned state for the native workspace shell and navigation surface.
//!
//! This aggregate owns shell layout and navigation selection. Query documents,
//! connection lifecycle and feature data remain in their own aggregates.

use super::*;

#[derive(Debug)]
pub(crate) struct WorkspaceShellState {
    pub(crate) activity: Activity,
    pub(crate) welcome_open: bool,
    /// Stays set after the ER Diagram tab is opened, until the user closes that tab.
    pub(crate) diagram_open: bool,
    /// Stays set after the Results tab is opened, until the user closes that tab.
    pub(crate) results_open: bool,
    pub(crate) active_tab: WorkspaceTab,
    pub(crate) sidebar_open: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) agent_open: bool,
    pub(crate) agent_width: f32,
    pub(crate) bottom_panel_open: bool,
    pub(crate) bottom_panel_height: f32,
    pub(crate) output_dock_position: OutputDockPosition,
    pub(crate) right_dock_width: f32,
    pub(crate) sidebar_open_before_agent: Option<bool>,
    pub(crate) pending_navigation_action: Option<PendingNavigationAction>,
    pub(crate) split_editor_secondary: Option<usize>,
    pub(crate) nav_back: std::collections::VecDeque<WorkspaceLocation>,
    pub(crate) nav_forward: Vec<WorkspaceLocation>,
    pub(crate) nav_current: WorkspaceLocation,
    pub(crate) nav_ready: bool,
    pub(crate) nav_suppress: bool,
}

/// One place the Back / Forward buttons can return to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkspaceLocation {
    pub(crate) tab: WorkspaceTab,
    pub(crate) query_index: usize,
}

impl Default for WorkspaceShellState {
    fn default() -> Self {
        Self {
            activity: Activity::Explorer,
            welcome_open: true,
            diagram_open: false,
            results_open: false,
            active_tab: WorkspaceTab::Welcome,
            sidebar_open: true,
            sidebar_width: 260.0,
            agent_open: false,
            agent_width: 360.0,
            bottom_panel_open: false,
            bottom_panel_height: 180.0,
            output_dock_position: OutputDockPosition::Bottom,
            right_dock_width: 480.0,
            sidebar_open_before_agent: None,
            pending_navigation_action: None,
            split_editor_secondary: None,
            nav_back: std::collections::VecDeque::new(),
            nav_forward: Vec::new(),
            nav_current: WorkspaceLocation {
                tab: WorkspaceTab::Welcome,
                query_index: 0,
            },
            nav_ready: false,
            nav_suppress: false,
        }
    }
}

impl WorkspaceShellState {
    pub(crate) fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
    }

    pub(crate) fn set_agent_width(&mut self, width: f32) {
        self.agent_width = width.clamp(AGENT_MIN_WIDTH, AGENT_MAX_WIDTH);
    }

    pub(crate) fn set_bottom_panel_height(&mut self, height: f32) {
        // The stored height is bounded by the absolute range; the per-frame layout
        // further clamps it to the space actually available (#576).
        self.bottom_panel_height = height.clamp(OUTPUT_MIN_HEIGHT, OUTPUT_MAX_HEIGHT);
    }

    pub(crate) fn set_right_dock_width(&mut self, width: f32) {
        self.right_dock_width = width.clamp(240.0, 1200.0);
    }
}

#[cfg(test)]
#[path = "tests/workspace_shell_tests.rs"]
mod tests;

