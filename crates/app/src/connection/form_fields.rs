// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::config::*;
use super::layout::*;
use crate::UiDriver;
use crate::components::input::{Input, PasswordInput};
use crate::components::selection::Checkbox;
use crate::components::tabs::SegmentedTabs;
use crate::components::Label;
use crate::tokens::*;
use crate::DbProTheme;
use egui::RichText;
use lucide_icons::Icon;

/// Muted section header used above each field group ("GENERAL", "SERVER & …").
fn section_label(ui: &mut egui::Ui, text: &str, theme: DbProTheme) {
    ui.label(
        RichText::new(text)
            .font(DbProTheme::ui_medium_font(FONT_SIZE_BADGE))
            .color(theme.text_muted),
    );
    ui.add_space(SPACE_XXS);
}

/// Label + widget pair for non-Input controls (SegmentedTabs, Checkbox rows).
fn labeled_cell(ui: &mut egui::Ui, label: &str, theme: DbProTheme, body: impl FnOnce(&mut egui::Ui)) {
    // Child UIs inherit egui's default `indent`, which would nudge the label
    // right of the widget's left edge — zero it so label and widget align.
    ui.spacing_mut().indent = 0.0;
    Label::new(label, theme).show(ui);
    ui.add_space(SPACE_XXS);
    body(ui);
}

impl<'view, 'bridge> super::view::ConnectionDialogView<'view, 'bridge> {
    /// Draw general connection profile fields (Name, Group, Favorite, Environment, Safety).
    pub(crate) fn draw_general_profile_fields(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width();
        let focus_name = self.dialog.focus_name_on_open;
        let gap = SPACE_SM;

        section_label(ui, &t!("connection.general"), self.theme);

        // Row 1: Connection Name (65%), Folder/Group + Favorite toggle (35%)
        let widths = row_widths(avail, gap, &[65.0, 35.0]);
        let show_name_error = !self.dialog.error.is_empty() && self.dialog.draft.name.trim().is_empty();
        let mut name_input = Input::new(
            &mut self.dialog.draft.name,
            t!("connection.connection_name_placeholder"),
            self.theme,
        )
        .label(t!("connection.connection_name"))
        .required(true)
        .id_salt(focus_id::NAME)
        .auto_focus(focus_name)
        .width(widths[0])
        .leading_icon(Icon::Tag)
        .clearable(true);
        if show_name_error {
            name_input = name_input.error_text(t!("connection.validation_name_required"));
        }
        form_row(
            ui,
            &widths,
            vec![
                Box::new(|ui| {
                    name_input.show(ui);
                }),
                Box::new(|ui| {
                    ui.spacing_mut().indent = 0.0;
                    Label::new(t!("connection.folder_group"), self.theme).show(ui);
                    ui.add_space(SPACE_XXS);
                    Input::new(
                        &mut self.dialog.draft.group,
                        t!("connection.folder_group_placeholder"),
                        self.theme,
                    )
                    .id_salt(focus_id::GROUP)
                    .width(widths[1])
                    .leading_icon(Icon::Folder)
                    .clearable(true)
                    .show(ui);
                }),
            ],
            gap,
        );
        if focus_name {
            self.dialog.focus_name_on_open = false;
        }
        ui.add_space(SPACE_SM);

        // Row 2: Environment & Safety Policy — even split
        let widths = row_widths(avail, gap, &[50.0, 50.0]);
        let is_production = self.dialog.draft.environment == "Production";
        form_row(
            ui,
            &widths,
            vec![
                Box::new(|ui| {
                    labeled_cell(ui, &t!("connection.environment"), self.theme, |ui| {
                        let mut env_idx = environment_to_index(&self.dialog.draft.environment);
                        SegmentedTabs::new(&mut env_idx, ENVIRONMENT_OPTIONS, self.theme)
                            .focusable(false)
                            .show(ui);
                        self.dialog.draft.environment = index_to_environment(env_idx).to_owned();
                    });
                }),
                Box::new(|ui| {
                    labeled_cell(ui, &t!("connection.safety_policy"), self.theme, |ui| {
                        ui.horizontal(|ui| {
                            Checkbox::new(
                                &mut self.dialog.draft.readonly,
                                t!("connection.readonly_mode").as_ref(),
                                self.theme,
                            )
                            .show(ui);
                            // SQLite: creating the file is a separate, explicit
                            // decision from opening one (#597).
                            if self.dialog.draft.driver == UiDriver::Sqlite {
                                Checkbox::new(
                                    &mut self.dialog.draft.create_if_missing,
                                    t!("connection.create_if_missing").as_ref(),
                                    self.theme,
                                )
                                .show(ui);
                            }
                            if is_production {
                                ui.label(
                                    RichText::new(t!("connection.production_guard"))
                                        .font(font_caption())
                                        .color(self.theme.warning),
                                );
                            }
                        });
                    });
                }),
            ],
            gap,
        );
        ui.add_space(SPACE_MD);
    }

