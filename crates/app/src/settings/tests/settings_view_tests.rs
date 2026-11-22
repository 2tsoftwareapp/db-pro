//! Tests for `../settings_view.rs` (moved out of the source file).
    use super::{backup_command, restore_command, DbProApp, OverlayState, RequestId, TaskBridge, UiCommand};
    use crate::{UiConnectionSummary, UiSslMode};

    #[test]
    fn backup_and_restore_commands_read_overlay_paths() {
        let state = OverlayState {
            backup_output_path: "/tmp/backup.sql".to_owned(),
            restore_input_path: "/tmp/input.sql".to_owned(),
            ..OverlayState::default()
        };

        assert!(matches!(
            backup_command(&state, RequestId(1), "source".to_owned()),
            UiCommand::Backup { output_path, connection_id, custom_format: false, .. }
                if output_path == "/tmp/backup.sql" && connection_id == "source"
        ));
        assert!(matches!(
            restore_command(&state, RequestId(2), "source".to_owned()),
            UiCommand::Restore { input_path, connection_id, custom_format: false, .. }
                if input_path == "/tmp/input.sql" && connection_id == "source"
        ));
    }

    #[test]
    fn failed_restore_dispatch_preserves_confirmation() {
        let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
        drop(command_rx);
        let mut app = DbProApp::with_task_bridge(bridge);
        *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
            id: "conn-1".to_owned(),
            name: "Local".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "app".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        }];
        app.connection.lifecycle.set_active_connection_id(Some("conn-1".to_owned()));
        app.overlay.restore_confirmation = true;
        app.overlay.restore_input_path = "/tmp/fixture.dump".to_owned();

        app.apply_backup_actions(vec![super::settings_backup_view::SettingsBackupAction::ConfirmRestore]);

        assert!(app.overlay.restore_confirmation);
        assert_eq!(app.feedback.runtime_message, "Runtime worker unavailable");
    }

    #[test]
    fn settings_general_labels_render_from_the_english_catalog() {
        fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, texts);
                    }
                }
                _ => {}
            }
        }

        let mut app = DbProApp::default();
        app.preferences.section = super::settings_model::SettingsSection::General;
        let ctx = egui::Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        let output = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.draw_settings(ui));
        });
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut texts);
        }
        for expected in ["Language", "Workspace", "General", "Confirm destructive queries"] {
            assert!(
                texts.iter().any(|text| text == expected),
                "rendered Settings General view omitted {expected:?}: {texts:?}"
            );
        }
    }