use super::super::command_dispatch;
use super::logic::*;
use super::mapper::*;
use super::view::*;
use crate::{DbProApp, DbProTheme, TaskBridge, UiCommand, UiConnectionDraft, UiDriver, UiEvent, UiSslMode};

fn rendered_texts(app: &mut DbProApp) -> Vec<String> {
    fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, texts);
                }
            }
            _ => {}
        }
    }

    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    let output = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ConnectionDialogView {
                dialog: &mut app.connection.dialog,
                lifecycle: &mut app.connection.lifecycle,
                command_dispatcher: &mut command_dispatcher,
                feedback: &mut app.feedback,
                theme: app.theme,
            }
            .draw_postgres_connection_fields(ui);
        });
    });

    let mut texts = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn ssh_section_discloses_its_unqualified_v0_1_status() {
    let mut app = DbProApp::default();
    app.connection.dialog.draft.ssh_tunnel_enabled = false;

    let texts = rendered_texts(&mut app);

    assert!(
        texts.iter().any(|text| text == SSH_QUALIFICATION_HINT),
        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
        "the SSH section must carry the LIM-006 qualification caveat before the tunnel is enabled; painted texts: {texts:?}"
    );
}

#[test]
fn ssh_caveat_is_non_blocking_and_keeps_the_control_usable() {
    let mut app = DbProApp::default();
    app.connection.dialog.draft.ssh_tunnel_enabled = true;

    let texts = rendered_texts(&mut app);

    assert!(
        texts.iter().any(|text| text == SSH_QUALIFICATION_HINT),
        "the caveat stays visible while the tunnel is enabled; painted texts: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("SSH Host")),
        "enabling the tunnel still renders its fields — the caveat changes no behaviour; painted texts: {texts:?}"
    );
}

#[test]
fn ssh_caveat_wording_tracks_the_recorded_limitation() {
    assert!(SSH_QUALIFICATION_HINT.contains("Unqualified in v0.1"));
    assert!(SSH_QUALIFICATION_HINT.contains("not been end-to-end tested"));
    assert!(SSH_QUALIFICATION_HINT.contains("may not work reliably"));
}

#[test]
fn locale_catalogs_have_matching_keys_and_placeholders() {
    use std::collections::BTreeMap;

    fn parse_catalog(source: &str) -> BTreeMap<String, (String, Vec<String>)> {
        let mut catalog = BTreeMap::new();
        let mut path: Vec<(usize, String)> = Vec::new();
        for (line_number, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let indent = line.len() - line.trim_start().len();
            while path.last().is_some_and(|(level, _)| *level >= indent) {
                path.pop();
            }
            let full_key = path
                .iter()
                .map(|(_, parent)| parent.as_str())
                .chain(std::iter::once(key))
                .collect::<Vec<_>>()
                .join(".");
            if value.trim().is_empty() {
                path.push((indent, key.to_owned()));
                continue;
            }
            let mut placeholders = Vec::new();
            let mut remaining = value;
            while let Some(start) = remaining.find("%{") {
                let after = &remaining[start + 2..];
                let Some(end) = after.find('}') else {
                    break;
                };
                placeholders.push(after[..end].to_owned());
                remaining = &after[end + 1..];
            }
            placeholders.sort();
            let value = value.trim().trim_matches('"').to_owned();
            assert!(
                catalog.insert(full_key.clone(), (value, placeholders)).is_none(),
                "duplicate locale key {full_key} at line {}",
                line_number + 1
            );
        }
        catalog
    }

    let english = parse_catalog(include_str!("../../../../ui/locales/en.yml"));
    for (locale, source) in [
        ("vi", include_str!("../../../../ui/locales/vi.yml")),
        ("zh", include_str!("../../../../ui/locales/zh.yml")),
        ("ja", include_str!("../../../../ui/locales/ja.yml")),
        ("ko", include_str!("../../../../ui/locales/ko.yml")),
    ] {
        let translated = parse_catalog(source);
        assert_eq!(
            translated.keys().collect::<Vec<_>>(),
            english.keys().collect::<Vec<_>>(),
            "locale {locale} has missing or extra translation keys"
        );
        for (key, (_, placeholders)) in &english {
            assert_eq!(
                translated.get(key).map(|(_, actual)| actual),
                Some(placeholders),
                "locale {locale} has mismatched placeholders for {key}"
            );
        }
        for key in ["query.explain", "query.explain_analyze"] {
            assert_ne!(
                translated[key].0, english[key].0,
                "locale {locale} leaves {key} untranslated"
            );
        }
        assert_ne!(
            translated["sidebar.filter_lint"].0, english["sidebar.filter_lint"].0,
            "locale {locale} leaves sidebar.filter_lint untranslated"
        );
        if locale == "vi" {
            for key in [
                "connection.access_token",
                "sidebar.filter_parser",
                "sidebar.filter_delimiter",
                "sidebar.filter_database",
            ] {
                assert_ne!(
                    translated[key].0, english[key].0,
                    "Vietnamese catalog leaves {key} untranslated"
                );
            }
        }
    }
}

