// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use super::{config::INPUT_MIN_WIDTH, layout::resolve_field_width};

#[test]
fn fills_available_width_when_unrequested() {
    assert_eq!(resolve_field_width(None, 320.0), 320.0);
}

#[test]
fn requested_width_never_exceeds_the_container() {
    assert_eq!(resolve_field_width(Some(420.0), 260.0), 260.0);
}

#[test]
fn field_shrinks_to_fit_containers_narrower_than_the_preferred_minimum() {
    assert_eq!(resolve_field_width(None, 40.0), 40.0);
    assert_eq!(resolve_field_width(Some(200.0), 40.0), 40.0);
}

#[test]
fn requested_width_below_floor_is_raised() {
    assert_eq!(resolve_field_width(Some(60.0), 320.0), INPUT_MIN_WIDTH);
}

#[test]
fn input_field_receives_click_when_clicked() {
    // Regression guard for the frame-click-steal bug: a surrounding `Frame` must not register a
    // `Sense::click()` region on top of the inner `TextEdit`. When it does, egui reports the click
    // as consumed by the frame, so the `TextEdit` never receives it — the caret never lands where
    // you click and a double-click never selects text. We assert the `TextEdit`'s own `clicked()`
    // (not just focus, which the buggy block faked via `request_focus()`), so the test fails when
    // the frame swallows the pointer.
    use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    use crate::{components::Input, DbProTheme};

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut value = String::from("seed");
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 200.0));

    let mut edit_rect = None;
    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                edit_rect = Some(Input::new(&mut value, "Host", DbProTheme::dark()).show(ui).rect);
            });
        },
    );
    let edit_rect = edit_rect.expect("the input field is laid out");

    let pos = edit_rect.center();
    let mut edit_clicked = false;
    for pressed in [true, false] {
        let _ = crate::test_frame::frame(
            &ctx,
            RawInput {
                screen_rect: Some(screen),
                events: vec![Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::default(),
                }],
                ..Default::default()
            },
            |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    // `clicked()` is true only on the release frame; assigning every iteration
                    // (not just on release) avoids a clippy unused-assignment on `edit_clicked`.
                    edit_clicked = Input::new(&mut value, "Host", DbProTheme::dark()).show(ui).clicked();
                });
            },
        );
    }

    assert!(
        edit_clicked,
        "the text field must receive the click (caret placement and double-click selection depend \
         on the inner widget seeing the pointer), not have it swallowed by the surrounding frame"
    );
}

#[test]
fn auto_focus_requests_once_without_stealing_later_focus() {
    use egui::{CentralPanel, Context, Pos2, RawInput, Rect, Vec2};

    use super::Input;
    use crate::DbProTheme;

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut value = String::new();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 200.0));
    let mut input_id = None;

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                input_id = Some(
                    Input::new(&mut value, "Connection name", DbProTheme::dark())
                        .id_salt("connection.name")
                        .auto_focus(true)
                        .show(ui)
                        .id,
                );
            });
        },
    );
    let input_id = input_id.expect("the input must be laid out");
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(input_id));

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                Input::new(&mut value, "Connection name", DbProTheme::dark())
                    .id_salt("connection.name")
                    .show(ui);
            });
        },
    );
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(input_id));
}

#[test]
fn tab_moves_between_single_line_inputs() {
    use egui::{CentralPanel, Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

    use super::Input;
    use crate::DbProTheme;

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut first = String::new();
    let mut second = String::new();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 240.0));
    let mut first_response = None;
    let mut second_response = None;

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                first_response = Some(
                    Input::new(&mut first, "First", DbProTheme::dark())
                        .id_salt("focus.first")
                        .auto_focus(true)
                        .show(ui),
                );
                second_response = Some(
                    Input::new(&mut second, "Second", DbProTheme::dark())
                        .id_salt("focus.second")
                        .show(ui),
                );
            });
        },
    );
    assert!(first_response.expect("first input must be laid out").has_focus());

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            events: vec![Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }],
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                Input::new(&mut first, "First", DbProTheme::dark())
                    .id_salt("focus.first")
                    .show(ui);
                second_response = Some(
                    Input::new(&mut second, "Second", DbProTheme::dark())
                        .id_salt("focus.second")
                        .show(ui),
                );
            });
        },
    );
    assert!(
        second_response.expect("second input must be laid out").has_focus(),
        "Tab must move focus from the first single-line input to the next input"
    );
}

