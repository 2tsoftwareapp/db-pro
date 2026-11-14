use super::*;
use crate::app::table_mutation_state::TableApplyPlan;
use crate::{UiCell, UiColumn, UiQueryResult, UiSslMode, UiTableColumn, UiTableForeignKey, UiTableInfo};

fn column(name: &str, data_type: &str, nullable: bool) -> UiTableColumn {
    UiTableColumn {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        is_primary_key: name == "id",
        ..UiTableColumn::default()
    }
}

fn orders_info() -> UiTableInfo {
    UiTableInfo {
        schema: "public".to_owned(),
        name: "orders".to_owned(),
        row_count: Some(1),
        columns: vec![
            column("id", "int4", false),
            column("customer_id", "int4", true),
            column("note", "text", true),
        ],
        primary_key: Some(vec!["id".to_owned()]),
        indexes: Vec::new(),
        foreign_keys: vec![UiTableForeignKey {
            name: "orders_customer_id_fkey".to_owned(),
            from_columns: vec!["customer_id".to_owned()],
            to_schema: "public".to_owned(),
            to_table: "customers".to_owned(),
            to_columns: vec!["id".to_owned()],
            on_update: "NO ACTION".to_owned(),
            on_delete: "NO ACTION".to_owned(),
            match_option: "SIMPLE".to_owned(),
            deferrable: false,
            initially_deferred: false,
        }],
        check_constraints: Vec::new(),
        dependencies: Vec::new(),
    }
}

