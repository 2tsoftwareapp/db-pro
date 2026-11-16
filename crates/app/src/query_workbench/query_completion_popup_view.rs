use super::query_editor_support::{apply_completion_item, close_completion};
use super::*;
use crate::editor::{CompletionItem, CompletionItemKind, SqlDialect};
use egui::RichText;

const COMPLETION_ROW_HEIGHT: f32 = 30.0;
const COMPLETION_ROW_OVERSCAN: usize = 2;
const COMPLETION_VISIBLE_ROWS: usize = 8;
const COMPLETION_KIND_WIDTH: f32 = 58.0;

pub(super) fn draw_floating_completion_popup(
    ctx: &egui::Context,
    theme: DbProTheme,
    dialect: SqlDialect,
    doc: &mut QueryDocument,
) {
    if !doc.completion.is_open || doc.completion.items.is_empty() {
        return;
    }

    let mut apply_item = None;
    let mut close_popup = false;

    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key { key, pressed: true, .. } = event {
                match key {
                    egui::Key::ArrowUp => {
                        doc.completion.select_prev();
                    }
                    egui::Key::ArrowDown => {
                        doc.completion.select_next();
                    }
                    egui::Key::PageUp => {
                        doc.completion.select_page_up(5);
                    }
                    egui::Key::PageDown => {
                        doc.completion.select_page_down(5);
                    }
                    egui::Key::Enter | egui::Key::Tab => {
                        if let Some(item) = doc.completion.current_item() {
                            apply_item = Some(item.clone());
                        }
                    }
                    egui::Key::Escape => {
                        close_popup = true;
                    }
                    _ => {}
                }
            }
        }
    });

    if let Some(item) = apply_item {
        apply_completion_item(doc, &item, dialect);
        return;
    }

    if close_popup {
        close_completion(doc);
        return;
    }

    let popup_width = 460.0;
    let list_rows = doc.completion.items.len().clamp(1, COMPLETION_VISIBLE_ROWS);
    let popup_height = list_rows as f32 * COMPLETION_ROW_HEIGHT + 36.0;
    let popup_pos = crate::components::clamp_popup_to_screen(
        doc.completion.popup_position,
        egui::vec2(popup_width, popup_height),
        ctx.content_rect(),
        10.0,
    );

    let mut clicked_item = None;

    let area_resp = FloatingPopup::new(egui::Id::new("floating_completion_popup"), theme, popup_pos)
        .order(egui::Order::Foreground)
        .surface_elevated()
        .radius(10.0)
        .margin(egui::Margin::symmetric(4.0 as i8, 4.0 as i8))
        .show(ctx, |ui| {
            ui.set_width(popup_width);
            ui.spacing_mut().item_spacing.y = 2.0;

            let prefix = doc.completion.query_prefix.clone();
            let count = doc.completion.items.len();
            let sel_idx = doc.completion.selected_index.min(count.saturating_sub(1));
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            let list_height = (count.min(COMPLETION_VISIBLE_ROWS) as f32 * COMPLETION_ROW_HEIGHT).max(COMPLETION_ROW_HEIGHT);
            let click_idx = ScrollArea::new(theme)
                .max_height(list_height)
                .show_viewport(ui, |ui, viewport| {
                    // Fixed content height keeps the scrollbar matched to the full list
                    // while only the visible rows (plus overscan) are built.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.set_height(count as f32 * COMPLETION_ROW_HEIGHT);
                    let scroll_y = viewport.min.y.max(0.0);
                    let view_h = viewport.height().max(COMPLETION_ROW_HEIGHT);
                    let first = (scroll_y / COMPLETION_ROW_HEIGHT).floor() as usize;
                    let last = ((scroll_y + view_h) / COMPLETION_ROW_HEIGHT).ceil() as usize;
                    let start = first.saturating_sub(COMPLETION_ROW_OVERSCAN).min(count);
                    let end = last.saturating_add(COMPLETION_ROW_OVERSCAN).min(count);
                    let top = ui.max_rect().top();
                    let left = ui.max_rect().left();
                    let width = ui.max_rect().width();
                    let mut clicked = None;
                    for idx in start..end {
                        let rect = completion_row_rect(left, top, width, idx);
                        if draw_completion_row(
                            ui,
                            rect,
                            idx,
                            &doc.completion.items[idx],
                            idx == sel_idx,
                            &prefix,
                            theme,
                        ) {
                            clicked = Some(idx);
                        }
                    }
                    // Selection can sit outside the viewport after a key press. Build that
                    // one row so scroll_to_me can bring it back; hover does not move it.
                    if !(start..end).contains(&sel_idx) {
                        let rect = completion_row_rect(left, top, width, sel_idx);
                        if draw_completion_row(
                            ui,
                            rect,
                            sel_idx,
                            &doc.completion.items[sel_idx],
                            true,
                            &prefix,
                            theme,
                        ) {
                            clicked = Some(sel_idx);
                        }
                    }
                    clicked
                })
                .inner;
            if let Some(idx) = click_idx {
                clicked_item = doc.completion.items.get(idx).cloned();
            }
            if let Some(note) = doc.completion.current_item().and_then(completion_note) {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(note).font(font_caption()).color(theme.text_muted)).truncate(),
                );
            }
        });

    if let Some(item) = clicked_item {
        apply_completion_item(doc, &item, dialect);
    }

    // Close on click outside
    let clicked_outside = ctx.input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area_resp.response.rect.contains(pos))
    });
    if clicked_outside {
        close_completion(doc);
    }
}

