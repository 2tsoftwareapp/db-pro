//! Interaction boundary for a connection row in the database explorer.
//!
//! This module owns row painting and user intent collection. The application
//! root owns the reducer that turns those intents into connection, workspace,
//! and overlay state changes.

use super::explorer_tree::{draw_codex_tree_row, CodexTreeRow};
use super::{context_action_menu, ctx_menu_item, is_context_menu_triggered, DbProTheme, UiConnectionSummary};
use crate::components::Separator;
use eframe::egui;
use lucide_icons::Icon;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionRowAction {
    Connect,
    Disconnect,
    Reconnect,
    RefreshSchema,
    NewScript,
    OpenErDiagram,
    AskAgent,
    CreateTable,
    CopyName,
    CopyConnectionString,
    Edit,
    Duplicate,
    Delete,
}

pub(crate) struct ConnectionRowRender {
    pub(crate) is_open: bool,
    pub(crate) actions: Vec<ConnectionRowAction>,
}

pub(crate) struct ConnectionRowContext<'a> {
    pub(crate) theme: DbProTheme,
    pub(crate) connection: &'a UiConnectionSummary,
    pub(crate) is_connected: bool,
    pub(crate) is_active: bool,
    pub(crate) is_connecting: bool,
    pub(crate) is_failed: bool,
    pub(crate) modifier: &'static str,
}

impl ConnectionRowContext<'_> {
    pub(crate) fn draw(&self, ui: &mut egui::Ui) -> ConnectionRowRender {
        let connection = self.connection;
        let id = ui.make_persistent_id(("codex_conn_node", &connection.id));
        let mut collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            self.is_connected || self.is_connecting || self.is_failed,
        );
        let is_open = collapsing.is_open();
        let (response, chevron_clicked) = self.draw_row(ui, is_open);
        let is_context_menu = is_context_menu_triggered(&response, ui);
        let mut actions = connection_context_menu(ui, &response, self.is_connected, self.theme, self.modifier);
        self.apply_row_interaction(
            ui,
            &mut collapsing,
            is_open,
            chevron_clicked,
            response.clicked() && !is_context_menu,
            &mut actions,
        );

        ConnectionRowRender {
            is_open: collapsing.is_open(),
            actions,
        }
    }

    fn draw_row(&self, ui: &mut egui::Ui, is_open: bool) -> (egui::Response, bool) {
        draw_codex_tree_row(
            ui,
            &self.theme,
            CodexTreeRow {
                depth: 0,
                is_expandable: true,
                is_expanded: is_open,
                icon: Icon::Database,
                icon_color: self.icon_color(),
                label: &self.connection.name,
                is_selected: self.is_connected,
                is_dimmed: !self.is_connected && !self.is_failed && !self.is_connecting,
                status_dot: Some(self.status_dot()),
                badge_text: Some(self.badge_text()),
                badge_accent: self.is_connected || self.is_failed,
                count_text: None,
                detail_text: None,
            },
        )
    }

    fn apply_row_interaction(
        &self,
        ui: &egui::Ui,
        collapsing: &mut egui::collapsing_header::CollapsingState,
        is_open: bool,
        chevron_clicked: bool,
        clicked: bool,
        actions: &mut Vec<ConnectionRowAction>,
    ) {
        if chevron_clicked {
            collapsing.set_open(!is_open);
            collapsing.store(ui.ctx());
        } else if clicked {
            if self.is_connected && self.is_active {
                collapsing.set_open(!is_open);
            } else {
                // Not the focused connection (possibly live behind a bound tab):
                // clicking makes it the active connection, even while its session lives.
                if !self.is_connecting {
                    actions.push(ConnectionRowAction::Connect);
                }
                collapsing.set_open(true);
            }
            collapsing.store(ui.ctx());
        }
    }

    fn status_dot(&self) -> egui::Color32 {
        if self.is_connected {
            self.theme.success
        } else if self.is_connecting {
            self.theme.accent
        } else if self.is_failed {
            self.theme.danger
        } else {
            self.theme.text_muted
        }
    }

    fn icon_color(&self) -> egui::Color32 {
        if self.is_connected {
            self.theme.accent
        } else if self.is_failed {
            self.theme.danger
        } else {
            self.theme.text_muted
        }
    }

    fn badge_text(&self) -> &'static str {
        if self.is_failed {
            "ERR"
        } else if self.connection.driver.eq_ignore_ascii_case("postgresql") {
            "PG"
        } else {
            "SQLITE"
        }
    }
}

fn connection_context_menu(
    ui: &mut egui::Ui,
    response: &egui::Response,
    is_connected: bool,
    theme: DbProTheme,
    modifier: &str,
) -> Vec<ConnectionRowAction> {
    let mut actions = Vec::new();
    context_action_menu(ui, response, theme, |ui, close_menu| {
        if is_connected {
            if ctx_menu_item(ui, Some(Icon::Unplug), "Disconnect", None, theme.text_primary, theme).clicked() {
                actions.push(ConnectionRowAction::Disconnect);
                *close_menu = true;
            }
            if ctx_menu_item(ui, Some(Icon::RefreshCw), "Reconnect", None, theme.text_primary, theme).clicked() {
                actions.push(ConnectionRowAction::Reconnect);
                *close_menu = true;
            }
            if ctx_menu_item(
                ui,
                Some(Icon::RotateCcw),
                "Refresh Schema",
                Some("F5"),
                theme.text_primary,
                theme,
            )
            .clicked()
            {
                actions.push(ConnectionRowAction::RefreshSchema);
                *close_menu = true;
            }
        } else if ctx_menu_item(ui, Some(Icon::Plug), "Connect", None, theme.text_primary, theme).clicked() {
            actions.push(ConnectionRowAction::Connect);
            *close_menu = true;
        }

        Separator::horizontal(theme).show(ui);
        let new_script_shortcut = format!("{modifier}T");
        if ctx_menu_item(
            ui,
            Some(Icon::FileCode2),
            "SQL Editor / New Script",
            Some(&new_script_shortcut),
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::NewScript);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::Workflow),
            "View ER Diagram",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::OpenErDiagram);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::Bot),
            "Ask AI Agent about Database",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::AskAgent);
            *close_menu = true;
        }

        Separator::horizontal(theme).show(ui);
        if ctx_menu_item(
            ui,
            Some(Icon::Plus),
            "Open sample CREATE TABLE SQL…",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::CreateTable);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::Copy),
            "Copy Connection Name",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::CopyName);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::Link),
            "Copy Connection String",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::CopyConnectionString);
            *close_menu = true;
        }

        Separator::horizontal(theme).show(ui);
        if ctx_menu_item(
            ui,
            Some(Icon::Pencil),
            "Edit Connection…",
            Some("F4"),
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::Edit);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::CopyPlus),
            "Duplicate Connection",
            None,
            theme.text_primary,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::Duplicate);
            *close_menu = true;
        }
        if ctx_menu_item(
            ui,
            Some(Icon::Trash2),
            "Delete Connection",
            Some("Del"),
            theme.danger,
            theme,
        )
        .clicked()
        {
            actions.push(ConnectionRowAction::Delete);
            *close_menu = true;
        }
    });
    actions
}