#[test]
fn test_validate_connection_draft() {
    let mut draft = UiConnectionDraft {
        name: String::new(),
        ..Default::default()
};
    assert!(validate_connection_draft(&draft).is_err());

    draft.name = "Test DB".into();
    draft.database = String::new();
    assert!(validate_connection_draft(&draft).is_err());

    draft.database = "postgres".into();
    draft.host = "db.test".into();
    draft.username = "app".into();
    draft.port = "5432".into();
    assert!(validate_connection_draft(&draft).is_ok());

    draft.port = "invalid".into();
    assert!(validate_connection_draft(&draft).is_err());
    draft.port = "0".into();
    assert!(
        validate_connection_draft(&draft).is_err(),
        "port zero is outside the validated 1–65535 range"
    );

    draft.port = "5432".into();
    draft.ssh_tunnel_enabled = true;
    assert!(validate_connection_draft(&draft).is_err());

    draft.ssh_host = "bastion.test".into();
    draft.ssh_user = "user".into();
    draft.ssh_private_key = "/path/to/key".into();
    draft.ssh_port = "0".into();
    assert!(
        validate_connection_draft(&draft).is_err(),
        "SSH port zero is outside the validated 1–65535 range"
    );
    draft.ssh_port = "22".into();
    assert!(validate_connection_draft(&draft).is_ok());
}

#[test]
fn driver_switch_preserves_typed_values() {
    let mut draft = UiConnectionDraft {
        port: "5433".to_owned(),
        ssl_mode: UiSslMode::VerifyFull,
        ..Default::default()
};

    select_driver(&mut draft, UiDriver::Mysql);
    assert_eq!(draft.driver, UiDriver::Mysql);
    assert_eq!(draft.port, "5433");
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);

    select_driver(&mut draft, UiDriver::SqlServer);
    assert_eq!(draft.driver, UiDriver::SqlServer);
    assert_eq!(draft.port, "5433");
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);
}

#[test]
fn test_mapper_snippet_parsing() {
    let mut draft = UiConnectionDraft::default();
    let uri = "postgresql://myuser:secret@myhost:5432/mydb?sslmode=verify-full";
    assert!(apply_connection_snippet(&mut draft, uri).is_ok());
    assert_eq!(draft.username, "myuser");
    assert_eq!(draft.password, "secret");
    assert_eq!(draft.host, "myhost");
    assert_eq!(draft.port, "5432");
    assert_eq!(draft.database, "mydb");
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);
}

#[test]
fn test_connection_config_mappings() {
    use super::config::*;
    assert_eq!(default_port_for_driver(UiDriver::Postgres), "5432");
    assert_eq!(default_port_for_driver(UiDriver::Mysql), "3306");
    assert_eq!(default_port_for_driver(UiDriver::SqlServer), "1433");
    assert_eq!(default_database_for_driver(UiDriver::SqlServer), "master");

    assert_eq!(environment_to_index("Development"), 0);
    assert_eq!(environment_to_index("Staging"), 1);
    assert_eq!(environment_to_index("Production"), 2);
    assert_eq!(environment_to_index("Custom"), 3);
    assert_eq!(index_to_environment(2), "Production");

    assert_eq!(ssl_mode_to_index(UiSslMode::Disable), 0);
    assert_eq!(ssl_mode_to_index(UiSslMode::Require), 1);
    assert_eq!(ssl_mode_to_index(UiSslMode::VerifyCa), 2);
    assert_eq!(ssl_mode_to_index(UiSslMode::VerifyFull), 3);
    assert_eq!(index_to_ssl_mode(3), UiSslMode::VerifyFull);
}

