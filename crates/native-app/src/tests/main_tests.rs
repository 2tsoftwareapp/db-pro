//! Tests for `../main.rs` (moved out of the source file).
use super::*;
use std::path::{Path, PathBuf};

#[test]
fn data_dir_override_wins_over_every_other_candidate() {
    let chosen = choose_data_dir(
        Some(PathBuf::from("/override/state")),
        Some(Path::new("/work")),
        true,
        Some(PathBuf::from("/platform/DB Pro")),
    );

    assert_eq!(chosen, PathBuf::from("/override/state"));
}

#[test]
fn existing_legacy_dir_wins_over_the_platform_dir() {
    let chosen = choose_data_dir(
        None,
        Some(Path::new("/work")),
        true,
        Some(PathBuf::from("/platform/DB Pro")),
    );

    assert_eq!(chosen, PathBuf::from("/work/.db-pro-data"));
}

/// Regression guard for the packaged-app startup blocker: a bundle launched
/// through LaunchServices runs with `cwd=/`, where no legacy directory
/// exists, so resolving `/.db-pro-data` made `DbProRuntime::new` fail with
/// `CreateDataDir(ReadOnlyFilesystem)` and the app quit before opening a
/// window. The per-user platform directory must be chosen instead.
#[test]
fn platform_dir_is_used_when_no_legacy_dir_exists() {
    let platform_dir = PathBuf::from("/platform/DB Pro");

    let chosen = choose_data_dir(None, Some(Path::new("/")), false, Some(platform_dir.clone()));

    assert_eq!(chosen, platform_dir);
}

#[test]
fn cwd_fallback_is_used_only_when_nothing_else_resolves() {
    let chosen = choose_data_dir(None, Some(Path::new("/work")), false, None);

    assert_eq!(chosen, PathBuf::from("/work/.db-pro-data"));

    // An unresolvable working directory must degrade to a relative path
    // rather than panic.
    let chosen = choose_data_dir(None, None, false, None);

    assert_eq!(chosen, PathBuf::from(".db-pro-data"));
}

/// Regression guard for the S-2 release-hygiene finding: the developer
/// connection preset is seeded by debug builds only. `cargo test --release`
/// exercises the release half of this assertion.
#[test]
fn developer_connection_preset_is_gated_to_debug_builds() {
    let preset = developer_preset_connection();

    assert_eq!(
        preset.is_some(),
        cfg!(debug_assertions),
        "the developer connection preset must exist in debug builds only"
    );

    if let Some(config) = preset {
        assert_eq!(config.name, "Xe Lạc Hồng (PostgreSQL)");
        assert_eq!(config.database, "fullstack_starter");
        assert_eq!(config.port, 5433);
    }
}

#[test]
fn sqlite_draft_drops_postgres_only_credentials() {
    let draft = UiConnectionDraft {
        driver: UiDriver::Sqlite,
        database: "/tmp/app.sqlite".to_owned(),
        password: "should-not-cross-boundary".to_owned(),
        ssh_tunnel_enabled: true,
        ssh_host: "bastion".to_owned(),
        ..Default::default()
    };

    let (config, password) = draft_to_domain(draft).expect("valid SQLite draft");

    assert!(password.is_empty());
    assert!(config.ssh_tunnel.is_none());
}

#[test]
fn window_size_override_parses_the_documented_gate_viewports() {
    for (raw, expected) in [
        ("1280x800", [1280.0, 800.0]),
        ("1440x900", [1440.0, 900.0]),
        ("1920x1080", [1920.0, 1080.0]),
        ("1280X800", [1280.0, 800.0]),
        (" 1280 x 800 ", [1280.0, 800.0]),
    ] {
        assert_eq!(parse_window_size(raw), Some(expected), "parsing {raw:?}");
    }
}

/// A malformed override must fall back to the maximized default rather than
/// pinning a degenerate window (or panicking on the launch path).
#[test]
fn malformed_window_size_falls_back_to_maximized() {
    for raw in [
        "",
        "1280",
        "1280x",
        "x800",
        "0x800",
        "1280x0",
        "-5x800",
        "widexhigh",
        "1280x800x600",
    ] {
        assert_eq!(parse_window_size(raw), None, "expected {raw:?} to be rejected");
    }
}

#[test]
fn application_icon_asset_is_square_and_decodes_for_native_windowing() {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/db-pro-logo.png"))
        .expect("the packaged application icon must be a valid PNG");

    assert_eq!(icon.width, icon.height);
    assert_eq!(icon.width, 256);
    assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
}

