use super::*;

fn render_editor_event(initial_text: &str, event: egui::Event, id_salt: &str) -> (String, SqlEditorResponse) {
    let ctx = egui::Context::default();
    let mut buffer = TextBuffer::from_string(initial_text);
    let end = buffer.len_bytes();
    let mut cursor = CursorPosition::from_offset(&buffer, end);
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();
    let mut editor_response = SqlEditorResponse::default();

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            events: vec![event],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor_id = ui.make_persistent_id(id_salt);
                ui.memory_mut(|memory| memory.request_focus(editor_id));
                editor_response = SqlEditor::new(
                    &mut buffer,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    id_salt,
                )
                .show(ui, egui::vec2(600.0, 400.0));
            });
        },
    );

    (buffer.text().to_owned(), editor_response)
}

fn render_editor_event_with_selection(
    initial_text: &str,
    selection: SelectionRange,
    cursor_offset: usize,
    event: egui::Event,
) -> (String, SelectionRange) {
    let ctx = egui::Context::default();
    let mut buffer = TextBuffer::from_string(initial_text);
    let mut cursor = CursorPosition::from_offset(&buffer, cursor_offset);
    let mut sel = selection;
    let theme = DbProTheme::dark();

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            events: vec![event],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor_id = ui.make_persistent_id("sel-enter");
                ui.memory_mut(|memory| memory.request_focus(editor_id));
                SqlEditor::new(
                    &mut buffer,
                    &mut cursor,
                    &mut sel,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "sel-enter",
                )
                .show(ui, egui::vec2(600.0, 400.0));
            });
        },
    );

    (buffer.text().to_owned(), sel)
}

/// Ctrl+A → Enter used to replace the whole buffer with a newline (#342):
/// Enter must collapse the selection and insert at its end instead.
#[test]
fn enter_with_selection_collapses_instead_of_deleting() {
    let text = "SELECT 1;";
    let (result, selection) = render_editor_event_with_selection(
        text,
        SelectionRange::new(0, text.len()),
        text.len(),
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
    );
    assert!(result.starts_with(text), "selection content survives: {result:?}");
    assert!(result.ends_with('\n'), "Enter still inserts a newline: {result:?}");
    assert!(selection.is_empty(), "the selection collapses");
}

#[test]
fn test_caret_geometry_calculation() {
    let text = "SELECT 1;\nSELECT 2;\nSELECT 3;";
    let buf = TextBuffer::from_string(text);
    let mut buf_clone = buf.clone();
    let mut cursor = CursorPosition::default();
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();
    let editor = SqlEditor::new(
        &mut buf_clone,
        &mut cursor,
        &mut selection,
        SqlDialect::Postgres,
        &theme,
        &[],
        None,
        "test",
    );

    let origin = Pos2::new(100.0, 50.0);
    let gutter_w = 40.0;
    let line_height = 20.0;
    let char_width = 8.0;

    // Line 1 ("SELECT 2;"), col 3 ("E") -> offset = 10 + 3 = 13
    let screen_pos = editor.offset_to_screen_pos(&buf, 13, origin, gutter_w, line_height, char_width);
    assert_eq!(screen_pos.x, 100.0 + 40.0 + PADDING_LEFT + 3.0 * 8.0);
    assert_eq!(screen_pos.y, 50.0 + PADDING_TOP + 1.0 * 20.0);

    let resolved_offset = editor.screen_pos_to_offset(&buf, screen_pos, origin, gutter_w, line_height, char_width);
    assert_eq!(resolved_offset, 13);
}

#[test]
fn test_ime_text_input_multilingual() {
    let mut buf = TextBuffer::from_string("SELECT ");
    let mut cursor = CursorPosition::from_offset(&buf, 7);
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();
    let mut editor = SqlEditor::new(
        &mut buf,
        &mut cursor,
        &mut selection,
        SqlDialect::Postgres,
        &theme,
        &[],
        None,
        "test",
    );

    // Simulate IME committing Vietnamese text
    editor.type_text("'tiếng Việt có dấu'");
    assert_eq!(editor.buffer.text(), "SELECT 'tiếng Việt có dấu'");
    assert_eq!(editor.cursor.offset, "SELECT 'tiếng Việt có dấu'".len());

    // Test undo
    assert!(editor.buffer.undo().is_some());
    assert_eq!(editor.buffer.text(), "SELECT ");
}