#[test]
fn test_connection_layout_calculations() {
    use super::layout::*;
    // Weighted rows always span the full width minus inter-cell gaps.
    for (weights, gaps) in [
        (vec![52.0, 36.0, 12.0], 2.0),
        (vec![55.0, 45.0], 1.0),
        (vec![68.0, 32.0], 1.0),
        (vec![1.0, 1.0], 1.0),
        (vec![38.0, 14.0, 20.0, 28.0], 3.0),
    ] {
        let total: f32 = row_widths(800.0, 8.0, &weights).iter().sum();
        assert!((total - (800.0 - gaps * 8.0)).abs() < 0.01);
    }

    let (input_w, button_w) = sqlite_file_row(800.0, 8.0);
    assert_eq!(button_w, 110.0);
    assert!((input_w - (800.0 - 118.0)).abs() < 0.01);
}
/// Drives the engine `Select` with real pointer events: the trigger must open
/// the option menu, and picking an item must update the draft driver.
#[test]
fn engine_select_opens_on_click_and_applies_the_chosen_driver() {
    use crate::components::Select;
    use std::cell::Cell;
    use std::rc::Rc;

    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);

    let mut draft = UiConnectionDraft {
        name: "t".into(),
        database: "t".into(),
        ..Default::default()
};
    let trigger_rect = Rc::new(Cell::new(egui::Rect::NOTHING));

    let collect_texts = |output: &egui::FullOutput| -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts
    };

    let render = |ctx: &egui::Context,
                  draft: &mut UiConnectionDraft,
                  trigger_rect: &Rc<Cell<egui::Rect>>,
                  events: Vec<egui::Event>| {
        let rect_cell = Rc::clone(trigger_rect);
        crate::test_frame::frame(
            &ctx,
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut idx = super::config::DRIVER_CARD_SPECS
                        .iter()
                        .position(|s| s.driver == Some(draft.driver))
                        .unwrap_or(0);
                    let options: Vec<String> = super::config::DRIVER_CARD_SPECS
                        .iter()
                        .map(|s| format!("{} — {}", s.name, s.subtitle))
                        .collect();
                    let response = Select::new("db_engine_select", &mut idx, &options)
                        .label(t!("connection.database_engine"))
                        .show(ui);
                    rect_cell.set(response.rect);
                    let spec = &super::config::DRIVER_CARD_SPECS[idx];
                    if let Some(driver) = spec.driver {
                        if driver != draft.driver {
                            select_driver(draft, driver);
                        }
                    }
                });
            },
        )
    };

    // Frame 1: lay out once so the trigger rect is known.
    let out = render(&ctx, &mut draft, &trigger_rect, vec![]);
    let trigger = trigger_rect.get();
    assert!(!trigger.is_negative(), "trigger rect must be laid out");
    assert!(!collect_texts(&out).iter().any(|t| t.contains("MySQL")));

    let click_pos = trigger.center();
    let press = egui::Event::PointerButton {
        pos: click_pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    };
    let release = egui::Event::PointerButton {
        pos: click_pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    };

    // Click down + up over two frames, then a settled frame for the popup.
    render(
        &ctx,
        &mut draft,
        &trigger_rect,
        vec![egui::Event::PointerMoved(click_pos), press],
    );
    render(&ctx, &mut draft, &trigger_rect, vec![release]);
    let out = render(&ctx, &mut draft, &trigger_rect, vec![]);

    let texts = collect_texts(&out);
    assert!(
        texts.iter().any(|t| t.contains("MySQL")),
        "engine menu must open after a trigger click; painted: {texts:?}"
    );
}
#[test]
fn sqlite_driver_keeps_existing_port_and_skips_port_validation() {
    let mut draft = UiConnectionDraft {
        name: "local".into(),
        database: "/tmp/app.db".into(),
        port: "9999".into(),
        ssl_mode: UiSslMode::VerifyFull,
        ..Default::default()
};
    draft.driver = UiDriver::Mysql;
    select_driver(&mut draft, UiDriver::Sqlite);
    // SQLite is file-backed: driver switch must not touch host-side fields.
    assert_eq!(draft.port, "9999");
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);

    // A non-numeric port must not fail validation for SQLite drafts.
    draft.port = "not-a-port".into();
    assert!(
        validate_connection_draft(&draft).is_ok(),
        "SQLite drafts skip port validation"
    );

    // But TCP drivers still enforce it.
    draft.driver = UiDriver::Postgres;
    assert!(validate_connection_draft(&draft).is_err());
}

