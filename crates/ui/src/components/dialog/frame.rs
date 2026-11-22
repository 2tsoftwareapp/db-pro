use egui::Ui;

use super::config::DIALOG_FOOTER_SPACING;
use crate::{components::Separator, DbProTheme};

pub struct DialogFrame<'a> {
    pub ui: &'a mut Ui,
    pub max_body_height: f32,
    /// Pinned body box from [`super::Dialog::body_height`]; `None` shrinks to content.
    pub body_height: Option<f32>,
    pub inner_width: f32,
    pub theme: DbProTheme,
    /// Optional Id salt for the body scroll area. The area otherwise derives
    /// its identity from its position in the UI hierarchy, which is stable
    /// across a dialog close/open cycle — pass a fresh value per open to
    /// reset the scroll offset back to the top (#609).
    pub body_scroll_salt: Option<egui::Id>,
}

impl<'a> DialogFrame<'a> {
    /// Renders the scrollable body of the dialog.
    pub fn body<R>(&mut self, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        let inner_w = self.inner_width;
        let area = egui::ScrollArea::vertical()
            // Stable per-dialog salt via the caller's ui — never a shared constant,
            // which would let two dialogs fight over one scroll offset (#609).
            .id_salt(self.body_scroll_salt.unwrap_or_else(|| self.ui.id().with("body_scroll")))
            .max_width(inner_w)
            // A body that overflows must advertise its scrollability; hiding the
            // bar made long dialogs look truncated with no way to discover more.
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        let area = match self.body_height {
            // Fill the pinned box instead of following content, so late-arriving
            // rows scroll inside a card that already has its final size.
            Some(height) => area.auto_shrink([false, false]).max_height(height),
            None => area.auto_shrink([false, true]).max_height(self.max_body_height),
        };
        area.show(self.ui, |body_ui| {
            body_ui.set_max_width(inner_w);
            add_contents(body_ui)
        })
        .inner
    }

    /// Renders a fixed sticky footer at the bottom of the card, outside the scroll area.
    pub fn footer<F>(&mut self, add_footer: impl FnOnce(&mut Ui) -> F) -> F {
        self.ui.add_space(DIALOG_FOOTER_SPACING);
        // A hairline above the footer splits the scrollable body from the fixed
        // actions — without it the card reads as one undifferentiated column.
        Separator::horizontal(self.theme).margin(0.0).show(self.ui);
        self.ui.add_space(DIALOG_FOOTER_SPACING);
        add_footer(self.ui)
    }
}
