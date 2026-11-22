//! Result-grid viewport composition and virtualized row presentation.
use super::super::*;
use super::{GridRows, GridSelectionLookup};
use egui::Vec2;

pub(super) struct ResultGridBodyContext<'a> {
    pub(super) result: &'a UiQueryResult,
    pub(super) indexes: &'a [usize],
    pub(super) widths: &'a [f32],
    pub(super) order: &'a [usize],
    pub(super) editable: bool,
    pub(super) row_offset: u64,
    pub(super) selection_lookup: &'a GridSelectionLookup,
    pub(super) theme: DbProTheme,
}

pub(super) struct ResultGridRowInput<'a> {
    pub(super) result: &'a UiQueryResult,
    pub(super) rows: &'a GridRows<'a>,
    pub(super) position: usize,
}

pub(super) trait ResultGridBodyRenderer {
    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        result: &UiQueryResult,
        indexes: &[usize],
        widths: &[f32],
        order: &[usize],
    );

    fn draw_row(&mut self, ui: &mut egui::Ui, input: ResultGridRowInput<'_>);
}

/// Paint the leftover viewport below the last row so the grid surface reaches
/// the pane bottom: editor fill, the pinned-width gutter strip, and row-height
/// separator lines continuing the grid cadence.
fn draw_empty_fill(
    ui: &mut egui::Ui,
    rows_top: f32,
    viewport_height: f32,
    row_count: usize,
    row_height: f32,
    theme: DbProTheme,
) {
    let filler_top = ui.cursor().top();
    let filler_bottom = rows_top + viewport_height;
    if filler_bottom - filler_top < 0.5 {
        return;
    }
    let left = ui.max_rect().left();
    let right = ui.max_rect().right();
    let rect = egui::Rect::from_min_max(egui::pos2(left, filler_top), egui::pos2(right, filler_bottom));
    let painter = ui.painter();
    let border = egui::Stroke::new(1.0, theme.border_subtle);
    painter.rect_filled(rect, egui::CornerRadius::ZERO, theme.surface_editor);
    let gutter = egui::Rect::from_min_max(rect.min, egui::pos2(left + GRID_ROW_NUMBER_WIDTH, rect.bottom()));
    painter.rect_filled(gutter, egui::CornerRadius::ZERO, theme.surface_panel);
    painter.vline(gutter.right(), gutter.y_range(), border);
    let mut y = rows_top + (row_count as f32 + 1.0) * row_height;
    while y <= filler_bottom {
        painter.hline(left..=right, y, border);
        y += row_height;
    }
}

pub(super) fn draw_body(
    ui: &mut egui::Ui,
    context: ResultGridBodyContext<'_>,
    renderer: &mut dyn ResultGridBodyRenderer,
) {
    let grid_height = ui.available_height().max(180.0);
    let grid_width = ui.available_width().max(0.0);
    let body_height = (grid_height - GRID_HEADER_HEIGHT).max(140.0);

    ui.allocate_ui_with_layout(
        egui::vec2(grid_width, grid_height),
        Layout::top_down(Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // #574: the header is drawn by its own horizontal scroll area and the
            // rows by a single two-axis area below it, with the row-number gutter
            // painted as a pinned overlay. Nesting the vertical scroll area inside
            // the horizontal one put the vertical scrollbar at the right edge of
            // the *content*, so on a wide table it sat off-screen until the user
            // scrolled horizontally to the very end.
            let content_width = GRID_ROW_NUMBER_WIDTH + context.widths.iter().sum::<f32>();
            let header_scroll = crate::components::ScrollArea::new(context.theme)
                .horizontal(true)
                .vertical(false)
                .id_salt("result-grid-columns-scroll")
                .show_with_output(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    // Rows/header paint a trailing filler up to the grid edge, so the
                    // scroll content must claim the viewport width even when the
                    // columns are narrower.
                    ui.set_min_width(content_width.max(grid_width));
                    renderer.draw_header(ui, context.result, context.indexes, context.widths, context.order);
                });
            crate::components::scroll_area::name_scroll_bars(
                ui.ctx(),
                header_scroll.id,
                "Result grid columns",
            );

            let rows = GridRows {
                indexes: context.indexes,
                widths: context.widths,
                order: context.order,
                editable: context.editable,
                row_offset: context.row_offset,
                selection_lookup: context.selection_lookup,
            };
            let row_height = GRID_ROW_HEIGHT;
            let rows_top = ui.cursor().top();
            let body_scroll = crate::components::ScrollArea::new(context.theme)
                .both()
                .max_height(body_height)
                .id_salt("result-grid-rows-scroll")
                // Scroll left/right with the header so columns never drift apart.
                .horizontal_scroll_offset(header_scroll.state.offset.x)
                .show_viewport(ui, |ui, viewport| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    ui.set_min_size(egui::vec2(
                        content_width.max(grid_width),
                        context.indexes.len() as f32 * row_height,
                    ));
                    // Same row window `show_rows` would compute, done here so the
                    // area can be two-axis (#574).
                    let first = grid_first_visible_row(viewport.min.y, row_height, context.indexes.len());
                    let last = grid_last_visible_row(viewport.max.y, row_height, context.indexes.len());
                    ui.add_space(first as f32 * row_height);
                    for position in first..last {
                        renderer.draw_row(
                            ui,
                            ResultGridRowInput {
                                result: context.result,
                                rows: &rows,
                                position,
                            },
                        );
                    }
                });
            crate::components::scroll_area::name_scroll_bars(ui.ctx(), body_scroll.id, "Result rows");

            draw_empty_fill(
                ui,
                rows_top,
                body_height,
                context.indexes.len(),
                row_height,
                context.theme,
            );
        },
    );

}

/// First row index visible at vertical `offset`.
///
/// Extracted so the two-axis body area can virtualize rows exactly like
/// the `show_rows` helper did before (#574).
fn grid_first_visible_row(offset: f32, row_height: f32, row_count: usize) -> usize {
    if row_height <= 0.0 {
        return 0;
    }
    ((offset.max(0.0) / row_height).floor() as usize).min(row_count)
}

/// One past the last row index visible at vertical `offset`.
fn grid_last_visible_row(offset: f32, row_height: f32, row_count: usize) -> usize {
    if row_height <= 0.0 {
        return row_count;
    }
    let visible = (offset.max(0.0) / row_height).ceil() as usize;
    (visible + 1).min(row_count)
}

#[cfg(test)]
#[path = "tests/result_grid_body_view_tests.rs"]
mod tests;
