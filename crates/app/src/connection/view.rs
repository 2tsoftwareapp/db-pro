// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::config::*;
use super::layout::*;
use super::logic::*;
use crate::components::alert::{Alert, AlertVariant};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::dialog::Dialog;
use crate::components::input::Input;

use crate::tokens::*;
use crate::{DbProTheme, UiCommand, UiDriver, UiSslMode};
use egui::RichText;
use lucide_icons::Icon;

use super::super::FeedbackState;
use super::{ConnectionDialogState, ConnectionLifecycleState, PendingConnectionOperation};

/// In-UI qualification caveat for the SSH tunnel control (#239).
pub const SSH_QUALIFICATION_HINT: &str =
    "Unqualified in v0.1: the tunnel has not been end-to-end tested and may not work reliably.";

/// Per-mode guidance for the SSL selector (#144 locked contract).
pub fn ssl_mode_guidance(mode: UiSslMode) -> std::borrow::Cow<'static, str> {
    match mode {
        UiSslMode::Disable => t!("ssl_guidance.disable"),
        UiSslMode::Require => t!("ssl_guidance.require"),
        UiSslMode::VerifyCa => t!("ssl_guidance.verify_ca"),
        UiSslMode::VerifyFull => t!("ssl_guidance.verify_full"),
    }
}


pub(crate) struct ConnectionDialogView<'view, 'bridge> {
    pub(crate) dialog: &'view mut ConnectionDialogState,
    pub(crate) lifecycle: &'view mut ConnectionLifecycleState,
    pub(crate) command_dispatcher: &'view mut super::super::command_dispatch::RuntimeCommandDispatcher<'bridge>,
    pub(crate) feedback: &'view mut FeedbackState,
    pub(crate) theme: DbProTheme,
}

impl<'view, 'bridge> ConnectionDialogView<'view, 'bridge> {
    pub(crate) fn dispatch_command(&mut self, command: UiCommand) -> bool {
        self.command_dispatcher.dispatch(command, self.feedback)
    }

    pub(crate) fn apply_cloud_preset(&mut self) {
        if let Err(err) = super::apply_cloud_preset(self.dialog) {
            self.dialog.set_error(err);
        }
    }

    pub(crate) fn save_draft_as_ssh_profile(&mut self) {
        match super::save_draft_as_ssh_profile(self.dialog) {
            Ok(id) => {
                self.dialog.draft.ssh_profile_id = id;
                self.feedback
                    .set_runtime_message("SSH profile saved — reusable by other connections");
            }
            Err(err) => self.feedback.set_runtime_message(err),
        }
    }

    pub(crate) fn apply_ssh_profile(&mut self, profile_id: &str) {
        super::apply_ssh_profile(self.dialog, profile_id);
    }

    pub(crate) fn dispatch_connection_command(&mut self, save: bool) {
        if let Err(err) = super::logic::validate_connection_draft(&self.dialog.draft) {
            self.dialog.set_error(err);
            return;
        }

        let request_id = self.command_dispatcher.next_request_id();
        let command = super::logic::build_connection_command(
            self.dialog.draft.clone(),
            self.dialog.editing_connection_id.clone(),
            request_id,
            save,
        );

        if self.dispatch_command(command) {
            self.lifecycle.set_pending_request(Some(request_id));
            self.lifecycle.set_pending_operation(Some(if save {
                PendingConnectionOperation::Save
            } else {
                PendingConnectionOperation::Test
            }));
            if save {
                self.dialog.set_test_valid(false);
            } else {
                self.dialog
                    .transition(super::state::ConnectionDialogAction::TestStarted {
                        draft: self.dialog.draft.clone(),
                    });
            }
            self.dialog.clear_error();
            self.feedback.set_runtime_message(if save {
                t!("status.saving").to_string()
            } else {
                t!("status.testing").to_string()
            });

            if !save {
                super::refresh_connection_diagnostics(self.dialog, false, &t!("status.auth_pending"));
            }
        }
    }
    pub(crate) fn cancel_connection_test(&mut self) -> bool {
        if self.lifecycle.pending_operation() != Some(PendingConnectionOperation::Test) {
            return true;
        }
        let Some(request_id) = self.lifecycle.pending_request() else {
            return false;
        };
        if !self.dispatch_command(UiCommand::CancelTestConnection { request_id }) {
            return false;
        }
        self.lifecycle.clear_pending_request();
        self.dialog
            .transition(super::state::ConnectionDialogAction::TestCancelled);
        self.feedback
            .set_runtime_message(t!("status.connection_test_cancelled").to_string());
        true
    }

