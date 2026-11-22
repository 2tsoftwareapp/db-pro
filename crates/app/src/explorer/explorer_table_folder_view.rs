//! Presentation boundary for the Tables folder in the explorer.

use super::explorer_tree::{draw_codex_tree_row, draw_hint_row, CodexTreeRow};
use super::{context_action_menu, ctx_menu_item, is_context_menu_triggered, DbProTheme, EXPLORER_ROW_HEIGHT};
use crate::components::Separator;
use eframe::egui;
use lucide_icons::Icon;

pub(crate) struct TableFolderContext<'a> {
    pub(crate) theme: DbProTheme,
    pub(crate) schema: &'a str,
    pub(crate) total_tables: usize,
    pub(crate) matching_tables: usize,
    pub(crate) search_query: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableFolderAction {
    RefreshSchema,
    CopySchemaName,
}

pub(crate) struct TableFolderRender {
    pub(crate) is_open: bool,
    pub(crate) actions: Vec<TableFolderAction>,
}

impl TableFolderContext<'_> {
    pub(crate) fn draw_header(&self, ui: &mut egui::Ui) -> TableFolderRender {
        let folder_id = ui.make_persistent_id(("codex_tbl_folder", self.schema));
        let default_open = !self.search_query.is_empty();
        let mut collapsing =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), folder_id, default_open);
        if default_open && !collapsing.is_open() {
            collapsing.set_open(true);
            collapsing.store(ui.ctx());
        }
        let is_open = collapsing.is_open();
        let (response, chevron_clicked) = self.draw_row(ui, is_open);
        let is_context_menu = is_context_menu_triggered(&response, ui);
        let actions = self.draw_menu(ui, &response);
        if (response.clicked() && !is_context_menu) || chevron_clicked {
            collapsing.set_open(!is_open);
            collapsing.store(ui.ctx());
        }
        TableFolderRender {
            is_open: collapsing.is_open(),
            actions,
        }
    }

    fn draw_menu(&self, ui: &mut egui::Ui, response: &egui::Response) -> Vec<TableFolderAction> {
        let mut actions = Vec::new();
        let theme = self.theme;
        context_action_menu(ui, response, theme, |ui, close_menu| {
            let refresh_label = t!("explorer.refresh_schema");
            if ctx_menu_item(ui, Some(Icon::RefreshCw), &refresh_label, None, theme.text_primary, theme).clicked()
            {
                actions.push(TableFolderAction::RefreshSchema);
                *close_menu = true;
            }
            Separator::horizontal(theme).show(ui);
            let copy_label = t!("explorer.copy_schema_name");
            if ctx_menu_item(ui, Some(Icon::Copy), &copy_label, None, theme.text_primary, theme).clicked() {
                actions.push(TableFolderAction::CopySchemaName);
                *close_menu = true;
            }
        });
        actions
    }

    pub(crate) fn draw_empty_state(&self, ui: &mut egui::Ui) {
        let label = if self.total_tables == 0 {
            t!("explorer.no_tables")
        } else {
            t!("explorer.no_matching_tables")
        };
        draw_hint_row(ui, &self.theme, 4, Icon::Info, &label);
    }

    pub(crate) fn draw_offscreen_row_spacer(&self, ui: &mut egui::Ui) {
        let _ = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), EXPLORER_ROW_HEIGHT),
            egui::Sense::hover(),
        );
    }

    pub(crate) fn draw_overflow_hint(&self, ui: &mut egui::Ui, visible_count: usize) {
        if self.matching_tables > visible_count {
            draw_hint_row(
                ui,
                &self.theme,
                4,
                Icon::Ellipsis,
                &t!(
                    "explorer.showing_filtered",
                    shown = visible_count,
                    total = self.matching_tables
                ),
            );
        }
    }

    fn draw_row(&self, ui: &mut egui::Ui, is_open: bool) -> (egui::Response, bool) {
        draw_codex_tree_row(
            ui,
            &self.theme,
            CodexTreeRow {
                depth: 3,
                is_expandable: true,
                is_expanded: is_open,
                icon: Icon::Table2,
                icon_color: self.theme.info,
                label: "Tables",
                is_selected: false,
                is_dimmed: self.total_tables == 0,
                status_dot: None,
                badge_text: None,
                badge_accent: false,
                count_text: Some(self.count_label()),
                detail_text: None,
            },
        )
    }

    fn count_label(&self) -> String {
        if self.search_query.is_empty() {
            self.total_tables.to_string()
        } else {
            format!("{}/{}", self.matching_tables, self.total_tables)
        }
    }
}