#[test]
fn editor_auto_focus_is_one_shot_and_focus_survives_next_frame() {
    let ctx = egui::Context::default();
    let mut buf = TextBuffer::from_string("select 1");
    let mut cursor = CursorPosition::default();
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::light();
    let editor_size = egui::vec2(400.0, 200.0);

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "auto-focus",
                )
                .with_auto_focus(true);
                assert!(editor.show(ui, editor_size).focused);
            });
        },
    );

    let mut focused = false;
    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "auto-focus",
                );
                focused = editor.show(ui, editor_size).focused;
            });
        },
    );

    assert!(focused, "one-shot auto-focus must not clear focus on the next frame");
}

#[test]
fn editor_keeps_focus_after_click_across_frames() {
    let ctx = egui::Context::default();
    let mut buf = TextBuffer::from_string("select 1");
    let mut cursor = CursorPosition::default();
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::light();
    let editor_size = egui::vec2(400.0, 200.0);

    // Frame 1: show the editor and request focus on its interactive id.
    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor_id = ui.make_persistent_id("focus-survive");
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "focus-survive",
                );
                let _ = editor.show(ui, editor_size);
                // Mimic a click: focus the same id `show` registers as interactive.
                ui.memory_mut(|m| m.request_focus(editor_id));
            });
        },
    );

    // Frame 2: editor must still report focused — the old bug cleared focus here
    // because focus was requested on an id that never registered as a widget.
    let mut focused = false;
    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "focus-survive",
                );
                focused = editor.show(ui, editor_size).focused;
            });
        },
    );
    assert!(
        focused,
        "SQL editor must keep keyboard focus after click; otherwise typing is impossible"
    );
}

#[test]
fn command_s_requests_save_without_inserting_text() {
    let ctx = egui::Context::default();
    let mut buf = TextBuffer::from_string("SELECT 1");
    let mut cursor = CursorPosition::default();
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();
    let editor_size = egui::vec2(400.0, 200.0);

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            events: vec![egui::Event::Key {
                key: egui::Key::S,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor_id = ui.make_persistent_id("save-shortcut");
                ui.memory_mut(|memory| memory.request_focus(editor_id));
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "save-shortcut",
                );
                let response = editor.show(ui, editor_size);
                assert!(response.wants_save);
                assert_eq!(buf.text(), "SELECT 1");
            });
        },
    );
}

#[test]
fn completion_opens_only_for_dot_or_explicit_shortcut() {
    let (typed_text, typed_response) =
        render_editor_event("", egui::Event::Text("select".to_owned()), "ordinary-completion-input");
    assert_eq!(typed_text, "select");
    assert!(!typed_response.wants_completion);
    assert!(!typed_response.wants_identifier_typed);

    let (dot_text, dot_response) =
        render_editor_event("users", egui::Event::Text(".".to_owned()), "dot-completion-input");
    assert_eq!(dot_text, "users.");
    assert!(dot_response.wants_completion);
    assert!(!dot_response.wants_manual_completion);

    let (shortcut_text, shortcut_response) = render_editor_event(
        "SELECT ",
        egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
        },
        "manual-completion-shortcut",
    );
    assert_eq!(shortcut_text, "SELECT ");
    assert!(shortcut_response.wants_completion);
    assert!(shortcut_response.wants_manual_completion);
}

#[test]
fn identifier_typing_sets_debounce_intent_without_opening_completion() {
    let (_, response) = render_editor_event("", egui::Event::Text("s".to_owned()), "identifier-completion-input");

    assert!(response.wants_identifier_typed);
    assert!(!response.wants_completion);
}

#[test]
fn test_ime_commit_event_in_editor_widget() {
    let ctx = egui::Context::default();
    let mut buf = TextBuffer::from_string("SELECT ");
    let mut cursor = CursorPosition::from_offset(&buf, 7);
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();

    let raw_input = egui::RawInput {
        focused: true,
        events: vec![egui::Event::Ime(egui::ImeEvent::Commit("tên_cột".to_owned()))],
        ..Default::default()
    };

    let _ = crate::test_frame::frame(&ctx, raw_input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Focus is acquired via the editor's interactive widget id inside `show`.
            let editor = SqlEditor::new(
                &mut buf,
                &mut cursor,
                &mut selection,
                SqlDialect::Postgres,
                &theme,
                &[],
                None,
                "test-ime",
            );
            // Pre-focus by interacting once so IME events are consumed.
            let editor_id = ui.make_persistent_id("test-ime");
            ui.memory_mut(|m| m.request_focus(editor_id));
            let resp = editor.show(ui, egui::vec2(600.0, 400.0));
            assert!(resp.changed, "IME commit should insert text when editor is focused");
            assert!(
                !resp.wants_completion,
                "ordinary identifier typing must not open completion automatically"
            );
        });
    });

    assert_eq!(buf.text(), "SELECT tên_cột");
    assert_eq!(cursor.offset, "SELECT tên_cột".len());
}