    /// Routes a close through the dirty-draft confirmation when the form
    /// has unsaved edits; the confirm dialog keeps `open` until answered.
    pub(crate) fn request_close_connection_dialog(&mut self) {
        if self.dialog.draft_is_dirty() {
            self.dialog.discard_draft_confirmation = true;
            return;
        }
        self.close_connection_dialog();
    }

    pub(crate) fn close_connection_dialog(&mut self) {
        if self.lifecycle.pending_operation() == Some(PendingConnectionOperation::Test)
            && !self.cancel_connection_test()
        {
            return;
        }

        self.dialog.discard_draft_confirmation = false;
        self.dialog
            .transition(super::state::ConnectionDialogAction::Close);
    }


    pub(crate) fn draw(&mut self, ctx: &egui::Context) {
        let mut open = self.dialog.open;
        let draft_before = self.dialog.draft.clone();
        let title = if self.dialog.editing_connection_id.is_some() {
            t!("connection.edit_connection")
        } else {
            t!("connection.new_connection")
        };
        let desc = if self.dialog.editing_connection_id.is_some() {
            t!("connection.edit_desc")
        } else {
            t!("connection.new_desc")
        };
        Dialog::new(&mut open, title, self.theme)
            .description(desc)
            .width(820.0)
            // Pinned body leaves headroom for the fixed diagnostics console in
            // the footer so the card fits a 752 px window (#609).
            .body_height(460.0)
            .id_salt("connection_form_dialog")
            .body_scroll_salt(egui::Id::new("connection_form_dialog_body").with(self.dialog.open_generation))
            .show_framed_ctx(ctx, |frame| {
                frame.body(|ui| {
                    self.draw_connection_form(ui);
                });
                frame.footer(|ui| {
                    self.draw_connection_footer(ui);
                });
            });
        if !open && self.dialog.open {
            self.request_close_connection_dialog();
            if self.dialog.open {
                return;
            }
        }
        self.dialog.open = open && self.dialog.open;
        if self.dialog.draft != draft_before {
            self.dialog
                .transition(super::state::ConnectionDialogAction::DraftChanged);
            self.feedback.set_runtime_message(t!("status.connection_changed"));
        }
        if !self.dialog.open {
            self.lifecycle.clear_pending_request();
        }

        // Dirty-draft guard: Esc/backdrop/Cancel on an edited form opens this
        // instead of dropping the draft. The form dialog stays mounted behind
        // it (`self.dialog.open` remains true) until Discard runs.
        if self.dialog.discard_draft_confirmation {
            let mut confirm_open = true;
            let action = crate::components::alert::AlertDialog::new(
                "Discard connection changes?",
                "This form has unsaved edits. Closing discards them.",
                self.theme,
            )
            .confirm_label("Discard")
            .cancel_label("Keep Editing")
            .destructive(true)
            .id_salt("connection_draft_discard")
            .show(ctx, &mut confirm_open);
            match action {
                Some(crate::components::alert::AlertDialogAction::Confirm) => {
                    self.dialog.discard_draft_confirmation = false;
                    self.close_connection_dialog();
                }
                Some(crate::components::alert::AlertDialogAction::Cancel) => {
                    self.dialog.discard_draft_confirmation = false;
                }
                None => {}
            }
            if !confirm_open {
                self.dialog.discard_draft_confirmation = false;
            }
        }
    }