    /// Draw server & credentials section for network-based databases (PostgreSQL, MySQL, SQL Server).
    pub(crate) fn draw_server_credentials_fields(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width();
        let gap = SPACE_SM;

        section_label(ui, &t!("connection.server_credentials"), self.theme);

        // Row 1: Host (68%) + Port (32%)
        let widths = row_widths(avail, gap, &[68.0, 32.0]);
        let port_hint = t!(
            "connection.field_example",
            value = default_port_for_driver(self.dialog.draft.driver)
        );
        let show_port_error = !self.dialog.error.is_empty() && !super::logic::is_valid_port(&self.dialog.draft.port);
        // Empty reads "required"; a filled-but-bad value keeps the range hint.
        let port_error_key: &'static str = if self.dialog.draft.port.trim().is_empty() {
            "connection.validation_port_required"
        } else {
            "connection.validation_port_invalid"
        };
        let show_host_error = !self.dialog.error.is_empty() && self.dialog.draft.host.trim().is_empty();
        form_row(
            ui,
            &widths,
            vec![
                Box::new(|ui| {
                    let mut host_input = Input::new(
                        &mut self.dialog.draft.host,
                        t!("connection.field_example", value = "localhost"),
                        self.theme,
                    )
                    .label(t!("connection.host"))
                    .required(true)
                    .id_salt(focus_id::HOST)
                    .width(widths[0])
                    .leading_icon(Icon::Server);
                    if show_host_error {
                        host_input = host_input.error_text(t!("connection.validation_host_required"));
                    }
                    host_input.show(ui);
                }),
                Box::new(|ui| {
                    let mut port_input = Input::new(&mut self.dialog.draft.port, port_hint, self.theme)
                        .label(t!("connection.port"))
                        .required(true)
                        .id_salt(focus_id::PORT)
                        .width(widths[1])
                        .leading_icon(Icon::Hash);
                    if show_port_error {
                        port_input = port_input.error_text(t!(port_error_key));
                    }
                    port_input.show(ui);
                }),
            ],
            gap,
        );
        ui.add_space(SPACE_SM);

