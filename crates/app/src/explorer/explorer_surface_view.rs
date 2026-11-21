// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Presentation boundary for the Explorer surface.
//!
//! This view owns toolbar, empty-state, scroll-container and connection-node
//! composition. The root receives only typed actions that cross feature
//! boundaries.

use super::explorer_connection_node_view::{
    ExplorerConnectionNodeAction, ExplorerConnectionNodeModel, ExplorerConnectionNodeView,
};
use super::explorer_connection_row_view::ConnectionRowAction;
use super::explorer_schema_tree_view::{ExplorerSchemaTreeAction, ExplorerSchemaTreeModel};
use super::explorer_toolbar_view::{ExplorerToolbarAction, ExplorerToolbarContext};
use super::{
    Button, ButtonSize, ButtonVariant, ConnectionCatalogState, ConnectionLifecycleState, DbProTheme, Icon,
    SchemaExplorerState, UiConnectionSummary, UiTableInfo,
};
use crate::components::ScrollArea;
use eframe::egui;
use crate::tokens::{SPACE_SM, SPACE_XS};

const EXPLORER_SCROLL_ID: &str = "codex_navigator_scroll";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CatalogSurface {
    Loading,
    Failed,
    Empty,
    Ready,
}

/// Empty catalog is not the same as a list that has not arrived or has failed.
pub(super) fn catalog_surface(pending: bool, error: Option<&str>, is_empty: bool) -> CatalogSurface {
    if !is_empty {
        return CatalogSurface::Ready;
    }
    if pending {
        return CatalogSurface::Loading;
    }
    if error.is_some() {
        return CatalogSurface::Failed;
    }
    CatalogSurface::Empty
}

pub(super) enum ExplorerSurfaceAction {
    NewConnection,
    /// Retries the connection-catalog load after `CatalogSurface::Failed`.
    RetryConnections,
    RefreshSchema,
    Connection {
        connection: UiConnectionSummary,
        action: ConnectionRowAction,
        is_connected: bool,
    },
    Schema {
        connection_id: String,
        action: ExplorerSchemaTreeAction,
    },
}

pub(super) struct ExplorerSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) catalog: &'a ConnectionCatalogState,
    pub(super) lifecycle: &'a ConnectionLifecycleState,
    pub(super) explorer: &'a mut SchemaExplorerState,
    pub(super) active_schema: String,
    pub(super) table_info: Option<UiTableInfo>,
    pub(super) reduce_motion: bool,
    pub(super) functions_enabled: bool,
    pub(super) trigger_toggle_unavailable_reason: Option<String>,
    pub(super) modifier: &'static str,
}

impl ExplorerSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<ExplorerSurfaceAction> {
        let mut actions = Vec::new();
        if !self.catalog.is_empty() {
            actions.extend(self.draw_toolbar(ui));
            ui.add_space(SPACE_SM);
            ui.horizontal(|ui| {
                ui.add_space(SPACE_XS);
                ui.label(
                    egui::RichText::new("CONNECTIONS")
                        .small()
                        .strong()
                        .color(self.theme.text_tertiary),
                );
            });
            ui.add_space(SPACE_XS);
        }