#[test]
fn long_buffer_scrolls_to_keep_end_caret_visible() {
    let ctx = egui::Context::default();
    let text = (0..80).map(|i| format!("SELECT {i};")).collect::<Vec<_>>().join("\n");
    let mut buf = TextBuffer::from_string(&text);
    let end = buf.len_bytes();
    let mut cursor = CursorPosition::from_offset(&buf, end);
    let mut selection = SelectionRange::new(end, end);
    let theme = DbProTheme::dark();
    let viewport = egui::vec2(420.0, 160.0);

    assert!(cursor.line > 40, "fixture caret should sit far below the fold");

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor = SqlEditor::new(
                    &mut buf,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    "scroll-long",
                );
                let _ = editor.show(ui, viewport);
                let editor_id = ui.make_persistent_id("scroll-long");
                let scroll = ui
                    .ctx()
                    .data(|d| d.get_temp::<egui::Vec2>(editor_id.with("scroll")))
                    .unwrap_or(egui::Vec2::ZERO);
                assert!(
                    scroll.y > 0.0,
                    "end caret must pull the viewport down, got scroll.y={}",
                    scroll.y
                );
            });
        },
    );
}

#[test]
fn hover_occurrences_match_case_insensitively_and_skip_anchor() {
    let text = "SELECT users.id FROM users u JOIN USERS_archive a ON a.id = u.id";
    let tokens = SqlHighlighter::new(SqlDialect::Postgres).tokenize(text);
    // Anchor on the first `users` token (byte 7..12).
    let occurrences = hover_occurrence_ranges(text, (7, 12), &tokens, 0, text.len());
    assert_eq!(
        occurrences,
        vec![(21, 26)],
        "only the standalone second `users` is an occurrence — `USERS_archive` is a different identifier"
    );
}

#[test]
fn hover_occurrences_respect_visible_window() {
    let text = "SELECT a.id FROM a_table a WHERE a.id > 0";
    let tokens = SqlHighlighter::new(SqlDialect::Postgres).tokenize(text);
    // Anchor `a` at byte 7..8; the full window finds both other `a` tokens.
    assert_eq!(
        hover_occurrence_ranges(text, (7, 8), &tokens, 0, text.len()),
        vec![(25, 26), (33, 34)]
    );
    // A window ending before WHERE clips the second occurrence.
    assert_eq!(hover_occurrence_ranges(text, (7, 8), &tokens, 0, 30), vec![(25, 26)]);
}

#[test]
fn hover_occurrences_return_empty_for_out_of_range_anchor() {
    let text = "SELECT 1";
    let tokens = SqlHighlighter::new(SqlDialect::Postgres).tokenize(text);
    assert!(hover_occurrence_ranges(text, (text.len(), text.len() + 5), &tokens, 0, text.len()).is_empty());
    assert!(hover_occurrence_ranges(text, (0, 6), &[], 0, text.len()).is_empty());
}

#[test]
fn str_eq_ignore_case_handles_unicode() {
    assert!(str_eq_ignore_case("bảng", "BẢNG"));
    assert!(str_eq_ignore_case("Users", "users"));
    assert!(!str_eq_ignore_case("users", "user"));
}

/// Shift-Tab must not slide the anchor when the first selected line has no
/// indent to remove (#470) — only bytes actually deleted before an edge
/// shift that edge.
#[test]
fn shift_tab_unindent_keeps_anchor_when_first_line_not_indented() {
    let text = "ab\n  cd";
    let (result, selection) = render_editor_event_with_selection(
        text,
        SelectionRange::new(2, text.len()),
        text.len(),
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        },
    );
    assert_eq!(result, "ab\ncd");
    assert_eq!(
        selection.normalized(),
        (2, 5),
        "anchor stays; end shifts by the 2 removed bytes"
    );
}

