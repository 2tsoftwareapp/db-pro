// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use egui::{Align, CornerRadius, Frame, Margin, Response, RichText, Stroke, TextEdit, Ui};

use crate::{
    components::{
        input::{
            config::{INPUT_AUX_FONT_SIZE, INPUT_LABEL_FONT_SIZE, TEXTAREA_INNER_MARGIN_Y},
            layout::{paint_field_chrome, FieldChromeState},
        },
        interact::text_input_info,
    },
    tokens::{LABEL_HELPER_GAP, RADIUS_INPUT, SPACE_SM},
    DbProTheme,
};

pub struct Textarea<'a> {
    label: Option<&'a str>,
    value: &'a mut String,
    placeholder: &'a str,
    min_rows: usize,
    max_chars: Option<usize>,
    /// Render the text selectable-but-not-editable (egui `interactive(false)`),
    /// for preview panes like the cell inspector's Pretty/Hex views.
    read_only: bool,
    /// Optional font override (e.g. monospace) for the editable text.
    font: Option<egui::FontId>,
    theme: DbProTheme,
}

impl<'a> Textarea<'a> {
    pub fn new(value: &'a mut String, placeholder: &'a str, theme: DbProTheme) -> Self {
        Self {
            label: None,
            value,
            placeholder,
            min_rows: 3,
            max_chars: None,
            read_only: false,
            font: None,
            theme,
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn min_rows(mut self, rows: usize) -> Self {
        self.min_rows = rows;
        self
    }

    pub fn max_chars(mut self, max: usize) -> Self {
        self.max_chars = Some(max);
        self
    }

    /// Render the buffer non-editable while staying selectable, for
    /// inspector/preview surfaces (maps to `TextEdit::interactive(false)`).
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Override the text font (e.g. monospace) for script/SQL buffers.
    pub fn font(mut self, font: egui::FontId) -> Self {
        self.font = Some(font);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            if let Some(label) = self.label {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .size(INPUT_LABEL_FONT_SIZE)
                            .strong()
                            .color(self.theme.text_secondary),
                    );
                    if let Some(max) = self.max_chars {
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(super::super::handler::character_count(self.value, max))
                                    .size(INPUT_AUX_FONT_SIZE)
                                    .color(self.theme.text_muted),
                            );
                        });
                    }
                });
                ui.add_space(LABEL_HELPER_GAP);
            }

            let frame_output = Frame {
                fill: self.theme.surface_field,
                stroke: Stroke::NONE,
                inner_margin: Margin::symmetric(SPACE_SM as i8, TEXTAREA_INNER_MARGIN_Y as i8),
                corner_radius: CornerRadius::same(RADIUS_INPUT as u8),
                ..Default::default()
            }
            .show(ui, |ui| {
                let mut text_edit = TextEdit::multiline(self.value)
                    .hint_text(RichText::new(self.placeholder).color(self.theme.text_muted))
                    .desired_rows(self.min_rows)
                    .desired_width(ui.available_width())
                    .frame(egui::Frame::NONE)
                    .margin(Margin::ZERO)
                    .interactive(!self.read_only)
                    .text_color(self.theme.text_primary);
                if let Some(font) = self.font {
                    text_edit = text_edit.font(font);
                }
                ui.add(text_edit)
            });

            let edit_response = frame_output.inner;
            let frame_rect = frame_output.response.rect;
            super::super::handler::register_text_field(ui.ctx(), edit_response.id, frame_rect);
            let info_label = self.label.unwrap_or(self.placeholder);
            edit_response.widget_info(|| text_input_info(true, info_label));

            paint_field_chrome(
                ui,
                edit_response.id,
                frame_rect,
                FieldChromeState {
                    focused: edit_response.has_focus(),
                    hovered: frame_output.response.hovered() || edit_response.hovered(),
                    enabled: true,
                    has_error: false,
                },
                self.theme,
            );

            edit_response
        })
        .inner
    }
}
