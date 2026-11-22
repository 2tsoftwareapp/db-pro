// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use egui::{Area, CornerRadius, FontFamily, FontId, Margin, Order, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use lucide_icons::Icon;

use super::{config, floating_surface, screen_rect};
use crate::{
    tokens::{DURATION_TOAST_DEFAULT_SECS, ICON_TEXT_GAP, STROKE_THIN},
    DbProTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastVariant {
    Default,
    Success,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

impl ToastPosition {
    pub fn alignment_and_pos(&self, screen: Rect) -> (Pos2, egui::Align2) {
        let margin_x = config::TOAST_SCREEN_MARGIN;
        let margin_y = config::TOAST_SCREEN_MARGIN;
        match self {
            ToastPosition::TopLeft => (
                Pos2::new(screen.left() + margin_x, screen.top() + margin_y),
                egui::Align2::LEFT_TOP,
            ),
            ToastPosition::TopCenter => (
                Pos2::new(screen.center().x, screen.top() + margin_y),
                egui::Align2::CENTER_TOP,
            ),
            ToastPosition::TopRight => (
                Pos2::new(screen.right() - margin_x, screen.top() + margin_y),
                egui::Align2::RIGHT_TOP,
            ),
            ToastPosition::BottomLeft => (
                Pos2::new(screen.left() + margin_x, screen.bottom() - margin_y),
                egui::Align2::LEFT_BOTTOM,
            ),
            ToastPosition::BottomCenter => (
                Pos2::new(screen.center().x, screen.bottom() - margin_y),
                egui::Align2::CENTER_BOTTOM,
            ),
            ToastPosition::BottomRight => (
                Pos2::new(screen.right() - margin_x, screen.bottom() - margin_y),
                egui::Align2::RIGHT_BOTTOM,
            ),
        }
    }
}

pub struct Toast<'a> {
    message: &'a str,
    variant: ToastVariant,
    position: ToastPosition,
    action_label: Option<&'a str>,
    closable: bool,
    theme: DbProTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToastResponse {
    pub action_clicked: bool,
    pub dismiss_clicked: bool,
}

impl<'a> Toast<'a> {
    pub fn new(message: &'a str, theme: DbProTheme) -> Self {
        Self {
            message,
            variant: ToastVariant::Default,
            position: ToastPosition::BottomRight,
            action_label: None,
            closable: true,
            theme,
        }
    }

    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    pub fn top_left(mut self) -> Self {
        self.position = ToastPosition::TopLeft;
        self
    }

    pub fn top_center(mut self) -> Self {
        self.position = ToastPosition::TopCenter;
        self
    }

    pub fn top_right(mut self) -> Self {
        self.position = ToastPosition::TopRight;
        self
    }

    pub fn bottom_left(mut self) -> Self {
        self.position = ToastPosition::BottomLeft;
        self
    }

    pub fn bottom_center(mut self) -> Self {
        self.position = ToastPosition::BottomCenter;
        self
    }

    pub fn bottom_right(mut self) -> Self {
        self.position = ToastPosition::BottomRight;
        self
    }

    pub fn action(mut self, label: &'a str) -> Self {
        self.action_label = Some(label);
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn show_floating(self, ui: &Ui, salt: impl std::hash::Hash + std::fmt::Debug) -> ToastResponse {
        let screen = screen_rect(ui);
        let (pos, pivot) = self.position.alignment_and_pos(screen);
        let mut response = ToastResponse::default();
        Area::new(egui::Id::new(("toast", salt)))
            .order(Order::Tooltip)
            .fixed_pos(pos)
            .pivot(pivot)
            .show(ui.ctx(), |ui| {
                response = self.show(ui);
            });
        response
    }

    pub fn show(self, ui: &mut Ui) -> ToastResponse {
        let (icon, icon_color, accent_color) = match self.variant {
            ToastVariant::Default => (Icon::Info, self.theme.text_secondary, self.theme.accent),
            ToastVariant::Success => (Icon::CheckCircle2, self.theme.success, self.theme.success),
            ToastVariant::Danger => (Icon::AlertCircle, self.theme.danger, self.theme.danger),
        };

        let mut toast_resp = ToastResponse::default();
        let frame_resp = floating_surface(
            self.theme,
            config::TOAST_RADIUS,
            Margin::symmetric(
                config::TOAST_HORIZONTAL_PADDING as i8,
                config::TOAST_VERTICAL_PADDING as i8,
            ),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(char::from(icon).to_string())
                        .font(FontId::new(config::TOAST_ICON_SIZE, FontFamily::Name("lucide".into())))
                        .color(icon_color),
                );
                ui.add_space(ICON_TEXT_GAP);
                ui.label(
                    RichText::new(self.message)
                        .size(config::TOAST_MESSAGE_SIZE)
                        .color(self.theme.text_primary),
                );

                if let Some(label) = self.action_label {
                    ui.add_space(config::TOAST_ACTION_GAP);
                    let action_btn = egui::Button::new(
                        RichText::new(label)
                            .size(config::TOAST_ACTION_SIZE)
                            .strong()
                            .color(self.theme.accent),
                    )
                    .fill(self.theme.surface_hover)
                    .stroke(Stroke::new(STROKE_THIN, self.theme.border_subtle))
                    .corner_radius(CornerRadius::same(config::TOAST_ACTION_RADIUS as u8));

                    if ui.add(action_btn).clicked() {
                        toast_resp.action_clicked = true;
                    }
                }

                if self.closable {
                    ui.add_space(config::TOAST_CLOSE_GAP);
                    let close_btn = egui::Button::new(
                        RichText::new(char::from(Icon::X).to_string())
                            .font(FontId::new(config::TOAST_CLOSE_SIZE, FontFamily::Name("lucide".into())))
                            .color(self.theme.text_muted),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .frame(false);

                    let close_resp = ui.add(close_btn);
                    if close_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if close_resp.clicked() {
                        toast_resp.dismiss_clicked = true;
                    }
                }
            });
        });

        // Paint accent bar on the left edge
        let frame_rect = frame_resp.response.rect;
        let accent_rect = Rect::from_min_size(
            frame_rect.left_top(),
            Vec2::new(config::TOAST_ACCENT_WIDTH, frame_rect.height()),
        );
        ui.painter().rect_filled(
            accent_rect,
            CornerRadius {
                nw: (config::TOAST_RADIUS) as u8,
                ne: 0.0 as u8,
                sw: (config::TOAST_RADIUS) as u8,
                se: 0.0 as u8,
            },
            accent_color,
        );

        toast_resp
    }
}

