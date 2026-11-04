use std::error::Error;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread;

use db_pro_app::DbProApp;
use db_pro_core::application::sql_builder::{FilterOp, SortClause, SortDir, TableFilter};
use db_pro_core::domain::query::CellValue;
use db_pro_runtime::{spawn_worker, DbProRuntime, RuntimeCommand, RuntimeEvent, RuntimeRequestId};
use db_pro_ui::{
    DbProTheme, RequestId, TaskBridge, UiCell, UiCheckConstraint, UiColumn, UiCommand, UiConnectionDraft,
    UiConnectionSummary, UiDependencyDirection, UiDependencyKind, UiDriver, UiEvent, UiFunctionSummary, UiQueryError,
    UiQueryExecutionOutput, UiQueryFolderSummary, UiQueryResult, UiSavedQuerySummary, UiSchemaColumn,
    UiSchemaForeignKey, UiSchemaSummary, UiSslMode, UiStatementOutput, UiTableColumn, UiTableDataFilter,
    UiTableDataSort, UiTableDependency, UiTableFilterOperator, UiTableForeignKey, UiTableIndex, UiTableInfo,
    UiTableMutation, UiTableSummary, UiTriggerSummary, UiViewSummary,
};
use eframe::egui;
use tokio::runtime::Builder;

#[cfg(target_os = "macos")]
mod app_menu;
#[cfg(feature = "capture")]
mod capture;
mod macos_quit;
mod table_copy_port;
mod translate;
#[cfg(test)]
#[path = "tests/translate_tests.rs"]
mod translate_tests;

#[cfg(test)]
pub(crate) use macos_quit::{macos_close_uses_terminate, take_over_macos_close};
#[cfg(test)]
pub(crate) use translate::draft_to_domain;
use translate::{translate_command, translate_event};

fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    // Local cargo binaries must not poke the OS keyring — every launch was prompting
    // Keychain / Credential Manager. Packaged installs keep the default (keyring on).
    configure_dev_secret_store();
    let tokio_runtime = Builder::new_multi_thread().enable_all().build()?;
    let data_dir = resolve_data_dir();
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();

    let (copy_runtime, runtime_tx, runtime_rx) = tokio_runtime.block_on(async {
        let runtime = DbProRuntime::new(data_dir).await?;
        seed_default_connection(&runtime).await;
        let copy_runtime = std::sync::Arc::clone(&runtime);
        let (runtime_tx, runtime_rx) = spawn_worker(runtime, 64);
        Ok::<_, Box<dyn Error>>((copy_runtime, runtime_tx, runtime_rx))
    })?;

    spawn_command_picker(
        command_rx,
        runtime_tx.clone(),
        event_tx.clone(),
        tokio_runtime.handle().clone(),
    );

    spawn_event_pump(runtime_rx, event_tx, tokio_runtime.handle().clone());

    run_native_app(
        bridge,
        table_copy_port::live(copy_runtime, tokio_runtime.handle().clone()),
    )
}

