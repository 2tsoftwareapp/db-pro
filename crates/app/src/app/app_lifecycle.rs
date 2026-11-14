use super::*;

impl eframe::App for DbProApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Match panel chrome so any sub-pixel seam between SidePanel and
        // CentralPanel cannot flash as a white strip (surface_app).
        self.theme.surface_panel.to_normalized_gamma_f32()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.persist_layout_and_connection_state(storage);
        self.persist_query_and_schema_state(storage);
        self.persist_workspace_state(storage);
        self.persist_ui_preferences(storage);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        db_pro_ui::components::interact::update_focus_visible(ctx);
        self.normalize_input_modifiers(ctx);
        self.prepare_frame(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        clear_stale_cursor(ui.ctx());
        self.draw_shell(ui);
        self.draw_overlays(ui.ctx());
        if cfg!(target_os = "linux") {
            shell_topbar_view::draw_window_resize_handles(ui.ctx());
        }
    }
}
/// Clears the previous pass's cursor before any widget draws.
///
/// egui keeps `PlatformOutput::cursor_icon` sticky between frames, so a hover
/// cursor would outlive the pointer leaving its widget. The clear must happen
/// *first*: widgets set their own cursor while they are drawn, and a reset
/// afterwards overwrites every one of them — which is exactly the bug that made
/// cursors feel wrong across the whole app.
pub(crate) fn clear_stale_cursor(ctx: &egui::Context) {
    ctx.set_cursor_icon(egui::CursorIcon::Default);
}

impl DbProApp {
    fn persist_layout_and_connection_state(&mut self, storage: &mut dyn eframe::Storage) {
        let scope = TableDataState::layout_scope(
            self.connection.lifecycle.active_connection_id(),
            self.active_schema(),
            self.schema.explorer.selected_table.as_deref(),
        );
        self.table.data.persist_layout(scope);
        self.sync_settings_from_runtime();
        if let Ok(settings) = serde_json::to_string(&self.preferences.settings) {
            storage.set_string(SETTINGS_STORAGE_KEY, settings);
        }
        self.persist_saved_tasks(storage);
        self.persist_workspace_sessions(storage);
        if let Ok(raw) = serde_json::to_string(self.connection.dialog.ssh_profiles()) {
            storage.set_string("dbpro.native.ssh-profiles-v1", raw);
        }
        if let Ok(layouts) = serde_json::to_string(&self.table.data.grid_layout_preferences) {
            storage.set_string("dbpro.native.grid-layouts", layouts);
        }
        if let Ok(widths) = serde_json::to_string(&self.table.data.grid_column_widths) {
            storage.set_string("dbpro.native.grid-widths", widths);
        }
        storage.set_string(
            "dbpro.native.grid-widths-customized",
            self.table.data.grid_columns_user_resized.to_string(),
        );
    }

    fn persist_query_and_schema_state(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(documents) = serde_json::to_string(&self.query.session.documents) {
            storage.set_string("dbpro.native.query-documents", documents);
        }
        self.persist_query_history(storage);
        if let Ok(pinned) = serde_json::to_string(&self.schema.explorer.pinned_tables) {
            storage.set_string("dbpro.native.pinned-tables-v1", pinned);
        }
        if let Ok(recent) = serde_json::to_string(&self.schema.explorer.recent_tables) {
            storage.set_string("dbpro.native.recent-tables-v1", recent);
        }
        if let Ok(layouts) = serde_json::to_string(&self.schema.diagram.saved_layouts) {
            storage.set_string(crate::diagram::ER_LAYOUTS_STORAGE_KEY, layouts);
        }
    }

    /// Writes query history to the workspace store.
    ///
    /// Statements such as `CREATE ROLE … PASSWORD '…'` are kept verbatim in the
    /// session (a DB IDE replays what the user typed) but would otherwise be
    /// written to disk in clear text. With the security setting on — the
    /// default — the credential literals are masked in the persisted copy only,
    /// so a secret never reaches the storage file.
    fn persist_query_history(&self, storage: &mut dyn eframe::Storage) {
        let mut entries = self.query.editor.query_history_entries.clone();
        if self.preferences.settings.security.redact_secrets_in_logs {
            for entry in &mut entries {
                entry.sql = db_pro_core::domain::audit::redact_secrets(&entry.sql).0;
            }
        }
        if let Ok(history) = serde_json::to_string(&entries) {
            storage.set_string("dbpro.native.query-history-v1", history);
        }
    }