#[test]
fn cloud_preset_applies_driver_port_ssl_and_only_overwrites_placeholder_hosts() {
    let mut draft = UiConnectionDraft {
        cloud_preset: "aws_rds:postgres".into(),
        ..Default::default()
};
    let guidance = apply_cloud_preset_to_draft(&mut draft, "aws_rds:postgres").expect("known preset applies");
    assert_eq!(draft.driver, UiDriver::Postgres);
    assert_eq!(draft.port, "5432");
    // Preset overrides ssl_mode with its own CA policy (RDS Postgres → VerifyFull).
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);
    assert!(!guidance.is_empty());

    // A user-typed real host must survive preset application.
    draft.host = "prod.cluster-abc.eu-west-1.rds.amazonaws.com".into();
    apply_cloud_preset_to_draft(&mut draft, "aws_rds:postgres").unwrap();
    assert_eq!(draft.host, "prod.cluster-abc.eu-west-1.rds.amazonaws.com");
    // Empty hosts get filled with the preset endpoint hint.
    draft.host.clear();
    apply_cloud_preset_to_draft(&mut draft, "aws_rds:postgres").unwrap();
    assert!(
        !draft.host.trim().is_empty() && draft.host != "localhost",
        "empty host should receive the preset's host hint"
    );

    assert!(
        apply_cloud_preset_to_draft(&mut draft, "no_such:preset").is_err(),
        "unknown preset keys must surface an error instead of mutating the draft"
    );
}

#[test]
fn apply_cloud_preset_clears_error_and_invalidates_test_state() {
    let mut dialog = super::state::ConnectionDialogState::default();
    dialog.draft_mut().cloud_preset = "gcp_cloudsql:mysql".into();
    dialog.set_error("previous failure".to_owned());
    dialog.set_test_valid(true);

    super::apply_cloud_preset(&mut dialog).expect("preset applies");

    assert_eq!(dialog.draft().driver, UiDriver::Mysql);
    assert_eq!(dialog.error(), "");
    assert!(
        !dialog.test_valid(),
        "applying a preset must invalidate a previously green test"
    );
    assert!(!dialog.draft().cloud_guidance.is_empty());
}

#[test]
fn ssh_profile_roundtrip_requires_identity_fields_and_restores_draft() {
    let mut profiles = Vec::new();
    let empty = UiConnectionDraft::default();
    assert!(save_ssh_profile(&mut profiles, &empty).is_err());
    assert!(profiles.is_empty());

    let mut draft = UiConnectionDraft {
        ssh_host: "bastion.internal".into(),
        ssh_user: "deploy".into(),
        ssh_private_key: "/keys/id_ed25519".into(),
        ssh_port: String::new(), // unset → profile defaults to 22
        ..Default::default()
};
    let profile_id = save_ssh_profile(&mut profiles, &draft).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].port, 22);
    assert_eq!(profiles[0].name, "deploy@bastion.internal");

    let mut other = UiConnectionDraft::default();
    apply_ssh_profile_to_draft(&mut other, &profiles[0]);
    assert!(other.ssh_tunnel_enabled);
    assert_eq!(other.ssh_host, "bastion.internal");
    assert_eq!(other.ssh_port, "22");
    assert_eq!(other.ssh_user, "deploy");
    assert_eq!(other.ssh_private_key, "/keys/id_ed25519");
    assert_eq!(other.ssh_profile_id, profile_id);
    draft.ssh_tunnel_enabled = true; // silence unused-mut lint path stays exercised
}