/// Bridges file-picker commands (rfd, blocking) onto a dedicated thread so
/// native dialogs never stall the egui update; everything else translates and
/// forwards to the runtime channel.
fn spawn_command_picker(
    command_rx: std::sync::mpsc::Receiver<UiCommand>,
    runtime_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
    event_tx: std::sync::mpsc::SyncSender<UiEvent>,
    command_handle: tokio::runtime::Handle,
) {
    thread::spawn(move || {
        let send_picked = |request_id, kind: &str, path: Option<String>| {
            // Fire-and-forget: a picker result is only dropped once the UI has
            // gone away, which means the whole app is shutting down.
            let _ = event_tx.send(UiEvent::FilePicked {
                request_id,
                kind: kind.to_owned(),
                path,
            });
        };
        while let Ok(command) = command_rx.recv() {
            match command {
                UiCommand::PickSqliteFile { request_id } => {
                    let path = rfd::FileDialog::new()
                        .add_filter("SQLite database", &["db", "sqlite", "sqlite3"])
                        .pick_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "sqlite", path);
                    continue;
                }
                UiCommand::PickBackupFile { request_id } => {
                    let path = rfd::FileDialog::new()
                        .set_title("Choose backup output")
                        .save_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "backup", path);
                    continue;
                }
                UiCommand::PickRestoreFile { request_id } => {
                    let path = rfd::FileDialog::new()
                        .set_title("Choose backup to restore")
                        .pick_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "restore", path);
                    continue;
                }
                UiCommand::PickWorkspaceFolder { request_id } => {
                    let path = rfd::FileDialog::new()
                        .set_title("Open workspace folder")
                        .pick_folder()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "workspace-folder", path);
                    continue;
                }
                UiCommand::PickAuditExportFile { request_id } => {
                    let path = rfd::FileDialog::new()
                        .set_title("Export audit events as CSV")
                        .add_filter("CSV", &["csv"])
                        .set_file_name("audit-events.csv")
                        .save_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "audit-export", path);
                    continue;
                }
                UiCommand::PickExportFile { request_id } => {
                    let path = rfd::FileDialog::new()
                        .set_title("Export results")
                        .save_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "export-results", path);
                    continue;
                }
                UiCommand::PickSshPrivateKey { request_id } => {
                    let path = rfd::FileDialog::new()
                        .pick_file()
                        .map(|path| path.to_string_lossy().into_owned());
                    send_picked(request_id, "ssh-key", path);
                    continue;
                }
                command => {
                    let Some(command) = translate_command(command) else {
                        continue;
                    };
                    let send_result = command_handle.block_on(runtime_tx.send(command));
                    if send_result.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

fn init_tracing() {
    // Best-effort: a global tracing subscriber may already be installed when
    // the app is embedded in a host process, which is not a failure.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "db_pro_runtime=info".to_owned()))
        .try_init();
}

/// Name of the legacy, working-directory-local state directory.
const LEGACY_DATA_DIR_NAME: &str = ".db-pro-data";
/// Application directory name used inside the per-user platform data root.
const PLATFORM_APP_DIR_NAME: &str = "DB Pro";

/// Resolves the directory holding the application state (`meta.db`, `secrets/`).
///
/// Precedence: `DB_PRO_DATA_DIR`, then an *existing* working-directory-local
/// `.db-pro-data`, then the per-user platform data directory. A bundle launched
/// through LaunchServices runs with `/` as its working directory, so a legacy
/// directory that does not already exist is never created there: that failed
/// with `CreateDataDir(Os { code: 30, kind: ReadOnlyFilesystem })` and the app
/// quit before it opened a window.
fn resolve_data_dir() -> std::path::PathBuf {
    let override_dir = non_empty_env("DB_PRO_DATA_DIR").map(std::path::PathBuf::from);
    let cwd = std::env::current_dir().ok();
    let legacy_exists = cwd
        .as_deref()
        .is_some_and(|dir| dir.join(LEGACY_DATA_DIR_NAME).is_dir());

    choose_data_dir(override_dir, cwd.as_deref(), legacy_exists, platform_data_dir())
}

/// The precedence rule behind [`resolve_data_dir`], free of process-global state
/// so it can be exercised directly.
fn choose_data_dir(
    override_dir: Option<std::path::PathBuf>,
    cwd: Option<&std::path::Path>,
    legacy_exists: bool,
    platform_dir: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    if let Some(dir) = override_dir {
        return dir;
    }
    if legacy_exists {
        if let Some(dir) = cwd {
            return dir.join(LEGACY_DATA_DIR_NAME);
        }
    }
    if let Some(dir) = platform_dir {
        return dir;
    }
    cwd.map(|dir| dir.join(LEGACY_DATA_DIR_NAME))
        .unwrap_or_else(|| std::path::PathBuf::from(LEGACY_DATA_DIR_NAME))
}

/// Per-user state root offered by the platform, if one can be resolved.
fn platform_data_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        non_empty_env("HOME").map(|home| {
            std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(PLATFORM_APP_DIR_NAME)
        })
    } else if cfg!(target_os = "windows") {
        non_empty_env("APPDATA").map(|appdata| std::path::PathBuf::from(appdata).join(PLATFORM_APP_DIR_NAME))
    } else {
        non_empty_env("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| non_empty_env("HOME").map(|home| std::path::PathBuf::from(home).join(".local").join("share")))
            .map(|data_root| data_root.join("db-pro"))
    }
}

