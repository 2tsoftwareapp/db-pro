//! Tests for `../tasks_view.rs` (moved out of the source file).
    use super::*;
    use crate::{DbProApp, TaskBridge, UiCell, UiColumn, UiCommand, UiConnectionSummary, UiEvent, UiQueryResult, UiSslMode};
    use db_pro_core::domain::monitoring::MaintenanceAction;
    use db_pro_core::domain::saved_task::{
        SavedTask, SavedTaskPayload, SavedTaskRunStatus, SavedTaskRunTrigger, SavedTaskStore,
    };

    fn postgres_app() -> (DbProApp, std::sync::mpsc::Receiver<UiCommand>, std::sync::mpsc::SyncSender<UiEvent>) {
        let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
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
        *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
        app.connection.lifecycle.set_connected(true);
        app.preferences.settings.general.confirm_destructive_queries = false;
        (app, command_rx, event_tx)
    }

    fn task(id: uuid::Uuid, payload: SavedTaskPayload) -> SavedTask {
        let now = chrono::Utc::now();
        SavedTask {
            id,
            name: "task".into(),
            description: String::new(),
            connection_id: "conn-1".into(),
            payload,
            created_at: now,
            updated_at: now,
            last_run: None,
            schedule: None,
        }
    }

    #[test]
    fn export_writes_a_file_and_maintenance_targets_the_table_when_runtime_finishes() {
        let (mut app, command_rx, event_tx) = postgres_app();
        let export_id = uuid::Uuid::new_v4();
        let maintenance_id = uuid::Uuid::new_v4();
        let mut store = SavedTaskStore::new();
        store
            .upsert(task(
                export_id,
                SavedTaskPayload::Export {
                    table: Some("public.orders".into()),
                    format: "csv".into(),
                },
            ))
            .unwrap();
        store
            .upsert(task(
                maintenance_id,
                SavedTaskPayload::Maintenance {
                    operation: "vacuum".into(),
                    target: Some("public.orders".into()),
                },
            ))
            .unwrap();
        app.saved_tasks.store = store;
        let path = task_export_path(export_id, "csv");
        let _ = std::fs::remove_file(&path);

        app.run_saved_task(export_id, SavedTaskRunTrigger::Manual);
        assert!(app.saved_tasks.store.runs.is_empty());
        assert!(app.saved_tasks.store.tasks.iter().all(|task| task.last_run.is_none()));
        assert!(!std::path::Path::new(&path).exists());
        assert!(!app.feedback.runtime_message.contains("Exported"), "{}", app.feedback.runtime_message);

        let UiCommand::RunQuery { request_id, sql, .. } = command_rx.try_recv().expect("export query") else {
            panic!("export did not read the table");
        };
        assert!(sql.contains("orders"), "{sql}");
        assert!(!sql.to_ascii_lowercase().contains("vacuum"), "{sql}");

        event_tx
            .send(UiEvent::QueryCompleted {
                request_id,
                result: UiQueryResult {
                    columns: vec![UiColumn {
                        name: "name".into(),
                        data_type: "text".into(),
                        nullable: true,
                    }],
                    rows: vec![vec![UiCell::Text("Alice".into())]],
                    row_count: 1,
                    duration_ms: 1,
                },
            })
            .unwrap();
        app.apply_runtime_events();

        let body = std::fs::read_to_string(&path).expect("export file");
        assert!(body.contains("name"), "{body}");
        assert!(body.contains("Alice"), "{body}");
        let export_run = app
            .saved_tasks
            .store
            .tasks
            .iter()
            .find(|task| task.id == export_id)
            .and_then(|task| task.last_run.clone())
            .expect("export run");
        assert_eq!(export_run.status, SavedTaskRunStatus::Success);
        assert!(export_run.message.contains(&path), "{}", export_run.message);
        let _ = std::fs::remove_file(&path);

        app.run_saved_task(maintenance_id, SavedTaskRunTrigger::Manual);
        assert!(app
            .saved_tasks
            .store
            .tasks
            .iter()
            .find(|task| task.id == maintenance_id)
            .unwrap()
            .last_run
            .is_none());
        let UiCommand::MonitoringMaintenance {
            request_id,
            schema,
            table,
            action,
            confirmed,
            ..
        } = command_rx.try_recv().expect("maintenance command")
        else {
            panic!("maintenance was not sent to the monitor");
        };
        assert_eq!(schema.as_deref(), Some("public"));
        assert_eq!(table.as_deref(), Some("orders"));
        assert_eq!(action, MaintenanceAction::Vacuum);
        assert!(confirmed);
        assert!(!matches!(command_rx.try_recv(), Ok(UiCommand::RunQuery { .. })));

        event_tx
            .send(UiEvent::MonitoringActionCompleted {
                request_id,
                action: "maintenance".into(),
                backend_id: 0,
                succeeded: true,
            })
            .unwrap();
        app.apply_runtime_events();

        let maintenance_run = app
            .saved_tasks
            .store
            .tasks
            .iter()
            .find(|task| task.id == maintenance_id)
            .and_then(|task| task.last_run.clone())
            .expect("maintenance run");
        assert_eq!(maintenance_run.status, SavedTaskRunStatus::Success);
        assert!(maintenance_run.message.contains("Maintenance finished"), "{}", maintenance_run.message);
        assert!(!maintenance_run.message.contains("1000"), "{}", maintenance_run.message);
        assert!(!maintenance_run.message.to_ascii_lowercase().contains("select"), "{}", maintenance_run.message);
    }