#[test]
fn tab_moves_between_inputs_in_horizontal_form_row() {
    use egui::{CentralPanel, Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

    use super::Input;
    use crate::DbProTheme;

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut first = String::from("existing name");
    let mut second = String::new();
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 240.0));
    let mut second_response = None;

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        Input::new(&mut first, "First", DbProTheme::dark())
                            .id_salt("row.first")
                            .clearable(true)
                            .auto_focus(true)
                            .show(ui);
                    });
                    ui.vertical(|ui| {
                        second_response = Some(
                            Input::new(&mut second, "Second", DbProTheme::dark())
                                .id_salt("row.second")
                                .show(ui),
                        );
                    });
                });
            });
        },
    );

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            events: vec![Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }],
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        Input::new(&mut first, "First", DbProTheme::dark())
                            .id_salt("row.first")
                            .clearable(true)
                            .show(ui);
                    });
                    ui.vertical(|ui| {
                        second_response = Some(
                            Input::new(&mut second, "Second", DbProTheme::dark())
                                .id_salt("row.second")
                                .show(ui),
                        );
                    });
                });
            });
        },
    );

    assert!(
        second_response.expect("second input must be laid out").has_focus(),
        "Tab must move between inputs in the same horizontal form row"
    );
}

#[test]
fn tab_follows_row_order_across_form_columns() {
    use egui::{CentralPanel, Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Response, Vec2};

    use super::Input;
    use crate::DbProTheme;

    fn show_form(
        ctx: &Context,
        values: &mut [String; 4],
        raw_input: RawInput,
        auto_focus_first: bool,
    ) -> Vec<Response> {
        let mut responses = Vec::with_capacity(values.len());
        let _ = crate::test_frame::frame(&ctx, raw_input, |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.columns(2, |columns| {
                    responses.push(
                        Input::new(&mut values[0], "Name", DbProTheme::dark())
                            .id_salt("form.name")
                            .auto_focus(auto_focus_first)
                            .show(&mut columns[0]),
                    );
                    responses.push(
                        Input::new(&mut values[1], "Database", DbProTheme::dark())
                            .id_salt("form.database")
                            .show(&mut columns[1]),
                    );
                });
                ui.add_space(16.0);
                ui.columns(2, |columns| {
                    responses.push(
                        Input::new(&mut values[2], "Host", DbProTheme::dark())
                            .id_salt("form.host")
                            .show(&mut columns[0]),
                    );
                    responses.push(
                        Input::new(&mut values[3], "Password", DbProTheme::dark())
                            .id_salt("form.password")
                            .show(&mut columns[1]),
                    );
                });
            });
        });
        responses
    }

    fn tab_event() -> Event {
        Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }
    }

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut values = std::array::from_fn(|_| String::new());
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 320.0));
    let initial = show_form(
        &ctx,
        &mut values,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        true,
    );
    assert!(
        initial[0].has_focus(),
        "the first form field should receive initial focus"
    );

    let second = show_form(
        &ctx,
        &mut values,
        RawInput {
            screen_rect: Some(screen),
            events: vec![tab_event()],
            ..Default::default()
        },
        false,
    );
    assert!(second[1].has_focus(), "Tab should advance across the first visual row");

    let third = show_form(
        &ctx,
        &mut values,
        RawInput {
            screen_rect: Some(screen),
            events: vec![tab_event()],
            ..Default::default()
        },
        false,
    );
    assert!(
        third[2].has_focus(),
        "Tab should continue at the start of the next visual row"
    );
}

#[test]
fn password_eye_toggles_when_clicked() {
    // Regression guard for the same frame-click-steal bug, but through `PasswordInput`. The eye
    // toggle button lives inside the field's `Frame`, immediately right of the text edit. If the
    // surrounding frame registers a `Sense::click()` region on top, it eats the click and the eye
    // button never fires — the password stays masked even when toggled. We sweep a few candidate
    // points over the eye region: under the fix exactly the eye point flips `show_password`; under
    // the bug the frame swallows every click in the frame and nothing toggles.
    use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    use super::PasswordInput;
    use crate::DbProTheme;

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut value = String::from("hunter2");
    let mut show_password = false;
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 200.0));

    let mut edit_rect = None;
    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                edit_rect = Some(
                    PasswordInput::new(&mut value, "Password", &mut show_password, DbProTheme::dark())
                        .show(ui)
                        .rect,
                );
            });
        },
    );
    let edit_rect = edit_rect.expect("the password field is laid out");

    // The eye button sits immediately right of the text edit, inside the same frame.
    let mut eye_toggled = false;
    for dx in [2.0, 5.0, 8.0, 11.0] {
        let pos = Pos2::new(edit_rect.right() + dx, edit_rect.center().y);
        let before = show_password;
        for pressed in [true, false] {
            let _ = crate::test_frame::frame(
                &ctx,
                RawInput {
                    screen_rect: Some(screen),
                    events: vec![Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::default(),
                    }],
                    ..Default::default()
                },
                |ctx| {
                    CentralPanel::default().show(ctx, |ui| {
                        let _ =
                            PasswordInput::new(&mut value, "Password", &mut show_password, DbProTheme::dark()).show(ui);
                    });
                },
            );
        }
        if show_password != before {
            eye_toggled = true;
            break;
        }
    }

    assert!(
        eye_toggled,
        "clicking the eye button must toggle `show_password`; if the surrounding frame swallows the \
         click, the eye never fires and the password stays masked"
    );
}