    fn persist_workspace_state(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(recent_ws) = serde_json::to_string(
            &self
                .workspace
                .files
                .ide_workspace
                .recent_roots
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        ) {
            storage.set_string("dbpro.native.workspace-recent-v1", recent_ws);
        }
        if let Ok(roots) = serde_json::to_string(
            &self
                .workspace
                .files
                .ide_workspace
                .roots
                .iter()
                .map(|root| root.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        ) {
            storage.set_string("dbpro.native.workspace-roots-v1", roots);
        }
        storage.set_string(
            "dbpro.native.workspace-trusted-v1",
            self.workspace.files.ide_workspace.is_trusted().to_string(),
        );
    }

    fn persist_ui_preferences(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            "dbpro.native.theme-version",
            app_storage::THEME_STORAGE_VERSION.to_owned(),
        );
        storage.set_string("dbpro.native.dark-mode", self.preferences.dark_mode.to_string());
        storage.set_string("dbpro.native.reduce-motion", self.preferences.reduce_motion.to_string());
        if let Ok(prediction_mode) = serde_json::to_string(&self.preferences.prediction_mode) {
            storage.set_string("dbpro.native.prediction-mode", prediction_mode);
        }
        storage.set_string("dbpro.native.sidebar-width", self.workspace.sidebar_width.to_string());
        storage.set_string("dbpro.native.agent-width", self.workspace.agent_width.to_string());
        storage.set_string("dbpro.native.output-open", self.workspace.bottom_panel_open.to_string());
        storage.set_string(
            "dbpro.native.output-height",
            self.workspace.bottom_panel_height.to_string(),
        );
    }

    fn normalize_input_modifiers(&mut self, ctx: &egui::Context) {
        // Map Ctrl to Command in input events so Ctrl+A/C/V/X/Z work seamlessly on macOS
        ctx.input_mut(|i| {
            if i.modifiers.ctrl {
                i.modifiers.command = true;
            }
            for event in &mut i.events {
                if let egui::Event::Key { modifiers, .. } = event {
                    if modifiers.ctrl {
                        modifiers.command = true;
                    }
                }
            }
        });
    }

