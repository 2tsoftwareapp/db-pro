mod egress_tests {
    use super::super::*;

    /// Every text run the frame actually painted, with the editor actions menu open.
    fn rendered_editor_actions_texts(app: &mut DbProApp) -> Vec<String> {
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
        let output = crate::test_frame::frame(&ctx, Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = app.draw_query_editor_actions(ui);
            });
        });

        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }

    #[test]
    fn ai_prediction_control_discloses_its_egress() {
        let mut app = DbProApp::default();

        let texts = rendered_editor_actions_texts(&mut app);

        assert!(
            texts.iter().any(|text| text == AI_PREDICTION_EGRESS_NOTE),
            "the AI prediction control must state that it sends SQL and schema context (#242); painted texts: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "AI prediction"),
            "the prediction mode control itself is still painted; painted texts: {texts:?}"
        );
    }

    #[test]
    fn prediction_default_is_off_with_the_note_visible() {
        // Idle Query must stay predictable: AI suggestions require explicit opt-in,
        // while the egress note still discloses the AI path when prediction runs.
        let app = DbProApp::default();

        assert_eq!(app.preferences.prediction_mode, PredictionMode::Off);
        assert!(AI_PREDICTION_EGRESS_NOTE.contains("configured AI provider"));
    }
}

mod export_tests {
    use super::super::*;
    use crate::app::overlay_state::{ExportFormat, ExportNullMode};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("db-pro-export-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn dir_entries(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read temp dir")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn atomic_write_publishes_the_whole_file_and_leaves_no_scratch_file() {
        let dir = temp_dir("publish");
        let path = dir.join("out.csv");

        write_file_atomically(&path, b"id,name\n1,alpha\n").expect("write");

        assert_eq!(std::fs::read_to_string(&path).expect("read"), "id,name\n1,alpha\n");
        assert_eq!(
            dir_entries(&dir),
            vec!["out.csv".to_owned()],
            "the temp file must not survive a successful write"
        );
    }

