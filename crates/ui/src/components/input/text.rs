// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use std::borrow::Cow;

use egui::{
    CornerRadius, FontFamily, FontId, Frame, Id, Margin, Response, RichText, Sense, Stroke, TextEdit, Ui, Vec2,
};
use lucide_icons::Icon;

use crate::{
    components::{
        input::{
            config::{
                CLEAR_BUTTON_HIT, INPUT_AUX_FONT_SIZE, INPUT_ICON_SIZE, INPUT_LABEL_FONT_SIZE, MIN_EDIT_WIDTH,
                MIN_FRAME_WIDTH,
            },
            layout::{paint_field_chrome, resolve_field_width, FieldChromeState},
        },
        interact::{button_info, text_input_info},
    },
    tokens::{component::input::INPUT_HEIGHT_DEFAULT, LABEL_HELPER_GAP, RADIUS_INPUT, SPACE_SM, SPACE_XS},
    DbProTheme,
};

pub struct Input<'a> {
    label: Option<Cow<'a, str>>,
    access_label: Option<Cow<'a, str>>,
    value: &'a mut String,
    placeholder: Cow<'a, str>,
    helper_text: Option<Cow<'a, str>>,
    error_text: Option<Cow<'a, str>>,
    leading_icon: Option<Icon>,
    clearable: bool,
    /// Mask the value with egui's password glyphs (no reveal affordance;
    /// callers that need a reveal toggle use `PasswordInput` instead).
    password: bool,
    required: bool,
    width: Option<f32>,
    id_salt: Option<Id>,
    auto_focus: bool,
    enabled: bool,
    theme: DbProTheme,
}

impl<'a> Input<'a> {
    pub fn new(value: &'a mut String, placeholder: impl Into<Cow<'a, str>>, theme: DbProTheme) -> Self {
        Self {
            label: None,
            access_label: None,
            value,
            placeholder: placeholder.into(),
            helper_text: None,
            error_text: None,
            leading_icon: None,
            clearable: false,
            password: false,
            required: false,
            width: None,
            id_salt: None,
            auto_focus: false,
            enabled: true,
            theme,
        }
    }

    pub fn label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn access_label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.access_label = Some(label.into());
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

    pub fn leading_icon(mut self, icon: Icon) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
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

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn id_salt(mut self, id_salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(Id::new(id_salt));
        self
    }

    pub fn auto_focus(mut self, auto_focus: bool) -> Self {
        self.auto_focus = auto_focus;
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
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
                    .enabled(self.enabled)
                    .show(ui);
                ui.add_space(LABEL_HELPER_GAP);
            }

            let has_error = self.error_text.is_some();

            let fill = if self.enabled {
                self.theme.surface_field
            } else {
                self.theme.surface_2
            };

            let frame = Frame {
                fill,
                stroke: Stroke::NONE,
                inner_margin: Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8),
                corner_radius: CornerRadius::same(RADIUS_INPUT as u8),
                ..Default::default()
            };

            let frame_w = (width - SPACE_SM * 2.0).max(MIN_FRAME_WIDTH);
            let frame_output = frame.show(ui, |ui| {
                ui.set_width(frame_w);
                ui.set_max_width(frame_w);
                ui.spacing_mut().interact_size.y = INPUT_HEIGHT_DEFAULT - SPACE_XS * 2.0;
                ui.horizontal(|ui| {
                    ui.set_min_height(INPUT_HEIGHT_DEFAULT - SPACE_XS * 2.0);
                    ui.spacing_mut().item_spacing.x = SPACE_SM;
                    if let Some(icon) = self.leading_icon {
                        ui.label(
                            RichText::new(char::from(icon).to_string())
                                .font(FontId::new(INPUT_ICON_SIZE, FontFamily::Name("lucide".into())))
                                .color(if self.enabled {
                                    self.theme.text_muted
                                } else {
                                    self.theme.border_subtle
                                }),
                        );
                    }

                    let has_text = !self.value.is_empty();
                    let show_clear = self.clearable && has_text && self.enabled;
                    let extra_width = if show_clear {
                        CLEAR_BUTTON_HIT + ui.spacing().item_spacing.x
                    } else {
                        0.0
                    };
                    let edit_w = (ui.available_width() - extra_width).max(MIN_EDIT_WIDTH);
                    let mut text_edit = TextEdit::singleline(self.value);
                    if let Some(id_salt) = self.id_salt {
                        text_edit = text_edit.id_salt(id_salt);
                    }
                    let edit_response = ui.add_enabled(
                        self.enabled,
                        text_edit
                            .hint_text(RichText::new(self.placeholder.as_ref()).color(self.theme.text_muted))
                            .desired_width(edit_w)
                            .margin(Margin::ZERO)
                            .frame(egui::Frame::NONE)
                            .password(self.password)
                            .text_color(if self.enabled {
                                self.theme.text_primary
                            } else {
                                self.theme.text_disabled
                            }),
                    );

                    if show_clear {
                        let (clear_rect, clear_response) = ui
                            .allocate_at_least(Vec2::new(CLEAR_BUTTON_HIT, ui.spacing().interact_size.y), Sense::CLICK);
                        let clear_response = clear_response.on_hover_text("Clear");
                        clear_response.widget_info(|| button_info(true, "Clear"));
                        ui.painter().text(
                            clear_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            char::from(Icon::X).to_string(),
                            FontId::new(INPUT_LABEL_FONT_SIZE, FontFamily::Name("lucide".into())),
                            self.theme.text_muted,
                        );
                        if clear_response.clicked() {
                            self.value.clear();
                        }
                    }

                    edit_response
                })
                .inner
            });

            let edit_response = frame_output.inner;
            if self.auto_focus {
                edit_response.request_focus();
            }
            let frame_rect = frame_output.response.rect;
            super::super::handler::register_text_field(ui.ctx(), edit_response.id, frame_rect);
            let info_label = super::super::handler::accessible_label(
                self.access_label.as_deref(),
                self.label.as_deref(),
                self.placeholder.as_ref(),
            );
            edit_response.widget_info(|| text_input_info(self.enabled, info_label));

            // NOTE: do NOT register `frame.interact(Sense::click())` here. That call lands on
            // top of the `TextEdit` added inside the frame, so egui reports the click as
            // consumed by the frame and the text field never receives it — double-click never
            // selects text and the caret never lands where you click. The `TextEdit` already
            // focuses itself on its own click, so removing this block restores that.

            paint_field_chrome(
                ui,
                edit_response.id,
                frame_rect,
                FieldChromeState {
                    focused: edit_response.has_focus(),
                    hovered: frame_output.response.hovered() || edit_response.hovered(),
                    enabled: self.enabled,
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