/// Shift-Tab must preserve a backwards selection's direction (#470).
#[test]
fn shift_tab_unindent_preserves_backward_selection() {
    let text = "ab\n  cd";
    let (result, selection) = render_editor_event_with_selection(
        text,
        SelectionRange::new(text.len(), 2),
        2,
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        },
    );
    assert_eq!(result, "ab\ncd");
    assert_eq!(selection.anchor, 5);
    assert_eq!(selection.active, 2);
}

/// Shift-Tab on an indented first line moves the anchor by only the bytes
/// deleted before it.
#[test]
fn shift_tab_unindent_shifts_anchor_by_actual_removal() {
    let text = "  ab\n  cd";
    let (result, selection) = render_editor_event_with_selection(
        text,
        SelectionRange::new(3, text.len()),
        text.len(),
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        },
    );
    assert_eq!(result, "ab\ncd");
    assert_eq!(selection.normalized(), (1, 5));
}

#[test]
fn ime_preedit_tracks_the_composition_string() {
    // #570: the editor used to ignore `ImeEvent::Preedit`, so a composing IME
    // showed nothing at all until commit.
    use super::{ImePreedit, Id};

    let event = egui::ImeEvent::Preedit("tiếng Việ".to_owned(), Some((3, 6)));
    let preedit = ImePreedit::from_event(&event).expect("preedit events are captured");
    assert_eq!(preedit.text, "tiếng Việ");
    assert!(!preedit.is_empty());
    let (before, after) = preedit.around_cursor();
    assert_eq!(before, "tiế");
    assert_eq!(after, "ng Việ");
    // `Id` round-trips through the key helper so the paint pass reads the same slot.
    let editor_id = Id::new("editor");
    assert_eq!(super::ime_preedit_key(editor_id), editor_id.with("ime_preedit"));
}

#[test]
fn commit_and_cancel_clear_the_preedit() {
    use super::ImePreedit;

    assert!(ImePreedit::from_event(&egui::ImeEvent::Commit("a".to_owned())).is_none());
    assert!(ImePreedit::from_event(&egui::ImeEvent::Enabled).is_none());
    let cleared = ImePreedit::from_event(&egui::ImeEvent::Preedit(String::new(), None))
        .expect("an empty preedit is still a preedit event");
    assert!(cleared.is_empty());
    assert_eq!(cleared.around_cursor(), (String::new(), String::new()));
}

/// Render the editor twice in one context: preedit, then a read-only pass that
/// observes whether the composition survived the frame boundary.
fn render_preedit_then_read(preedit: &str) -> (String, super::ImePreedit) {
    use super::{read_ime_preedit, SqlEditor};

    let ctx = egui::Context::default();
    let mut buffer = TextBuffer::from_string("");
    let mut cursor = CursorPosition::from_offset(&buffer, 0);
    let mut selection = SelectionRange::default();
    let theme = DbProTheme::dark();
    let id_salt = "ime-preedit-editor";

    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            focused: true,
            events: vec![egui::Event::Ime(egui::ImeEvent::Preedit(
                preedit.to_owned(),
                Some((0, preedit.chars().count())),
            ))],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let editor_id = ui.make_persistent_id(id_salt);
                ui.memory_mut(|memory| memory.request_focus(editor_id));
                let _ = SqlEditor::new(
                    &mut buffer,
                    &mut cursor,
                    &mut selection,
                    SqlDialect::Postgres,
                    &theme,
                    &[],
                    None,
                    id_salt,
                )
                .show(ui, egui::vec2(600.0, 400.0));
            });
        },
    );

    let mut observed = super::ImePreedit::default();
    let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let editor_id = ui.make_persistent_id(id_salt);
            observed = read_ime_preedit(ui, editor_id);
        });
    });
    (buffer.text().to_owned(), observed)
}

#[test]
fn preedit_is_visible_to_the_next_frame_without_touching_the_buffer() {
    // #570: the composition string is state, not text — it must survive the
    // frame so the paint pass can draw it, and it must not be inserted.
    let (text, preedit) = render_preedit_then_read("tiếng Việt");
    assert_eq!(text, "", "preedit must not be committed to the buffer");
    assert_eq!(preedit.text, "tiếng Việt");
}
