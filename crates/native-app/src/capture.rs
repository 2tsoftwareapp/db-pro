//! Deterministic UI evidence capture.
//!
//! eframe's built-in `EFRAME_SCREENSHOT_TO` harness fires on a fixed egui pass and
//! reads the framebuffer *after* `swap_buffers`, so it captures the frame before
//! the one it was asked for. On this app that is the frame before the connection
//! list arrives, so every capture showed a start page still in its loading state.
//!
//! This driver waits for the layout to settle and then requests the screenshot
//! itself, so the PNG holds the state the run was actually asked to document.
//!
//! Enabled by `DB_PRO_CAPTURE_TO=<path.png>` together with `DB_PRO_WINDOW_SIZE`.

use std::path::PathBuf;

use db_pro_app::DbProApp;
use eframe::egui;

/// Environment variable naming the PNG to write.
const PATH_ENV: &str = "DB_PRO_CAPTURE_TO";

/// Environment variable overriding [`SETTLE_FRAMES`], so a run can deliberately
/// document a transient state such as the loading skeletons.
const SETTLE_ENV: &str = "DB_PRO_CAPTURE_SETTLE_FRAMES";

/// Environment variable that, when set, asks the capture run to open the
/// new-connection dialog before capturing. This documents the password input and
/// eye toggle — the affected surface for the input click-steal fix — instead of the
/// default window. Inert for a normal launch.
const NEW_CONNECTION_ENV: &str = "DB_PRO_CAPTURE_NEW_CONNECTION";
/// Environment variable capturing the New Connection modal while Test Connection is pending.
const CONNECTION_TEST_PENDING_ENV: &str = "DB_PRO_CAPTURE_CONNECTION_TEST_PENDING";

/// Environment variable that opens the New Connection dialog with a stable
/// validation error for the error-state acceptance capture.
const CONNECTION_ERROR_ENV: &str = "DB_PRO_CAPTURE_CONNECTION_ERROR";
/// When set with either connection-dialog capture mode, use the light theme.
const CONNECTION_LIGHT_ENV: &str = "DB_PRO_CAPTURE_CONNECTION_LIGHT";

/// Environment variable that holds the Welcome connection request pending for
/// the loading-state acceptance capture.
const LOADING_ENV: &str = "DB_PRO_CAPTURE_LOADING";

/// Environment variable that, when set, asks the capture run to open the
/// Edit Connection dialog with a test draft, so the password input + eye toggle
/// on the edit surface can be documented. Inert for a normal launch.
const EDIT_CONNECTION_ENV: &str = "DB_PRO_CAPTURE_EDIT_CONNECTION";

/// When set, switch to the Query workspace before capturing (UI05 editor-first shots).
const QUERY_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_QUERY";

/// When set with [`QUERY_WORKSPACE_ENV`], force light theme for the Query capture.
const QUERY_LIGHT_ENV: &str = "DB_PRO_CAPTURE_QUERY_LIGHT";

/// When set, switch to the Table workspace before capturing (data grid / structure shots).
const TABLE_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_TABLE";

/// When set with [`TABLE_WORKSPACE_ENV`], force light theme for the Table capture.
const TABLE_LIGHT_ENV: &str = "DB_PRO_CAPTURE_TABLE_LIGHT";

/// When set with [`TABLE_WORKSPACE_ENV`], capture the deterministic Profile pane.
const TABLE_PROFILE_ENV: &str = "DB_PRO_CAPTURE_TABLE_PROFILE";

/// When set with [`TABLE_WORKSPACE_ENV`], capture the deterministic Indexes pane.
const TABLE_INDEXES_ENV: &str = "DB_PRO_CAPTURE_TABLE_INDEXES";

/// When set with [`TABLE_WORKSPACE_ENV`], focus the condition and show a
/// deterministic column completion result instead of the ordinary table state.
const TABLE_COMPLETION_ENV: &str = "DB_PRO_CAPTURE_TABLE_COMPLETION";

/// When set, switch to the ER Diagram canvas before capturing.
const DIAGRAM_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_DIAGRAM";

/// When set, switch to the Settings activity before capturing.
const SETTINGS_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_SETTINGS";

