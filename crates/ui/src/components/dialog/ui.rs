use std::{borrow::Cow, hash::Hash};

use egui::{Area, CornerRadius, Frame, Id, Margin, Order, Pos2, Response, RichText, Stroke, Ui};
use lucide_icons::Icon;

use super::{
    config::{
        DIALOG_CHROME_HEIGHT, DIALOG_CONTENT_HORIZONTAL_PADDING, DIALOG_CONTENT_MIN_SIZE,
        DIALOG_CONTENT_VERTICAL_PADDING, DIALOG_FOOTER_SPACING, DIALOG_HEADER_MIN_HEIGHT, DIALOG_HORIZONTAL_MARGIN,
        DIALOG_RADIUS, DIALOG_SECTION_SPACING, DIALOG_TOTAL_HORIZONTAL_PADDING, DIALOG_TRANSLATE_PX,
        DIALOG_VERTICAL_MARGIN, DIALOG_WIDTH,
    },
    frame::DialogFrame,
    handler::{dialog_centering_height, modal_dismissal, pinned_body_height, ModalDismissal},
    layout::{overlay_widget_id, paint_dim, screen_rect_fallback, OverlayPaint},
};
use crate::{
    components::{
        animation::{fade_alpha, overlay_t, small_translate},
        button::{Button, ButtonSize, ButtonVariant},
        common::dialog_body_cap,
    },
    tokens::{font_caption, font_subheading, SPACE_XXS, STROKE_THIN},
    DbProTheme,
};

pub struct Dialog<'a> {
    open: &'a mut bool,
    title: Option<Cow<'a, str>>,
    description: Option<Cow<'a, str>>,
    width: f32,
    body_height: Option<f32>,
    id_salt: Option<Id>,
    theme: DbProTheme,
    body_scroll_salt: Option<Id>,
}

impl<'a> Dialog<'a> {
    pub fn new(open: &'a mut bool, title: impl Into<Cow<'a, str>>, theme: DbProTheme) -> Self {
        Self {
            open,
            title: Some(title.into()),
            description: None,
            width: DIALOG_WIDTH,
            body_height: None,
            id_salt: None,
            theme,
            body_scroll_salt: None,
        }
    }

    pub fn without_title(mut self) -> Self {
        self.title = None;
        self
    }

