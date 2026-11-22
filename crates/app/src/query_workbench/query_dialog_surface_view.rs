use super::query_execution_state::PendingDestructiveRun;
use super::*;
use crate::app::overlay_state::{ExportFormat, ExportNullMode};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use egui::RichText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DestructiveDialogAction {
    Confirm,
    Cancel,
}

pub(super) struct DestructiveDialogContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) pending: &'a PendingDestructiveRun,
}

impl DestructiveDialogContext<'_> {
    /// ctx-level overlay variant: the held destructive statement must be
    /// confirmable no matter which output tab or dock state is active.
    pub(super) fn draw_ctx(&self, ctx: &egui::Context) -> Option<DestructiveDialogAction> {
        let mut open = true;
        let mut action = None;
        Dialog::new(&mut open, t!("query.destructive_title").as_ref(), self.theme)
            .id_salt("destructive_run_dialog")
            .width(560.0)
            .show_framed_ctx(ctx, |frame| {
                frame.body(|ui| {
                    self.draw_destructive_body(ui, &mut action);
                });
            });
        if !open {
            return Some(DestructiveDialogAction::Cancel);
        }
        action
    }


    fn draw_destructive_body(&self, ui: &mut egui::Ui, action: &mut Option<DestructiveDialogAction>) {
        let warning = if self.pending.all_statements() {
            t!("query.destructive_script_warning").into_owned()
        } else {
            t!("query.destructive_statement_warning").into_owned()
        };
        ui.label(RichText::new(warning).color(self.theme.text_primary));
        ui.add_space(SPACE_SM);
        let mut preview = self.pending.sql().to_owned();
        if preview.chars().count() > 600 {
            preview = preview.chars().take(600).collect::<String>() + "…";
        }
        Card::new(self.theme).show(ui, |ui| {
            ui.label(
                RichText::new(preview)
                    .font(font_mono_sm())
                    .color(self.theme.text_secondary),
            );
        });
        if !self.pending.target_label().is_empty() {
            ui.add_space(SPACE_SM);
            ui.label(
                RichText::new(t!("query.destructive_target", target = self.pending.target_label()).into_owned())
                    .strong()
                    .color(self.theme.warning),
            );
        }
        ui.add_space(SPACE_SM);
        ui.colored_label(
            self.theme.warning,
            t!("query.destructive_server_warning").into_owned(),
        );
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if Button::new(self.theme)
                .text(t!("query.destructive_confirm").as_ref())
                .variant(ButtonVariant::Destructive)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(DestructiveDialogAction::Confirm);
            }
            if Button::new(self.theme)
                .text(t!("common.cancel").as_ref())
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(DestructiveDialogAction::Cancel);
            }
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportDialogAction {
    Export,
    Cancel,
    Overwrite,
    KeepExisting,
    Browse,
}

pub(super) struct ExportDialogContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) overlay: &'a mut OverlayState,
    /// `COPY … FROM stdin` is PostgreSQL syntax; SQLite/MySQL cannot consume it.
    pub(super) copy_supported: bool,
}