/// When set, open the Agent workspace panel before capturing.
const AGENT_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_AGENT";

/// When set, switch to the native Component Gallery before capturing.
const COMPONENT_GALLERY_ENV: &str = "DB_PRO_CAPTURE_COMPONENT_GALLERY";

/// When set, switch the sidebar to the History activity with seeded execution
/// records before capturing (unified execution history shots).
const HISTORY_ACTIVITY_ENV: &str = "DB_PRO_CAPTURE_HISTORY";

/// Show the schema-compare workspace seeded with a diff + plan recorded
/// against a divergent target (exercises the spec-09 safety lock).
const COMPARE_WORKSPACE_ENV: &str = "DB_PRO_CAPTURE_COMPARE";

/// When set, open the Explorer with a non-default object filter before
/// capturing (object-filter workbench evidence shots).
const EXPLORER_FILTER_ENV: &str = "DB_PRO_CAPTURE_FILTER";

/// Open the workspace tab strip with an off-screen query selected.
const WORKSPACE_TABS_OVERFLOW_ENV: &str = "DB_PRO_CAPTURE_WORKSPACE_TABS";
const WORKSPACE_TABS_LIGHT_ENV: &str = "DB_PRO_CAPTURE_WORKSPACE_TABS_LIGHT";

/// When set, open Quick Open with its Schema filter selected before capturing.
const QUICK_OPEN_ENV: &str = "DB_PRO_CAPTURE_QUICK_OPEN";

/// When set with [`QUICK_OPEN_ENV`], use the light theme for the Quick Open capture.
const QUICK_OPEN_LIGHT_ENV: &str = "DB_PRO_CAPTURE_QUICK_OPEN_LIGHT";

/// When set with [`QUICK_OPEN_ENV`], show the Quick Open empty state.
const QUICK_OPEN_EMPTY_ENV: &str = "DB_PRO_CAPTURE_QUICK_OPEN_EMPTY";

/// Environment variable pinning the viewport size for evidence runs (the same key
/// `main.rs` reads for the initial window). The capture driver re-asserts it each
/// frame so the window cannot maximize itself away from the requested size.
const SIZE_ENV: &str = "DB_PRO_WINDOW_SIZE";

/// Parses [`SIZE_ENV`] into a viewport size, or `None` when unset/malformed.
fn capture_size_from_env() -> Option<egui::Vec2> {
    let raw = std::env::var(SIZE_ENV).ok()?;
    let (w, h) = raw.split_once(['x', 'X'])?;
    let w: f32 = w.trim().parse().ok()?;
    let h: f32 = h.trim().parse().ok()?;
    if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
        Some(egui::Vec2::new(w, h))
    } else {
        None
    }
}

/// Frames to render before asking for a screenshot. The connection list arrives
/// asynchronously within the first couple of frames; the margin covers a viewport
/// resize landing and the first layout pass settling.
const SETTLE_FRAMES: u32 = 60;

/// Re-send `ViewportCommand::Screenshot` when no `Event::Screenshot` has arrived
/// after this many frames. The request can be dropped when it lands on the same
/// pass as a viewport resize, so a single-shot send can hang the capture run.
const SCREENSHOT_RETRY_FRAMES: u32 = 30;

/// Hard stop after the settle budget: if the screenshot reply still has not
/// arrived, exit instead of letting the capture process run forever.
const CAPTURE_TIMEOUT_FRAMES: u32 = 600;

/// Env var enabling headless-audit mode: `DB_PRO_AUDIT_JSON=<path>` runs the
/// inspector's audit pipeline, writes schema-v1 JSON, and exits with the
/// audit verdict code (0 clean / 1 findings / 2 tool error). Shares the fixture
/// (`DB_PRO_INSPECTOR_AUDIT_FIXTURE`), surface-selection, and
/// [`SIZE_ENV`] env vars with capture mode.
const AUDIT_ENV: &str = "DB_PRO_AUDIT_JSON";

/// Fallback frame budget for audit mode when the semantic tree never warms up
/// (e.g. accesskit disabled): emit anyway rather than hang forever.
const AUDIT_TIMEOUT_FRAMES: u32 = 180;