    fn prepare_frame(&mut self, ctx: &egui::Context) {
        // #573: the window is no longer re-maximized on the first frames. eframe
        // restores the persisted frame (size *and* position); a viewport command
        // issued here overrode it, so a resized window came back maximized.
        self.initial_frames_count = self.initial_frames_count.saturating_add(1);
        self.request_connections_once();
        let runtime_events_pending = self.apply_runtime_events();
        if self.workspace.activity == Activity::Monitor
            && self.connection.lifecycle.is_connected()
            && self.connection.lifecycle.active_connection_id().is_some()
            && self.management.monitoring.monitoring_poll
        {
            ctx.request_repaint_after(crate::app::monitoring_state::monitoring_poll_repaint_delay(
                self.management.monitoring.monitoring_last_poll,
                std::time::Instant::now(),
            ));
        }
        self.tick_saved_task_scheduler();
        // Worker-thread results (git ops, workspace scans, file reads,
        // transfer harness) land here so no draw path blocks on I/O (#391,
        // #393, #377, #421).
        self.poll_background_tasks();
        // #545: the 50 ms cadence is for in-flight work only. Previously any
        // enabled schedule pinned the shell to ~20 wakeups/s even when the next
        // run was an hour away; now an idle scheduler sleeps to its real next
        // deadline (capped so a wall-clock jump cannot strand a slot for long).
        let work_in_flight = runtime_events_pending
            || self.runtime_work_pending()
            || self.workspace.files.background_tasks_in_flight()
            || !self.management.transfer.pending_transfers.is_empty()
            || self.saved_tasks.has_inflight_runs();
        if work_in_flight {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if let Some(delay) = saved_task_state::scheduler_repaint_delay(
            &self.saved_tasks.enabled_schedule_deadlines(),
            chrono::Utc::now(),
        ) {
            ctx.request_repaint_after(delay);
        }
        // Monitoring auto-refresh is draw-driven; egui never wakes on its own,
        // so schedule the repaint that lets request_monitoring_poll_if_due
        // fire while the user is idle (#489). Gated on the Monitor activity —
        // when it is hidden last_poll goes stale and the remaining time would
        // clamp to the floor, repainting at ~20fps for nothing.
        if self.workspace.activity == Activity::Monitor
            && self.management.monitoring.monitoring_poll
            && self.connection.lifecycle.is_connected()
            && self.connection.lifecycle.active_connection_id().is_some()
        {
            let remaining = monitoring_activity_view::monitoring_poll_remaining(
                self.management.monitoring.monitoring_last_poll,
                std::time::Instant::now(),
            );
            ctx.request_repaint_after(remaining.max(Duration::from_millis(50)));
        }
        self.theme = if self.preferences.dark_mode {
            DbProTheme::dark()
        } else {
            DbProTheme::light()
        };
        self.theme.reduce_motion = self.preferences.reduce_motion;
        self.theme.apply(ctx);
        self.handle_shortcuts(ctx);
    }

    fn draw_shell(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // eframe creates the root Ui before App::logic updates the global style.
        // Refresh its snapshot so panels inherit this frame's selected theme.
        ui.set_style(ctx.global_style());
        let settings_mode = self.workspace.activity == Activity::Settings;
        if !settings_mode || cfg!(target_os = "linux") {
            self.draw_topbar(ui);
        }
        // Query owns its rich output dock and the Results workspace tab renders
        // the grid itself; the shell panel is for the remaining tabs.
        if !settings_mode && self.shows_shell_output_panel() {
            self.draw_output_panel(ui);
        }
        if !settings_mode {
            self.draw_statusbar(ui);
        }
        let activity_context = activity_bar_view::ActivityBarContext {
            theme: self.theme,
            activity: self.workspace.activity,
            active_tab: self.workspace.active_tab,
            agent_open: self.workspace.agent_open,
        };
        if let Some(action) = activity_bar_view::draw_activity_bar(ui, &activity_context) {
            self.apply_activity_bar_action(action, &ctx);
        }

        if self.workspace.sidebar_open && !settings_mode {
            self.draw_sidebar(ui);
        }

        if self.workspace.agent_open {
            self.draw_agent_panel(ui);
        }

        shell_frame_view::draw_central_panel(
            ui,
            &shell_frame_view::ShellFrameContext {
                theme: self.theme,
                settings_mode,
            },
            |ui| self.draw_workspace(ui),
        );
    }

    /// The shell-level output panel only draws on tabs that do not own their
    /// own output surface: Query has its rich dock and Results renders the
    /// grid itself.
    fn shows_shell_output_panel(&self) -> bool {
        !matches!(self.workspace.active_tab, WorkspaceTab::Query | WorkspaceTab::Results)
    }

    fn apply_activity_bar_action(&mut self, action: activity_bar_view::ActivityBarAction, ctx: &egui::Context) {
        use activity_bar_view::ActivityBarAction as Action;

        match action {
            Action::SelectActivity(activity) => {
                self.workspace.activity = activity;
                self.workspace.sidebar_open = true;
            }
            Action::OpenQuery => {
                self.workspace.activity = Activity::Queries;
                self.workspace.active_tab = WorkspaceTab::Query;
                self.workspace.sidebar_open = true;
            }
            Action::OpenDiagram => {
                self.workspace.activity = Activity::Diagram;
                self.open_diagram_tab();
                self.workspace.sidebar_open = true;
            }
            Action::OpenSchemaCompare => {
                self.workspace.activity = Activity::Compare;
                self.workspace.active_tab = WorkspaceTab::SchemaCompare;
                self.workspace.sidebar_open = true;
            }
            Action::OpenSchemaWorkbench => self.open_schema_workbench(),
            Action::OpenSettings => {
                self.workspace.activity = Activity::Settings;
                self.workspace.sidebar_open = false;
            }
            Action::ToggleAgent => self.set_agent_open(!self.workspace.agent_open, ctx),
        }
    }

    fn draw_overlays(&mut self, ctx: &egui::Context) {
        if self.connection.dialog.is_open() {
            let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
            connection::view::draw_connection_dialog(
                ctx,
                self.theme,
                &mut self.connection.dialog,
                &mut self.connection.lifecycle,
                &mut command_dispatcher,
                &mut self.feedback,
            );
        }
        if self.overlay.connection_delete_confirmation_id.is_some() {
            let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
            connection::delete_dialog::draw(
                ctx,
                self.theme,
                &mut self.overlay,
                &self.connection.catalog,
                &mut self.connection.lifecycle,
                &mut command_dispatcher,
                &mut self.feedback,
            );
        }
        if self.overlay.folder_delete_confirmation.is_some() {
            let mut command_dispatcher = command_dispatch::RuntimeCommandDispatcher::new(&mut self.task_bridge);
            query_folder_delete_dialog::draw(
                ctx,
                self.theme,
                &mut self.overlay,
                &self.query.library,
                &mut command_dispatcher,
                &mut self.feedback,
            );
        }
        if self.table.editing.insert_row_open {
            self.draw_insert_row_dialog(ctx);
        }
        self.draw_workbench_apply_confirmation(ctx);
        if self.query.execution.pending_destructive_run().is_some() {
            self.draw_destructive_run_dialog(ctx);
        }
        if self.query.session.pending_bulk_close.is_some() {
            self.draw_bulk_dirty_close_dialog(ctx);
        }
        if self.palette.mode.is_some() {
            self.draw_palette(ctx);
        }

        // Debug-build inspector; compiled out of release, no-op when closed.
        #[cfg(debug_assertions)]
        self.dev_tools.draw(ctx, self.theme);

        self.feedback.toasts.render_ctx(ctx, self.theme);
        if !self.feedback.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

/// Headless audit (`DB_PRO_AUDIT_JSON=<path>`): the same 12-rule pipeline the
/// inspector draws, emitted as schema-v1 JSON with an exit code. Debug builds
/// only — the dev-tools module doesn't exist in release.
#[cfg(debug_assertions)]
impl DbProApp {
    /// Take a pending audit emission produced inside `dev_tools.draw`.
    /// `None` until the pipeline is warm — callers poll per frame. The caller
    /// (capture driver) owns the write so it can attach `visual_review`
    /// first; `write_audit_report` is the shared write step.
    pub fn take_audit_emission(&mut self) -> Option<(serde_json::Value, i32)> {
        self.dev_tools.take_audit_emission()
    }

    /// Write a finalized audit JSON to `DB_PRO_AUDIT_JSON`; returns the
    /// verdict code to exit with (2 on write failure).
    pub fn write_audit_report(&self, json: &serde_json::Value, code: i32) -> i32 {
        let Some(out) = std::env::var_os(crate::dev_tools::report::OUTPUT_ENV) else {
            return crate::dev_tools::report::EXIT_TOOL_ERROR;
        };
        match std::fs::write(&out, serde_json::to_string_pretty(json).unwrap_or_default()) {
            Ok(()) => code,
            Err(e) => {
                tracing::error!(path = ?out, "audit report write failed: {e}");
                crate::dev_tools::report::EXIT_TOOL_ERROR
            }
        }
    }

    /// Scenario name used for artifact file naming (`DB_PRO_AUDIT_SCENARIO`).
    pub fn audit_scenario() -> String {
        std::env::var(crate::dev_tools::report::SCENARIO_ENV).unwrap_or_else(|_| "shell".to_owned())
    }

    /// AI visual review (P11): enabled iff `DB_PRO_REVIEW_DIR` is set.
    /// Builds the whitelisted bundle (no labels/text/credentials), runs the
    /// provider command if configured, and returns the `visual_review`
    /// section value plus the bundle path so the caller can drop the
    /// screenshot next to it. `None` when review is off — offline default.
    pub fn run_visual_review(
        &self,
        report: &serde_json::Value,
        screenshot_ok: bool,
    ) -> Option<(serde_json::Value, std::path::PathBuf, std::path::PathBuf)> {
        use crate::dev_tools::visual_review as vr;
        let dir = std::env::var_os(vr::REVIEW_DIR_ENV).map(std::path::PathBuf::from)?;
        if dir.as_os_str().is_empty() {
            return None;
        }
        let scenario = Self::audit_scenario();
        let bundle = vr::bundle_path(&dir, &scenario);
        let shot = vr::screenshot_path(&dir, &scenario);
        let shot_name = shot.file_name()?.to_string_lossy().into_owned();
        // Privacy gate: a live (non-fixture) session's screenshot may carry
        // real query results — blocked before any provider sees it. The
        // whitelisted bundle text is still written for manual review.
        let outcome = if !screenshot_ok {
            vr::ReviewOutcome::Failed("screenshot capture failed — no visual evidence".into())
        } else {
            let bundle_json = vr::build_bundle(report, &shot_name);
            match std::fs::write(&bundle, serde_json::to_string_pretty(&bundle_json).unwrap_or_default()) {
                Err(e) => vr::ReviewOutcome::Failed(format!("bundle write failed: {e}")),
                Ok(()) if !vr::screenshot_is_safe(self.has_live_connection(), self.fixture_provenance()) => {
                    vr::ReviewOutcome::Blocked(
                        "live connection without fixture provenance — screenshot withheld from provider".into(),
                    )
                }
                Ok(()) => vr::run_provider(&bundle),
            }
        };
        Some((outcome.to_json(&bundle.to_string_lossy(), &shot_name), bundle, shot))
    }
}

#[cfg(test)]
#[path = "tests/app_lifecycle_tests.rs"]
mod tests;