/// Reads an environment variable, treating an empty value as unset.
fn non_empty_env(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

/// For binaries launched from `target/debug` or `target/release`, disable the OS keyring
/// unless the developer explicitly opted in. Packaged installs are unaffected.
fn configure_dev_secret_store() {
    use db_pro_runtime::{DISABLE_KEYRING_ENV, USE_KEYRING_ENV};

    if non_empty_env(USE_KEYRING_ENV).is_some() || non_empty_env(DISABLE_KEYRING_ENV).is_some() {
        return;
    }
    if !is_cargo_target_binary() {
        return;
    }
    // SAFETY: single-threaded — called before the tokio runtime is built.
    std::env::set_var(DISABLE_KEYRING_ENV, "1");
    tracing::info!("cargo-target binary: OS keyring disabled (set {USE_KEYRING_ENV}=1 to enable Keychain prompts)");
}

fn is_cargo_target_binary() -> bool {
    std::env::current_exe()
        .ok()
        .as_ref()
        .and_then(|path| path.to_str())
        .is_some_and(|path| {
            path.contains("/target/debug/")
                || path.contains("/target/release/")
                || path.contains("\\target\\debug\\")
                || path.contains("\\target\\release\\")
        })
}

/// The developer-convenience connection seeded on first launch, if any.
///
/// Debug builds seed the developer's local PostgreSQL fixture so `cargo run`
/// starts with a usable connection. Release builds must not: a shipped binary
/// must never write a developer's private connection metadata into a user's
/// connection store (goal-3 §16 — never package local connection metadata or
/// credentials).
#[cfg(debug_assertions)]
fn developer_preset_connection() -> Option<db_pro_core::domain::connection::ConnectionConfig> {
    Some(db_pro_core::domain::connection::ConnectionConfig {
        name: "Xe Lạc Hồng (PostgreSQL)".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 5433,
        database: "fullstack_starter".to_owned(),
        username: "postgres".to_owned(),
        driver: db_pro_core::domain::connection::DriverType::Postgres,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: Some("#6366f1".to_owned()),
        tags: vec!["docker".to_owned(), "xe-lac-hong".to_owned()],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    })
}

/// Release builds seed nothing — see `developer_preset_connection`.
#[cfg(not(debug_assertions))]
fn developer_preset_connection() -> Option<db_pro_core::domain::connection::ConnectionConfig> {
    None
}

/// Log text for a failed seeding attempt, kept next to the preset it names so
/// the developer label does not survive into a release binary.
#[cfg(debug_assertions)]
const SEED_FAILURE_MESSAGE: &str = "failed to seed default Xe Lạc Hồng connection";
#[cfg(not(debug_assertions))]
const SEED_FAILURE_MESSAGE: &str = "failed to seed the developer default connection";

fn sample_sqlite_connection_config(path: &std::path::Path) -> db_pro_core::domain::connection::ConnectionConfig {
    db_pro_core::domain::connection::ConnectionConfig {
        name: "Sample E-Commerce (SQLite)".to_owned(),
        host: String::new(),
        port: 0,
        database: path.to_string_lossy().into_owned(),
        username: String::new(),
        driver: db_pro_core::domain::connection::DriverType::SQLite,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 5_000,
        color: Some("#10A37F".to_owned()),
        tags: vec!["sample".to_owned(), "sqlite".to_owned()],
        group: Some("Local Samples".to_owned()),
        favorite: true,
        environment: Default::default(),
        readonly: true,
    }
}

const SAMPLE_SQLITE_APPLICATION_ID: i32 = 0x4442_5052;

fn sample_database_open_path(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    // SQLITE_OPEN_NOFOLLOW fails when any parent is a symlink. macOS `/tmp` and
    // `/var` are symlinks, so a freshly created regular file still will not open.
    // Resolve the parent only; a symlink at the file itself stays rejected.
    let Some(parent) = path.parent() else {
        return Ok(path.to_path_buf());
    };
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let parent = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
    let Some(name) = path.file_name() else {
        return Err("sample database path has no file name".to_owned());
    };
    Ok(parent.join(name))
}

fn initialize_sample_sqlite(path: &std::path::Path) -> Result<(), String> {
    use rusqlite::OpenFlags;
    use std::fs::OpenOptions;
    use std::io::ErrorKind;

    let path = sample_database_open_path(path)?;

    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => {
            drop(file);
            let result = (|| {
                let connection = rusqlite::Connection::open_with_flags(
                    &path,
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
                )
                .map_err(|error| error.to_string())?;
                connection
                    .execute_batch(&format!(
                        "PRAGMA application_id = {SAMPLE_SQLITE_APPLICATION_ID};\
                         CREATE TABLE _db_pro_sample_fixture (marker TEXT NOT NULL);\
                         INSERT INTO _db_pro_sample_fixture (marker) VALUES ('db-pro-sample-v1');"
                    ))
                    .map_err(|error| error.to_string())?;
                const FIXTURE_SQL: &str = include_str!("../../../fixtures/sqlite/fixture.sql");
                connection
                    .execute_batch(FIXTURE_SQL)
                    .map_err(|error| error.to_string())?;
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&path);
            }
            result
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("sample database path is not a regular file".to_owned());
            }
            let connection = rusqlite::Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|error| format!("existing sample database is not a valid fixture: {error}"))?;
            let application_id: i32 = connection
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .map_err(|error| error.to_string())?;
            let marker: String = connection
                .query_row("SELECT marker FROM _db_pro_sample_fixture LIMIT 1", [], |row| {
                    row.get(0)
                })
                .map_err(|error| format!("existing sample database has no fixture marker: {error}"))?;
            if application_id != SAMPLE_SQLITE_APPLICATION_ID || marker != "db-pro-sample-v1" {
                return Err("existing sample database is not owned by DB Pro".to_owned());
            }
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

async fn seed_sqlite_sample_fixture(runtime: &DbProRuntime) {
    let sample_path = runtime.data_dir().join("sample-ecommerce.sqlite");
    if let Err(error) = initialize_sample_sqlite(&sample_path) {
        tracing::warn!(%error, "failed to initialize Sample E-Commerce SQLite fixture");
        return;
    }
    let database_path = sample_path.to_string_lossy();
    let connections = runtime.connections();
    let existing = connections.list().await.unwrap_or_default();
    if existing
        .iter()
        .any(|connection| connection.config.database == database_path)
    {
        return;
    }

    const LEGACY_SAMPLE_PATH: &str = "/tmp/db_pro_sample.db";
    if let Some(legacy) = existing.iter().find(|connection| {
        connection.config.driver == db_pro_core::domain::connection::DriverType::SQLite
            && connection.config.database == LEGACY_SAMPLE_PATH
            && connection.config.name == "Sample E-Commerce (SQLite)"
            && connection.config.tags.iter().any(|tag| tag == "sample")
    }) {
        let mut config = legacy.config.clone();
        config.database = database_path.into_owned();
        config.readonly = true;
        if let Err(error) = connections.update(&legacy.id, config, None).await {
            tracing::warn!(%error, "failed to migrate legacy SQLite sample connection");
        }
        return;
    }

    let config = sample_sqlite_connection_config(&sample_path);
    if let Err(err) = connections.create(config, "").await {
        tracing::warn!("failed to seed Sample E-Commerce SQLite connection: {err}");
    }
}

fn sample_enterprise_pg_connection_config() -> db_pro_core::domain::connection::ConnectionConfig {
    db_pro_core::domain::connection::ConnectionConfig {
        name: "Enterprise Multi-Domain (PostgreSQL 115 Tables)".to_owned(),
        host: "localhost".to_owned(),
        port: 5432,
        database: "enterprise_db".to_owned(),
        username: "truongdq".to_owned(),
        driver: db_pro_core::domain::connection::DriverType::Postgres,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 5_000,
        color: Some("#3B82F6".to_owned()),
        tags: vec!["sample".to_owned(), "postgres".to_owned(), "115-tables".to_owned()],
        group: Some("Local Samples".to_owned()),
        favorite: true,
        environment: Default::default(),
        readonly: false,
    }
}

async fn seed_enterprise_pg_sample_connection(runtime: &DbProRuntime) {
    let existing = runtime.connections().list().await.unwrap_or_default();
    if existing.iter().any(|c| c.config.database == "enterprise_db") {
        return;
    }
    let config = sample_enterprise_pg_connection_config();
    if let Err(err) = runtime.connections().create(config, "").await {
        tracing::warn!("failed to seed Enterprise PostgreSQL connection: {err}");
    }
}

/// Seeds the demo PostgreSQL connection the first time the app runs.
async fn seed_default_connection(runtime: &DbProRuntime) {
    seed_sqlite_sample_fixture(runtime).await;
    seed_enterprise_pg_sample_connection(runtime).await;
    let Some(config) = developer_preset_connection() else {
        return;
    };
    let database = config.database.clone();
    let port = config.port;
    let existing = runtime.connections().list().await.unwrap_or_default();
    if existing
        .iter()
        .any(|c| c.config.database == database && c.config.port == port)
    {
        return;
    }
    if let Err(err) = runtime.connections().create(config, "postgres").await {
        tracing::warn!("{SEED_FAILURE_MESSAGE}: {err}");
    }
}

/// Forwards translated runtime events to the UI, stopping when the UI is gone.
fn spawn_event_pump(
    mut runtime_rx: tokio::sync::mpsc::Receiver<RuntimeEvent>,
    event_tx: SyncSender<UiEvent>,
    event_handle: tokio::runtime::Handle,
) {
    event_handle.spawn(async move {
        while let Some(event) = runtime_rx.recv().await {
            if let Some(event) = translate_event(event) {
                if !send_ui_event_with_backpressure(&event_tx, event).await {
                    break;
                }
            }
        }
    });
}

async fn send_ui_event_with_backpressure(event_tx: &SyncSender<UiEvent>, mut event: UiEvent) -> bool {
    loop {
        match event_tx.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(next_event)) => {
                event = next_event;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }
}

/// Pins the initial window size from `DB_PRO_WINDOW_SIZE` (`1280x800`), instead of
/// maximizing.
///
/// UI acceptance evidence has to be captured at exact viewports
/// (`docs/10-egui-native-migration-plan.md`), and a maximized window renders at
/// whatever the monitor happens to be. Unset or malformed means "maximize as usual",
/// so this is inert for a normal launch.
fn capture_window_size() -> Option<[f32; 2]> {
    let raw = non_empty_env("DB_PRO_WINDOW_SIZE")?;
    parse_window_size(&raw.to_string_lossy())
}

/// The parsing half of [`capture_window_size`], free of process-global state.
fn parse_window_size(raw: &str) -> Option<[f32; 2]> {
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width: f32 = width.trim().parse().ok()?;
    let height: f32 = height.trim().parse().ok()?;
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        Some([width, height])
    } else {
        None
    }
}

// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
fn run_native_app(
    bridge: TaskBridge,
    table_copy: std::sync::Arc<dyn db_pro_core::application::TableCopyConnector>,
) -> Result<(), Box<dyn Error>> {
    let pinned_size = capture_window_size();
    tracing::info!(?pinned_size, "capture: resolved window size override");
    // The same RGBA PNG feeds winit's window/taskbar icon on macOS, Windows, and Linux.
    // Platform bundle formats are shipped beside it for installers that need native metadata.
    let app_icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/db-pro-logo.png"))?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("DB Pro")
        .with_icon(app_icon)
        .with_decorations(!cfg!(target_os = "linux"))
        .with_min_inner_size([1024.0, 640.0]);
    viewport = match pinned_size {
        // A `visible:false` window never receives drawRect on macOS, so the
        // first frame may never paint and the capture stalls before the first
        // update. Normal launches get their first redraw from the maximize
        // resize storm instead. X11 keeps painting an unmapped window, so there
        // a capture run stays off-screen instead of flashing over the user.
        Some(size) => viewport.with_inner_size(size).with_visible(!cfg!(target_os = "linux")),
        None => viewport.with_maximized(true),
    };

    let options = eframe::NativeOptions {
        viewport,
        // Capture runs pin an exact evidence size; restoring the user's
        // persisted (usually maximized) window state fights that pin, and on
        // exit a capture run would overwrite the user's real window frame.
        persist_window: std::env::var_os("DB_PRO_CAPTURE_TO").is_none(),
        ..Default::default()
    };

    eframe::run_native(
        "DB Pro",
        options,
        Box::new(move |creation_context| {
            // Re-apply the product default after eframe restores its persisted window
            // frame — unless a capture run pinned an exact viewport.
            if pinned_size.is_none() {
                creation_context
                    .egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            DbProTheme::install_fonts(&creation_context.egui_ctx);
            // egui paints a red "Nth use of ScrollArea ID …" overlay wherever two
            // widgets claim the same id. That is a developer signal, so it stays on
            // in debug builds; a shipped binary must never paint it over the UI.
            creation_context.egui_ctx.options_mut(|opts| {
                opts.warn_on_id_clash = cfg!(debug_assertions);
            });
            creation_context.egui_ctx.enable_accesskit();
            let mut app = DbProApp::with_task_bridge_and_storage(bridge, creation_context.storage);
            app.set_table_copy_connector(table_copy);
            #[cfg(target_os = "macos")]
            {
                let app_menu = app_menu::AppMenu::install()?;
                app_menu.connect_to_ui(&creation_context.egui_ctx);
                Ok(wrap_for_capture(app, app_menu))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Ok(wrap_for_capture(app))
            }
        }),
    )?;
    Ok(())
}

/// Wraps the app in the evidence capture driver when one was requested.
#[cfg(all(feature = "capture", target_os = "macos"))]
fn wrap_for_capture(app: DbProApp, menu: app_menu::AppMenu) -> Box<dyn eframe::App> {
    capture::CaptureApp::wrap(app, menu)
}

#[cfg(all(feature = "capture", not(target_os = "macos")))]
fn wrap_for_capture(app: DbProApp) -> Box<dyn eframe::App> {
    capture::CaptureApp::wrap(app)
}

#[cfg(all(not(feature = "capture"), target_os = "macos"))]
fn wrap_for_capture(app: DbProApp, menu: app_menu::AppMenu) -> Box<dyn eframe::App> {
    Box::new(NativeMenuApp { inner: app, menu })
}

/// Without the `capture` feature the app runs unwrapped.
#[cfg(all(not(feature = "capture"), not(target_os = "macos")))]
fn wrap_for_capture(app: DbProApp) -> Box<dyn eframe::App> {
    Box::new(app)
}

#[cfg(target_os = "macos")]
pub(crate) struct NativeMenuApp {
    inner: DbProApp,
    menu: app_menu::AppMenu,
}

#[cfg(target_os = "macos")]
impl eframe::App for NativeMenuApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.inner.clear_color(visuals)
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.inner.save(storage);
    }
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.inner, ctx, frame);
        macos_quit::on_macos_window_close(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.menu.apply_pending_actions(&mut self.inner, ui.ctx());
        eframe::App::ui(&mut self.inner, ui, frame);
    }
}

#[cfg(test)]
#[path = "tests/main_tests.rs"]
mod tests;