/// True when the headless audit run is requested (a non-empty output path).
fn audit_requested() -> bool {
    std::env::var_os(AUDIT_ENV).is_some_and(|v| !v.is_empty())
}

/// The settle frame count, honouring [`SETTLE_ENV`]. A malformed or zero value
/// falls back to the default, so a typo cannot produce a blank capture.
fn settle_frames() -> u32 {
    std::env::var(SETTLE_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|frames| *frames > 0)
        .unwrap_or(SETTLE_FRAMES)
}

/// Wraps [`DbProApp`] to write one framebuffer PNG and then close.
pub(super) struct CaptureApp {
    inner: DbProApp,
    #[cfg(target_os = "macos")]
    menu: super::app_menu::AppMenu,
    path: PathBuf,
    settle: u32,
    frames: u32,
    requested_at: Option<u32>,
    opened_dialog: bool,
    prepared_loading: bool,
    pinned: Option<egui::Vec2>,
    /// Headless audit mode (`DB_PRO_AUDIT_JSON`): emit once warm, then exit.
    audit: bool,
    /// Set once the report has been written so a straggler frame can't
    /// double-emit before the process exits.
    audit_done: bool,
    /// Audit emission captured but waiting on the bundle screenshot frame —
    /// `Some((report json, verdict code))` while stage 2 runs.
    #[cfg(debug_assertions)]
    pending_review: Option<(serde_json::Value, i32)>,
}

impl CaptureApp {
    /// Wraps `app` when [`PATH_ENV`] is set, otherwise hands it back untouched so a
    /// normal launch carries no capture behaviour at all.
    #[cfg(target_os = "macos")]
    pub(super) fn wrap(inner: DbProApp, menu: super::app_menu::AppMenu) -> Box<dyn eframe::App> {
        match std::env::var_os(PATH_ENV).filter(|value| !value.is_empty()) {
            Some(raw) => Box::new(Self {
                inner,
                menu,
                path: PathBuf::from(raw),
                settle: settle_frames(),
                frames: 0,
                requested_at: None,
                opened_dialog: false,
                prepared_loading: false,
                pinned: capture_size_from_env(),
                audit: audit_requested(),
                audit_done: false,
                #[cfg(debug_assertions)]
                pending_review: None,
            }),
            None if audit_requested() && cfg!(debug_assertions) => Box::new(Self {
                inner,
                menu,
                // Audit mode ignores `path` — no screenshot is taken.
                path: PathBuf::new(),
                settle: settle_frames(),
                frames: 0,
                requested_at: None,
                opened_dialog: false,
                prepared_loading: false,
                pinned: capture_size_from_env(),
                audit: true,
                audit_done: false,
                #[cfg(debug_assertions)]
                pending_review: None,
            }),
            None => Box::new(super::NativeMenuApp { inner, menu }),
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn wrap(inner: DbProApp) -> Box<dyn eframe::App> {
        match std::env::var_os(PATH_ENV).filter(|value| !value.is_empty()) {
            Some(raw) => Box::new(Self {
                inner,
                path: PathBuf::from(raw),
                settle: settle_frames(),
                frames: 0,
                requested_at: None,
                opened_dialog: false,
                prepared_loading: false,
                pinned: capture_size_from_env(),
                audit: audit_requested(),
                audit_done: false,
                #[cfg(debug_assertions)]
                pending_review: None,
            }),
            None if audit_requested() && cfg!(debug_assertions) => Box::new(Self {
                inner,
                // Audit mode ignores `path` — no screenshot is taken.
                path: PathBuf::new(),
                settle: settle_frames(),
                frames: 0,
                requested_at: None,
                opened_dialog: false,
                prepared_loading: false,
                pinned: capture_size_from_env(),
                audit: true,
                audit_done: false,
                #[cfg(debug_assertions)]
                pending_review: None,
            }),
            None => Box::new(inner),
        }
    }

    /// Writes a captured frame to `self.path`, reporting the outcome. Named
    /// `write_png` rather than `save` because `eframe::App` already defines `save`.
    fn write_png(&self, image: &egui::ColorImage) -> bool {
        self.write_png_to(&self.path.clone(), image)
    }

    /// PNG write to an explicit path — the review bundle's screenshot lands
    /// in `DB_PRO_REVIEW_DIR`, separate from the capture output path.
    fn write_png_to(&self, path: &std::path::Path, image: &egui::ColorImage) -> bool {
        let [width, height] = image.size;
        let Ok(width) = u32::try_from(width) else {
            tracing::error!(path = %path.display(), width, "capture: framebuffer width exceeds PNG limits");
            return false;
        };
        let Ok(height) = u32::try_from(height) else {
            tracing::error!(path = %path.display(), height, "capture: framebuffer height exceeds PNG limits");
            return false;
        };
        let rgba: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
        match image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8) {
            Ok(()) => {
                tracing::info!(path = %path.display(), width, height, "capture: wrote framebuffer");
                true
            }
            Err(err) => {
                tracing::error!(path = %path.display(), %err, "capture: failed to write framebuffer");
                false
            }
        }
    }
}

/// The reply to `ViewportCommand::Screenshot` — shared by the capture path
/// and the audit-review stage.
fn take_screenshot(ctx: &egui::Context) -> Option<std::sync::Arc<egui::ColorImage>> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    })
}

