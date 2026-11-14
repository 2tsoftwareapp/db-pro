//! Logical-replication presentation and typed user intents.
use super::super::replication_state::ReplicationState;
use super::super::*;
use crate::components::dialog::Dialog;

/// Maximum rows painted per replication list (#569/#583).
const REPLICATION_ROW_CAP: usize = 40;

/// "Showing 40 of 213" hint above a truncated list (#569).
fn draw_shown_total(ui: &mut egui::Ui, theme: DbProTheme, cap_then_total: (usize, usize)) {
    ui.label(
        RichText::new(format_shown_total(cap_then_total.0, cap_then_total.1))
            .small()
            .color(theme.text_muted),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReplicationSurfaceAction {
    Refresh,
    PreviewDropPublication(String),
    RequestDropPublication(String),
    PreviewDropSubscription(String),
    RequestDropSubscription(String),
    PreviewCreatePublication(String),
    CreatePublication,
    ConfirmCreatePublication,
    CancelCreatePublication,
    ClosePreview,
    ConfirmDropPublication(String),
    ConfirmDropSubscription(String),
    CancelDropPublication,
    CancelDropSubscription,
}

pub(super) struct ReplicationSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) state: &'a mut ReplicationState,
}

impl ReplicationSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<ReplicationSurfaceAction> {
        let mut actions = Vec::new();
        self.draw_header(ui, &mut actions);
        self.draw_inventory(ui, &mut actions);
        self.draw_create_form(ui, &mut actions);
        self.draw_preview(ui, &mut actions);
        self.draw_create_confirmation(ui, &mut actions);
        self.draw_drop_confirmations(ui, &mut actions);
        actions
    }

    fn draw_header(&self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        ui.add_space(SPACE_MD);
        section_label(ui, "LOGICAL REPLICATION", self.theme);
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "PostgreSQL-only · subscription conninfo redacted · CREATE SUBSCRIPTION not offered (secrets)",
            )
            .small()
            .color(self.theme.text_muted),
        );
        if Button::new(self.theme)
            .icon(Icon::RefreshCw)
            .text("Load replication inventory")
            .variant(ButtonVariant::Secondary)
            .show(ui)
            .clicked()
        {
            actions.push(ReplicationSurfaceAction::Refresh);
        }
        if let Some(error) = &self.state.replication_error {
            ui.colored_label(self.theme.danger, error);
        }
    }

    fn draw_inventory(&self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        let Some(inventory) = self.state.replication_inventory.as_ref() else {
            return;
        };
        ui.label(
            RichText::new(&inventory.message)
                .small()
                .color(self.theme.text_secondary),
        );
        self.draw_publications(ui, &inventory.publications, actions);
        self.draw_subscriptions(ui, &inventory.subscriptions, actions);
        self.draw_slots(ui, &inventory.slots);
    }

    fn draw_publications(
        &self,
        ui: &mut egui::Ui,
        publications: &[db_pro_core::domain::replication::PublicationInfo],
        actions: &mut Vec<ReplicationSurfaceAction>,
    ) {
        if publications.len() > REPLICATION_ROW_CAP {
            draw_shown_total(ui, self.theme, (REPLICATION_ROW_CAP, publications.len()));
        }
        for publication in publications.iter().take(REPLICATION_ROW_CAP) {
            let name = publication.name.clone();
            Card::new(self.theme).frame().show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "publication {} · all_tables={} · owner={}",
                        publication.name,
                        publication.all_tables,
                        fmt_opt(publication.owner.as_deref())
                    ))
                    .strong()
                    .monospace(),
                );
                if !publication.tables.is_empty() {
                    ui.label(
                        RichText::new(format!("tables: {}", publication.tables.join(", ")))
                            .small()
                            .color(self.theme.text_muted),
                    );
                }
                ui.horizontal(|ui| {
                    if Button::new(self.theme)
                        .icon(Icon::FileCode2)
                        .text("Preview DROP")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::PreviewDropPublication(name.clone()));
                    }
                    if Button::new(self.theme)
                        .text("Drop…")
                        .variant(ButtonVariant::Destructive)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::RequestDropPublication(name.clone()));
                    }
                });
            });
            ui.add_space(SPACE_XS);
        }
    }

    fn draw_subscriptions(
        &self,
        ui: &mut egui::Ui,
        subscriptions: &[db_pro_core::domain::replication::SubscriptionInfo],
        actions: &mut Vec<ReplicationSurfaceAction>,
    ) {
        for subscription in subscriptions.iter().take(REPLICATION_ROW_CAP) {
            let name = subscription.name.clone();
            Card::new(self.theme).frame().show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "subscription {} · enabled={} · slot={}",
                        subscription.name,
                        subscription.enabled,
                        fmt_opt(subscription.slot_name.as_deref())
                    ))
                    .strong()
                    .monospace(),
                );
                ui.label(
                    RichText::new(format!(
                        "pubs={} · conninfo={}",
                        subscription.publications.join(","),
                        subscription.conninfo_redacted
                    ))
                    .small()
                    .color(self.theme.text_muted),
                );
                ui.horizontal(|ui| {
                    if Button::new(self.theme)
                        .icon(Icon::FileCode2)
                        .text("Preview DROP")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::PreviewDropSubscription(name.clone()));
                    }
                    if Button::new(self.theme)
                        .text("Drop…")
                        .variant(ButtonVariant::Destructive)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::RequestDropSubscription(name.clone()));
                    }
                });
            });
            ui.add_space(SPACE_XS);
        }
    }

    fn draw_slots(&self, ui: &mut egui::Ui, slots: &[db_pro_core::domain::replication::ReplicationSlotInfo]) {
        if slots.len() > REPLICATION_ROW_CAP {
            draw_shown_total(ui, self.theme, (REPLICATION_ROW_CAP, slots.len()));
        }
        for slot in slots.iter().take(REPLICATION_ROW_CAP) {
            ui.label(
                RichText::new(format!(
                    "slot {} · type={} · active={} · restart={}",
                    slot.slot_name,
                    fmt_opt(slot.slot_type.as_deref()),
                    slot.active,
                    fmt_opt(slot.restart_lsn.as_deref())
                ))
                .small()
                .monospace()
                .color(self.theme.text_secondary),
            );
        }
    }

    fn draw_create_form(&mut self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        ui.add_space(SPACE_SM);
        ui.label(RichText::new("Create publication (FOR ALL TABLES)").small().strong());
        ui.horizontal(|ui| {
            Input::new(&mut self.state.replication_create_name, "publication name", self.theme)
                .width(160.0)
                .show(ui);
            if Button::new(self.theme)
                .icon(Icon::FileCode2)
                .text("Preview CREATE")
                .variant(ButtonVariant::Ghost)
                .show(ui)
                .clicked()
            {
                actions.push(ReplicationSurfaceAction::PreviewCreatePublication(
                    self.state.replication_create_name.clone(),
                ));
            }
            if Button::new(self.theme)
                .text("Create (confirm)")
                .variant(ButtonVariant::Secondary)
                .show(ui)
                .clicked()
            {
                actions.push(ReplicationSurfaceAction::CreatePublication);
            }
        });
    }

    fn draw_preview(&self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        let Some(preview) = self.state.replication_ddl_preview.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "Replication DDL preview", self.theme)
            .width(560.0)
            .id_salt("replication_preview_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(RichText::new(preview).monospace());
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Close")
                        .variant(ButtonVariant::Secondary)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::ClosePreview);
                    }
                });
            });
        if !open {
            actions.push(ReplicationSurfaceAction::ClosePreview);
        }
    }

    fn draw_create_confirmation(&self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        let Some(pending) = self.state.replication_create_confirm.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, format!("Create publication `{}`?", pending.name), self.theme)
            .width(560.0)
            .id_salt("replication_create_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(
                        RichText::new("FOR ALL TABLES publishes every current and future table.")
                            .small()
                            .color(self.theme.warning),
                    );
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
                        actions.push(ReplicationSurfaceAction::ConfirmCreatePublication);
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Secondary)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(ReplicationSurfaceAction::CancelCreatePublication);
                    }
                });
            });
        if !open {
            actions.push(ReplicationSurfaceAction::CancelCreatePublication);
        }
    }

    fn draw_drop_confirmations(&self, ui: &mut egui::Ui, actions: &mut Vec<ReplicationSurfaceAction>) {
        if let Some(name) = self.state.replication_drop_publication.as_ref() {
            let mut open = true;
            Dialog::new(&mut open, "Drop publication?", self.theme)
                .width(440.0)
                .id_salt("replication_drop_publication_dialog")
                .show_framed_ctx(ui.ctx(), |frame| {
                    frame.body(|ui| {
                        ui.label(format!("Drop publication `{name}`?"));
                    });
                    frame.footer(|ui| {
                        if Button::new(self.theme)
                            .text("Drop")
                            .variant(ButtonVariant::Destructive)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(ReplicationSurfaceAction::ConfirmDropPublication(name.clone()));
                        }
                        if Button::new(self.theme)
                            .text("Cancel")
                            .variant(ButtonVariant::Secondary)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(ReplicationSurfaceAction::CancelDropPublication);
                        }
                    });
                });
            if !open {
                actions.push(ReplicationSurfaceAction::CancelDropPublication);
            }
        }
        if let Some(name) = self.state.replication_drop_subscription.as_ref() {
            let mut open = true;
            Dialog::new(&mut open, "Drop subscription?", self.theme)
                .width(500.0)
                .id_salt("replication_drop_subscription_dialog")
                .show_framed_ctx(ui.ctx(), |frame| {
                    frame.body(|ui| {
                        ui.label(format!(
                            "Drop subscription `{name}`? Conninfo is never shown or logged."
                        ));
                    });
                    frame.footer(|ui| {
                        if Button::new(self.theme)
                            .text("Drop")
                            .variant(ButtonVariant::Destructive)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(ReplicationSurfaceAction::ConfirmDropSubscription(name.clone()));
                        }
                        if Button::new(self.theme)
                            .text("Cancel")
                            .variant(ButtonVariant::Secondary)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(ReplicationSurfaceAction::CancelDropSubscription);
                        }
                    });
                });
            if !open {
                actions.push(ReplicationSurfaceAction::CancelDropSubscription);
            }
        }
    }
}