        let tree_width = ui
            .max_rect()
            .width()
            .max(ui.available_width())
            .min(ui.clip_rect().width());
        let scroll_h = ui.available_height();
        ScrollArea::new(self.theme)
            .id_salt(EXPLORER_SCROLL_ID)
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                ui.set_min_width(tree_width);
                ui.set_max_width(tree_width);
                ui.expand_to_include_x(ui.max_rect().left() + tree_width);
                ui.allocate_exact_size(egui::vec2(tree_width, 0.0), egui::Sense::hover());
                let pending = self.lifecycle.connections_request_pending();
                let error = self.lifecycle.connections_error().map(str::to_owned);
                match catalog_surface(pending, error.as_deref(), self.catalog.is_empty()) {
                    CatalogSurface::Ready => actions.extend(self.draw_connections(ui)),
                    CatalogSurface::Loading => {
                        ui.label(egui::RichText::new("Loading connections…").color(self.theme.text_secondary));
                    }
                    CatalogSurface::Failed => {
                        let message = error.as_deref().unwrap_or("Could not load connections");
                        // Error card with retry + CTA — a bare red label left
                        // the sidebar in a dead end with no toolbar to retry.
                        crate::components::card::Card::new(self.theme).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(char::from(Icon::TriangleAlert).to_string())
                                        .color(self.theme.danger),
                                );
                                ui.label(
                                    egui::RichText::new("Could not load connections")
                                        .strong()
                                        .color(self.theme.danger),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if Button::new(self.theme)
                                        .icon(Icon::RotateCcw)
                                        .text("Retry")
                                        .variant(ButtonVariant::Secondary)
                                        .size(ButtonSize::Sm)
                                        .show(ui)
                                        .clicked()
                                    {
                                        actions.push(ExplorerSurfaceAction::RetryConnections);
                                    }
                                    if Button::new(self.theme)
                                        .icon(Icon::Plus)
                                        .text("New connection")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .show(ui)
                                        .clicked()
                                    {
                                        actions.push(ExplorerSurfaceAction::NewConnection);
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(message)
                                    .small()
                                    .color(self.theme.text_secondary),
                            );
                        });
                    }
                    CatalogSurface::Empty => actions.extend(self.draw_empty_state(ui)),
                }
            });
        actions
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) -> Vec<ExplorerSurfaceAction> {
        let toolbar_actions = ExplorerToolbarContext {
            theme: self.theme,
            search: &mut self.explorer.explorer_search,
            filter: &mut self.explorer.explorer_filter,
        }
        .draw_toolbar(ui);
        toolbar_actions
            .into_iter()
            .filter_map(|action| match action {
                ExplorerToolbarAction::RefreshSchema => Some(ExplorerSurfaceAction::RefreshSchema),
                ExplorerToolbarAction::NewConnection => None,
            })
            .collect()
    }

    fn draw_empty_state(&mut self, ui: &mut egui::Ui) -> Vec<ExplorerSurfaceAction> {
        let actions = ExplorerToolbarContext {
            theme: self.theme,
            search: &mut self.explorer.explorer_search,
            filter: &mut self.explorer.explorer_filter,
        }
        .draw_empty_state(ui);
        actions
            .into_iter()
            .filter_map(|action| match action {
                ExplorerToolbarAction::NewConnection => Some(ExplorerSurfaceAction::NewConnection),
                ExplorerToolbarAction::RefreshSchema => None,
            })
            .collect()
    }

    fn draw_connections(&mut self, ui: &mut egui::Ui) -> Vec<ExplorerSurfaceAction> {
        let mut actions = Vec::new();
        for connection in self.catalog.iter().cloned() {
            let is_active = self.lifecycle.active_connection_id() == Some(connection.id.as_str());
            let is_connected = (self.lifecycle.is_connected() && is_active)
                || self.lifecycle.has_live_session(&connection.id);
            let schema_model = (is_connected && is_active).then(|| self.schema_tree_model(&connection));
            let model = ExplorerConnectionNodeModel {
                is_connected,
                is_active,
                is_connecting: self.is_connection_connecting(&connection, is_active),
                is_failed: self.lifecycle.has_failed_connection(&connection.id),
                error: self.lifecycle.connection_error(&connection.id).map(str::to_owned),
            };
            let node_actions = ExplorerConnectionNodeView::new(self.theme, &connection, model, self.modifier)
                .draw(ui, schema_model.map(|model| (&mut *self.explorer, model)));
            for action in node_actions {
                match action {
                    ExplorerConnectionNodeAction::Connection(action) => {
                        actions.push(ExplorerSurfaceAction::Connection {
                            connection: connection.clone(),
                            action,
                            is_connected,
                        });
                    }
                    ExplorerConnectionNodeAction::Schema(action) => {
                        actions.push(ExplorerSurfaceAction::Schema {
                            connection_id: connection.id.clone(),
                            action,
                        });
                    }
                }
            }
            ui.add_space(2.0);
        }
        actions
    }

    fn is_connection_connecting(&self, connection: &UiConnectionSummary, is_active: bool) -> bool {
        self.lifecycle.pending_request().is_some()
            && (self.lifecycle.pending_connection_id() == Some(connection.id.as_str())
                || (self.lifecycle.pending_connection_id().is_none() && is_active))
    }

    fn schema_tree_model(&self, connection: &UiConnectionSummary) -> ExplorerSchemaTreeModel {
        ExplorerSchemaTreeModel {
            connection_id: connection.id.clone(),
            database: connection.database.clone(),
            driver: connection.driver.clone(),
            active_schema: self.active_schema.clone(),
            schema_error: self.explorer.schema_error.clone(),
            schema_loading: self.explorer.schema_request.is_some(),
            reduce_motion: self.reduce_motion,
            selected_table: self.explorer.selected_table.clone(),
            table_info: self.table_info.clone(),
            functions_enabled: self.functions_enabled,
            trigger_toggle_unavailable_reason: self.trigger_toggle_unavailable_reason.clone(),
        }
    }
}
