//! Tests for `../explorer_connections.rs` (moved out of the source file).
    use super::*;
    use crate::UiSslMode;

    fn connection(id: &str, database: &str) -> UiConnectionSummary {
        UiConnectionSummary {
            id: id.to_owned(),
            name: id.to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: database.to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Require,
            readonly: false,
            create_if_missing: false,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        }
    }

    fn run_row_action(app: &mut DbProApp, action: ConnectionRowAction, connection: &UiConnectionSummary) {
        let ctx = egui::Context::default();
        let _ = crate::test_frame::frame(&ctx, Default::default(), |ui| {
            app.apply_connection_row_action(action, connection, false, ui);
        });
    }

    #[test]
    fn tree_query_uses_clicked_connection() {
        let (bridge, _command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut app = DbProApp::with_task_bridge(bridge);
        app.connection
            .lifecycle
            .set_active_connection_id(Some("active-conn".to_owned()));

        app.new_query_document();
        let baseline_docs = app.query.session.documents.len();
        assert_eq!(app.active_query_connection_id(), Some("active-conn"));

        let clicked = connection("clicked-conn", "clicked_db");
        run_row_action(&mut app, ConnectionRowAction::NewScript, &clicked);
        assert_eq!(app.active_query_connection_id(), Some("clicked-conn"));
        assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);

        let table = connection("table-conn", "table_db");
        run_row_action(&mut app, ConnectionRowAction::CreateTable, &table);
        assert_eq!(app.active_query_connection_id(), Some("table-conn"));
        assert!(app.query.session.active_text().contains("table_db"));

        // The sample DDL opens a dedicated document instead of overwriting
        // the one the user had open (#420).
        assert_eq!(app.query.session.documents.len(), baseline_docs + 2);
    }

    #[test]
    fn disconnect_closes_handle_and_reconnect_opens_a_new_connection() {
        let (bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut app = DbProApp::with_task_bridge(bridge);
        app.connection
            .lifecycle
            .set_active_connection_id(Some("clicked-conn".to_owned()));
        app.connection.lifecycle.set_connected(true);
        let clicked = connection("clicked-conn", "clicked_db");

        run_row_action(&mut app, ConnectionRowAction::Disconnect, &clicked);
        assert!(!app.connection.lifecycle.is_connected());
        assert!(matches!(
            command_rx.try_recv(),
            Ok(UiCommand::Disconnect { connection_id, .. }) if connection_id == "clicked-conn"
        ));
        assert!(command_rx.try_recv().is_err());

        app.connection.lifecycle.set_connected(true);
        run_row_action(&mut app, ConnectionRowAction::Reconnect, &clicked);
        assert!(matches!(
            command_rx.try_recv(),
            Ok(UiCommand::Disconnect { connection_id, .. }) if connection_id == "clicked-conn"
        ));
        assert!(matches!(
            command_rx.try_recv(),
            Ok(UiCommand::Connect { connection_id, .. }) if connection_id == "clicked-conn"
        ));
    }

    #[test]
    fn create_table_sample_sql_follows_the_driver() {
        let postgres = sample_create_table_sql("PostgreSQL", "app");
        assert!(postgres.contains("Sample SQL for PostgreSQL"));
        assert!(postgres.contains("id SERIAL PRIMARY KEY"));
        assert!(!postgres.contains("AUTO_INCREMENT"));
        assert!(!postgres.contains("IDENTITY"));

        let mysql = sample_create_table_sql("MySQL", "app");
        assert!(mysql.contains("AUTO_INCREMENT"));
        assert!(!mysql.contains("SERIAL"));

        let sqlite = sample_create_table_sql("SQLite", "app.db");
        assert!(sqlite.contains("INTEGER PRIMARY KEY"));
        assert!(!sqlite.contains("SERIAL"));

        let sql_server = sample_create_table_sql("SQL Server", "sales");
        assert!(sql_server.contains("IDENTITY(1,1)"));
        assert!(sql_server.contains("SYSUTCDATETIME()"));
        assert!(!sql_server.contains("SERIAL"));

        let (bridge, _command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut app = DbProApp::with_task_bridge(bridge);
        let mut server = connection("sqlserver-conn", "sales");
        server.driver = "SQL Server".to_owned();
        run_row_action(&mut app, ConnectionRowAction::CreateTable, &server);
        assert_eq!(app.active_query_connection_id(), Some("sqlserver-conn"));
        assert_eq!(app.query.session.active_text(), sql_server);

        let menu = include_str!("../explorer_connection_row_view.rs");
        assert!(menu.contains("Open sample CREATE TABLE SQL…"));
        assert!(!menu.contains("Create Table (DDL)"));
    }

    #[test]
    fn catalog_states_stay_separate_and_refresh_targets_the_selected_node() {
        use super::super::explorer_surface_view::{catalog_surface, CatalogSurface};
        use crate::RequestId;

        assert_eq!(catalog_surface(true, None, true), CatalogSurface::Loading);
        assert_eq!(catalog_surface(false, Some("catalog down"), true), CatalogSurface::Failed);
        assert_eq!(catalog_surface(false, None, true), CatalogSurface::Empty);
        assert_eq!(catalog_surface(true, Some("catalog down"), false), CatalogSurface::Ready);

        let (bridge, command_rx, _event_tx) = crate::TaskBridge::with_channels();
        let mut app = DbProApp::with_task_bridge(bridge);
        let request_id = RequestId(19);
        app.connection.lifecycle.note_connections_list_started(request_id);
        assert_eq!(
            catalog_surface(
                app.connection.lifecycle.connections_request_pending(),
                app.connection.lifecycle.connections_error(),
                true
            ),
            CatalogSurface::Loading
        );

        app.on_query_failed(request_id, "catalog down".to_owned(), None, None);
        assert!(!app.connection.lifecycle.connections_request_pending());
        assert_eq!(app.connection.lifecycle.connections_error(), Some("catalog down"));
        assert!(app.feedback.runtime_message.contains("Could not load connections"));

        app.on_connections_loaded(Vec::new());
        assert!(app.connection.lifecycle.connections_error().is_none());
        assert!(!app.connection.lifecycle.connections_request_pending());

        app.connection
            .lifecycle
            .set_active_connection_id(Some("conn-1".to_owned()));
        app.refresh_explorer_node("conn-1", Some("public"), Some("orders"));
        assert!(matches!(
            command_rx.try_recv(),
            Ok(UiCommand::LoadTableInfo {
                connection_id,
                schema,
                table,
                ..
            }) if connection_id == "conn-1" && schema == "public" && table == "orders"
        ));

        app.refresh_explorer_node("conn-1", Some("analytics"), None);
        assert!(matches!(
            command_rx.try_recv(),
            Ok(UiCommand::IntrospectSchema {
                connection_id,
                force_refresh: true,
                ..
            }) if connection_id == "conn-1"
        ));
        assert_eq!(app.schema.explorer.selected_schema.as_deref(), Some("analytics"));

        let tree = include_str!("../explorer_schema_tree_view.rs");
        assert!(tree.contains("schema: Some(schema.to_owned())"));
    }