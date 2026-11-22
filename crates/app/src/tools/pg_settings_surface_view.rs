//! PostgreSQL settings presentation and typed user intents.
use super::super::pg_settings_state::PgSettingsState;
use super::super::*;
use crate::components::dialog::Dialog;

/// Settings rows painted per frame (#569).
const PG_SETTINGS_ROW_CAP: usize = 60;


#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PgSettingsSurfaceAction {
    Refresh,
    BeginEdit { name: String, value: String },
    Reset(String),
    PreviewAlterSystem { name: String, value: String },
    ApplySession { name: String, value: String },
    CancelEdit,
    ClosePreview,
}

pub(super) struct PgSettingsSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) state: &'a mut PgSettingsState,
}

impl PgSettingsSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<PgSettingsSurfaceAction> {
        let mut actions = Vec::new();
        self.draw_header(ui, &mut actions);
        self.draw_settings(ui, &mut actions);
        self.draw_edit_dialog(ui, &mut actions);
        self.draw_preview(ui, &mut actions);
        actions
    }

    fn draw_header(&mut self, ui: &mut egui::Ui, actions: &mut Vec<PgSettingsSurfaceAction>) {
        ui.add_space(SPACE_MD);
        section_label(ui, "SERVER SETTINGS (pg_settings)", self.theme);
        if std::mem::take(&mut self.state.capture_focus_once) {
            ui.scroll_to_cursor(Some(egui::Align::Min));
        }
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new("PostgreSQL-only · session SET/RESET for user-context GUCs · ALTER SYSTEM is preview-only")
                .small()
                .color(self.theme.text_muted),
        );
        if Button::new(self.theme)
            .icon(Icon::RefreshCw)
            .text("Load settings")
            .variant(ButtonVariant::Secondary)
            .show(ui)
            .clicked()
        {
            actions.push(PgSettingsSurfaceAction::Refresh);
        }
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").small().color(self.theme.text_muted));
            crate::components::input::SearchInput::new(
                &mut self.state.pg_settings_filter,
                "name, category, source…",
                self.theme,
            )
            .width(260.0)
            .show(ui);
        });
        if let Some(error) = &self.state.pg_settings_error {
            ui.colored_label(self.theme.danger, error);
        }
    }

    fn draw_settings(&self, ui: &mut egui::Ui, actions: &mut Vec<PgSettingsSurfaceAction>) {
        let Some(snapshot) = self.state.pg_settings.as_ref() else {
            return;
        };
        ui.label(
            RichText::new(format!(
                "{} · fetched @ {} ms",
                snapshot.message, snapshot.fetched_at_ms
            ))
            .small()
            .color(self.theme.text_muted),
        );
        let filter = self.state.pg_settings_filter.to_ascii_lowercase();
        let settings: Vec<_> = snapshot
            .settings
            .iter()
            .filter(|setting| {
                filter.is_empty()
                    || setting.name.to_ascii_lowercase().contains(&filter)
                    || setting.category.to_ascii_lowercase().contains(&filter)
                    || setting.source.to_ascii_lowercase().contains(&filter)
            })
            .take(PG_SETTINGS_ROW_CAP)
            .collect();
        // #569: say how many settings are hidden by the cap.
        if let Some(hint) = crate::components::common::shown_total_hint(settings.len(), snapshot.settings.len()) {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        for setting in settings {
            self.draw_setting_card(ui, setting, actions);
        }
    }

    fn draw_setting_card(
        &self,
        ui: &mut egui::Ui,
        setting: &db_pro_core::domain::pg_settings::PgSetting,
        actions: &mut Vec<PgSettingsSurfaceAction>,
    ) {
        let name = setting.name.clone();
        let value = setting.setting.clone();
        panel_frame(self.theme)
            .inner_margin(egui::Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&setting.name)
                            .strong()
                            .monospace()
                            .color(self.theme.text_primary),
                    );
                    if setting.pending_restart {
                        Badge::new("pending restart", self.theme)
                            .tint(self.theme.warning)
                            .compact(true)
                            .show(ui);
                    }
                    if setting.sensitive {
                        Badge::new("redacted", self.theme)
                            .variant(BadgeVariant::Secondary)
                            .compact(true)
                            .show(ui);
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "{} · context={} · source={} · {}",
                        setting.category,
                        setting.context,
                        setting.source,
                        setting.display_setting()
                    ))
                    .small()
                    .color(self.theme.text_secondary),
                );
                if let Some(description) = &setting.short_desc {
                    ui.label(RichText::new(description).small().color(self.theme.text_muted));
                }
                ui.label(
                    RichText::new(setting.mutability_reason())
                        .small()
                        .color(self.theme.text_muted),
                );
                ui.horizontal(|ui| {
                    if setting.session_mutable() && !setting.sensitive {
                        if Button::new(self.theme)
                            .icon(Icon::Pencil)
                            .text("Edit session")
                            .variant(ButtonVariant::Ghost)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(PgSettingsSurfaceAction::BeginEdit {
                                name: name.clone(),
                                value: value.clone(),
                            });
                        }
                        if Button::new(self.theme)
                            .text("RESET")
                            .variant(ButtonVariant::Secondary)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(PgSettingsSurfaceAction::Reset(name.clone()));
                        }
                    }
                    if !setting.sensitive
                        && Button::new(self.theme)
                            .icon(Icon::FileCode2)
                            .text("Preview ALTER SYSTEM")
                            .variant(ButtonVariant::Ghost)
                            .show(ui)
                            .clicked()
                    {
                        actions.push(PgSettingsSurfaceAction::PreviewAlterSystem {
                            name: name.clone(),
                            value: value.clone(),
                        });
                    }
                });
            });
        ui.add_space(SPACE_XS);
    }

    fn draw_edit_dialog(&mut self, ui: &mut egui::Ui, actions: &mut Vec<PgSettingsSurfaceAction>) {
        if self.state.pg_settings_edit_name.is_empty() {
            return;
        }
        let name = self.state.pg_settings_edit_name.clone();
        let mut open = true;
        Dialog::new(&mut open, format!("SET SESSION · {name}"), self.theme)
            .width(460.0)
            .id_salt("pg_settings_edit_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    crate::components::input::Input::new(
                        &mut self.state.pg_settings_edit_value,
                        "new value",
                        self.theme,
                    )
                    .show(ui);
                    // Show the exact statement Apply will run — SET is a live
                    // session mutation, not preview-only like ALTER SYSTEM (#399).
                    match db_pro_core::domain::pg_settings::preview_session_set(
                        &name,
                        &self.state.pg_settings_edit_value,
                    ) {
                        Ok(preview) => {
                            ui.add_space(SPACE_XS);
                            ui.label(
                                RichText::new(&preview.sql)
                                    .monospace()
                                    .small()
                                    .color(self.theme.text_secondary),
                            );
                        }
                        Err(error) => {
                            ui.colored_label(self.theme.danger, error);
                        }
                    }
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Apply SET")
                        .variant(ButtonVariant::Secondary)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(PgSettingsSurfaceAction::ApplySession {
                            name: name.clone(),
                            value: self.state.pg_settings_edit_value.clone(),
                        });
                    }
                    if Button::new(self.theme)
                        .icon(Icon::X)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(PgSettingsSurfaceAction::CancelEdit);
                    }
                });
            });
        if !open {
            actions.push(PgSettingsSurfaceAction::CancelEdit);
        }
    }

    fn draw_preview(&self, ui: &mut egui::Ui, actions: &mut Vec<PgSettingsSurfaceAction>) {
        let Some(preview) = self.state.pg_settings_preview.as_ref() else {
            return;
        };
        let mut open = true;
        Dialog::new(&mut open, "ALTER SYSTEM preview", self.theme)
            .width(560.0)
            .id_salt("pg_settings_preview_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(RichText::new(&preview.note).small().color(self.theme.warning));
                    ui.label(RichText::new(&preview.sql).monospace());
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Close")
                        .variant(ButtonVariant::Secondary)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(PgSettingsSurfaceAction::ClosePreview);
                    }
                });
            });
        if !open {
            actions.push(PgSettingsSurfaceAction::ClosePreview);
        }
    }
}
