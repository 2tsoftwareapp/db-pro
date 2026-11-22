//! Presentation and intent mapping for one connection node in the Explorer.

use super::explorer_connection_row_view::{ConnectionRowAction, ConnectionRowContext};
use super::explorer_schema_tree_view::{ExplorerSchemaTreeAction, ExplorerSchemaTreeModel, ExplorerSchemaTreeView};
use super::explorer_tree::draw_hint_row;
use super::{DbProTheme, SchemaExplorerState, UiConnectionSummary};
use eframe::egui;
use lucide_icons::Icon;

pub(super) enum ExplorerConnectionNodeAction {
    Connection(ConnectionRowAction),
    Schema(ExplorerSchemaTreeAction),
}

pub(super) struct ExplorerConnectionNodeModel {
    pub(super) is_connected: bool,
    pub(super) is_active: bool,
    pub(super) is_connecting: bool,
    pub(super) is_failed: bool,
    pub(super) error: Option<String>,
}

pub(super) struct ExplorerConnectionNodeView<'a> {
    theme: DbProTheme,
    connection: &'a UiConnectionSummary,
    model: ExplorerConnectionNodeModel,
    modifier: &'static str,
}

impl<'a> ExplorerConnectionNodeView<'a> {
    pub(super) fn new(
        theme: DbProTheme,
        connection: &'a UiConnectionSummary,
        model: ExplorerConnectionNodeModel,
        modifier: &'static str,
    ) -> Self {
        Self {
            theme,
            connection,
            model,
            modifier,
        }
    }

    pub(super) fn draw(
        &self,
        ui: &mut egui::Ui,
        schema: Option<(&mut SchemaExplorerState, ExplorerSchemaTreeModel)>,
    ) -> Vec<ExplorerConnectionNodeAction> {
        let render = ConnectionRowContext {
            theme: self.theme,
            connection: self.connection,
            is_connected: self.model.is_connected,
            is_active: self.model.is_active,
            is_connecting: self.model.is_connecting,
            is_failed: self.model.is_failed,
            modifier: self.modifier,
        }
        .draw(ui);
        let mut actions = render
            .actions
            .into_iter()
            .map(ExplorerConnectionNodeAction::Connection)
            .collect::<Vec<_>>();

        if render.is_open {
            if self.model.is_connected {
                if let Some((explorer, model)) = schema {
                    actions.extend(
                        ExplorerSchemaTreeView::new(self.theme, explorer, model)
                            .draw(ui)
                            .into_iter()
                            .map(ExplorerConnectionNodeAction::Schema),
                    );
                } else if !self.model.is_active {
                    let hint = draw_hint_row(ui, &self.theme, 1, Icon::CircleCheck, "Connected — click to make active");
                    if hint.clicked() {
                        actions.push(ExplorerConnectionNodeAction::Connection(ConnectionRowAction::Connect));
                    }
                }
            } else if self.model.is_failed {
                let error = self.model.error.as_deref().unwrap_or("Connection failed");
                let hint = format!("Failed: {error} — Click to retry");
                if draw_hint_row(ui, &self.theme, 1, Icon::AlertCircle, &hint).clicked() {
                    actions.push(ExplorerConnectionNodeAction::Connection(ConnectionRowAction::Connect));
                }
            } else if self.model.is_connecting {
                draw_hint_row(ui, &self.theme, 1, Icon::LoaderCircle, "Connecting…");
            } else if draw_hint_row(ui, &self.theme, 1, Icon::Circle, "Disconnected — click to connect").clicked() {
                actions.push(ExplorerConnectionNodeAction::Connection(ConnectionRowAction::Connect));
            }
        }
        actions
    }
}