    /// The regression #244 (E-1) describes: a write that cannot complete must leave **no** truncated
    /// file at the destination. The directory is made read-only so the temp file cannot be created —
    /// the destination is then untouched, where `std::fs::write` would have failed mid-stream.
    #[cfg(unix)]
    #[test]
    fn failed_atomic_write_leaves_the_destination_untouched() {
        let dir = temp_dir("failure");
        let path = dir.join("out.csv");
        std::fs::write(&path, "previous export\n").expect("seed existing file");

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("chmod read-only");
        let error = write_file_atomically(&path, b"id,name\n1,alpha\n")
            .expect_err("a write into a read-only directory must fail");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("chmod back");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "previous export\n",
            "the destination must keep its previous content, never a truncated prefix"
        );
        assert_eq!(
            dir_entries(&dir),
            vec!["out.csv".to_owned()],
            "no scratch file may survive"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_atomic_write_into_a_missing_directory_creates_nothing() {
        let dir = temp_dir("missing");
        let path = dir.join("no-such-dir").join("out.csv");

        write_file_atomically(&path, b"id,name\n").expect_err("the parent does not exist");

        assert!(!path.exists());
        assert_eq!(dir_entries(&dir), Vec::<String>::new(), "nothing may be created");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pins that the **export path** publishes rather than rewrites in place — the property the
    /// acceptance cares about ("no partial file survives"), which a test of the helper alone does not
    /// establish: reverting `export_result_to_disk` to `std::fs::write` leaves a helper-only suite
    /// green. An in-place rewrite truncates the destination before writing into it; a publish
    /// replaces it, so the destination is a different file afterwards even though the path is the
    /// same.
    #[cfg(unix)]
    #[test]
    fn export_replaces_the_destination_instead_of_truncating_it() {
        use std::os::unix::fs::MetadataExt as _;

        let dir = temp_dir("publish-semantics");
        let path = dir.join("out.csv");
        std::fs::write(&path, "previous export\n").expect("seed existing file");
        let inode_before = std::fs::metadata(&path).expect("metadata").ino();

        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: path.to_string_lossy().into_owned(),
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        app.export_result(&result);
        app.export_result_confirming_overwrite(&result);

        // #612: CSV files now start with a UTF-8 BOM, so compare the payload.
        let written = std::fs::read_to_string(&path).expect("read");
        assert_eq!(written.trim_start_matches('\u{feff}'), "id\n1\n");
        assert_ne!(
            std::fs::metadata(&path).expect("metadata").ino(),
            inode_before,
            "the destination must be replaced by a rename, not rewritten in place: an in-place write \
             is what leaves a truncated file when it fails part-way"
        );
        assert_eq!(dir_entries(&dir), vec!["out.csv".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #612: CSV export phải mở đầu bằng BOM UTF-8 để Excel (locale vi-VN) đọc
    /// đúng tiếng Việt; các format khác không được thêm BOM.
    #[test]
    fn csv_export_starts_with_a_utf8_bom_612() {
        let dir = temp_dir("bom");
        for (format, file, wants_bom) in [
            (ExportFormat::Csv, "out.csv", true),
            (ExportFormat::Tsv, "out.tsv", false),
            (ExportFormat::Json, "out.json", false),
        ] {
            let path = dir.join(file);
            let mut app = DbProApp {
                overlay: OverlayState {
                    export_open: true,
                    export_path: path.to_string_lossy().into_owned(),
                    export_format: format,
                    ..Default::default()
                },
                ..Default::default()
            };
            let result = UiQueryResult {
                columns: vec![crate::UiColumn {
                    name: "name".to_owned(),
                    data_type: "text".to_owned(),
                    nullable: false,
                }],
                rows: vec![vec![UiCell::Text("Nguyễn Văn An".to_owned())]],
                row_count: 1,
                duration_ms: 0,
            };
            app.export_result(&result);

            let bytes = std::fs::read(&path).expect("exported file");
            let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
            assert_eq!(has_bom, wants_bom, "{file} BOM mismatch");
            let text = String::from_utf8_lossy(&bytes);
            assert!(text.contains("Nguyễn Văn An"), "{file} lost its text: {text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #612: dialog không được mở với ô Output file trống.
    #[test]
    fn export_dialog_prefills_a_default_path_612() {
        let ctx = egui::Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };
        let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            app.draw_export_dialog(ui, Some(&result));
        });

        let path = app.overlay.export_path.clone();
        assert!(path.ends_with(".csv"), "default path must carry the format extension: {path}");
        assert!(
            std::path::Path::new(&path).is_absolute(),
            "default path must be usable without typing: {path}"
        );
        assert!(path.contains("results"), "unnamed result falls back to a readable stem: {path}");
    }

    /// #621: đường dẫn do app tự đặt mang timestamp nên phải sinh lại mỗi lần
    /// mở dialog; nếu giữ nguyên thì lần export sau lại dính prompt ghi đè.
    #[test]
    fn export_dialog_refreshes_an_app_generated_path_621() {
        let ctx = egui::Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_format: ExportFormat::Csv,
                export_path: "/tmp/stale-from-last-export.csv".to_owned(),
                export_path_is_default: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };
        let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            app.draw_export_dialog(ui, Some(&result));
        });

        assert_ne!(
            app.overlay.export_path, "/tmp/stale-from-last-export.csv",
            "a stale app-generated path must be regenerated"
        );
        assert!(app.overlay.export_path.ends_with(".csv"), "{}", app.overlay.export_path);
        assert!(app.overlay.export_path_is_default);
    }

    /// #621: path người dùng tự gõ/Browse là lựa chọn của họ, không được ghi đè.
    #[test]
    fn export_dialog_keeps_a_user_chosen_path_621() {
        let ctx = egui::Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_format: ExportFormat::Csv,
                export_path: "/tmp/my-own-target.csv".to_owned(),
                export_path_is_default: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![],
            rows: vec![],
            row_count: 0,
            duration_ms: 0,
        };
        let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            app.draw_export_dialog(ui, Some(&result));
        });

