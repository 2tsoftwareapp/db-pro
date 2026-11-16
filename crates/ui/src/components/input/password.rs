// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use std::borrow::Cow;

use egui::{CornerRadius, FontFamily, FontId, Frame, Id, Margin, Response, RichText, Stroke, TextEdit, Ui};
use lucide_icons::Icon;

use crate::{
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        input::{
            config::{
                INPUT_AUX_FONT_SIZE, INPUT_ICON_SIZE, INPUT_LABEL_FONT_SIZE, MIN_EDIT_WIDTH, MIN_FRAME_WIDTH,
                PASSWORD_EYE_RESERVATION,
            },
            layout::{paint_field_chrome, resolve_field_width, FieldChromeState},
        },
        interact::text_input_info,
    },
    tokens::{component::input::INPUT_HEIGHT_DEFAULT, LABEL_HELPER_GAP, RADIUS_INPUT, SPACE_SM, SPACE_XS},
    DbProTheme,
};

pub struct PasswordInput<'a> {
    label: Option<Cow<'a, str>>,
    value: &'a mut String,
    placeholder: Cow<'a, str>,
    show_password: &'a mut bool,
    helper_text: Option<Cow<'a, str>>,
    error_text: Option<Cow<'a, str>>,
    required: bool,
    width: Option<f32>,
    id_salt: Option<Id>,
    theme: DbProTheme,
}

impl<'a> PasswordInput<'a> {
    pub fn new(
        value: &'a mut String,
        placeholder: impl Into<Cow<'a, str>>,
        show_password: &'a mut bool,
        theme: DbProTheme,
    ) -> Self {
        Self {
            label: None,
            value,
            placeholder: placeholder.into(),
            show_password,
            helper_text: None,
            error_text: None,
            required: false,
            width: None,
            id_salt: None,
            theme,
        }
    }

    pub fn label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn helper_text(mut self, text: impl Into<Cow<'a, str>>) -> Self {
        self.helper_text = Some(text.into());
        self
    }

    pub fn error_text(mut self, text: impl Into<Cow<'a, str>>) -> Self {
        self.error_text = Some(text.into());
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn id_salt(mut self, id_salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(Id::new(id_salt));
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let width = resolve_field_width(self.width, ui.available_width());

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(width);
            ui.set_max_width(width);
            if let Some(label) = &self.label {
                crate::components::Label::new(label.as_ref(), self.theme)
                    .required(self.required)
                    .show(ui);
                ui.add_space(LABEL_HELPER_GAP);
            }

            let has_error = self.error_text.is_some();
            let frame_w = (width - SPACE_SM * 2.0).max(MIN_FRAME_WIDTH);
            let frame_output = Frame {
                fill: self.theme.surface_field,
                stroke: Stroke::NONE,
                inner_margin: Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8),
                corner_radius: CornerRadius::same(RADIUS_INPUT as u8),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.set_width(frame_w);
                ui.set_max_width(frame_w);
                ui.spacing_mut().interact_size.y = INPUT_HEIGHT_DEFAULT - SPACE_XS * 2.0;
                ui.horizontal(|ui| {
                    ui.set_min_height(INPUT_HEIGHT_DEFAULT - SPACE_XS * 2.0);
                    ui.spacing_mut().item_spacing.x = SPACE_SM;
                    ui.label(
                        RichText::new(char::from(Icon::Lock).to_string())
                            .font(FontId::new(INPUT_ICON_SIZE, FontFamily::Name("lucide".into())))
                            .color(self.theme.text_muted),
                    );

                    // Reserve the eye button and the spacing egui inserts before it;
                    // omitting item_spacing lets the frame grow wider than sibling inputs.
                    let toggle_and_gap = PASSWORD_EYE_RESERVATION + ui.spacing().item_spacing.x;
                    let edit_w = (ui.available_width() - toggle_and_gap).max(MIN_EDIT_WIDTH);
                    let mut text_edit = TextEdit::singleline(self.value).password(!*self.show_password);
                    if let Some(id_salt) = self.id_salt {
                        text_edit = text_edit.id_salt(id_salt);
                    }
                    let edit_response = ui.add(
                        text_edit
                            .hint_text(RichText::new(self.placeholder.as_ref()).color(self.theme.text_muted))
                            .desired_width(edit_w)
                            .margin(Margin::ZERO)
                            .frame(egui::Frame::NONE)
                            .text_color(self.theme.text_primary),
                    );

                    let eye_icon = if *self.show_password { Icon::EyeOff } else { Icon::Eye };
                    let eye_label = if *self.show_password {
                        "Hide password"
                    } else {
                        "Show password"
                    };
                    if Button::new(self.theme)
                        .icon(eye_icon)
                        .size(ButtonSize::IconSm)
                        .variant(ButtonVariant::Ghost)
                        .access_label(eye_label)
                        .tooltip(eye_label)
                        .show(ui)
                        .clicked()
                    {
                        *self.show_password = !*self.show_password;
                    }

                    edit_response
                })
                .inner
            });

            let edit_response = frame_output.inner;
            let frame_rect = frame_output.response.rect;
            super::super::input::handler::register_text_field(ui.ctx(), edit_response.id, frame_rect);
            let mut info_label = self.label.as_deref().unwrap_or("Password").to_string();
            if self.required {
                info_label.push_str(", required");
            }
            if let Some(error) = self.error_text.as_deref() {
                info_label.push_str(". Error: ");
                info_label.push_str(error);
            }
            edit_response.widget_info(|| text_input_info(true, &info_label));

            // NOTE: do NOT register `frame.interact(Sense::click())` here. That call lands on
            // top of the children added inside the frame (the `TextEdit` and the eye button),
            // so egui reports the click as consumed by the frame and the inner widgets never
            // receive it — the eye toggle never fires and the text field never gets the
            // double-click that selects text. The `TextEdit` already focuses itself on its own
            // click, so removing this block restores both behaviours.

            paint_field_chrome(
                ui,
                edit_response.id,
                frame_rect,
                FieldChromeState {
                    focused: edit_response.has_focus(),
                    hovered: frame_output.response.hovered() || edit_response.hovered(),
                    enabled: true,
                    has_error,
                },
                self.theme,
            );

            if let Some(err) = &self.error_text {
                ui.add_space(LABEL_HELPER_GAP);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_SM;
                    ui.label(
                        RichText::new(char::from(Icon::AlertCircle).to_string())
                            .font(FontId::new(INPUT_LABEL_FONT_SIZE, FontFamily::Name("lucide".into())))
                            .color(self.theme.danger),
                    );
                    ui.label(
                        RichText::new(err.as_ref())
                            .size(INPUT_AUX_FONT_SIZE)
                            .color(self.theme.danger),
                    );
                });
            } else if let Some(helper) = &self.helper_text {
                ui.add_space(LABEL_HELPER_GAP);
                ui.label(
                    RichText::new(helper.as_ref())
                        .size(INPUT_AUX_FONT_SIZE)
                        .color(self.theme.text_muted),
                );
            }

            edit_response
        })
        .inner
    }
}
