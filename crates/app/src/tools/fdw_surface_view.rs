// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Foreign-data-wrapper presentation and typed user intents.
use super::super::fdw_state::FdwState;
use super::super::*;

/// Row caps for the FDW inventory lists (#569/#583).
const FDW_WRAPPER_CAP: usize = 20;
const FDW_SERVER_CAP: usize = 30;
const FDW_MAPPING_CAP: usize = 30;
const FDW_TABLE_CAP: usize = 40;


#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FdwSurfaceAction {
    Refresh,
    PreviewDropServer(String),
    RequestDropServer(String),
    PreviewCreate {
        name: String,
        wrapper: String,
        host: String,
        dbname: String,
        port: String,
    },
    Create,
    ConfirmCreate,
    CancelCreate,
    ClosePreview,
    ConfirmDropServer {
        name: String,
        cascade: bool,
    },
    CancelDrop,
}

pub(super) struct FdwSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) state: &'a mut FdwState,
}

impl FdwSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<FdwSurfaceAction> {
        let mut actions = Vec::new();
        self.draw_header(ui, &mut actions);
        self.draw_inventory(ui, &mut actions);
        self.draw_create_form(ui, &mut actions);
        self.draw_preview(ui, &mut actions);
        self.draw_drop_confirmation(ui, &mut actions);
        self.draw_create_confirmation(ui, &mut actions);
        actions
    }

    fn draw_header(&self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("FOREIGN DATA (FDW)")
                .font(font_caption())
                .strong()
                .color(self.theme.text_secondary),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new("PostgreSQL-only · passwords/options redacted · CREATE EXTENSION never auto-run")
                .small()
                .color(self.theme.text_muted),
        );
        if Button::new(self.theme)
            .icon(Icon::RefreshCw)
            .text("Load FDW inventory")
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            actions.push(FdwSurfaceAction::Refresh);
        }
        if let Some(error) = &self.state.fdw_error {
            ui.colored_label(self.theme.danger, error);
        }
    }

    fn draw_inventory(&self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        let Some(inventory) = self.state.fdw_inventory.as_ref() else {
            return;
        };
        ui.label(
            RichText::new(&inventory.message)
                .small()
                .color(self.theme.text_secondary),
        );
        if let Some(hint) = &inventory.extension_hint {
            ui.colored_label(self.theme.warning, hint);
        }
        let shown_hint = shown_total_hint(
            FDW_WRAPPER_CAP,
            inventory.wrappers.len(),
            FDW_SERVER_CAP,
            inventory.servers.len(),
        );
        if let Some(hint) = shown_hint {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        for wrapper in inventory.wrappers.iter().take(FDW_WRAPPER_CAP) {
            ui.label(
                RichText::new(format!(
                    "wrapper {} · handler={}",
                    wrapper.name,
                    fmt_opt(wrapper.handler.as_deref())
                ))
                    .small()
                    .monospace(),
            );
        }
        for server in inventory.servers.iter().take(FDW_SERVER_CAP) {
            self.draw_server(ui, server, actions);
        }
        for mapping in inventory.user_mappings.iter().take(FDW_MAPPING_CAP) {
            let options = mapping
                .options
                .iter()
                .map(|option| format!("{}={}", option.key, option.display_value()))
                .collect::<Vec<_>>()
                .join(", ");
            ui.label(
                RichText::new(format!(
                    "mapping {}@{} · {}",
                    mapping.user_name, mapping.server_name, options
                ))
                .small()
                .monospace()
                .color(self.theme.text_secondary),
            );
        }
        for table in inventory.foreign_tables.iter().take(FDW_TABLE_CAP) {
            ui.label(
                RichText::new(format!(
                    "foreign {}.{} → {}",
                    table.schema, table.name, table.server_name
                ))
                .small()
                .monospace(),
            );
        }
    }

    fn draw_server(
        &self,
        ui: &mut egui::Ui,
        server: &db_pro_core::domain::fdw::ForeignServer,
        actions: &mut Vec<FdwSurfaceAction>,
    ) {
        let server_name = server.name.clone();
        let options = server
            .options
            .iter()
            .map(|option| format!("{}={}", option.key, option.display_value()))
            .collect::<Vec<_>>()
            .join(", ");
        Card::new(self.theme).frame().show(ui, |ui| {
            ui.label(
                RichText::new(format!("server {} · fdw={}", server.name, server.fdw_name))
                    .strong()
                    .monospace(),
            );
            if !options.is_empty() {
                ui.label(RichText::new(options).small().color(self.theme.text_muted));
            }
            ui.horizontal(|ui| {
                if Button::new(self.theme)
                    .icon(Icon::FileCode2)
                    .text("Preview DROP")
                    .variant(ButtonVariant::Ghost)
                    .show(ui)
                    .clicked()
                {
                    actions.push(FdwSurfaceAction::PreviewDropServer(server_name.clone()));
                }
                if Button::new(self.theme)
                    .text("Drop…")
                    .variant(ButtonVariant::Destructive)
                    .show(ui)
                    .clicked()
                {
                    actions.push(FdwSurfaceAction::RequestDropServer(server_name.clone()));
                }
            });
        });
        ui.add_space(SPACE_XS);
    }

    fn draw_create_form(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        ui.add_space(SPACE_SM);
        ui.label(RichText::new("Create foreign server").small().strong());
        ui.horizontal(|ui| {
            Input::new(&mut self.state.fdw_create_name, "server name", self.theme)
                .width(130.0)
                .show(ui);
            Input::new(&mut self.state.fdw_create_wrapper, "fdw", self.theme)
                .width(90.0)
                .show(ui);
        });
        ui.horizontal(|ui| {
            Input::new(&mut self.state.fdw_create_host, "host", self.theme)
                .width(110.0)
                .show(ui);
            Input::new(&mut self.state.fdw_create_dbname, "dbname", self.theme)
                .width(110.0)
                .show(ui);
            Input::new(&mut self.state.fdw_create_port, "port", self.theme)
                .width(56.0)
                .show(ui);
        });
        ui.horizontal(|ui| {
            if Button::new(self.theme)
                .icon(Icon::FileCode2)
                .text("Preview CREATE")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(FdwSurfaceAction::PreviewCreate {
                    name: self.state.fdw_create_name.clone(),
                    wrapper: self.state.fdw_create_wrapper.clone(),
                    host: self.state.fdw_create_host.clone(),
                    dbname: self.state.fdw_create_dbname.clone(),
                    port: self.state.fdw_create_port.clone(),
                });
            }
            if Button::new(self.theme)
                .text("Create (confirm)")
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(FdwSurfaceAction::Create);
            }
        });
    }

    fn draw_preview(&self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        let Some(preview) = self.state.fdw_ddl_preview.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "FDW DDL preview", self.theme)
            .width(520.0)
            .id_salt("fdw_preview_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(RichText::new(preview).monospace());
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Close")
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(FdwSurfaceAction::ClosePreview);
                    }
                });
            });
        if !open {
            actions.push(FdwSurfaceAction::ClosePreview);
        }
    }

    fn draw_create_confirmation(&self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        let Some(pending) = self.state.fdw_create_confirm.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(
            &mut open,
            format!("Create foreign server `{}`?", pending.name),
            self.theme,
        )
        .width(520.0)
        .id_salt("fdw_create_dialog")
        .show_framed_ctx(ui.ctx(), |frame| {
            frame.body(|ui| {
                ui.label(RichText::new(&pending.sql).monospace());
            });
            frame.footer(|ui| {
                if Button::new(self.theme)
                    .text("Create")
                    .variant(ButtonVariant::Default)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(FdwSurfaceAction::ConfirmCreate);
                }
                if Button::new(self.theme)
                    .text("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(FdwSurfaceAction::CancelCreate);
                }
            });
        });
        if !open {
            actions.push(FdwSurfaceAction::CancelCreate);
        }
    }

    fn draw_drop_confirmation(&self, ui: &mut egui::Ui, actions: &mut Vec<FdwSurfaceAction>) {
        let Some(name) = self.state.fdw_drop_confirm.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "Drop foreign server?", self.theme)
            .width(500.0)
            .id_salt("fdw_drop_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(format!(
                        "Drop server `{name}` CASCADE? This removes dependent foreign tables/mappings."
                    ));
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Drop CASCADE")
                        .variant(ButtonVariant::Destructive)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(FdwSurfaceAction::ConfirmDropServer {
                            name: name.clone(),
                            cascade: true,
                        });
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(FdwSurfaceAction::CancelDrop);
                    }
                });
            });
        if !open {
            actions.push(FdwSurfaceAction::CancelDrop);
        }
    }
}

/// Truncation hint for the two capped FDW lists (#569).
fn shown_total_hint(
    wrapper_cap: usize,
    wrappers: usize,
    server_cap: usize,
    servers: usize,
) -> Option<String> {
    if wrappers <= wrapper_cap && servers <= server_cap {
        return None;
    }
    Some(format!(
        "{} · {}",
        format_shown_total(wrappers.min(wrapper_cap), wrappers),
        format_shown_total(servers.min(server_cap), servers)
    ))
}