        // Row 2: Database Name (50%) + Username (50%)
        let widths = row_widths(avail, gap, &[1.0, 1.0]);
        let db_hint = t!(
            "connection.field_example",
            value = default_database_for_driver(self.dialog.draft.driver)
        );
        let user_hint = t!(
            "connection.field_example",
            value = default_username_for_driver(self.dialog.draft.driver)
        );
        let show_database_error = !self.dialog.error.is_empty() && self.dialog.draft.database.trim().is_empty();
        let show_username_error = !self.dialog.error.is_empty() && self.dialog.draft.username.trim().is_empty();
        form_row(
            ui,
            &widths,
            vec![
                Box::new(|ui| {
                    let mut db_input = Input::new(&mut self.dialog.draft.database, db_hint, self.theme)
                        .label(t!("connection.database"))
                        .required(true)
                        .id_salt(focus_id::DATABASE)
                        .width(widths[0])
                        .leading_icon(Icon::Database)
                        .clearable(true);
                    if show_database_error {
                        db_input = db_input.error_text(t!("connection.validation_database_required"));
                    }
                    db_input.show(ui);
                }),
                Box::new(|ui| {
                    let mut user_input = Input::new(&mut self.dialog.draft.username, user_hint, self.theme)
                        .label(t!("connection.username"))
                        .required(true)
                        .id_salt(focus_id::USERNAME)
                        .width(widths[1])
                        .leading_icon(Icon::User);
                    if show_username_error {
                        user_input = user_input.error_text(t!("connection.validation_username_required"));
                    }
                    user_input.show(ui);
                }),
            ],
            gap,
        );
        ui.add_space(SPACE_SM);

        // Row 3: Password/Auth Token (50%) + SSL Mode (50%)
        let is_token = self.dialog.draft.auth_kind == "ephemeral_token";
        let is_editing = self.dialog.editing_connection_id.is_some();
        let pwd_placeholder = if is_token {
            t!("connection.password_token_placeholder")
        } else if is_editing {
            t!("connection.password_blank_keep")
        } else {
            t!("connection.password_blank_optional")
        };
        let pwd_label = if is_token {
            t!("connection.access_token")
        } else {
            t!("connection.password")
        };
        form_row(
            ui,
            &widths,
            vec![
                Box::new(|ui| {
                    PasswordInput::new(
                        &mut self.dialog.draft.password,
                        pwd_placeholder,
                        &mut self.dialog.show_password,
                        self.theme,
                    )
                    .label(pwd_label)
                    .id_salt(focus_id::PASSWORD)
                    .width(widths[0])
                    .show(ui);
                }),
                Box::new(|ui| {
                    labeled_cell(ui, &t!("connection.ssl_mode"), self.theme, |ui| {
                        let mut ssl_idx = ssl_mode_to_index(self.dialog.draft.ssl_mode);
                        SegmentedTabs::new(&mut ssl_idx, SSL_MODE_OPTIONS, self.theme)
                            .focusable(false)
                            .show(ui);
                        self.dialog.draft.ssl_mode = index_to_ssl_mode(ssl_idx);
                        ui.add_space(SPACE_XXS);
                        let guidance_color = match self.dialog.draft.ssl_mode {
                            crate::UiSslMode::Disable => self.theme.danger,
                            crate::UiSslMode::Require | crate::UiSslMode::VerifyCa => self.theme.warning,
                            crate::UiSslMode::VerifyFull => self.theme.text_muted,
                        };
                        let guidance_width = widths[1];
                        let guidance = super::view::ssl_mode_guidance(self.dialog.draft.ssl_mode);
                        // Painter call keeps the hint pinned to the cell's left
                        // edge — `Label` placement proved inconsistent inside
                        // `add_sized` + wrap (renders centered/offset).
                        let galley = ui.painter().layout(
                            guidance.to_string(),
                            font_caption(),
                            guidance_color,
                            guidance_width,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(guidance_width, galley.size().y.max(14.0)),
                            egui::Sense::hover(),
                        );
                        ui.painter().galley(rect.left_top(), galley, guidance_color);
                    });
                }),
            ],
            gap,
        );

        ui.add_space(SPACE_XS);
        ui.scope(|ui| {
            ui.set_min_width(avail);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(char::from(Icon::ShieldCheck).to_string())
                        .font(font_icon(ICON_XS))
                        .color(self.theme.accent),
                );
                ui.label(
                    RichText::new(t!("connection.credentials_encrypted_hint"))
                        .size(FONT_SIZE_CAPTION)
                        .color(self.theme.text_muted),
                );
            });
        });

        ui.add_space(SPACE_MD);
    }
}
