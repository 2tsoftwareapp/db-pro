use egui::Ui;

use super::{config, handler};
use crate::DbProTheme;

pub struct ScrollArea {
    horizontal: bool,
    vertical: bool,
    auto_shrink: [bool; 2],
    max_height: Option<f32>,
    // egui wraps whatever we pass in `IdSalt::new(...)`, so the caller's salt
    // must reach egui unchanged — storing `egui::Id`/`IdSalt` here would
    // re-hash it and land state under a different id (broke navigator scroll
    // test after the 0.36 upgrade). `AsIdSalt` is not dyn compatible, so the
    // salt is carried as an applier closure.
    id_salt: Option<Box<dyn FnOnce(egui::ScrollArea) -> egui::ScrollArea>>,
    stick_to_bottom: Option<bool>,
    vertical_scroll_offset: Option<f32>,
    horizontal_scroll_offset: Option<f32>,
    // Retained as part of the stable constructor contract; ScrollArea currently uses egui's
    // native visuals directly and has no theme-specific paint decisions.
    #[allow(dead_code)]
    theme: DbProTheme,
}

impl ScrollArea {
    pub fn new(theme: DbProTheme) -> Self {
        Self {
            horizontal: false,
            vertical: config::DEFAULT_VERTICAL,
            auto_shrink: config::DEFAULT_AUTO_SHRINK,
            max_height: None,
            id_salt: None,
            stick_to_bottom: None,
            vertical_scroll_offset: None,
            horizontal_scroll_offset: None,
            theme,
        }
    }

    pub fn horizontal(mut self, enabled: bool) -> Self {
        self.horizontal = enabled;
        self
    }
    pub fn vertical(mut self, enabled: bool) -> Self {
        self.vertical = enabled;
        self
    }
    pub fn both(mut self) -> Self {
        self.horizontal = true;
        self.vertical = true;
        self
    }
    pub fn auto_shrink(mut self, shrink: [bool; 2]) -> Self {
        self.auto_shrink = shrink;
        self
    }
    pub fn max_height(mut self, max_height: f32) -> Self {
        self.max_height = Some(max_height);
        self
    }
    pub fn id_salt(mut self, id_salt: impl egui::AsIdSalt + 'static) -> Self {
        self.id_salt = Some(Box::new(move |area| area.id_salt(id_salt)));
        self
    }
    pub fn stick_to_bottom(mut self, stick_to_bottom: bool) -> Self {
        self.stick_to_bottom = Some(stick_to_bottom);
        self
    }
    pub fn vertical_scroll_offset(mut self, offset: f32) -> Self {
        self.vertical_scroll_offset = Some(offset);
        self
    }

    fn build(self) -> egui::ScrollArea {
        // The handler owns the axis-ordering decision; this layer only applies the
        // selected presentation settings on egui's native scroll container.
        let mut area = egui::ScrollArea::new(handler::scroll_axes(self.horizontal, self.vertical))
            .auto_shrink(self.auto_shrink)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        if let Some(max_height) = self.max_height {
            area = area.max_height(max_height);
        }
        if let Some(apply) = self.id_salt {
            area = apply(area);
        }
        if let Some(stick_to_bottom) = self.stick_to_bottom {
            area = area.stick_to_bottom(stick_to_bottom);
        }
        if let Some(offset) = self.vertical_scroll_offset {
            area = area.vertical_scroll_offset(offset);
        }
        if let Some(offset) = self.horizontal_scroll_offset {
            area = area.horizontal_scroll_offset(offset);
        }
        area
    }

    /// External horizontal offset to follow, e.g. a header scrolled by another
    /// area. The offset is applied through egui's scroll state before the area
    /// is built, so both areas stay column-aligned without nested scroll areas.
    pub fn horizontal_scroll_offset(mut self, offset: f32) -> Self {
        self.horizontal_scroll_offset = Some(offset);
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.show_with_output(ui, add_contents).inner
    }

    /// Variant of [`ScrollArea::show`] for callers that need the scroll-area id
    /// (e.g. to name the native scroll bars via [`name_scroll_bars`]).
    pub fn show_with_output<R>(
        self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        self.build().show(ui, add_contents)
    }

    /// Virtualized rows variant matching `egui::ScrollArea::show_rows`.
    pub fn show_rows(
        self,
        ui: &mut Ui,
        row_height_sans_spacing: f32,
        total_rows: usize,
        add_contents: impl FnOnce(&mut Ui, std::ops::Range<usize>),
    ) -> egui::scroll_area::ScrollAreaOutput<()> {
        self.build()
            .show_rows(ui, row_height_sans_spacing, total_rows, add_contents)
    }

    /// Viewport variant matching `egui::ScrollArea::show_viewport`.
    pub fn show_viewport<R>(
        self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui, egui::Rect) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        self.build().show_viewport(ui, add_contents)
    }
}

/// Give a native `egui::ScrollArea`'s scroll bars an accessible name.
///
/// egui emits `ScrollBar` accesskit nodes for its native bars but never names
/// them, so audits and assistive tech see anonymous controls. The bar's node
/// id is `ScrollAreaOutput::id.with(d)` (d: 0 = horizontal, 1 = vertical) —
/// an egui-internal id derivation; if egui changes it, the call is a no-op
/// (`accesskit_node_builder` returns `None` for unknown ids), never a crash.
/// Call once after `ScrollArea::show`; safe to call when accesskit is off.
pub fn name_scroll_bars(ctx: &egui::Context, scroll_area_id: egui::Id, label: &str) {
    for (d, axis) in [(0usize, "horizontal"), (1, "vertical")] {
        ctx.accesskit_node_builder(scroll_area_id.with(d), |builder| {
            builder.set_label(format!("{label} · {axis} scroll bar"));
        });
    }
}