impl eframe::App for CaptureApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.inner.save(storage);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.inner, ctx, frame);
        // The picker fixture seeds/clears catalog data, so it must land *after* the
        // inner logic applied this frame's runtime events but *before* the frame
        // draws — otherwise a live introspection event refills a cleared list.
        if std::env::var_os("DB_PRO_CAPTURE_QUERY_CONTEXT_PICKER").is_some() {
            self.inner.open_query_context_picker_for_capture();
        }
        super::macos_quit::on_macos_window_close(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(target_os = "macos")]
        self.menu.apply_pending_actions(&mut self.inner, &ctx);
        self.prepare_loading();
        eframe::App::ui(&mut self.inner, ui, frame);
        self.open_requested_surface(&ctx);
        if std::env::var_os("DB_PRO_CAPTURE_EXPLORER_FILTER_POPOVER").is_some() {
            egui::Popup::open_id(&ctx, egui::Id::new("explorer_object_filter_workbench"));
        }
        self.pin_viewport(&ctx);
        #[cfg(debug_assertions)]
        if self.audit {
            // Audit mode owns the frame loop end-to-end: collect the emission
            // or keep painting — the screenshot path below is for captures.
            self.run_audit(&ctx);
            return;
        }
        if self.capture_screenshot(&ctx) {
            return;
        }
        self.advance_capture(&ctx);
    }
}

impl CaptureApp {
    fn prepare_loading(&mut self) {
        if !self.prepared_loading && std::env::var_os(LOADING_ENV).is_some() {
            self.inner.prepare_loading_for_capture();
            self.inner.mark_capture_provenance();
            self.prepared_loading = true;
        }
    }

    fn open_requested_surface(&mut self, ctx: &egui::Context) {
        // A late async event (schema load) can switch the active workspace tab
        // away from the requested fixture. Opt-in reassertion keeps the fixture
        // in place through the settle window.
        let reassert = std::env::var_os("DB_PRO_CAPTURE_REASSERT_SURFACE").is_some();
        if (self.opened_dialog && !reassert) || self.frames < 2 {
            return;
        }
        // Evidence hook: when asked, open the requested fixture surface. Gated
        // by env vars so a normal launch is unaffected; any opener that ran
        // stamps provenance — a code path, not an env string.
        self.opened_dialog = self.open_feature_cleanup_capture()
            || self.open_dialog_capture()
            || self.open_query_capture(ctx)
            || self.open_table_capture()
            || self.open_workspace_capture();
        if self.opened_dialog {
            self.inner.mark_capture_provenance();
        }
    }

    fn open_feature_cleanup_capture(&mut self) -> bool {
        let Ok(surface) = std::env::var("DB_PRO_CAPTURE_FEATURE") else {
            return false;
        };
        let light = std::env::var_os("DB_PRO_CAPTURE_FEATURE_LIGHT").is_some();
        self.inner.open_workstation_cleanup_for_capture(&surface, light)
            || self.inner.open_data_cleanup_for_capture(&surface, light)
    }

