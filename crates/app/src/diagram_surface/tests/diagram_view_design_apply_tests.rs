//! Tests for `../diagram_view.rs` (moved out of the source file).
    use super::super::diagram_design_actions::{er_design_confirm_apply, er_design_request_apply};
    use super::{accept_layout_result, ensure_diagram_graph, DiagramAction, DiagramViewContext};
    use crate::runtime::UiSchemaColumn;
    use crate::{DbProApp, DbProTheme, TaskBridge, UiCommand, UiConnectionSummary, UiEvent, UiSslMode, UiTableSummary};
    use std::time::{Duration, Instant};

    fn connected_app() -> (
        DbProApp,
        std::sync::mpsc::Receiver<UiCommand>,
        std::sync::mpsc::SyncSender<UiEvent>,
    ) {
        let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
        let mut app = DbProApp::with_task_bridge(bridge);
        *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
            id: "active".to_owned(),
            name: "Active".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "active".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        }];
        *app.connection.lifecycle.active_connection_id_mut() = Some("active".to_owned());
        app.connection.lifecycle.set_connected(true);
        app.schema.explorer.schema.table_details.push(UiTableSummary {
            schema: "public".to_owned(),
            name: "customers".to_owned(),
            row_count: None,
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![],
        });
        (app, command_rx, event_tx)
    }

    fn view<'a>(app: &'a mut DbProApp) -> DiagramViewContext<'a> {
        DiagramViewContext {
            theme: DbProTheme::light(),
            diagram: &mut app.schema.diagram,
            explorer: &app.schema.explorer,
            active_driver: "PostgreSQL",
            connected: true,
            connection_id: Some("active".to_owned()),
        }
    }

    fn node_names(app: &DbProApp) -> Vec<String> {
        app.schema
            .diagram
            .graph
            .nodes
            .iter()
            .map(|node| node.table.name.clone())
            .collect()
    }

    fn wait_for_nodes(app: &mut DbProApp, predicate: impl Fn(&[String]) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            {
                let mut ctx = view(app);
                accept_layout_result(&mut ctx);
            }
            if predicate(&node_names(app)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn design_draft_shows_on_canvas_and_apply_runs_after_ddl_succeeds() {
        let (mut app, command_rx, event_tx) = connected_app();
        {
            let mut ctx = view(&mut app);
            ensure_diagram_graph(&mut ctx, (2, 120.0).into());
        }
        assert_eq!(node_names(&app), vec!["customers".to_owned()]);

        app.schema.diagram.design.enabled = true;
        app.schema.diagram.design.add_draft_table("public", "draft_orders");
        app.schema.diagram.design.add_draft_table("public", "draft_items");
        {
            let mut ctx = view(&mut app);
            ensure_diagram_graph(&mut ctx, (2, 120.0).into());
        }
        wait_for_nodes(&mut app, |names| {
            names.iter().any(|name| name == "draft_orders") && names.iter().any(|name| name == "draft_items")
        });
        let names = node_names(&app);
        assert!(names.iter().any(|name| name == "customers"), "{names:?}");
        assert!(names.iter().any(|name| name == "draft_orders"), "{names:?}");
        assert!(names.iter().any(|name| name == "draft_items"), "{names:?}");

        // The apply gate must hold the design until the dialog confirms —
        // the request only raises `apply_confirm` with the previewed SQL.
        {
            let mut ctx = view(&mut app);
            er_design_request_apply(&mut ctx);
        }
        assert!(app.schema.diagram.design.apply_confirm);
        assert!(command_rx.try_recv().is_err());
        let action = {
            let mut ctx = view(&mut app);
            er_design_confirm_apply(&mut ctx).expect("design apply")
        };
        assert_eq!(app.schema.diagram.design.draft.tables.len(), 2);
        let DiagramAction::ApplyDesign(sql) = action else {
            panic!("design apply must send the full script through DDL");
        };
        let statements = db_pro_core::domain::safety::split_statements(&sql);
        assert!(statements.len() >= 2, "{sql}");
        assert!(sql.contains("draft_orders"), "{sql}");
        assert!(sql.contains("draft_items"), "{sql}");

        app.apply_diagram_action(DiagramAction::ApplyDesign(sql));
        assert_eq!(app.schema.diagram.design.draft.tables.len(), 2);
        let UiCommand::ExecuteDdl { connection_id, sql, request_id } =
            command_rx.try_recv().expect("DDL command")
        else {
            panic!("expected ExecuteDdl, not the query runtime");
        };
        assert_eq!(connection_id, "active");
        assert!(db_pro_core::domain::safety::split_statements(&sql).len() >= 2, "{sql}");
        assert!(command_rx.try_recv().is_err());

        event_tx
            .send(UiEvent::QueryFailed {
                request_id,
                message: "syntax error".to_owned(),
            })
            .unwrap();
        app.apply_runtime_events();
        assert_eq!(app.schema.diagram.design.draft.tables.len(), 2);
        assert!(app
            .schema
            .diagram
            .design
            .error
            .as_deref()
            .is_some_and(|error| error.contains("syntax error")));

        {
            let mut ctx = view(&mut app);
            er_design_request_apply(&mut ctx);
        }
        assert!(app.schema.diagram.design.apply_confirm);
        let action = {
            let mut ctx = view(&mut app);
            er_design_confirm_apply(&mut ctx).expect("design apply after failure")
        };
        app.apply_diagram_action(action);
        assert_eq!(app.schema.diagram.design.draft.tables.len(), 2);
        let UiCommand::ExecuteDdl { request_id, sql, .. } = command_rx.try_recv().expect("second DDL command") else {
            panic!("expected ExecuteDdl");
        };
        assert!(sql.contains("draft_orders") && sql.contains("draft_items"), "{sql}");

        event_tx
            .send(UiEvent::DdlCompleted {
                request_id,
                affected_rows: 0,
            })
            .unwrap();
        app.apply_runtime_events();
        assert!(app.schema.diagram.design.draft.tables.is_empty());

        {
            let mut ctx = view(&mut app);
            ensure_diagram_graph(&mut ctx, (2, 120.0).into());
        }
        wait_for_nodes(&mut app, |names| !names.iter().any(|name| name == "draft_orders"));
        let names = node_names(&app);
        assert!(names.iter().any(|name| name == "customers"), "{names:?}");
        assert!(!names.iter().any(|name| name == "draft_orders"), "{names:?}");
        assert!(!names.iter().any(|name| name == "draft_items"), "{names:?}");
    }