    pub fn description(mut self, description: impl Into<Cow<'a, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Pins the scrollable body to `height` points instead of sizing it to content.
    ///
    /// Content-driven dialogs resize every time asynchronous data arrives (a
    /// catalog filling in, a validation line appearing), and because the card
    /// re-centers on its measured height, each resize also slides the whole
    /// dialog upward. Forms should pin a height and let the body scroll.
    pub fn body_height(mut self, height: f32) -> Self {
        self.body_height = Some(height);
        self
    }

    pub fn id_salt(mut self, salt: impl Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(Id::new(salt));
        self
    }

    /// Salt the body scroll area's identity; pass a fresh value per dialog
    /// open to reset the scroll offset to the top (#609).
    pub fn body_scroll_salt(mut self, salt: Id) -> Self {
        self.body_scroll_salt = Some(salt);
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show_framed(ui, |frame| frame.body(add_contents))
    }

    pub fn show_ctx<R>(self, ctx: &egui::Context, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show_framed_ctx(ctx, |frame| frame.body(add_contents))
    }

    pub fn show_framed<R>(self, ui: &mut Ui, add_frame: impl FnOnce(&mut DialogFrame<'_>) -> R) -> Option<R> {
        let id = overlay_widget_id(ui, self.id_salt, "dialog");
        self.show_framed_impl(ui.ctx(), id, add_frame)
    }

    pub fn show_framed_ctx<R>(
        self,
        ctx: &egui::Context,
        add_frame: impl FnOnce(&mut DialogFrame<'_>) -> R,
    ) -> Option<R> {
        let id = self.id_salt.unwrap_or_else(|| Id::new("dbpro_dialog_overlay"));
        self.show_framed_impl(ctx, id, add_frame)
    }

    fn show_framed_impl<R>(
        self,
        ctx: &egui::Context,
        id: Id,
        add_frame: impl FnOnce(&mut DialogFrame<'_>) -> R,
    ) -> Option<R> {
        let progress = overlay_t(ctx, id.with("motion"), *self.open);
        if progress <= 0.0 {
            return None;
        }

        // Modal coordination (ui-core-audit P0-1/P0-3/P0-4): only the topmost dialog
        // reacts to Esc and the backdrop, and owns the keyboard focus trap, so stacked
        // dialogs never all close on one Esc and Tab can never escape the active dialog.
        let is_topmost = super::modal_guard::register(ctx, id);
        let card_layer = egui::LayerId::new(Order::Foreground, id.with("card"));
        let anchor_id = id.with("focus_anchor");

        if is_topmost {
            // Consume (not just observe) so the Escape press that dismisses the
            // topmost dialog cannot also reach `handle_escape` and cancel a
            // running query or toggle the agent panel.
            let escape_pressed = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if modal_dismissal(true, escape_pressed, false, None, None) == Some(ModalDismissal::Escape) {
                *self.open = false;
            }
            // Run before the card content is drawn so an explicit request_focus inside
            // the content (e.g. the first text field) later this pass wins over the anchor.
            super::modal_guard::trap_focus(ctx, card_layer, anchor_id);
        }

        let screen = screen_rect_fallback(ctx);
        let mut inner = None;
        let theme = self.theme;
        let title = self.title;
        let description = self.description;
        let open = self.open;
        let body_scroll_salt = self.body_scroll_salt;

        let prev_height = ctx.data(|d| d.get_temp::<f32>(id.with("prev_height")));
        let pinned_body = pinned_body_height(
            self.body_height,
            dialog_body_cap(screen.height(), DIALOG_VERTICAL_MARGIN, DIALOG_CHROME_HEIGHT),
        );
        let layout = crate::components::common::calculate_dialog_layout(
            screen,
            self.width,
            dialog_centering_height(pinned_body, prev_height, DIALOG_CHROME_HEIGHT),
            DIALOG_HORIZONTAL_MARGIN,
            DIALOG_VERTICAL_MARGIN,
            DIALOG_CHROME_HEIGHT,
        );

        let translate = small_translate(progress, DIALOG_TRANSLATE_PX);
        let origin = Pos2::new(layout.target_pos.x, layout.target_pos.y + translate);
        let card_alpha = fade_alpha(progress);

        // 1. Dim backdrop layer (Foreground)
        let mut backdrop_clicked = false;
        let dim_layer = egui::LayerId::new(Order::Foreground, id.with("dim"));
        Area::new(id.with("dim"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |dim_ui| {
                dim_ui.set_min_size(screen.size());
                paint_dim(
                    dim_ui,
                    OverlayPaint {
                        screen,
                        overlay: theme.overlay,
                        progress,
                    },
                );
                let resp = dim_ui.allocate_response(screen.size(), egui::Sense::click());
                if resp.clicked() {
                    backdrop_clicked = true;
                }
            });

        // 2. Dialog card — same Foreground family, pinned as a sublayer of the
        // dim so it always paints/hits directly above the overlay (never under it).
        let mut card_rect = None;
        Area::new(id.with("card"))
            .order(Order::Foreground)
            .fixed_pos(origin)
            .interactable(true)
            .show(ctx, |card_ui| {
                card_ui.set_opacity(card_alpha);
                card_ui.set_width(layout.width);
                card_ui.set_max_width(layout.width);
                let res = paint_dialog_card(
                    DialogCardPaint {
                        open,
                        title: title.as_deref(),
                        description: description.as_deref(),
                        width: layout.width,
                        max_body_height: layout.max_body_height,
                        body_height: pinned_body,
                        theme,
                        anchor_id,
                        body_scroll_salt,
                    },
                    card_ui,
                    add_frame,
                );
                let rect = card_ui.min_rect();
                card_ui
                    .ctx()
                    .data_mut(|d| d.insert_temp(id.with("prev_height"), rect.height()));
                card_rect = Some(rect);
                inner = Some(res);
            });

        ctx.set_sublayer(dim_layer, card_layer);
        ctx.memory_mut(|m| {
            m.areas_mut().move_to_top(dim_layer);
            m.areas_mut().move_to_top(card_layer);
        });

        let pointer_position = ctx.input(|input| input.pointer.interact_pos());
        if modal_dismissal(is_topmost, false, backdrop_clicked, pointer_position, card_rect)
            == Some(ModalDismissal::Backdrop)
        {
            *open = false;
        }

        inner
    }
}

struct DialogCardPaint<'a> {
    open: &'a mut bool,
    title: Option<&'a str>,
    description: Option<&'a str>,
    width: f32,
    max_body_height: f32,
    body_height: Option<f32>,
    theme: DbProTheme,
    anchor_id: Id,
    body_scroll_salt: Option<Id>,
}

fn paint_dialog_card<R>(
    card: DialogCardPaint<'_>,
    ui: &mut Ui,
    add_frame: impl FnOnce(&mut DialogFrame<'_>) -> R,
) -> R {
    let DialogCardPaint {
        open,
        title,
        description,
        width,
        max_body_height,
        body_height,
        theme,
        anchor_id,
        body_scroll_salt,
    } = card;
    Frame {
        fill: theme.surface_floating,
        stroke: Stroke::new(STROKE_THIN, theme.border_subtle),
        inner_margin: Margin::symmetric(
            DIALOG_CONTENT_HORIZONTAL_PADDING as i8,
            DIALOG_CONTENT_VERTICAL_PADDING as i8,
        ),
        corner_radius: CornerRadius::same(DIALOG_RADIUS as u8),
        shadow: theme.floating_shadow(),
        ..Default::default()
    }
    .show(ui, |ui| {
        // Hidden focus anchor: the first focusable widget in the dialog's own layer,
        // so `modal_guard::trap_focus` always has an in-dialog target to fall back to
        // when keyboard focus tries to escape to the window behind the dialog.
        ui.interact(
            egui::Rect::from_min_size(ui.cursor().min, egui::Vec2::ZERO),
            anchor_id,
            egui::Sense::focusable_noninteractive(),
        );

        let inner_w = (width - DIALOG_TOTAL_HORIZONTAL_PADDING).max(DIALOG_CONTENT_MIN_SIZE);
        ui.set_width(inner_w);
        ui.set_max_width(inner_w);

        draw_dialog_header(ui, title, description, open, theme, inner_w);

        ui.add_space(DIALOG_SECTION_SPACING);
        // Header/body boundary: same hairline the footer gets, so the card reads
        // as three zones instead of one undifferentiated column.
        crate::components::Separator::horizontal(theme).margin(0.0).show(ui);
        ui.add_space(DIALOG_SECTION_SPACING);

        let mut frame = DialogFrame {
            ui,
            max_body_height,
            body_height,
            inner_width: inner_w,
            theme,
            body_scroll_salt,
        };
        add_frame(&mut frame)
    })
    .inner
}

fn draw_dialog_header(
    ui: &mut Ui,
    title: Option<&str>,
    description: Option<&str>,
    open: &mut bool,
    theme: DbProTheme,
    width: f32,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y.max(DIALOG_HEADER_MIN_HEIGHT)),
        egui::Layout::left_to_right(egui::Align::TOP),
        |ui| {
            if title.is_some() || description.is_some() {
                ui.vertical(|ui| {
                    if let Some(title) = title {
                        ui.add(
                            egui::Label::new(RichText::new(title).font(font_subheading()).color(theme.text_primary))
                                .truncate(),
                        )
                        .on_hover_text(title);
                    }
                    if let Some(description) = description {
                        if title.is_some() {
                            ui.add_space(SPACE_XXS);
                        }
                        ui.label(RichText::new(description).font(font_caption()).color(theme.text_muted));
                    }
                });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if close_icon_button(ui, theme).clicked() {
                    *open = false;
                }
            });
        },
    );
}

pub fn close_icon_button(ui: &mut Ui, theme: DbProTheme) -> Response {
    Button::new(theme)
        .icon(Icon::X)
        .size(ButtonSize::Icon)
        .variant(ButtonVariant::Ghost)
        .tooltip("Close (Esc)")
        .access_label("Close (Esc)")
        .show(ui)
}

pub struct DialogActionLabels<'a> {
    pub secondary: &'a str,
    pub primary: &'a str,
}

pub fn dialog_actions(ui: &mut Ui, theme: DbProTheme, labels: DialogActionLabels<'_>) -> (bool, bool) {
    let mut secondary_clicked = false;
    let mut primary_clicked = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        primary_clicked = Button::new(theme).text(labels.primary).show(ui).clicked();
        ui.add_space(DIALOG_FOOTER_SPACING);
        secondary_clicked = Button::new(theme)
            .text(labels.secondary)
            .variant(ButtonVariant::Ghost)
            .show(ui)
            .clicked();
    });
    (secondary_clicked, primary_clicked)
}
