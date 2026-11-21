//! Input behavior decisions that do not require an egui painter or widget tree.

/// Returns the counter text for a textarea, keeping the displayed value consistent with the
/// same character-count rule used by the component's public `max_chars` option.
pub(crate) fn character_count(value: &str, max_chars: usize) -> String {
    format!("{}/{}", value.chars().count(), max_chars)
}

/// Chooses the accessible label for a text input, preferring an explicit label over its hint.
pub(crate) fn accessible_label<'a>(
    access_label: Option<&'a str>,
    label: Option<&'a str>,
    placeholder: &'a str,
) -> &'a str {
    access_label.or(label).unwrap_or(placeholder)
}

#[cfg(test)]
#[path = "tests/handler_tests.rs"]
mod tests;

// Pre-pass focus resolution for text fields (#609).
//
// egui applies `request_focus` only from the next pass onwards, so a pointer
// press on a text field and the characters typed in the same frame would be
// delivered to whichever field was focused when the frame started. The
// pre-pass below runs before any widget is laid out and hands focus to the
// field under the press, using the rect registry the inputs publish on each
// drawn frame.

/// The id the rect registry lives under in `ctx.data`.
pub const TEXT_FIELD_REGISTRY_ID: &str = "dbpro.text_field_registry";

/// Remember the rect of a drawn text field for the next frame's pre-pass.
pub(crate) fn register_text_field(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    let key = egui::Id::new(TEXT_FIELD_REGISTRY_ID);
    ctx.data_mut(|data| {
        let mut fields = data.get_temp::<Vec<(egui::Id, egui::Rect)>>(key).unwrap_or_default();
        fields.push((id, rect));
        data.insert_temp(key, fields);
    });
}

/// Give keyboard focus to the text field under a primary press from the input
/// events of this frame. Runs before any widget draws, so every field sees a
/// consistent focus state when it consumes this frame's text events (#609).
pub fn focus_text_field_under_pointer(ctx: &egui::Context) {
    let key = egui::Id::new(TEXT_FIELD_REGISTRY_ID);
    let Some(fields) = ctx.data_mut(|data| data.remove_temp::<Vec<(egui::Id, egui::Rect)>>(key)) else {
        return;
    };
    let press_pos = ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } => Some(*pos),
            _ => None,
        })
    });
    let Some(press_pos) = press_pos else {
        // No press: keep the registry for the next frame.
        ctx.data_mut(|data| data.insert_temp(key, fields));
        return;
    };
    if let Some((id, _)) = fields.iter().rev().find(|(_, rect)| rect.contains(press_pos)) {
        ctx.memory_mut(|memory| memory.request_focus(*id));
    }
}
