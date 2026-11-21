//! Tests for `../monitoring_activity_view.rs` (moved out of the source file).
    use super::{
        maintenance_command, workload_command, MonitoringActivityContext, MonitoringState, RequestId, UiCommand,
    };

    /// Block until the workspace read/task workers publish their results (#579).
    fn drain_workspace_workers(
        files: &mut super::super::workspace_files_state::WorkspaceFilesState,
        feedback: &mut crate::app::feedback_state::FeedbackState,
    ) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while files.background_tasks_in_flight() && std::time::Instant::now() < deadline {
            files.poll_read_task(feedback);
            files.poll_task(feedback);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn workload_command_uses_state_sort_and_bounded_limit() {
        let state = MonitoringState {
            monitoring_stat_sort: db_pro_core::domain::monitoring::StatStatementSort::Calls,
            ..MonitoringState::default()
        };

        assert!(matches!(
            workload_command(&state, RequestId(7), "source".to_owned()),
            UiCommand::MonitoringStatStatements {
                request_id: RequestId(7),
                connection_id,
                sort: db_pro_core::domain::monitoring::StatStatementSort::Calls,
                limit: 100,
            } if connection_id == "source"
        ));
    }

    #[test]
    fn maintenance_command_is_explicitly_confirmed() {
        let action = db_pro_core::domain::monitoring::MaintenanceAction::Vacuum;

        assert!(matches!(
            maintenance_command(&MonitoringState::default(), RequestId(8), "source".to_owned(), action),
            UiCommand::MonitoringMaintenance {
                request_id: RequestId(8),
                connection_id,
                action: db_pro_core::domain::monitoring::MaintenanceAction::Vacuum,
                confirmed: true,
                ..
            } if connection_id == "source"
        ));
    }

    #[test]
    fn failed_maintenance_dispatch_preserves_confirmation() {
        let (mut bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        drop(command_rx);
        let mut dispatcher = crate::app::command_dispatch::RuntimeCommandDispatcher::new(&mut bridge);
        let mut state = MonitoringState {
            monitoring_maintenance_confirm: Some(db_pro_core::domain::monitoring::MaintenanceAction::Vacuum),
            ..MonitoringState::default()
        };
        let mut feedback = crate::app::FeedbackState::default();

        MonitoringActivityContext {
            theme: crate::DbProTheme::default(),
            state: &mut state,
            connected: true,
            driver: "PostgreSQL",
            connection_name: "source",
            connection_id: Some("conn-1"),
            command_dispatcher: &mut dispatcher,
            feedback: &mut feedback,
        }
        .apply_monitoring_confirmation_actions(vec![
            crate::app::monitoring_confirmation_view::MonitoringConfirmationAction::ConfirmMaintenance(
                db_pro_core::domain::monitoring::MaintenanceAction::Vacuum,
            ),
        ]);

        assert!(state.monitoring_maintenance_confirm.is_some());
    }

    #[test]
    fn snapshot_sort_vacuum_target_and_hub_name_drivers_without_sessions() {
        let (mut bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut dispatcher = crate::app::command_dispatch::RuntimeCommandDispatcher::new(&mut bridge);
        let mut state = MonitoringState {
            monitoring_stat_sort: db_pro_core::domain::monitoring::StatStatementSort::Calls,
            monitoring_maintenance_schema: "public".to_owned(),
            monitoring_maintenance_table: "orders".to_owned(),
            monitoring_maintenance_confirm: Some(db_pro_core::domain::monitoring::MaintenanceAction::Vacuum),
            ..MonitoringState::default()
        };
        let mut feedback = crate::app::FeedbackState::default();
        let mut activity = MonitoringActivityContext {
            theme: crate::DbProTheme::default(),
            state: &mut state,
            connected: true,
            driver: "PostgreSQL",
            connection_name: "source",
            connection_id: Some("conn-1"),
            command_dispatcher: &mut dispatcher,
            feedback: &mut feedback,
        };

        activity.request_monitoring_snapshot();
        match command_rx.try_recv() {
            Ok(UiCommand::MonitoringSnapshot { sort, connection_id, .. }) => {
                assert_eq!(connection_id, "conn-1");
                assert_eq!(sort, db_pro_core::domain::monitoring::StatStatementSort::Calls);
            }
            Ok(_) => panic!("snapshot did not use the selected sort"),
            Err(error) => panic!("snapshot sent no command: {error}"),
        }

        activity.apply_monitoring_confirmation_actions(vec![
            crate::app::monitoring_confirmation_view::MonitoringConfirmationAction::ConfirmMaintenance(
                db_pro_core::domain::monitoring::MaintenanceAction::Vacuum,
            ),
        ]);
        match command_rx.try_recv() {
            Ok(UiCommand::MonitoringMaintenance {
                schema,
                table,
                action,
                confirmed,
                ..
            }) => {
                assert_eq!(schema.as_deref(), Some("public"));
                assert_eq!(table.as_deref(), Some("orders"));
                assert_eq!(action, db_pro_core::domain::monitoring::MaintenanceAction::Vacuum);
                assert!(confirmed);
            }
            Ok(_) => panic!("vacuum did not target the chosen table"),
            Err(error) => panic!("vacuum sent no command: {error}"),
        }

        assert_eq!(
            super::super::tools_hub_surface_view::monitor_card_description("MySQL"),
            "MySQL has no sessions"
        );
        assert_eq!(
            super::super::tools_hub_surface_view::monitor_card_description("SQL Server"),
            "SQL Server has no sessions"
        );
        let hub = include_str!("../../tools/tools_hub_surface_view.rs");
        assert!(hub.contains("monitor_card_description(self.driver)"));
        let locks = include_str!("../monitoring_snapshot_view.rs");
        let error_at = locks.find("lock_error").expect("lock panel reads lock_error");
        let empty_at = locks.find("blocking.is_empty()").expect("empty locks still hide");
        assert!(error_at < empty_at);
    }

    #[test]
    fn cancel_delete_replace_and_shell_wait_for_confirm() {
        let (mut bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut dispatcher = crate::app::command_dispatch::RuntimeCommandDispatcher::new(&mut bridge);
        let mut state = MonitoringState::default();
        let mut feedback = crate::app::FeedbackState::default();
        {
            let mut activity = MonitoringActivityContext {
                theme: crate::DbProTheme::default(),
                state: &mut state,
                connected: true,
                driver: "PostgreSQL",
                connection_name: "source",
                connection_id: Some("conn-1"),
                command_dispatcher: &mut dispatcher,
                feedback: &mut feedback,
            };
            activity.apply_monitoring_sessions_action(
                super::super::monitoring_sessions_view::MonitoringSessionsAction::Cancel(
                    db_pro_core::domain::monitoring::MonitorSession {
                        backend_id: 42,
                        database: Some("db".into()),
                        username: Some("u".into()),
                        application_name: None,
                        client_addr: None,
                        state: Some("active".into()),
                        wait_event_type: None,
                        wait_event: None,
                        query_text: Some("SELECT 1".into()),
                        query_duration_ms: None,
                        backend_start: Some("2026-10-17 08:15:00.123456+00".into()),
                        xact_start: None,
                        query_start: None,
                        xact_age_ms: None,
                        backend_age_ms: None,
                        idle_in_transaction: false,
                        is_current: false,
                    },
                ),
                &mut Vec::new(),
            );
            assert_eq!(activity.state.monitoring_cancel_confirm.as_ref().map(|s| s.backend_id), Some(42));
            assert!(matches!(
                command_rx.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ));
            activity.apply_monitoring_confirmation_actions(vec![
                crate::app::monitoring_confirmation_view::MonitoringConfirmationAction::ConfirmCancelBackend {
                    backend_id: 42,
                    backend_start: Some("2026-10-17 08:15:00.123456+00".into()),
                },
            ]);
        }
        match command_rx.try_recv() {
            Ok(UiCommand::MonitoringCancelBackend {
                backend_id,
                expected_backend_start,
                ..
            }) => {
                assert_eq!(backend_id, 42);
                assert_eq!(expected_backend_start, "2026-10-17 08:15:00.123456+00");
            }
            Ok(_) => panic!("confirm did not cancel the backend"),
            Err(error) => panic!("cancel confirm sent no command: {error}"),
        }

        let task_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        let mut tasks = super::super::saved_task_state::SavedTaskState::default();
        tasks
            .store
            .upsert(db_pro_core::domain::saved_task::SavedTask {
                id: task_id,
                name: "nightly".into(),
                description: String::new(),
                connection_id: "conn-1".into(),
                payload: db_pro_core::domain::saved_task::SavedTaskPayload::Sql {
                    sql: "SELECT 1".into(),
                },
                created_at: now,
                updated_at: now,
                last_run: None,
                schedule: None,
            })
            .unwrap();
        tasks.request_delete(task_id);
        assert_eq!(tasks.store.tasks.len(), 1);
        assert_eq!(tasks.pending_delete_task_id, Some(task_id));
        assert!(tasks.confirm_delete());
        assert!(tasks.store.tasks.is_empty());

        let dir = std::env::temp_dir().join(format!(
            "dbpro-confirm-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        let sql_path = dir.join("migrations/001_init.sql");
        std::fs::write(&sql_path, "CREATE TABLE users (id INT);").unwrap();
        let mut files = super::super::workspace_files_state::WorkspaceFilesState::default();
        files.ide_workspace.open_root(dir.clone()).unwrap();
        files.ide_workspace.set_trusted(true);
        files.workspace_search_query = "users".into();
        files.workspace_replace_query = "accounts".into();
        files.request_replace_all(&mut feedback);
        assert!(files.pending_replace_all);
        assert!(std::fs::read_to_string(&sql_path).unwrap().contains("users"));
        files.confirm_replace_all(&mut feedback);
        // #579: the replace runs on a worker thread so the frame loop keeps
        // painting; drain it here instead of expecting a synchronous write.
        drain_workspace_workers(&mut files, &mut feedback);
        let replaced = std::fs::read_to_string(&sql_path).unwrap();
        assert!(replaced.contains("accounts"), "{replaced}");
        assert!(!files.pending_replace_all);

        files.workspace_task_command = "echo shell-confirmed".into();
        files.request_shell(&mut feedback);
        assert!(files.pending_shell);
        assert!(files.ide_workspace.last_task.is_none());
        files.confirm_shell(&mut feedback);
        drain_workspace_workers(&mut files, &mut feedback);
        let result = files.ide_workspace.last_task.as_ref().expect("confirmed shell ran");
        assert_eq!(result.command, "echo shell-confirmed");
        assert!(result.exit_code.is_some());
        assert!(feedback.runtime_message.contains("Task exit"));
        let _ = std::fs::remove_dir_all(dir);

        let tasks_view = include_str!("../../tools/tasks_view.rs");
        assert!(tasks_view.contains("self.saved_tasks.request_delete(task_id)"));
        let files_tab = include_str!("../../files/files_activity_tabs.rs");
        assert!(files_tab.contains("request_replace_all"));
        assert!(files_tab.contains("request_shell"));
        assert!(files_tab.contains("confirm_replace_all"));
        assert!(files_tab.contains("confirm_shell"));
    }

    #[test]
    fn terminate_confirm_carries_backend_start_into_command() {
        let (mut bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut dispatcher = crate::app::command_dispatch::RuntimeCommandDispatcher::new(&mut bridge);
        let mut state = MonitoringState::default();
        let mut feedback = crate::app::FeedbackState::default();
        {
            let mut activity = MonitoringActivityContext {
                theme: crate::DbProTheme::default(),
                state: &mut state,
                connected: true,
                driver: "PostgreSQL",
                connection_name: "source",
                connection_id: Some("conn-1"),
                command_dispatcher: &mut dispatcher,
                feedback: &mut feedback,
            };
            activity.apply_monitoring_confirmation_actions(vec![
                crate::app::monitoring_confirmation_view::MonitoringConfirmationAction::ConfirmTerminate {
                    backend_id: 77,
                    backend_start: Some("2026-10-17 08:15:00.5+00".into()),
                },
            ]);
        }
        match command_rx.try_recv() {
            Ok(UiCommand::MonitoringTerminateBackend {
                backend_id,
                expected_backend_start,
                ..
            }) => {
                assert_eq!(backend_id, 77);
                assert_eq!(expected_backend_start, "2026-10-17 08:15:00.5+00");
            }
            Ok(_) => panic!("confirm did not terminate the backend"),
            Err(error) => panic!("terminate confirm sent no command: {error}"),
        }
    }

    #[test]
    fn terminate_without_backend_start_refuses_to_signal_ambiguous_pid() {
        // A missing snapshot timestamp means the pid is unverifiable — the
        // action must be a no-op, never a blind terminate (#501).
        let (mut bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut dispatcher = crate::app::command_dispatch::RuntimeCommandDispatcher::new(&mut bridge);
        let mut state = MonitoringState::default();
        let mut feedback = crate::app::FeedbackState::default();
        {
            let mut activity = MonitoringActivityContext {
                theme: crate::DbProTheme::default(),
                state: &mut state,
                connected: true,
                driver: "PostgreSQL",
                connection_name: "source",
                connection_id: Some("conn-1"),
                command_dispatcher: &mut dispatcher,
                feedback: &mut feedback,
            };
            activity.apply_monitoring_confirmation_actions(vec![
                crate::app::monitoring_confirmation_view::MonitoringConfirmationAction::ConfirmTerminate {
                    backend_id: 77,
                    backend_start: None,
                },
            ]);
        }
        assert!(matches!(
            command_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(feedback.runtime_message.contains("session start time"));
    }

    #[test]
    fn monitoring_poll_remaining_drives_idle_repaint() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        // Never polled → due immediately.
        assert_eq!(super::monitoring_poll_remaining(None, now), Duration::ZERO);
        // Just polled → full interval remains.
        assert_eq!(
            super::monitoring_poll_remaining(Some(now), now),
            super::MONITORING_POLL_INTERVAL
        );
        // Mid-interval → the leftover is what prepare_frame must wait (#489).
        let last = now - Duration::from_secs(2);
        assert_eq!(
            super::monitoring_poll_remaining(Some(last), now),
            Duration::from_secs(3)
        );
        // Overdue (e.g. monitor was hidden) → due immediately, not negative.
        let stale = now - Duration::from_secs(60);
        assert_eq!(
            super::monitoring_poll_remaining(Some(stale), now),
            Duration::ZERO
        );
        // A future last_poll (clock skew) must saturate, not panic.
        let future = now + Duration::from_secs(60);
        assert_eq!(
            super::monitoring_poll_remaining(Some(future), now),
            super::MONITORING_POLL_INTERVAL
        );
        // prepare_frame must request the repaint — otherwise egui idles and
        // the poll never fires without user input.
        let lifecycle = include_str!("../../app/app_lifecycle.rs");
        assert!(
            lifecycle.contains("request_repaint_after(remaining.max"),
            "prepare_frame must schedule the monitoring poll repaint"
        );
        assert!(
            lifecycle.contains("monitoring_poll_remaining"),
            "prepare_frame must compute the remaining poll interval"
        );
    }