impl ExportDialogContext<'_> {
    pub(super) fn draw(&mut self, ctx: &egui::Context) -> Option<ExportDialogAction> {
        let mut open = true;
        let mut action = None;
        Dialog::new(&mut open, t!("query.export_results").as_ref(), self.theme)
            .id_salt("export_results_dialog")
            .width(520.0)
            .description(t!("query.export_results_desc").as_ref())
            .show_ctx(ctx, |ui| self.draw_body(ui, &mut action));
        if !open {
            return Some(ExportDialogAction::Cancel);
        }
        action
    }

    fn draw_body(&mut self, ui: &mut egui::Ui, action: &mut Option<ExportDialogAction>) {
        ui.label(
            RichText::new(t!("query.export_format_label"))
                .font(font_ui_label())
                .color(self.theme.text_muted),
        );
        ui.add_space(SPACE_XS);
        self.draw_format_picker(ui);
        // #612: NULL spelling only matters for delimited formats.
        if matches!(self.overlay.export_format, ExportFormat::Csv | ExportFormat::Tsv) {
            ui.add_space(SPACE_XS);
            self.draw_null_picker(ui);
        }
        ui.add_space(SPACE_SM);
        self.draw_output_fields(ui, action);
        ui.add_space(SPACE_XS);
        ui.label(
            RichText::new(t!("query.export_visible_only"))
                .font(font_caption())
                .color(self.theme.text_muted),
        );
        if self.overlay.export_overwrite_pending.is_some() {
            self.draw_overwrite_confirmation(ui, action);
        }
        ui.add_space(SPACE_MD);
        self.draw_footer(ui, action);
    }

    fn draw_output_fields(&mut self, ui: &mut egui::Ui, action: &mut Option<ExportDialogAction>) {
        ui.label(
            RichText::new(t!("query.export_output_file"))
                .font(font_ui_label())
                .color(self.theme.text_muted),
        );
        ui.add_space(SPACE_XS);
        let placeholder = format!("/path/to/results.{}", self.overlay.export_format.extension());
        let path_before = self.overlay.export_path.clone();
        ui.horizontal(|ui| {
            let input_width = ui.available_width() - 108.0;
            Input::new(&mut self.overlay.export_path, placeholder, self.theme)
                .width(input_width.max(0.0))
                .show(ui);
            ui.add_space(SPACE_XS);
            if Button::new(self.theme)
                .text(t!("connection.browse").as_ref())
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(ExportDialogAction::Browse);
            }
        });
        // #539: a path typed after the prompt appeared is a different destination.
        if self.overlay.export_path != path_before {
            self.overlay.export_overwrite_pending = None;
        }
        self.draw_effective_path_hint(ui);
        if matches!(self.overlay.export_format, ExportFormat::SqlInsert | ExportFormat::SqlCopy) {
            ui.add_space(SPACE_XS);
            ui.label(
                RichText::new(t!("query.export_table_name"))
                    .font(font_ui_label())
                    .color(self.theme.text_muted),
            );
            ui.add_space(SPACE_XS);
            Input::new(&mut self.overlay.export_table_name, "exported_rows", self.theme).show(ui);
        }
    }

    /// #396: surface the effective file name live so a missing/wrong extension is never a
    /// silent surprise — the write itself normalizes the same way.
    fn draw_effective_path_hint(&self, ui: &mut egui::Ui) {
        let trimmed = self.overlay.export_path.trim().to_owned();
        if trimmed.is_empty() {
            return;
        }
        let effective = self.overlay.effective_export_path();
        if effective != std::path::Path::new(&trimmed) {
            ui.add_space(SPACE_XS);
            ui.colored_label(
                self.theme.warning,
                t!("query.export_path_adjusted", path = effective.display().to_string().as_str()).into_owned(),
            );
        }
    }

    fn draw_format_picker(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for format in ExportFormat::offered(self.copy_supported) {
                let selected = self.overlay.export_format == format;
                if Button::new(self.theme)
                    .text(format.label())
                    .variant(if selected {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                    && !selected
                {
                    self.overlay.export_format = format;
                    // #539: the prompt named the previous format's effective path.
                    self.overlay.export_overwrite_pending = None;
                }
            }
        });
    }

    /// NULL spelling for CSV/TSV (#612): empty, the word `NULL`, or the `\N`
    /// sentinel that keeps a `COPY` round-trip lossless (#487).
    fn draw_null_picker(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(t!("query.export_null_label"))
                    .font(font_caption())
                    .color(self.theme.text_muted),
            );
            for mode in [
                ExportNullMode::Empty,
                ExportNullMode::Literal,
                ExportNullMode::Sentinel,
            ] {
                let selected = self.overlay.export_null == mode;
                let label = match mode {
                    ExportNullMode::Empty => t!("query.export_null_empty").to_string(),
                    ExportNullMode::Literal => mode.label().to_owned(),
                    ExportNullMode::Sentinel => mode.label().to_owned(),
                };
                if Button::new(self.theme)
                    .text(label)
                    .variant(if selected {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                    && !selected
                {
                    self.overlay.export_null = mode;
                }
            }
        });
    }

    fn draw_footer(&self, ui: &mut egui::Ui, action: &mut Option<ExportDialogAction>) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Button::new(self.theme)
                .text(t!("common.export").as_ref())
                .variant(ButtonVariant::Default)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(ExportDialogAction::Export);
            }
            if Button::new(self.theme)
                .text(t!("common.cancel").as_ref())
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(ExportDialogAction::Cancel);
            }
        });
    }

    fn draw_overwrite_confirmation(&self, ui: &mut egui::Ui, action: &mut Option<ExportDialogAction>) {
        ui.add_space(SPACE_XS);
        // #539: name the file that will actually be replaced, not the raw typed path.
        let effective = self.overlay.effective_export_path();
        let effective = effective.display().to_string();
        ui.colored_label(
            self.theme.warning,
            t!("query.export_overwrite_prompt", path = effective.as_str()).into_owned(),
        );
        ui.horizontal(|ui| {
            if Button::new(self.theme)
                .text(t!("query.overwrite").as_ref())
                .variant(ButtonVariant::Destructive)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(ExportDialogAction::Overwrite);
            }
            if Button::new(self.theme)
                .text(t!("query.keep_existing_file").as_ref())
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                *action = Some(ExportDialogAction::KeepExisting);
            }
        });
    }
}

#[cfg(test)]
#[path = "tests/query_dialog_surface_view_tests.rs"]
mod tests;

