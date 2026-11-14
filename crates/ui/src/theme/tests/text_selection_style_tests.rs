//! Tests for `../text_selection_style.rs` (moved out of the source file).
use super::*;

#[test]
fn native_label_drag_selection_is_white_without_recoloring_unselected_text() {
    let ctx = Context::default();
    crate::DbProTheme::install_fonts(&ctx);
    let theme = crate::DbProTheme::light();
    theme.apply(&ctx);
    let mut rect = egui::Rect::NOTHING;
    let _ = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new("Correct the highlighted fields, then save again.")
                            .color(theme.text_secondary),
                    )
                    .selectable(true),
                )
                .rect;
        });
    });
    let start = rect.left_center() + egui::vec2(2.0, 0.0);
    let end = start + egui::vec2(70.0, 0.0);
    let mut output = None;
    for events in [
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        output = Some(crate::test_frame::frame(
            &ctx,
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("Correct the highlighted fields, then save again.")
                                .color(theme.text_secondary),
                        )
                        .selectable(true),
                    );
                });
            },
        ));
    }
    let output = output.unwrap();
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Text(text) if text.galley.text().starts_with("Correct") => Some(text),
            _ => None,
        })
        .unwrap();
    let row = &text.galley.rows[0];
    assert!(selection_bounds(row, ctx.global_style().visuals.selection.bg_fill).is_some());
    let vertices = &row.visuals.mesh.vertices[row.visuals.glyph_vertex_range.clone()];
    assert!(vertices.iter().any(|vertex| vertex.color == Color32::WHITE));
    assert!(vertices.iter().any(|vertex| vertex.color == theme.text_secondary));
}

#[test]
fn native_text_edit_selection_recolors_only_selected_glyphs_in_both_themes() {
    for theme in [crate::DbProTheme::light(), crate::DbProTheme::dark()] {
        let ctx = Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        theme.apply(&ctx);
        theme.apply(&ctx); // Installation must not multiply callbacks each frame.
        let id = egui::Id::new("selected-input");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(1),
            egui::text::CCursor::new(4),
        )));
        state.store(&ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
        let mut value = "abcdef".to_owned();
        let output = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut value) // ui-rules: test deliberately exercises the raw egui widget to verify global selection styling
                        .id(id)
                        .text_color(theme.text_secondary),
                );
            });
        });
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.text() == "abcdef" => Some(text),
                _ => None,
            })
            .expect("native TextEdit paint output");
        let row = &text.galley.rows[0];
        assert!(selection_bounds(row, ctx.global_style().visuals.selection.bg_fill).is_some());
        for (index, quad) in row.visuals.mesh.vertices[row.visuals.glyph_vertex_range.clone()]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            let expected = if (1..4).contains(&index) {
                Color32::WHITE
            } else {
                theme.text_secondary
            };
            assert!(quad.iter().all(|vertex| vertex.color == expected), "glyph {index}");
        }
        assert_eq!(value, "abcdef");
    }
}

/// #585: a focused-but-unselected editor must not trigger the shape walk.
#[test]
fn caret_only_focus_does_not_count_as_a_selection() {
    let ctx = Context::default();
    crate::DbProTheme::install_fonts(&ctx);
    let mut text_edit_id = None;
    let _ = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut text = "SELECT 1;".to_owned();
            let response = ui.add(
                egui::TextEdit::singleline(&mut text) // ui-rules: #585 probe for the raw egui TextEdit state
                    .text_color(crate::DbProTheme::light().text_secondary),
            );
            text_edit_id = Some(response.id.with("text"));
        });
    });
    let id = text_edit_id.expect("text edit id");
    // The widget stored a cursor with `primary == secondary` (no selection).
    let state = egui::text_edit::TextEditState::load(&ctx, id).expect("text edit state");
    if let Some(cursor) = state.cursor.char_range() {
        assert_eq!(cursor.primary, cursor.secondary);
    }
    ctx.memory_mut(|memory| memory.request_focus(id));
    assert!(!has_selected_text(&ctx), "caret-only focus selects nothing");
}

/// #585: an explicit cursor range (anchor != primary) *is* a selection, so the
/// styling pass still runs for real selections.
#[test]
fn explicit_cursor_range_counts_as_a_selection() {
    let ctx = Context::default();
    crate::DbProTheme::install_fonts(&ctx);
    let id = egui::Id::new("db-pro-selection-probe");
    let mut state = egui::text_edit::TextEditState::default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(4),
    )));
    state.store(&ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
    assert!(has_selected_text(&ctx));
}