        assert_eq!(app.overlay.export_path, "/tmp/my-own-target.csv");
        assert!(!app.overlay.export_path_is_default);
    }

    /// #612: NULL được viết theo lựa chọn — mặc định CSV là ô rỗng, `\N` vẫn
    /// chọn được để round-trip `COPY` (#487).
    #[test]
    fn delimited_export_null_spelling_follows_the_selected_mode_612() {
        let dir = temp_dir("null-mode");
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "value".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            }],
            rows: vec![
                vec![UiCell::Null],
                vec![UiCell::Text(String::new())],
            ],
            row_count: 2,
            duration_ms: 0,
        };

        for (mode, expected) in [
            (ExportNullMode::Empty, "value\n\n\n"),
            (ExportNullMode::Literal, "value\nNULL\n\n"),
            (ExportNullMode::Sentinel, "value\n\\N\n\n"),
        ] {
            let file = format!("out-{}.csv", mode.label().replace('\\', "sentinel"));
            let path = dir.join(file);
            let mut app = DbProApp {
                overlay: OverlayState {
                    export_open: true,
                    export_path: path.to_string_lossy().into_owned(),
                    export_format: ExportFormat::Csv,
                    export_null: mode,
                    ..Default::default()
                },
                ..Default::default()
            };
            app.export_result(&result);

            let written = std::fs::read_to_string(&path).expect("exported file");
            assert_eq!(
                written.trim_start_matches('\u{feff}'),
                expected,
                "mode {:?} spelled NULL wrong",
                mode
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #616: dialog Export phải thật sự được dựng, không chỉ set cờ. UAT headless
    /// chỉ đọc được accessibility tree (Screen Recording bị từ chối), nên đây là
    /// mức kiểm chứng đúng: dialog có node trong tree hay không.
    #[test]
    fn export_dialog_is_drawn_into_the_accessibility_tree() {
        let ctx = egui::Context::default();
        crate::DbProTheme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: "out.csv".to_owned(),
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        let mut labels: Vec<String> = Vec::new();
        // `Dialog` fades in over a few passes, so drive several frames.
        for _ in 0..10 {
            let output = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
                app.draw_export_dialog(ui, Some(&result));
            });
            if let Some(update) = output.platform_output.accesskit_update {
                labels = update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| node.label().map(str::to_owned))
                    .collect();
                if labels.iter().any(|label| label.contains("Export")) {
                    break;
                }
            }
        }
        assert!(
            labels.iter().any(|label| label.contains("Export")),
            "export dialog never reached the accessibility tree: {labels:?}"
        );
    }

    #[test]
    fn export_refuses_to_replace_an_existing_file_without_confirmation() {
        let dir = temp_dir("confirm");
        let path = dir.join("out.csv");
        std::fs::write(&path, "previous export\n").expect("seed existing file");
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: path.to_string_lossy().into_owned(),
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        app.export_result(&result);

        assert!(
            app.overlay.export_overwrite_pending.is_some(),
            "the first click must ask, not overwrite"
        );
        assert!(app.overlay.export_open, "the dialog stays open for the confirmation");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "previous export\n",
            "nothing may be written before the user confirms"
        );

        app.export_result_confirming_overwrite(&result);

        assert!(app.overlay.export_overwrite_pending.is_none());
        assert!(!app.overlay.export_open);
        // #612: CSV files now start with a UTF-8 BOM, so compare the payload.
        let written = std::fs::read_to_string(&path).expect("read");
        assert_eq!(written.trim_start_matches('\u{feff}'), "id\n1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #539: the overwrite prompt is bound to the effective path it named. Changing the
    /// Format while the prompt is open must not let `Overwrite` replace the new format's
    /// file without asking again.
    #[test]
    fn overwrite_confirmation_does_not_follow_a_format_change() {
        let dir = temp_dir("format-change");
        let csv_path = dir.join("out.csv");
        let json_path = dir.join("out.csv.json");
        std::fs::write(&csv_path, "previous csv\n").expect("seed csv");
        std::fs::write(&json_path, "previous json\n").expect("seed json");
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: csv_path.to_string_lossy().into_owned(),
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        app.export_result(&result);
        assert_eq!(
            app.overlay.export_overwrite_pending.as_deref(),
            Some(csv_path.as_path()),
            "the prompt names the CSV destination"
        );

        app.overlay.export_format = ExportFormat::Json;
        app.export_result_confirming_overwrite(&result);

        assert_eq!(
            std::fs::read_to_string(&csv_path).expect("read csv"),
            "previous csv\n",
            "the confirmed CSV file must stay untouched"
        );
        assert_eq!(
            std::fs::read_to_string(&json_path).expect("read json"),
            "previous json\n",
            "the JSON destination was never named by the prompt, so Overwrite must ask again"
        );
        assert_eq!(
            app.overlay.export_overwrite_pending.as_deref(),
            Some(json_path.as_path()),
            "the prompt is re-raised for the new effective path"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_message_names_a_capped_result_as_capped() {
        let dir = temp_dir("capped");
        let path = dir.join("out.csv");
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: path.to_string_lossy().into_owned(),
                export_format: ExportFormat::Csv,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 500,
            duration_ms: 0,
        };

        app.export_result(&result);

        assert!(
            app.feedback.runtime_message.contains("Exported 1 rows")
                && app.feedback.runtime_message.contains("500 rows matched"),
            "a capped export must not read as a complete one: {}",
            app.feedback.runtime_message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #396: an arbitrary query result has no derivable source table, so the export dialog must
    /// not pre-fill the target with whatever table is selected in the explorer — that produced
    /// INSERT/COPY scripts aimed at the wrong table.
    #[test]
    fn export_dialog_leaves_table_name_empty_for_query_results() {
        let mut app = DbProApp::default();
        app.schema.explorer.selected_table = Some("orders".to_owned());
        app.workspace.active_tab = WorkspaceTab::Query;
        app.overlay.export_open = true;
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        let ctx = egui::Context::default();
        DbProTheme::install_fonts(&ctx);
        crate::test_frame::frame(&ctx, Default::default(), |ui| {
            app.draw_export_dialog(ui, Some(&result));
        });

        assert_eq!(
            app.overlay.export_table_name, "",
            "the explorer's selected table must not leak into a query-result export"
        );
    }

    /// #396: a table-data export does know its source — the field pre-fills with the browsed
    /// table's real name so the generated script targets it out of the box.
    #[test]
    fn export_dialog_prefills_table_name_in_table_data_view() {
        let mut app = DbProApp::default();
        app.schema.explorer.selected_table = Some("orders".to_owned());
        app.workspace.active_tab = WorkspaceTab::Table;
        app.table.state.table_view = TableView::Data;
        app.overlay.export_open = true;
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        let ctx = egui::Context::default();
        DbProTheme::install_fonts(&ctx);
        crate::test_frame::frame(&ctx, Default::default(), |ui| {
            app.draw_export_dialog(ui, Some(&result));
        });

        assert_eq!(app.overlay.export_table_name, "orders");
    }

    /// #396: a finished export clears the seeded name so reopening the dialog against a
    /// different table does not reuse the previous target.
    #[test]
    fn export_clears_seeded_table_name_after_writing() {
        let dir = temp_dir("clear-name");
        let path = dir.join("out.sql");
        let mut app = DbProApp {
            overlay: OverlayState {
                export_open: true,
                export_path: path.to_string_lossy().into_owned(),
                export_format: ExportFormat::SqlInsert,
                export_table_name: "orders".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };

        app.export_result(&result);

        assert_eq!(app.overlay.export_table_name, "");
        assert!(
            std::fs::read_to_string(&path).expect("read").contains("INSERT INTO \"orders\""),
            "the seeded table name must still drive the generated SQL"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// #541 — the query output dock used to deep-clone the entire active result every
/// frame just to pick the output pane (measured 67.4 ms/frame at 100k rows x 8 cols
/// on this machine, 58.6 ms in the issue's Linux run). The dock now takes a
/// refcount-bump `Rc` handle and paints by reference. These tests pin both halves:
/// the structural sharing and the release-mode frame budget.
mod issue541_dock_clone_tests {
    use super::super::*;
    use std::rc::Rc;
    use std::time::Instant;

    /// Same shape as the issue fixture: N rows x 8 Text columns of ~25 chars.
    fn dock_fixture(rows: usize, cols: usize) -> UiQueryResult {
        UiQueryResult {
            columns: (0..cols)
                .map(|c| crate::UiColumn {
                    name: format!("column_{c}"),
                    data_type: "text".to_owned(),
                    nullable: false,
                })
                .collect(),
            rows: (0..rows)
                .map(|r| {
                    (0..cols)
                        .map(|c| UiCell::Text(format!("value-{r}-{c}-padding-text!")))
                        .collect()
                })
                .collect(),
            row_count: rows as u64,
            duration_ms: 12,
        }
    }

    fn seeded_app(rows: usize) -> DbProApp {
        let mut app = DbProApp::default();
        if app.query.session.active_document().is_none() {
            app.new_query_document();
        }
        app.query
            .session
            .active_document_mut()
            .unwrap()
            .query_results = vec![Rc::new(dock_fixture(rows, 8))];
        app.query
            .session
            .active_document_mut()
            .unwrap()
            .active_result_index = 0;
        app
    }

    /// One real dock frame — the production `draw_query_output_dock`, not a mimic.
    fn dock_frame(app: &mut DbProApp, ctx: &egui::Context) {
        crate::test_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1024.0, 640.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    app.draw_query_output_dock(ui, 240.0);
                });
            },
        );
    }

    /// Structural proof of the fix: the frame handle aliases the stored grid —
    /// cloning it bumps a refcount, it never copies cells. A regression to
    /// `active_result().cloned()` cannot compile past this.
    #[test]
    fn dock_frame_handle_shares_the_stored_result() {
        let app = seeded_app(10);
        let handle = app
            .query
            .session
            .active_result_handle()
            .expect("seeded document must expose an active result");
        let stored = &app.query.session.active_document().unwrap().query_results[0];
        assert!(
            Rc::ptr_eq(&handle, stored),
            "the draw handle must point at the stored grid, not a copy"
        );

        // A second frame handle (what the dock takes per repaint) adds no row copy:
        // same allocation, refcount grows.
        let second = app
            .query
            .session
            .active_result_handle()
            .expect("handle stays available across frames");
        assert!(Rc::ptr_eq(&handle, &second));
        assert_eq!(
            Rc::strong_count(&handle),
            3,
            "storage + two frame handles must alias one grid"
        );

        // The draw path itself is reference-only: `draw_output_pane` accepts
        // `Option<&UiQueryResult>` derived from the local handle, so a 100k-row
        // grid needs no ownership to paint.
        let result = app.query.session.active_result_handle();
        let by_ref: Option<&UiQueryResult> = result.as_deref();
        assert!(by_ref.is_some(), "as_deref must expose the grid by reference");
    }

    /// Release-mode acceptance: a full dock frame at the issue's worst fixture
    /// (100k x 8) must fit the 16.7 ms/60 fps budget — it measured 67.4 ms before
    /// the fix because every frame deep-copied all 800k cells. Debug builds keep
    /// constant overhead, so they only paint (and print) without the assert.
    #[test]
    fn query_output_dock_frame_fits_budget_at_100k_rows() {
        let sizes = [1_000usize, 10_000, 100_000];
        let ctx = egui::Context::default();
        DbProTheme::install_fonts(&ctx);
        for &rows in &sizes {
            let mut app = seeded_app(rows);
            // Warm-up: projection build, egui internals, font atlas.
            dock_frame(&mut app, &ctx);
            let mut best = std::time::Duration::MAX;
            let mut median_samples = Vec::new();
            for _ in 0..5 {
                let t = Instant::now();
                dock_frame(&mut app, &ctx);
                let elapsed = t.elapsed();
                best = best.min(elapsed);
                median_samples.push(elapsed);
            }
            median_samples.sort();
            println!(
                "rows={rows:<7} dock_frame_best={best:?} dock_frame_median={:?}",
                median_samples[median_samples.len() / 2]
            );
            assert!(
                app.table.data.grid_projection_cache.rebuilds() >= 1,
                "the Results pane must actually receive the seeded grid (rows={rows})"
            );
            if !cfg!(debug_assertions) {
                assert!(
                    best.as_millis() < 17,
                    "query dock frame exceeded the 16.7 ms budget at {rows} rows: {best:?}"
                );
            }
        }
    }
}