    /// Connection-dialog captures: open the dialog surface, nothing else.
    fn open_dialog_capture(&mut self) -> bool {
        let inner = &mut self.inner;
        if std::env::var_os(CONNECTION_TEST_PENDING_ENV).is_some() {
            inner.open_connection_test_pending_for_capture(std::env::var_os(CONNECTION_LIGHT_ENV).is_some());
        } else if std::env::var_os(NEW_CONNECTION_ENV).is_some() {
            inner.open_connection_dialog_for_capture(Some(std::env::var_os(CONNECTION_LIGHT_ENV).is_some()), false);
        } else if std::env::var_os(CONNECTION_ERROR_ENV).is_some() {
            inner.open_connection_dialog_for_capture(Some(std::env::var_os(CONNECTION_LIGHT_ENV).is_some()), true);
        } else if std::env::var_os(EDIT_CONNECTION_ENV).is_some() {
            inner.open_edit_connection_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_WELCOME").is_some() {
            inner.open_welcome_workspace_for_capture(std::env::var_os("DB_PRO_CAPTURE_WELCOME_LIGHT").is_some());
        } else {
            return false;
        }
        true
    }

    /// Query workspace capture; DB_PRO_CAPTURE_QUERY_LIMIT additionally opens the
    /// row-limit popup so evidence covers it.
    fn open_query_capture(&mut self, ctx: &egui::Context) -> bool {
        if std::env::var_os(QUERY_WORKSPACE_ENV).is_none() {
            return false;
        }
        if std::env::var_os(QUERY_LIGHT_ENV).is_some() {
            self.inner.open_query_workspace_for_capture_light();
        } else {
            self.inner.open_query_workspace_for_capture();
        }
        if std::env::var_os("DB_PRO_CAPTURE_QUERY_LIMIT").is_some() {
            // Select stores its open flag in temp data, not egui's Popup memory;
            // opening the Popup id left the menu closed in every capture.
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("query_row_limit"), true));
        }
        if std::env::var_os("DB_PRO_CAPTURE_QUERY_CONTEXT_PICKER").is_some() {
            self.inner.open_query_context_picker_for_capture();
        }
        true
    }

    /// Table workspace capture; the specific surface env wins over the base
    /// workspace, and DB_PRO_CAPTURE_TABLE_RECORD primes a record view.
    // cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
    fn open_table_capture(&mut self) -> bool {
        if std::env::var_os(TABLE_WORKSPACE_ENV).is_none() {
            return false;
        }
        let light = std::env::var_os(TABLE_LIGHT_ENV).is_some();
        if std::env::var_os("DB_PRO_CAPTURE_TABLE_DDL").is_some() {
            self.inner.open_table_ddl_for_capture(light);
        } else if std::env::var_os(TABLE_PROFILE_ENV).is_some() {
            self.inner.open_table_profile_for_capture(light);
        } else if std::env::var_os("DB_PRO_CAPTURE_TABLE_STRUCTURE").is_some() {
            self.inner.open_table_structure_for_capture(light);
        } else if std::env::var_os("DB_PRO_CAPTURE_TABLE_FOREIGN_KEYS").is_some() {
            self.inner.open_table_foreign_keys_for_capture(light);
        } else if std::env::var_os("DB_PRO_CAPTURE_TABLE_CONSTRAINTS").is_some() {
            self.inner.open_table_constraints_for_capture(light);
        } else if std::env::var_os("DB_PRO_CAPTURE_TABLE_DEPENDENCIES").is_some() {
            self.inner.open_table_dependencies_for_capture(light);
        } else if std::env::var_os(TABLE_INDEXES_ENV).is_some() {
            self.inner.open_table_indexes_for_capture(light);
        } else if std::env::var_os(TABLE_COMPLETION_ENV).is_some() {
            self.inner.open_table_condition_completion_for_capture(light);
        } else if light {
            self.inner.open_table_workspace_for_capture_light();
        } else {
            self.inner.open_table_workspace_for_capture();
        }
        if let Ok(state) = std::env::var("DB_PRO_CAPTURE_TABLE_RECORD") {
            self.inner.prepare_table_record_for_capture(&state);
        }
        true
    }

    /// Remaining single-shot workspace captures, dispatched by env var.
    fn open_workspace_capture(&mut self) -> bool {
        if self.open_tools_files_capture() {
            return true;
        }
        let inner = &mut self.inner;
        if std::env::var_os(DIAGRAM_WORKSPACE_ENV).is_some() {
            inner.open_diagram_workspace_for_capture();
        } else if std::env::var_os(SETTINGS_WORKSPACE_ENV).is_some() {
            inner.open_settings_workspace_for_capture();
        } else if std::env::var_os(AGENT_WORKSPACE_ENV).is_some() {
            inner.open_agent_workspace_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_AGENT_THREAD").is_some() {
            inner.open_agent_thread_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_DDL_CONFIRM").is_some() {
            inner.open_table_ddl_confirm_for_capture(std::env::var_os(TABLE_LIGHT_ENV).is_some());
        } else if std::env::var_os("DB_PRO_CAPTURE_DESTRUCTIVE").is_some() {
            inner.open_destructive_run_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_EXPLAIN").is_some() {
            inner.open_explain_plan_for_capture();
        } else if std::env::var_os(COMPONENT_GALLERY_ENV).is_some() {
            inner.open_component_gallery_for_capture();
        } else if std::env::var_os(WORKSPACE_TABS_OVERFLOW_ENV).is_some() {
            inner.open_workspace_tabs_overflow_for_capture(std::env::var_os(WORKSPACE_TABS_LIGHT_ENV).is_some());
        } else if std::env::var_os(EXPLORER_FILTER_ENV).is_some() {
            inner.open_explorer_filter_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_ROUTINE_DEBUG").is_some() {
            inner.open_routine_debug_for_capture(std::env::var_os("DB_PRO_CAPTURE_ROUTINE_DEBUG_LIGHT").is_some());
        } else if std::env::var_os(HISTORY_ACTIVITY_ENV).is_some() {
            inner.open_history_activity_for_capture();
        } else if std::env::var_os(COMPARE_WORKSPACE_ENV).is_some() {
            inner.open_schema_compare_for_capture();
        } else if std::env::var_os("DB_PRO_CAPTURE_RESULTS").is_some() {
            inner.open_results_dock_for_capture(std::env::var_os("DB_PRO_CAPTURE_RESULTS_LIGHT").is_some());
        } else if std::env::var_os(QUICK_OPEN_ENV).is_some() {
            inner.open_quick_open_for_capture(
                std::env::var_os(QUICK_OPEN_LIGHT_ENV).is_some(),
                std::env::var_os(QUICK_OPEN_EMPTY_ENV).is_some(),
            );
        } else {
            return false;
        }
        true
    }

    /// Evidence hooks for the Tools hub and Files workspace, kept out of the
    /// main dispatch chain to stay within its complexity budget.
    fn open_tools_files_capture(&mut self) -> bool {
        let inner = &mut self.inner;
        if std::env::var_os("DB_PRO_CAPTURE_TOOLS").is_some() {
            inner.open_tools_hub_for_capture(std::env::var_os("DB_PRO_CAPTURE_TOOLS_LIGHT").is_some());
        } else if std::env::var_os("DB_PRO_CAPTURE_FILES").is_some() {
            inner.open_files_workspace_for_capture(
                std::path::PathBuf::from(std::env::var("DB_PRO_CAPTURE_FILES_PATH").unwrap_or_default()),
                std::env::var_os("DB_PRO_CAPTURE_FILES_LIGHT").is_some(),
            );
        } else {
            return false;
        }
        true
    }

    fn pin_viewport(&self, ctx: &egui::Context) {
        // Re-assert the pinned viewport size every frame so the window cannot
        // maximize itself away from the requested evidence size.
        if let Some(size) = self.pinned {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
    }

    /// Headless audit (`DB_PRO_AUDIT_JSON`): the emission is produced inside
    /// `dev_tools.draw` once warm — this collects it. With `DB_PRO_REVIEW_DIR`
    /// set the run continues: request a screenshot for the review bundle,
    /// attach `visual_review`, then write the report and exit. Release builds
    /// lack the dev-tools module; the env var is inert.
    #[cfg(debug_assertions)]
    fn run_audit(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        // Stage 2: emission captured, waiting on the screenshot frame.
        if let Some((json, code)) = self.pending_review.take() {
            let shot_ok = take_screenshot(ctx)
                .map(|image| {
                    let target = std::env::var_os("DB_PRO_REVIEW_DIR")
                        .map(std::path::PathBuf::from)
                        .map(|d| d.join(format!("{}.png", db_pro_app::DbProApp::audit_scenario())));
                    target.map(|p| self.write_png_to(&p, &image)).unwrap_or(false)
                })
                .unwrap_or(false);
            if shot_ok || self.frames > AUDIT_TIMEOUT_FRAMES {
                self.finish_audit(json, code, shot_ok);
            } else {
                self.pending_review = Some((json, code));
                ctx.request_repaint();
            }
            return;
        }
        // Stage 1: poll the pipeline; when review is enabled, arm the
        // screenshot request instead of writing immediately.
        if let Some((json, code)) = self.inner.take_audit_emission() {
            if std::env::var_os("DB_PRO_REVIEW_DIR").is_some_and(|v| !v.is_empty()) {
                self.pending_review = Some((json, code));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                ctx.request_repaint();
                return;
            }
            let code = self.inner.write_audit_report(&json, code);
            self.audit_done = true;
            tracing::info!(code, "audit: report written");
            std::process::exit(code);
        }
        // Nothing emitted yet → the pipeline isn't warm; keep repainting so
        // egui doesn't idle, bail with a tool error on timeout.
        if self.frames >= AUDIT_TIMEOUT_FRAMES {
            tracing::error!(frames = self.frames, "audit: report never became ready");
            std::process::exit(2);
        }
        ctx.request_repaint();
    }

    /// Attach `visual_review` (bundle written, provider consulted if
    /// configured), write the report, exit with the AUDIT code — the review
    /// is advisory and can never change the verdict.
    #[cfg(debug_assertions)]
    fn finish_audit(&mut self, mut json: serde_json::Value, code: i32, shot_ok: bool) -> ! {
        if let Some((section, _, _)) = self.inner.run_visual_review(&json, shot_ok) {
            json["visual_review"] = section;
        }
        let code = self.inner.write_audit_report(&json, code);
        self.audit_done = true;
        tracing::info!(code, shot_ok, "audit: report written with review section");
        std::process::exit(code);
    }

    fn capture_screenshot(&self, ctx: &egui::Context) -> bool {
        if let Some(image) = take_screenshot(ctx) {
            self.write_png(&image);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            true
        } else {
            false
        }
    }

    fn advance_capture(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        if self.frames >= self.settle {
            let retry_due = self
                .requested_at
                .is_none_or(|at| self.frames - at >= SCREENSHOT_RETRY_FRAMES);
            if retry_due {
                self.requested_at = Some(self.frames);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            if self.frames - self.settle > CAPTURE_TIMEOUT_FRAMES {
                tracing::error!(
                    frames = self.frames,
                    "capture: screenshot reply never arrived; aborting"
                );
                std::process::exit(1);
            }
        }
        // The app repaints on demand, so without this the settle count would stall
        // on an idle frame and the screenshot would never be requested.
        ctx.request_repaint();
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod capture_save_tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStorage(HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    fn capture_app() -> CaptureApp {
        CaptureApp {
            inner: DbProApp::default(),
            path: PathBuf::new(),
            settle: 0,
            frames: 0,
            requested_at: None,
            opened_dialog: false,
            prepared_loading: false,
            pinned: None,
            audit: false,
            audit_done: false,
            #[cfg(debug_assertions)]
            pending_review: None,
        }
    }

    #[test]
    fn capture_app_save_forwards_theme_and_query_history() {
        let mut app = capture_app();
        let mut storage = MemoryStorage::default();
        eframe::App::save(&mut app, &mut storage);
        assert!(
            storage.0.contains_key("dbpro.native.dark-mode"),
            "CaptureApp::save must forward to DbProApp so capture Close persists theme"
        );
        assert!(
            storage.0.contains_key("dbpro.native.query-history-v1"),
            "CaptureApp::save must forward query history"
        );
    }
}
