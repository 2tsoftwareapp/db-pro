// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! egui 0.29 paints selection backgrounds but leaves selected glyph colors unchanged.
//! Adapt its native text meshes after label selection has finalized, without reshaping text.

use std::sync::Arc;

use egui::{epaint::TextShape, Color32, Context, Shape};

pub(crate) fn install(ctx: &Context) {
    let id = egui::Id::new("db-pro-text-selection-style");
    let installed = ctx.data_mut(|data| {
        let installed = data.get_temp::<bool>(id).unwrap_or(false);
        data.insert_temp(id, true);
        installed
    });
    if installed {
        return;
    }
    ctx.on_end_pass(
        "db-pro-text-selection-style",
        Arc::new(|ctx| {
            // #585: a focused widget is *not* a selection. The old guard only
            // bailed out when nothing was focused, so typing in the SQL editor
            // walked every layer / every shape / every text row of the frame
            // just to find no selection quad. Require real selected text.
            if !has_selected_text(ctx) {
                return;
            }
            let selection = ctx.global_style().visuals.selection;
            let layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
            ctx.graphics_mut(|graphics| {
                for layer in layers {
                    if let Some(list) = graphics.get_mut(layer) {
                        for index in 0..list.next_idx().0 {
                            list.mutate_shape(egui::layers::ShapeIdx(index), |shape| {
                                style_shape(&mut shape.shape, selection.bg_fill, selection.stroke.color);
                            });
                        }
                    }
                }
            });
        }),
    );
}

/// True when any text widget actually shows selected text (#585).
///
/// Covers native label selection plus a focused `TextEdit` whose primary cursor
/// differs from its selection anchor. A caret-only editor selects nothing, so
/// the whole shape walk is skipped.
fn has_selected_text(ctx: &Context) -> bool {
    if ctx
        .plugin::<egui::text_selection::LabelSelectionState>()
        .lock()
        .has_selection()
    {
        return true;
    }
    let Some(focused) = ctx.memory(|memory| memory.focused()) else {
        return false;
    };
    let Some(state) = egui::text_edit::TextEditState::load(ctx, focused) else {
        return false;
    };
    let Some(cursor) = state.cursor.char_range() else {
        return false;
    };
    cursor.primary != cursor.secondary
}

fn style_shape(shape: &mut Shape, background: Color32, foreground: Color32) {
    match shape {
        Shape::Text(text) => style_text(text, background, foreground),
        Shape::Vec(shapes) => {
            for shape in shapes {
                style_shape(shape, background, foreground);
            }
        }
        _ => {}
    }
}

fn selection_bounds(row: &egui::epaint::text::Row, background: Color32) -> Option<egui::Rect> {
    let mesh = &row.visuals.mesh;
    let start = mesh.vertices.len().checked_sub(4)?;
    let indices = mesh
        .indices
        .get(row.visuals.glyph_index_start..row.visuals.glyph_index_start + 6)?;
    // Native Label and TextEdit append a selection quad, then insert its triangles
    // immediately before glyphs. Ordinary rich-text backgrounds do not have this layout.
    if start < row.visuals.glyph_vertex_range.end
        || !indices
            .iter()
            .all(|&index| (start..start + 4).contains(&(index as usize)))
        || !mesh.vertices[start..].iter().all(|vertex| vertex.color == background)
    {
        return None;
    }
    Some(mesh.vertices[start..].iter().fold(egui::Rect::NOTHING, |rect, vertex| {
        rect.union(egui::Rect::from_min_max(vertex.pos, vertex.pos))
    }))
}

fn style_text(text: &mut TextShape, background: Color32, foreground: Color32) {
    if !text
        .galley
        .rows
        .iter()
        .any(|row| selection_bounds(row, background).is_some())
    {
        return;
    }
    let galley = Arc::make_mut(&mut text.galley);
    if let Some(color) = text.override_text_color.take() {
        for row in &mut galley.rows {
            let row = Arc::make_mut(&mut row.row);
            for vertex in &mut row.visuals.mesh.vertices[row.visuals.glyph_vertex_range.clone()] {
                vertex.color = color;
            }
        }
    }
    for row in &mut galley.rows {
        let Some(bounds) = selection_bounds(row, background) else {
            continue;
        };
        let row = Arc::make_mut(&mut row.row);
        let mut vertices = row.visuals.mesh.vertices[row.visuals.glyph_vertex_range.clone()]
            .as_chunks_mut::<4>()
            .0
            .iter_mut();
        for glyph in &row.glyphs {
            if glyph.uv_rect.is_nothing() {
                continue;
            }
            let Some(quad) = vertices.next() else { break };
            let center = glyph.pos.x + glyph.advance_width * 0.5;
            if bounds.left() <= center && center < bounds.right() {
                for vertex in quad {
                    vertex.color = foreground;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/text_selection_style_tests.rs"]
mod tests;
