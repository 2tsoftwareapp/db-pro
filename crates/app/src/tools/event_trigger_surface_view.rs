// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! PostgreSQL event-trigger presentation and typed user intents.
use super::super::event_trigger_state::{EventTriggerApply, EventTriggerState};
use super::super::*;

/// Event-trigger rows painted per frame (#569).
const EVENT_TRIGGER_ROW_CAP: usize = 50;


#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EventTriggerSurfaceAction {
    Refresh,
    PreviewDrop(String),
    Alter {
        name: String,
        mode: String,
    },
    RequestDrop(String),
    PreviewCreate {
        name: String,
        event: String,
        function_ref: String,
        tags_csv: String,
    },
    Create,
    ConfirmApply,
    CancelApply,
    ClosePreview,
    ConfirmDrop(String),
    CancelDrop,
}

pub(super) struct EventTriggerSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) state: &'a mut EventTriggerState,
}

impl EventTriggerSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<EventTriggerSurfaceAction> {
        let mut actions = Vec::new();
        self.draw_header(ui, &mut actions);
        self.draw_inventory(ui, &mut actions);
        self.draw_create_form(ui, &mut actions);
        self.draw_preview(ui, &mut actions);
        self.draw_drop_confirmation(ui, &mut actions);
        self.draw_apply_confirmation(ui, &mut actions);
        actions
    }

    fn draw_header(&self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("EVENT TRIGGERS")
                .font(font_caption())
                .strong()
                .color(self.theme.text_secondary),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                "PostgreSQL-only · database-level DDL hooks · not table/row triggers · create requires existing function",
            )
            .small()
            .color(self.theme.text_muted),
        );
        if Button::new(self.theme)
            .icon(Icon::RefreshCw)
            .text("Load event triggers")
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            actions.push(EventTriggerSurfaceAction::Refresh);
        }
        if let Some(error) = &self.state.event_trigger_error {
            ui.colored_label(self.theme.danger, error);
        }
    }

    fn draw_inventory(&self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        let Some(inventory) = self.state.event_trigger_inventory.as_ref() else {
            return;
        };
        ui.label(
            RichText::new(&inventory.message)
                .small()
                .color(self.theme.text_secondary),
        );
        // #569: disclose the cap.
        if let Some(hint) =
            crate::components::common::shown_total_hint(EVENT_TRIGGER_ROW_CAP, inventory.triggers.len())
        {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        for trigger in inventory.triggers.iter().take(EVENT_TRIGGER_ROW_CAP) {
            self.draw_trigger(ui, trigger, actions);
        }
    }

    fn draw_trigger(
        &self,
        ui: &mut egui::Ui,
        trigger: &db_pro_core::domain::event_trigger::EventTriggerInfo,
        actions: &mut Vec<EventTriggerSurfaceAction>,
    ) {
        let trigger_name = trigger.name.clone();
        Card::new(self.theme).frame().show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "{} · on {} · {} · fn={}",
                    trigger.name, trigger.event, trigger.enabled_label, trigger.function_signature
                ))
                .strong()
                .monospace(),
            );
            if !trigger.tags.is_empty() {
                ui.label(
                    RichText::new(format!("tags: {}", trigger.tags.join(", ")))
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
                    actions.push(EventTriggerSurfaceAction::PreviewDrop(trigger_name.clone()));
                }
                if Button::new(self.theme)
                    .text("Disable")
                    .variant(ButtonVariant::Ghost)
                    .show(ui)
                    .clicked()
                {
                    actions.push(EventTriggerSurfaceAction::Alter {
                        name: trigger_name.clone(),
                        mode: "disable".to_owned(),
                    });
                }
                if Button::new(self.theme)
                    .text("Enable")
                    .variant(ButtonVariant::Ghost)
                    .show(ui)
                    .clicked()
                {
                    actions.push(EventTriggerSurfaceAction::Alter {
                        name: trigger_name.clone(),
                        mode: "enable".to_owned(),
                    });
                }
                if Button::new(self.theme)
                    .text("Drop…")
                    .variant(ButtonVariant::Destructive)
                    .show(ui)
                    .clicked()
                {
                    actions.push(EventTriggerSurfaceAction::RequestDrop(trigger_name.clone()));
                }
            });
        });
        ui.add_space(SPACE_XS);
    }

    fn draw_create_form(&mut self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        ui.add_space(SPACE_SM);
        ui.label(RichText::new("Create event trigger").small().strong());
        ui.horizontal(|ui| {
            Input::new(&mut self.state.event_trigger_create_name, "name", self.theme)
                .width(130.0)
                .show(ui);
            Input::new(&mut self.state.event_trigger_create_event, "event", self.theme)
                .width(110.0)
                .show(ui);
        });
        ui.horizontal(|ui| {
            Input::new(
                &mut self.state.event_trigger_create_function,
                "schema.func()",
                self.theme,
            )
            .width(160.0)
            .show(ui);
            Input::new(
                &mut self.state.event_trigger_create_tags,
                "tags CSV optional",
                self.theme,
            )
            .width(140.0)
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
                actions.push(EventTriggerSurfaceAction::PreviewCreate {
                    name: self.state.event_trigger_create_name.clone(),
                    event: self.state.event_trigger_create_event.clone(),
                    function_ref: self.state.event_trigger_create_function.clone(),
                    tags_csv: self.state.event_trigger_create_tags.clone(),
                });
            }
            if Button::new(self.theme)
                .text("Create (confirm)")
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(EventTriggerSurfaceAction::Create);
            }
        });
    }

    fn draw_preview(&self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        let Some(preview) = self.state.event_trigger_ddl_preview.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "Event trigger DDL preview", self.theme)
            .width(560.0)
            .id_salt("event_trigger_preview_dialog")
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
                        actions.push(EventTriggerSurfaceAction::ClosePreview);
                    }
                });
            });
        if !open {
            actions.push(EventTriggerSurfaceAction::ClosePreview);
        }
    }

    fn draw_apply_confirmation(&self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        let Some(pending) = self.state.event_trigger_apply_confirm.as_ref() else {
            return;
        };
        let (title, sql) = match pending {
            EventTriggerApply::Create { name, sql, .. } => (format!("Create event trigger `{name}`?"), sql.as_str()),
            EventTriggerApply::Alter { name, mode, sql } => {
                (format!("Apply ALTER EVENT TRIGGER `{name}` {mode}?"), sql.as_str())
            }
        };
        let mut open = true;
        Dialog::new(&mut open, &title, self.theme)
            .width(560.0)
            .id_salt("event_trigger_apply_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(
                        RichText::new("This changes global DDL hook behavior.")
                            .small()
                            .color(self.theme.warning),
                    );
                    ui.label(RichText::new(sql).monospace());
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Apply")
                        .variant(ButtonVariant::Default)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(EventTriggerSurfaceAction::ConfirmApply);
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(EventTriggerSurfaceAction::CancelApply);
                    }
                });
            });
        if !open {
            actions.push(EventTriggerSurfaceAction::CancelApply);
        }
    }

    fn draw_drop_confirmation(&self, ui: &mut egui::Ui, actions: &mut Vec<EventTriggerSurfaceAction>) {
        let Some(name) = self.state.event_trigger_drop_confirm.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "Drop event trigger?", self.theme)
            .width(460.0)
            .id_salt("event_trigger_drop_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(format!(
                        "Drop event trigger `{name}`? This changes global DDL hook behavior."
                    ));
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Drop")
                        .variant(ButtonVariant::Destructive)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(EventTriggerSurfaceAction::ConfirmDrop(name.clone()));
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(EventTriggerSurfaceAction::CancelDrop);
                    }
                });
            });
        if !open {
            actions.push(EventTriggerSurfaceAction::CancelDrop);
        }
    }
}