fn orders_result(customer_id: UiCell) -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            UiColumn {
                name: "id".to_owned(),
                data_type: "int4".to_owned(),
                nullable: false,
            },
            UiColumn {
                name: "customer_id".to_owned(),
                data_type: "int4".to_owned(),
                nullable: true,
            },
            UiColumn {
                name: "note".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![vec![
            UiCell::Number("10".to_owned()),
            customer_id,
            UiCell::Text("ship".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 1,
    }
}

fn customer_ids() -> UiQueryResult {
    UiQueryResult {
        columns: vec![UiColumn {
            name: "id".to_owned(),
            data_type: "int4".to_owned(),
            nullable: false,
        }],
        rows: vec![
            vec![UiCell::Number("1".to_owned())],
            vec![UiCell::Number("2".to_owned())],
            vec![UiCell::Number("3".to_owned())],
        ],
        row_count: 3,
        duration_ms: 1,
    }
}

fn orders_app() -> DbProApp {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp {
        task_bridge: bridge,
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![UiConnectionSummary {
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
                    tags: vec![],
                    group: None,
                    favorite: false,
                    environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
                }],
            },
            lifecycle: ConnectionLifecycleState {
                connected: true,
                active_connection_id: Some("conn-1".to_owned()),
                ..Default::default()
            },
            dialog: ConnectionDialogState::default(),
        },
        table: TableEditorState {
            state: TableState {
                table_info: Some(orders_info()),
                table_view: TableView::Data,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.workspace.active_tab = WorkspaceTab::Table;
    app.schema.explorer.selected_schema = Some("app".to_owned());
    app.schema.explorer.selected_table = Some("orders".to_owned());
    app
}

fn staged_customer_id(plan: &TableApplyPlan) -> Option<UiCell> {
    plan.changes.iter().find_map(|change| match change {
        UiTableMutation::Update { columns, values, .. } => columns
            .iter()
            .position(|column| column == "customer_id")
            .and_then(|index| values.get(index).cloned()),
        _ => None,
    })
}

/// An FK column stays editable, offers destination values, and opens that row.
#[test]
fn foreign_key_lookup_edits_and_opens_referenced_row() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = orders_app();
    app.task_bridge = bridge;
    let result = orders_result(UiCell::Number("2".to_owned()));

    assert_eq!(app.result_edit_reason(&result), None);
    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    assert_eq!(app.table.editing.data_editing_cell, Some((0, 1)));
    assert_eq!(app.table.editing.fk_lookup_column.as_deref(), Some("customer_id"));
    // Editing an FK column dispatches the destination key lookup…
    let lookup_request = app
        .table
        .editing
        .pending_fk_lookup_request
        .expect("fk lookup request pending");
    match command_rx.try_recv().expect("fk lookup sql dispatched") {
        UiCommand::RunQuery { request_id, sql, .. } => {
            assert_eq!(request_id, lookup_request);
            assert!(sql.contains(r#""customers""#), "{sql}");
        }
        command => panic!("expected RunQuery, got {command:?}"),
    }
    // …and until it returns, no select values are offered.
    assert!(app.table.editing.fk_lookup_options.is_empty());

    app.apply_foreign_key_lookup("customer_id", &customer_ids());
    assert_eq!(
        app.table.editing.fk_lookup_options,
        ["1".to_owned(), "2".to_owned(), "3".to_owned()]
    );

    app.table.editing.data_edit_value = "9".to_owned();
    assert!(
        app.submit_data_cell_edit(&result, 0, 1),
        "{}",
        app.feedback.runtime_message
    );
    let plan = app.table.mutation.build_apply_plan();
    assert_eq!(staged_customer_id(&plan), Some(UiCell::Number("9".to_owned())));

    let sql = app.foreign_key_lookup_sql("customer_id").expect("lookup sql");
    assert!(sql.contains(r#""customers""#), "{sql}");
    assert!(sql.contains(r#""id""#), "{sql}");
    assert!(app.foreign_key_lookup_sql("note").is_none());

    assert!(!app.open_referenced_row(&result, 0, 2));
    assert_eq!(app.feedback.runtime_message, "This column is not a foreign key");
    assert_eq!(app.schema.explorer.selected_table.as_deref(), Some("orders"));

    let missing = orders_result(UiCell::Null);
    assert!(!app.open_referenced_row(&missing, 0, 1));
    assert_eq!(app.feedback.runtime_message, "The referenced value is missing");
    assert_eq!(app.schema.explorer.selected_table.as_deref(), Some("orders"));

    app.workspace.active_tab = WorkspaceTab::Query;
    app.table.state.table_view = TableView::Structure;

    // Staged edits park the navigation behind the discard confirmation.
    assert!(!app.open_referenced_row(&result, 0, 1));
    assert_eq!(app.schema.explorer.selected_table.as_deref(), Some("orders"));
    assert!(app.table.editing.discard_changes_confirmation);

    // Discard the staged change, then the navigation proceeds.
    app.discard_staged_changes();
    app.table.editing.discard_changes_confirmation = false;
    app.workspace.pending_navigation_action = None;
    assert!(app.open_referenced_row(&result, 0, 1));
    assert_eq!(app.schema.explorer.selected_schema.as_deref(), Some("public"));
    assert_eq!(app.schema.explorer.selected_table.as_deref(), Some("customers"));
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Table);
    assert_eq!(app.table.state.table_view, TableView::Data);

    // Discard above also dispatches a mutation command — scan past it.
    let (connection_id, schema, table, limit, offset, filters) = loop {
        match command_rx.try_recv().expect("LoadTableData for customers") {
            UiCommand::LoadTableData {
                connection_id,
                schema,
                table,
                limit,
                offset,
                filters,
                ..
            } if table == "customers" => break (connection_id, schema, table, limit, offset, filters),
            _ => {}
        }
    };
    assert_eq!(connection_id, "conn-1");
    assert_eq!(schema, "public");
    assert_eq!(table, "customers");
    assert_eq!(limit, 1);
    assert_eq!(offset, 0);
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].column, "id");
    assert_eq!(filters[0].operator, UiTableFilterOperator::Equals);
    assert_eq!(filters[0].value, "2");
    assert_eq!(filters[0].data_type, "int4");
}

/// #496: a composite foreign key cannot offer a single-cell dropdown — the joined
/// "7, 42" label cannot be written back into one column — so it falls back to a
/// plain text edit. (`open_referenced_row` still navigates it via all key parts.)
#[test]
fn composite_foreign_key_gets_no_value_dropdown() {
    let mut app = orders_app();

    // 1 -> 2 targets: not 1:1, no dropdown.
    let info = app.table.state.table_info.as_mut().unwrap();
    info.foreign_keys[0].to_columns.push("tenant".to_owned());
    assert!(app.foreign_key_lookup_sql("customer_id").is_none());

    // 2 -> 1 sources: still not 1:1, and every member column stays a text edit.
    let info = app.table.state.table_info.as_mut().unwrap();
    info.foreign_keys[0].to_columns.pop();
    info.foreign_keys[0].from_columns.push("note".to_owned());
    assert!(app.foreign_key_lookup_sql("customer_id").is_none());
    assert!(app.foreign_key_lookup_sql("note").is_none());
}

/// #497: the dropdown lookup must never pull the whole referenced table.
#[test]
fn foreign_key_lookup_sql_is_bounded_and_ordered() {
    let app = orders_app();
    let sql = app.foreign_key_lookup_sql("customer_id").expect("lookup sql");
    assert!(sql.contains("LIMIT 500"), "{sql}");
    assert!(sql.contains(r#"ORDER BY "id""#), "{sql}");
    assert!(sql.starts_with(r#"SELECT "id" FROM "public"."customers""#), "{sql}");
}

/// #497: a lookup that lands exactly on the cap flags the dropdown as possibly
/// truncated instead of implying the list is complete.
#[test]
fn foreign_key_lookup_reports_truncated_options() {
    let mut app = orders_app();
    let mut many = customer_ids();
    many.rows = (0..table_fk_lookup::FK_LOOKUP_LIMIT)
        .map(|id| vec![UiCell::Number(id.to_string())])
        .collect();
    many.row_count = table_fk_lookup::FK_LOOKUP_LIMIT as u64;

    app.apply_foreign_key_lookup("customer_id", &many);

    assert_eq!(app.table.editing.fk_lookup_options.len(), table_fk_lookup::FK_LOOKUP_LIMIT);
    assert!(
        app.feedback.runtime_message.contains("500"),
        "{}",
        app.feedback.runtime_message
    );
}