    pub(crate) fn draw_connection_form(&mut self, ui: &mut egui::Ui) {
        self.draw_connection_feedback(ui);

        // ── 1. Database Engine Picker (Select — add providers in DRIVER_CARD_SPECS) ──
        let avail = ui.available_width();
        // Engine picker as a Select: adding a provider is one spec entry, no
        // new card layout. Select is not required-marked — a driver is always
        // selected by default.
        let mut selected_idx = DRIVER_CARD_SPECS
            .iter()
            .position(|spec| spec.driver == Some(self.dialog.draft.driver))
            .unwrap_or(0);
        let options: Vec<String> = DRIVER_CARD_SPECS
            .iter()
            .map(|spec| format!("{} — {}", spec.name, spec.subtitle))
            .collect();
        let icon_ids: Vec<Option<egui::TextureId>> = DRIVER_CARD_SPECS
            .iter()
            .map(|spec| super::driver_icons::texture(ui.ctx(), spec.icon_png))
            .collect();
        let disabled_flags: Vec<bool> = DRIVER_CARD_SPECS
            .iter()
            .map(|spec| spec.is_disabled)
            .collect();
        let resp = crate::components::Select::new("db_engine_select", &mut selected_idx, &options)
            .theme(self.theme)
            .label(t!("connection.database_engine"))
            .width(avail)
            .option_icons(&icon_ids)
            .option_disabled(&disabled_flags)
            .show(ui);
        if std::env::var_os("DB_PRO_DEBUG_SELECT").is_some() {
            let pos = ui.ctx().input(|i| i.pointer.interact_pos());
            let layer = pos.and_then(|p| ui.ctx().layer_id_at(p));
            let open = ui.ctx().data_mut(|d| d.get_temp::<bool>(egui::Id::new("db_engine_select")).unwrap_or(false));
            tracing::debug!(
                target: "db_pro::ui::select",
                ?pos,
                ?layer,
                clicked = resp.clicked(),
                open,
                "database engine select debug"
            );
        }
        // Disabled specs carry `driver: None` — ignore them so a coming-soon row
        // can't overwrite the real driver behind the scenes.
        if let Some(driver) = DRIVER_CARD_SPECS[selected_idx].driver {
            if driver != self.dialog.draft.driver {
                select_driver(&mut self.dialog.draft, driver);
                self.dialog.focus_name_on_open = true;
            }
        }

        ui.add_space(SPACE_MD);

        // ── 2. Engine-specific Fields ─────────────────────────────────
        match self.dialog.draft.driver {
            UiDriver::Postgres | UiDriver::Mysql | UiDriver::SqlServer => self.draw_postgres_connection_fields(ui),
            UiDriver::Sqlite => self.draw_sqlite_connection_fields(ui),
        }
    }

    fn draw_connection_feedback(&mut self, _ui: &mut egui::Ui) {
        // Test failures surface inline in the fixed diagnostics strip next to
        // the Test button; the toast (kept for the feedback log) anchors to
        // the top-right so it never overlaps the footer's Save button (#609).
        if let Some(error) = self.dialog.take_error_toast() {
            self.feedback
                .show_error_toast_at(error, crate::components::overlay::ToastPosition::TopRight);
        }
    }