fn temp_dir() -> PathBuf {
    static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("dbpro-sample-seed-{nanos}-{count}"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

#[test]
fn sample_seed_preserves_unrecognized_existing_database() {
    let dir = temp_dir();
    let path = dir.join("sample.sqlite");
    let original = b"user-owned database bytes";
    std::fs::write(&path, original).unwrap();

    assert!(initialize_sample_sqlite(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let unmarked = dir.join("unmarked.sqlite");
    let connection = rusqlite::Connection::open(&unmarked).unwrap();
    connection
        .execute_batch("CREATE TABLE user_data (value TEXT); INSERT INTO user_data VALUES ('keep');")
        .unwrap();
    drop(connection);
    let before = std::fs::read(&unmarked).unwrap();
    assert!(initialize_sample_sqlite(&unmarked).is_err());
    assert_eq!(std::fs::read(&unmarked).unwrap(), before);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sample_seed_creates_and_verifies_only_marked_fixture() {
    let dir = temp_dir();
    let path = dir.join("sample.sqlite");

    initialize_sample_sqlite(&path).unwrap();
    let fixture_before_reuse = std::fs::read(&path).unwrap();
    initialize_sample_sqlite(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), fixture_before_reuse);
    assert!(path.is_file());
    let config = sample_sqlite_connection_config(&path);
    assert_eq!(config.database, path.to_string_lossy());
    assert!(config.readonly);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sample_seed_opens_when_parent_path_contains_a_symlink() {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let via_tmp = std::path::PathBuf::from(format!("/tmp/dbpro-sample-probe-{id}"));
    std::fs::create_dir_all(&via_tmp).expect("mkdir");
    let canonical = std::fs::canonicalize(&via_tmp).expect("canonicalize");
    let via_tmp_result = initialize_sample_sqlite(&via_tmp.join("sample.sqlite"));
    let via_canonical = initialize_sample_sqlite(&canonical.join("sample.sqlite"));
    let _ = std::fs::remove_dir_all(&canonical);
    assert!(via_tmp_result.is_ok(), "symlink parent should open: {via_tmp_result:?}");
    assert!(via_canonical.is_ok(), "canonical parent should open: {via_canonical:?}");
}

#[test]
fn saved_sample_connection_returns_product_rows_through_the_runtime() {
    let dir = temp_dir();
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let runtime = DbProRuntime::new(&dir).await.expect("app runtime");
            seed_sqlite_sample_fixture(&runtime).await;
            let saved = runtime
                .connections()
                .list()
                .await
                .expect("saved connections")
                .into_iter()
                .find(|connection| connection.config.name == "Sample E-Commerce (SQLite)")
                .expect("sample connection should be saved");
            let connection_id = saved.id.to_string();
            let (command_tx, mut event_rx) = spawn_worker(runtime, 8);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
                .await
                .expect("worker ready");
            command_tx
                .send(RuntimeCommand::Connect {
                    request_id: RuntimeRequestId(1),
                    connection_id: connection_id.clone(),
                })
                .await
                .expect("connect reaches worker");
            let connected = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
                .await
                .expect("connect finishes")
                .expect("worker stays open");
            assert!(matches!(connected, RuntimeEvent::Connected { .. }), "{connected:?}");
            command_tx
                .send(RuntimeCommand::ExecuteQuery {
                    request_id: RuntimeRequestId(2),
                    connection_id: connection_id.clone(),
                    sql: "SELECT COUNT(*) FROM products".to_owned(),
                    params: Vec::new(),
                    schema: None,
                    session_id: None,
                })
                .await
                .expect("query reaches worker");
            let queried = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
                .await
                .expect("query finishes")
                .expect("worker stays open");
            command_tx
                .send(RuntimeCommand::Disconnect {
                    request_id: RuntimeRequestId(3),
                    connection_id,
                })
                .await
                .expect("disconnect reaches worker");
            let disconnected = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
                .await
                .expect("disconnect finishes")
                .expect("worker stays open");
            assert!(
                matches!(
                    disconnected,
                    RuntimeEvent::OperationCompleted { operation, .. } if operation == "connection.disconnected"
                ),
                "{disconnected:?}"
            );
            drop(command_tx);
            queried
        });
    let _ = std::fs::remove_dir_all(&dir);
    match outcome {
        RuntimeEvent::QueryCompleted { result, .. } => {
            assert_eq!(result.row_count, 1, "{result:?}");
            assert!(
                matches!(result.rows[0].0.first(), Some(CellValue::Int64(4))),
                "{result:?}"
            );
        }
        other => panic!("saved sample query did not complete: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn sample_seed_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir();
    let target = dir.join("user.sqlite");
    let link = dir.join("sample.sqlite");
    let original = b"outside user file";
    std::fs::write(&target, original).unwrap();
    symlink(&target, &link).unwrap();

    assert!(initialize_sample_sqlite(&link).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), original);

    let _ = std::fs::remove_dir_all(dir);
}

/// Compiles only when eframe's `persistence` feature is on — the gate that
/// actually calls `App::save` + storage flush on quit/terminate.
#[test]
fn eframe_persistence_is_enabled_so_quit_can_save() {
    assert!(
        eframe::storage_dir("DB Pro").is_some(),
        "eframe persistence must resolve a native storage directory"
    );
}

/// Close-button / Cmd+W / ViewportCommand::Close: cancel eframe teardown and
/// take the platform quit (terminate), matching craft-quit's proven M2 shape.
#[test]
fn macos_close_request_cancels_window_teardown_for_terminate() {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);

    let mut taken = false;
    let mut quit_calls = 0u32;
    let mut out = ctx.run_logic(&input, |ctx| {
        taken = take_over_macos_close(ctx, || {
            quit_calls += 1;
            true
        });
    });
    let commands = out
        .viewport_commands
        .remove(&egui::ViewportId::ROOT)
        .unwrap_or_default();

    assert!(taken, "close request must be taken over by the terminate path");
    assert_eq!(quit_calls, 1);
    assert!(
        commands.contains(&egui::ViewportCommand::CancelClose),
        "eframe must not destroy the window; NSApp terminate runs App::save first"
    );
}

#[test]
fn macos_close_respects_cancel_close_already_queued() {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);

    let mut taken = false;
    let mut quit_calls = 0u32;
    let _ = ctx.run_logic(&input, |ctx| {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        taken = take_over_macos_close(ctx, || {
            quit_calls += 1;
            true
        });
    });

    assert!(!taken);
    assert_eq!(quit_calls, 0, "CancelClose must skip terminate");
}