fn completion_row_rect(left: f32, top: f32, width: f32, idx: usize) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(left, top + idx as f32 * COMPLETION_ROW_HEIGHT),
        egui::vec2(width, COMPLETION_ROW_HEIGHT),
    )
}

fn draw_completion_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    idx: usize,
    item: &CompletionItem,
    selected: bool,
    prefix: &str,
    theme: DbProTheme,
) -> bool {
    if selected {
        let highlight = rect.shrink2(egui::vec2(2.0, 1.0));
        ui.painter()
            .rect_filled(highlight, egui::CornerRadius::same(6.0 as u8), theme.accent_soft);
    }
    let content = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(content)
            .id_salt(("completion_row_body", idx))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            ui.add_sized(
                [COMPLETION_KIND_WIDTH, content.height()],
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                egui::Label::new(RichText::new(completion_kind_label(item.kind)).font(font_caption()).color(theme.text_muted)),
            );
            let detail = completion_row_detail(item);
            let available = ui.available_width().max(0.0);
            let detail_budget = if detail.is_empty() {
                0.0
            } else {
                (available * 0.38).clamp(72.0, 180.0).min(available)
            };
            let label_budget = (available - detail_budget).max(0.0);
            ui.add_sized(
                [label_budget, content.height()],
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                egui::Label::new(completion_label_job(&item.label, prefix, theme.text_primary, theme.accent)).truncate(),
            );
            if !detail.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_sized(
                        [detail_budget, content.height()],
                        egui::Label::new(RichText::new(detail).font(font_caption()).color(theme.text_muted)).truncate(),
                    );
                });
            }
        },
    );

    // Hover must not change selected_index. Click applies; scroll_to_me keeps the key selection in view.
    let response = ui.interact(rect, ui.id().with(("completion_row", idx)), egui::Sense::click());
    if selected {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response.clicked()
}

fn completion_kind_label(kind: CompletionItemKind) -> &'static str {
    match kind {
        CompletionItemKind::Keyword => "key",
        CompletionItemKind::Table => "table",
        CompletionItemKind::View => "view",
        CompletionItemKind::Column => "column",
        CompletionItemKind::Function => "fn",
        CompletionItemKind::Schema => "schema",
        CompletionItemKind::Snippet => "join",
        CompletionItemKind::Cte => "cte",
    }
}

/// Drop the leading kind word. The row already shows it in the left column.
fn completion_row_detail(item: &CompletionItem) -> &str {
    let Some(detail) = item.detail.as_deref() else {
        return "";
    };
    detail.split_once(" · ").map(|(_, rest)| rest.trim()).unwrap_or(detail)
}

fn completion_note(item: &CompletionItem) -> Option<&str> {
    item.documentation
        .as_deref()
        .filter(|text| !text.is_empty())
        .or_else(|| item.detail.as_deref().filter(|text| !text.is_empty()))
}

fn completion_label_job(
    label: &str,
    prefix: &str,
    text_color: egui::Color32,
    accent: egui::Color32,
) -> egui::text::LayoutJob {
    let font_id = FontId::proportional(13.0);
    let normal = egui::TextFormat {
        font_id: font_id.clone(),
        color: text_color,
        ..Default::default()
    };
    let matched = egui::TextFormat {
        font_id,
        color: accent,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    match prefix_byte_span(label, prefix) {
        Some((start, end)) => {
            job.append(&label[..start], 0.0, normal.clone());
            job.append(&label[start..end], 0.0, matched);
            job.append(&label[end..], 0.0, normal);
        }
        None => job.append(label, 0.0, normal),
    }
    job
}

/// Byte range of `prefix` inside `label`, case-insensitive. Empty prefix matches nothing,
/// so the whole label stays in the normal text color.
fn prefix_byte_span(label: &str, prefix: &str) -> Option<(usize, usize)> {
    if prefix.is_empty() || label.is_empty() {
        return None;
    }
    let prefix_lower = prefix.to_lowercase();
    let mut label_lower = String::with_capacity(label.len());
    let mut lower_to_byte = Vec::with_capacity(label.len());
    for (byte, ch) in label.char_indices() {
        for lower in ch.to_lowercase() {
            label_lower.push(lower);
            lower_to_byte.push(byte);
        }
    }
    let start = label_lower.find(&prefix_lower)?;
    let end = start + prefix_lower.len();
    let byte_start = *lower_to_byte.get(start)?;
    let byte_end = lower_to_byte.get(end).copied().unwrap_or(label.len());
    if byte_start < byte_end && label.is_char_boundary(byte_start) && label.is_char_boundary(byte_end) {
        Some((byte_start, byte_end))
    } else {
        None
    }
}