#[test]
fn standard_field_geometry_is_independent_of_parent_spacing_and_accessories() {
    use egui::{CentralPanel, Context, RawInput, Shape};

    use crate::{
        components::{Input, PasswordInput, Select},
        DbProTheme,
    };

    for gap in [0.0, 8.0, 16.0] {
        for control in ["input", "clearable", "disabled-clearable", "password", "select"] {
            let ctx = Context::default();
            let theme = DbProTheme::light();
            DbProTheme::install_fonts(&ctx);
            theme.apply(&ctx);
            let mut value = String::from("database");
            let mut visible = false;
            let mut selected = 0;
            let options = vec!["PostgreSQL".to_owned()];
            let output = crate::test_frame::frame(&ctx, RawInput::default(), |ctx| {
                CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(300.0);
                    ui.set_max_width(300.0);
                    ui.spacing_mut().item_spacing.x = gap;
                    match control {
                        "password" => {
                            PasswordInput::new(&mut value, "", &mut visible, theme)
                                .width(300.0)
                                .show(ui);
                        }
                        "select" => {
                            // Use non-default padding to guard against a hardcoded width deduction.
                            ui.spacing_mut().button_padding.x = 12.0;
                            Select::new("geometry", &mut selected, &options)
                                .theme(theme)
                                .width(300.0)
                                .show(ui);
                        }
                        _ => {
                            Input::new(&mut value, "", theme)
                                .clearable(control != "input")
                                .enabled(control != "disabled-clearable")
                                .width(300.0)
                                .show(ui);
                        }
                    }
                });
            });
            let fill = if control == "disabled-clearable" {
                theme.surface_2
            } else {
                theme.surface_field
            };
            let frame = output
                .shapes
                .iter()
                .rev()
                .find_map(|shape| match &shape.shape {
                    Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
                    _ => None,
                })
                .expect("field frame was painted");
            assert_eq!(frame.width(), 300.0, "{control}, gap={gap}");
            assert_eq!(
                frame.height(),
                crate::tokens::component::input::INPUT_HEIGHT_DEFAULT,
                "{control}, gap={gap}"
            );
        }
    }
}

#[test]
fn click_then_type_in_the_same_frame_goes_to_the_clicked_field() {
    // Regression guard (#609): egui applies `request_focus` immediately, but the
    // field that had focus when the frame started is drawn first and consumes
    // this frame's text events. The pre-pass runs before any widget draws and
    // hands focus to the pressed field, so characters typed alongside a click
    // land in the clicked field — not the previously focused one.
    use egui::{CentralPanel, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    use crate::{components::input::focus_text_field_under_pointer, components::Input, DbProTheme};

    let ctx = Context::default();
    DbProTheme::install_fonts(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 300.0));
    let mut first = String::from("name");
    let mut second = String::new();
    let mut first_id = None;
    let mut second_rect = None;

    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.vertical(|ui| {
                    let first_response =
                        Input::new(&mut first, "Name", DbProTheme::dark()).id_salt("test.first").show(ui);
                    first_id = Some(first_response.id);
                    let second_response =
                        Input::new(&mut second, "Port", DbProTheme::dark()).id_salt("test.second").show(ui);
                    second_rect = Some(second_response.rect);
                });
            });
        },
    );
    let first_id = first_id.expect("first field drawn");
    let second_rect = second_rect.expect("second field drawn");
    // The user had been editing the first field.
    ctx.memory_mut(|memory| memory.request_focus(first_id));

    let pos = second_rect.center();
    let _ = crate::test_frame::frame(
        &ctx,
        RawInput {
            screen_rect: Some(screen),
            events: vec![
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
                Event::Text("2".to_owned()),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::default(),
                },
            ],
            ..Default::default()
        },
        |ctx| {
            // Same pre-pass the app runs before drawing the shell.
            focus_text_field_under_pointer(ctx);
            CentralPanel::default().show(ctx, |ui| {
                ui.vertical(|ui| {
                    Input::new(&mut first, "Name", DbProTheme::dark()).id_salt("test.first").show(ui);
                    Input::new(&mut second, "Port", DbProTheme::dark()).id_salt("test.second").show(ui);
                });
            });
        },
    );

    assert_eq!(second, "2", "the typed character must land in the clicked field");
    assert_eq!(first, "name", "the previously focused field must not receive the character");
}