#[test]
fn cancelling_connection_test_dispatches_cancel_for_the_active_request() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.dialog.set_open(true);
    app.connection.dialog.draft_mut().name = "Local".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(false);
    let request_id = match command_rx.try_recv().expect("test command expected") {
        UiCommand::TestConnection { request_id, .. } => request_id,
        _ => panic!("expected TestConnection command"),
    };

    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    ConnectionDialogView {
        dialog: &mut app.connection.dialog,
        lifecycle: &mut app.connection.lifecycle,
        command_dispatcher: &mut command_dispatcher,
        feedback: &mut app.feedback,
        theme: app.theme,
    }
    .close_connection_dialog();

    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::CancelTestConnection { request_id: cancelled }) if cancelled == request_id
    ));
    assert!(!app.connection.dialog.is_open());
    assert!(app.connection.lifecycle.pending_request().is_none());
    assert_eq!(app.feedback.runtime_message, "Connection test cancelled");
    event_tx
        .send(UiEvent::OperationCompleted {
            request_id,
            operation: "connection.tested".to_owned(),
        })
        .expect("late test completion should be queued");
    app.apply_runtime_events();

    assert!(!app.connection.dialog.test_valid());
    assert_eq!(app.feedback.runtime_message, "Connection test cancelled");
}

#[test]
fn cancel_test_request_keeps_connection_dialog_open() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.dialog.set_open(true);
    app.connection.dialog.draft_mut().name = "Local".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(false);
    let request_id = match command_rx.try_recv().expect("test command expected") {
        UiCommand::TestConnection { request_id, .. } => request_id,
        _ => panic!("expected TestConnection command"),
    };

    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    ConnectionDialogView {
        dialog: &mut app.connection.dialog,
        lifecycle: &mut app.connection.lifecycle,
        command_dispatcher: &mut command_dispatcher,
        feedback: &mut app.feedback,
        theme: app.theme,
    }
    .cancel_connection_test();

    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::CancelTestConnection { request_id: cancelled }) if cancelled == request_id
    ));
    assert!(app.connection.dialog.is_open());
    assert!(app.connection.lifecycle.pending_request().is_none());
    assert!(!app.connection.dialog.test_valid());
    assert!(app.connection.dialog.test_draft().is_none());
    assert_eq!(app.feedback.runtime_message, "Connection test cancelled");
}
#[test]
fn failed_connection_test_cancel_dispatch_keeps_the_dialog_open() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.dialog.set_open(true);
    app.connection.dialog.draft_mut().name = "Local".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(false);
    let request_id = match command_rx.try_recv().expect("test command expected") {
        UiCommand::TestConnection { request_id, .. } => request_id,
        _ => panic!("expected TestConnection command"),
    };
    drop(command_rx);

    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    ConnectionDialogView {
        dialog: &mut app.connection.dialog,
        lifecycle: &mut app.connection.lifecycle,
        command_dispatcher: &mut command_dispatcher,
        feedback: &mut app.feedback,
        theme: app.theme,
    }
    .close_connection_dialog();

    assert!(app.connection.dialog.is_open());
    assert_eq!(app.connection.lifecycle.pending_request(), Some(request_id));
    assert_eq!(app.feedback.runtime_message, "Runtime worker unavailable");
}

#[test]
fn closing_dirty_connection_draft_requires_discard_confirmation() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.open_new();
    app.connection.dialog.draft_mut().name = "Local".to_owned();

    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    ConnectionDialogView {
        dialog: &mut app.connection.dialog,
        lifecycle: &mut app.connection.lifecycle,
        command_dispatcher: &mut command_dispatcher,
        feedback: &mut app.feedback,
        theme: app.theme,
    }
    .request_close_connection_dialog();

    // The dirty draft stays open behind the confirmation instead of being dropped.
    assert!(app.connection.dialog.is_open());
    assert!(app.connection.dialog.discard_draft_confirmation);
}

#[test]
fn closing_pristine_connection_draft_skips_the_confirmation() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.open_new();

    let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut app.task_bridge);
    ConnectionDialogView {
        dialog: &mut app.connection.dialog,
        lifecycle: &mut app.connection.lifecycle,
        command_dispatcher: &mut command_dispatcher,
        feedback: &mut app.feedback,
        theme: app.theme,
    }
    .request_close_connection_dialog();

    assert!(!app.connection.dialog.is_open());
    assert!(!app.connection.dialog.discard_draft_confirmation);
}