/// Item in the ToastManager queue.
#[derive(Debug, Clone)]
pub struct ToastItem {
    pub id: u64,
    pub message: String,
    pub variant: ToastVariant,
    pub position: ToastPosition,
    pub action_label: Option<String>,
    pub duration_secs: f32,
    pub elapsed_secs: f32,
    /// Errors stay until dismissed or the user moves on: a 5 s timer hid the
    /// message before anyone could read or copy it (#571).
    pub persistent: bool,
    /// Set while the pointer is over the toast or it holds keyboard focus, so
    /// the countdown pauses instead of expiring mid-read (#571).
    pub hovered: bool,
}

/// Multi-toast manager supporting simultaneous queued/stacked toasts across all positions.
///
/// Error storms can't bury the screen: identical `(variant, position, message)`
/// pushes refresh the existing toast instead of stacking, and each position
/// keeps at most [`MAX_TOASTS_PER_POSITION`] entries (oldest drops) (#392).
#[derive(Debug, Clone, Default)]
pub struct ToastManager {
    toasts: Vec<ToastItem>,
    next_id: u64,
}

/// Max stacked toasts kept per position; oldest drop past the cap (#392).
pub const MAX_TOASTS_PER_POSITION: usize = 5;

impl ToastManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a toast with dedupe + per-position cap (#392). An identical live
    /// toast just resets its timer instead of stacking a duplicate.
    fn push(&mut self, item: ToastItem) -> u64 {
        if let Some(existing) = self.toasts.iter_mut().find(|existing| {
            existing.variant == item.variant
                && existing.position == item.position
                && existing.message == item.message
                && existing.action_label == item.action_label
        }) {
            existing.elapsed_secs = 0.0;
            existing.hovered = false;
            return existing.id;
        }
        let id = item.id;
        let position = item.position;
        self.toasts.push(item);
        let at_position: Vec<usize> = self
            .toasts
            .iter()
            .enumerate()
            .filter(|(_, t)| t.position == position)
            .map(|(index, _)| index)
            .collect();
        if at_position.len() > MAX_TOASTS_PER_POSITION {
            for &index in at_position[..at_position.len() - MAX_TOASTS_PER_POSITION].iter().rev() {
                self.toasts.remove(index);
            }
        }
        id
    }

    fn next_item(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Advance expiry without rendering — callers on a per-frame tick keep
    /// toasts expiring even while the surface that renders them is hidden.
    pub fn tick(&mut self, dt_secs: f32) {
        for item in &mut self.toasts {
            if item.hovered || item.persistent {
                continue;
            }
            item.elapsed_secs += dt_secs.max(0.0);
        }
        self.toasts
            .retain(|item| item.persistent || item.hovered || item.elapsed_secs < item.duration_secs);
    }

    /// Mark a toast as hovered/focused for the next [`Self::tick`].
    pub fn set_hovered(&mut self, id: u64, hovered: bool) {
        if let Some(item) = self.toasts.iter_mut().find(|item| item.id == id) {
            item.hovered = hovered;
        }
    }

    pub fn show(&mut self, message: impl Into<String>, variant: ToastVariant, position: ToastPosition) -> u64 {
        // Errors are sticky by default: the status line has a single slot, so an
        // error that disappears leaves no trace of what failed (#571).
        let persistent = variant == ToastVariant::Danger;
        self.show_with_duration(message, variant, position, persistent, DURATION_TOAST_DEFAULT_SECS)
    }

    /// Info/success toasts can opt into the manual-dismiss behaviour as well.
    pub fn show_persistent(
        &mut self,
        message: impl Into<String>,
        variant: ToastVariant,
        position: ToastPosition,
    ) -> u64 {
        self.show_with_duration(message, variant, position, true, DURATION_TOAST_DEFAULT_SECS)
    }

    fn show_with_duration(
        &mut self,
        message: impl Into<String>,
        variant: ToastVariant,
        position: ToastPosition,
        persistent: bool,
        duration_secs: f32,
    ) -> u64 {
        let id = self.next_item();
        self.push(ToastItem {
            id,
            message: message.into(),
            variant,
            position,
            action_label: None,
            duration_secs,
            elapsed_secs: 0.0,
            persistent,
            hovered: false,
        })
    }

    pub fn show_with_action(
        &mut self,
        message: impl Into<String>,
        variant: ToastVariant,
        position: ToastPosition,
        action: impl Into<String>,
    ) -> u64 {
        let id = self.next_item();
        self.push(ToastItem {
            id,
            message: message.into(),
            variant,
            position,
            action_label: Some(action.into()),
            duration_secs: config::TOAST_ACTION_DURATION_SECS,
            elapsed_secs: 0.0,
            persistent: false,
            hovered: false,
        })
    }

    pub fn success(&mut self, message: impl Into<String>, position: ToastPosition) -> u64 {
        self.show(message, ToastVariant::Success, position)
    }

    /// Errors are persistent (#571) and carry a copy action so the message can
    /// be pasted into a bug report after the toast is dismissed.
    pub fn error(&mut self, message: impl Into<String>, position: ToastPosition) -> u64 {
        let message = message.into();
        let id = self.next_item();
        self.push(ToastItem {
            id,
            message,
            variant: ToastVariant::Danger,
            position,
            action_label: Some(config::TOAST_COPY_ACTION_LABEL.to_owned()),
            duration_secs: DURATION_TOAST_DEFAULT_SECS,
            elapsed_secs: 0.0,
            persistent: true,
            hovered: false,
        })
    }

    pub fn info(&mut self, message: impl Into<String>, position: ToastPosition) -> u64 {
        self.show(message, ToastVariant::Default, position)
    }

    pub fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Renders all stacked toasts grouped by their position using an egui Context.
    pub fn render_ctx(&mut self, ctx: &egui::Context, theme: DbProTheme) -> Vec<(u64, ToastResponse)> {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        self.tick(dt);

        if self.toasts.is_empty() {
            return Vec::new();
        }

        let screen = ctx.content_rect();
        let mut results = Vec::new();
        let mut to_dismiss = Vec::new();

        let positions = [
            ToastPosition::TopLeft,
            ToastPosition::TopCenter,
            ToastPosition::TopRight,
            ToastPosition::BottomLeft,
            ToastPosition::BottomCenter,
            ToastPosition::BottomRight,
        ];

        for pos in positions {
            let items: Vec<(usize, ToastItem)> = self
                .toasts
                .iter()
                .enumerate()
                .filter(|(_, t)| t.position == pos)
                .map(|(i, t)| (i, t.clone()))
                .collect();

            if items.is_empty() {
                continue;
            }

            let (base_pos, pivot) = pos.alignment_and_pos(screen);
            let is_top = matches!(
                pos,
                ToastPosition::TopLeft | ToastPosition::TopCenter | ToastPosition::TopRight
            );

            // Clamp the stack to the screen: past the cap extra toasts never
            // render off-viewport (#392).
            let max_stack = ((screen.height() - 2.0 * config::TOAST_SCREEN_MARGIN) / config::TOAST_STACK_OFFSET)
                .floor()
                .max(1.0) as usize;
            for (stack_idx, (_idx, item)) in items.into_iter().take(max_stack).enumerate() {
                let offset_y = stack_idx as f32 * config::TOAST_STACK_OFFSET;
                let actual_y = if is_top {
                    (base_pos.y + offset_y).min(screen.bottom() - config::TOAST_SCREEN_MARGIN)
                } else {
                    (base_pos.y - offset_y).max(screen.top() + config::TOAST_SCREEN_MARGIN)
                };
                let item_pos = Pos2::new(base_pos.x, actual_y);

                let mut toast_widget = Toast::new(&item.message, theme)
                    .variant(item.variant)
                    .position(item.position)
                    .closable(true);

                if let Some(ref action) = item.action_label {
                    toast_widget = toast_widget.action(action);
                }

                let mut resp = ToastResponse::default();
                Area::new(egui::Id::new(("toast_manager", item.id)))
                    .order(Order::Tooltip)
                    .fixed_pos(item_pos)
                    .pivot(pivot)
                    .show(ctx, |ui| {
                        resp = toast_widget.show(ui);
                    });

                // Pause the countdown while the toast is hovered (#571).
                self.set_hovered(item.id, ctx.pointer_hover_pos().is_some_and(|pos| {
                    ctx.memory(|memory| memory.area_rect(egui::Id::new(("toast_manager", item.id))))
                        .is_some_and(|rect| rect.contains(pos))
                }));

                if resp.dismiss_clicked {
                    to_dismiss.push(item.id);
                }
                if resp.action_clicked {
                    // The error toast's action copies the message (#571) so it
                    // survives dismissal.
                    if item.persistent {
                        let message = item.message.clone();
                        ctx.output_mut(|output| {
                            output.commands.push(egui::OutputCommand::CopyText(message.clone()));
                        });
                    }
                    results.push((item.id, resp));
                }
            }
        }

        for id in to_dismiss {
            self.dismiss(id);
        }

        results
    }

    /// Renders all stacked toasts grouped by their position, handling auto-dismiss and clicks.
    pub fn render(&mut self, ui: &Ui, theme: DbProTheme) -> Vec<(u64, ToastResponse)> {
        self.render_ctx(ui.ctx(), theme)
    }
}

#[cfg(test)]
#[path = "tests/toast_tests.rs"]
mod tests;