    /// Diagnostics console pinned above the footer's button row.
    ///
    /// The strip always reserves the same height, so running a test never
    /// inserts or removes rows above the inputs — the form cannot shift
    /// under the user's cursor (#609).
    fn draw_diagnostics_strip(&mut self, ui: &mut egui::Ui) {
        const ROW_HEIGHT: f32 = 16.0;
        const ROWS: usize = 5;

        let theme = self.theme;
        egui::Frame {
            fill: theme.surface_2,
            stroke: egui::Stroke::new(STROKE_THIN, theme.border_subtle),
            inner_margin: egui::Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8),
            corner_radius: egui::CornerRadius::same(RADIUS_SM as u8),
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ROW_HEIGHT * ROWS as f32),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if let Some(report) = &self.dialog.diagnostics {
                        for stage in report.stages.iter().take(ROWS) {
                            let (mark, color) = if stage.ok {
                                ("OK", theme.success)
                            } else {
                                ("FAIL", theme.danger)
                            };
                            ui.add(egui::Label::new(
                                RichText::new(format!("{mark} · {} · {}", stage.stage.label(), stage.message))
                                    .small()
                                    .monospace()
                                    .color(color),
                            ).truncate());
                        }
                    } else {
                        let is_testing = self.lifecycle.pending_operation() == Some(PendingConnectionOperation::Test);
                        let hint = if is_testing {
                            t!("status.testing").to_string()
                        } else {
                            t!("connection.diagnostics_hint").to_string()
                        };
                        ui.label(RichText::new(hint).small().color(theme.text_muted));
                    }
                },
            );
        });
        ui.add_space(SPACE_XS);
    }

    pub(crate) fn draw_connection_footer(&mut self, ui: &mut egui::Ui) {
        self.draw_diagnostics_strip(ui);
        ui.horizontal(|ui| {
            let pending_operation = self.lifecycle.pending_operation();
            let has_pending_request = self.lifecycle.pending_request().is_some();
            let is_testing = pending_operation == Some(PendingConnectionOperation::Test);
            let is_saving = pending_operation == Some(PendingConnectionOperation::Save);
            let test_btn = if is_testing {
                Button::new(self.theme)
                    .text(t!("connection.cancel_test"))
                    .variant(ButtonVariant::Secondary)
                    .icon(Icon::X)
                    .show(ui)
            } else {
                Button::new(self.theme)
                    .text(t!("connection.test_connection"))
                    .variant(ButtonVariant::Secondary)
                    .icon(Icon::Zap)
                    .enabled(!has_pending_request)
                    .loading(is_saving)
                    .show(ui)
            };

            if is_testing {
                if test_btn.clicked() {
                    self.cancel_connection_test();
                }
                ui.add_space(SPACE_XS);
                ui.label(
                    RichText::new(t!("status.testing"))
                        .font(font_caption())
                        .color(self.theme.text_muted),
                );
            } else if test_btn.clicked() && !has_pending_request {
                self.dispatch_connection_command(false);
            } else if is_saving {
                ui.add_space(SPACE_XS);
                ui.label(
                    RichText::new(t!("status.saving"))
                        .font(font_caption())
                        .color(self.theme.text_muted),
                );
            } else if self.dialog.test_valid {
                ui.add_space(SPACE_XS);
                ui.label(
                    RichText::new(char::from(Icon::Check).to_string())
                        .font(font_icon(ICON_XS))
                        .color(self.theme.success),
                );
                ui.label(
                    RichText::new(t!("alerts.verified"))
                        .font(font_caption())
                        .color(self.theme.success),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save_label = if self.dialog.editing_connection_id.is_some() {
                    t!("connection.update_connection")
                } else {
                    t!("connection.save_connection")
                };

                let save_btn = Button::new(self.theme)
                    .text(&*save_label)
                    .variant(ButtonVariant::Default)
                    .icon(Icon::Check)
                    .show(ui);

                if save_btn.clicked() {
                    self.dispatch_connection_command(true);
                }

                if Button::new(self.theme)
                    .text(t!("connection.cancel"))
                    .variant(ButtonVariant::Ghost)
                    .show(ui)
                    .clicked()
                {
                    self.request_close_connection_dialog();
                }
            });
        });
    }

    pub(crate) fn draw_postgres_connection_fields(&mut self, ui: &mut egui::Ui) {
        // General Profile
        self.draw_general_profile_fields(ui);

        // Server & Credentials
        self.draw_server_credentials_fields(ui);

        // Advanced Panels
        self.draw_cloud_presets_panel(ui);
        self.draw_ssl_certificates_panel(ui);
        self.draw_ssh_tunnel_panel(ui);
        self.draw_tags_metadata_panel(ui, "conn_tags_panel", t!("connection.tags_placeholder_pg").as_ref());
    }

    pub(crate) fn draw_sqlite_connection_fields(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width();
        let gap = SPACE_SM;

        // General Profile
        self.draw_general_profile_fields(ui);

        // Database File Path
        ui.label(
            RichText::new(t!("connection.database_file"))
                .font(DbProTheme::ui_medium_font(FONT_SIZE_BADGE))
                .color(self.theme.text_muted),
        );
        ui.add_space(SPACE_XXS);

        let (input_w, button_w) = sqlite_file_row(avail, gap);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.vertical(|ui| {
                ui.set_width(input_w);
                ui.set_max_width(input_w);
                let show_database_error =
                    !self.dialog.error.is_empty() && self.dialog.draft.database.trim().is_empty();
                let mut db_input = Input::new(&mut self.dialog.draft.database, "/path/to/database.db", self.theme)
                    .label(t!("connection.database_file_path"))
                    .required(true)
                    .id_salt(focus_id::SQLITE_DATABASE_FILE)
                    .width(input_w)
                    .leading_icon(Icon::FolderArchive)
                    .clearable(true);
                if show_database_error {
                    db_input = db_input.error_text(t!("connection.validation_database_required"));
                }
                db_input.show(ui);
            });
            ui.vertical(|ui| {
                ui.set_width(button_w);
                ui.set_max_width(button_w);
                ui.add_space(20.0); // Align with input below label
                if Button::new(self.theme)
                    .icon(Icon::FolderOpen)
                    .text(t!("connection.browse_file"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    let request_id = self.command_dispatcher.next_request_id();
                    self.dispatch_command(UiCommand::PickSqliteFile { request_id });
                }
            });
        });
        ui.add_space(SPACE_SM);

        // Embedded SQLite Engine Info Card
        Alert::title_only(t!("connection.sqlite_info_hint"), self.theme)
            .variant(AlertVariant::Info)
            .icon(Icon::Info)
            .show(ui);

        // Panel: Tags & Metadata
        self.draw_tags_metadata_panel(
            ui,
            "sqlite_tags_panel",
            t!("connection.tags_placeholder_sqlite").as_ref(),
        );
    }
}

pub(crate) fn draw_connection_dialog(
    ctx: &egui::Context,
    theme: DbProTheme,
    dialog: &mut ConnectionDialogState,
    lifecycle: &mut ConnectionLifecycleState,
    command_dispatcher: &mut super::super::command_dispatch::RuntimeCommandDispatcher<'_>,
    feedback: &mut FeedbackState,
) {
    ConnectionDialogView {
        dialog,
        lifecycle,
        command_dispatcher,
        feedback,
        theme,
    }
    .draw(ctx);
}
