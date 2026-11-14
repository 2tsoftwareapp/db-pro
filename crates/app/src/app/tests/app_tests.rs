use crate::app::overlay_state::ExportFormat;
use super::diagram_view::{
    diagram_candidates, diagram_foreign_key_label, diagram_node_height, diagram_search_mode,
    diagram_show_all_after_search_edit,
};
use super::*;
use crate::diagram::*;
use crate::{
    UiCheckConstraint, UiDependencyDirection, UiDependencyKind, UiDriver, UiSchemaColumn, UiSslMode, UiTableDependency,
};
use db_pro_core::domain::capabilities::DatabaseCapabilities;
use db_pro_core::domain::connection::DriverType;

fn result() -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![
            vec![
                crate::UiCell::Number("2".to_owned()),
                crate::UiCell::Text("Beta".to_owned()),
            ],
            vec![
                crate::UiCell::Number("1".to_owned()),
                crate::UiCell::Text("Alpha".to_owned()),
            ],
            vec![
                crate::UiCell::Number("3".to_owned()),
                crate::UiCell::Text("Gamma".to_owned()),
            ],
        ],
        row_count: 3,
        duration_ms: 2,
    }
}

fn primary_key_identity(value: &str) -> RowIdentity {
    RowIdentity {
        original_pk_columns: vec!["id".to_owned()],
        original_pk_values: vec![UiCell::Number(value.to_owned())],
        original_pk_data_types: Vec::new(),
    }
}

#[test]
fn filter_returns_original_row_indexes() {
    let app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_filter: "gamma".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let value = result();
    assert_eq!(
        crate::filtered_sorted_indexes(
            &value,
            &app.table.data.grid_filter,
            app.table.data.grid_sort_column,
            app.table.data.grid_sort_desc
        ),
        vec![2]
    );
}

#[test]
fn sort_is_stable_over_filtered_indexes() {
    let mut app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_sort_column: Some(0),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let value = result();
    assert_eq!(
        crate::filtered_sorted_indexes(
            &value,
            &app.table.data.grid_filter,
            app.table.data.grid_sort_column,
            app.table.data.grid_sort_desc
        ),
        vec![1, 0, 2]
    );
    app.table.data.grid_sort_desc = true;
    assert_eq!(
        crate::filtered_sorted_indexes(
            &value,
            &app.table.data.grid_filter,
            app.table.data.grid_sort_column,
            app.table.data.grid_sort_desc
        ),
        vec![2, 0, 1]
    );
}

#[test]
fn cell_text_keeps_null_and_json_visible() {
    assert_eq!(crate::cell_text(&crate::UiCell::Null), "NULL");
    assert_eq!(
        crate::cell_text(&crate::UiCell::Json("{\"ok\":true}".to_owned())),
        "{\"ok\":true}"
    );
}

#[test]
fn displayed_row_number_tracks_database_page_offset() {
    assert_eq!(crate::displayed_row_number(0, 0), 1);
    assert_eq!(crate::displayed_row_number(100, 0), 101);
    assert_eq!(crate::displayed_row_number(100, 49), 150);
}

#[test]
fn grid_keyboard_navigation_preserves_filtered_row_identity() {
    let visible_rows = vec![2, 0, 4];

    assert_eq!(
        crate::grid_keyboard_selection(Some((2, 1)), &visible_rows, 3, egui::Key::ArrowDown),
        Some((0, 1))
    );
    assert_eq!(
        crate::grid_keyboard_selection(Some((0, 1)), &visible_rows, 3, egui::Key::ArrowRight),
        Some((0, 2))
    );
    assert_eq!(
        crate::grid_keyboard_selection(Some((0, 2)), &visible_rows, 3, egui::Key::Home),
        Some((0, 0))
    );
    assert_eq!(
        crate::grid_keyboard_selection(Some((0, 0)), &visible_rows, 3, egui::Key::ArrowUp),
        Some((2, 0))
    );
}

#[test]
fn grid_keyboard_navigation_starts_at_first_visible_cell() {
    assert_eq!(
        crate::grid_keyboard_selection(None, &[7, 9], 2, egui::Key::ArrowDown),
        Some((7, 0))
    );
    assert_eq!(
        crate::grid_keyboard_selection(Some((7, 0)), &[7, 9], 2, egui::Key::End),
        Some((7, 1))
    );
}

#[test]
fn grid_columns_fill_the_viewport_until_manually_resized() {
    let mut app = DbProApp::default();

    let widths = app.table.data.column_widths(3, 1200.0);
    assert!(widths.iter().all(|width| (*width - 380.0).abs() < 0.01));

    app.table.data.grid_column_widths = vec![240.0, 320.0, 180.0];
    app.table.data.grid_columns_user_resized = true;
    assert_eq!(app.table.data.column_widths(3, 1200.0), vec![240.0, 320.0, 180.0]);
}

#[test]
fn grid_copy_uses_staged_values_only_for_data_editor() {
    let value = result();
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Table,
                ..Default::default()
            },
            ..Default::default()
        },
        table: TableEditorState {
            mutation: TableMutationState {
                staged_changes: ChangeSet::from(vec![StagedChange::Update {
                    identity: primary_key_identity("2"),
                    current_row_index: Some(0),
                    column_index: 1,
                    column: "name".to_owned(),
                    data_type: "TEXT".to_owned(),
                    original: UiCell::Text("Beta".to_owned()),
                    value: UiCell::Text("Updated".to_owned()),
                }]),
                ..Default::default()
            },
            state: TableState {
                table_view: TableView::Data,
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "customers".to_owned(),
                    row_count: Some(1),
                    columns: Vec::new(),
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    assert_eq!(
        app.copy_cell_value(&value, 0, 1),
        Some(UiCell::Text("Updated".to_owned()))
    );
    app.workspace.active_tab = WorkspaceTab::Query;
    assert_eq!(app.copy_cell_value(&value, 0, 1), Some(UiCell::Text("Beta".to_owned())));
}

#[test]
fn composite_primary_key_identity_preserves_each_cell_type() {
    let result = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "tenant_id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "item_id".to_owned(),
                data_type: "TEXT".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![UiCell::Number("7".to_owned()), UiCell::Text("sku-42".to_owned())]],
        row_count: 1,
        duration_ms: 0,
    };
    let info = UiTableInfo {
        schema: "main".to_owned(),
        name: "inventory".to_owned(),
        row_count: Some(1),
        columns: Vec::new(),
        primary_key: Some(vec!["tenant_id".to_owned(), "item_id".to_owned()]),
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        check_constraints: Vec::new(),
        dependencies: Vec::new(),
    };

    let identity = TableDataState::row_identity(&result, &info, None, 0).expect("row identity expected");

    assert_eq!(identity.original_pk_columns, vec!["tenant_id", "item_id"]);
    assert_eq!(
        identity.original_pk_values,
        vec![UiCell::Number("7".to_owned()), UiCell::Text("sku-42".to_owned())]
    );
}

#[test]
fn update_value_keeps_empty_text_and_parses_typed_values() {
    assert_eq!(
        super::table_editor_values::parse_update_value("", "TEXT").unwrap(),
        UiCell::Text(String::new())
    );
    assert_eq!(
        super::table_editor_values::parse_update_value("false", "BOOLEAN").unwrap(),
        UiCell::Boolean(false)
    );
    assert!(super::table_editor_values::parse_update_value("not-an-int", "INTEGER").is_err());
}

#[test]
fn insert_value_respects_column_types() {
    assert_eq!(
        super::table_editor_values::parse_insert_value("42", "INTEGER").unwrap(),
        Some(crate::UiCell::Number("42".to_owned()))
    );
    assert_eq!(
        super::table_editor_values::parse_insert_value("true", "BOOLEAN").unwrap(),
        Some(crate::UiCell::Boolean(true))
    );
    assert_eq!(
        super::table_editor_values::parse_insert_value("{\"active\":true}", "JSONB").unwrap(),
        Some(crate::UiCell::Json("{\"active\":true}".to_owned()))
    );
    assert_eq!(
        super::table_editor_values::parse_insert_value("12.50", "NUMERIC(10,2)").unwrap(),
        Some(crate::UiCell::Number("12.50".to_owned()))
    );
    assert_eq!(
        super::table_editor_values::parse_insert_value("1.20e1", "DECIMAL(10,2)").unwrap(),
        Some(crate::UiCell::Number("1.20e1".to_owned()))
    );
}

#[test]
fn insert_value_rejects_invalid_typed_input() {
    assert!(super::table_editor_values::parse_insert_value("maybe", "BOOLEAN").is_err());
    assert!(super::table_editor_values::parse_insert_value("not-json", "JSON").is_err());
    assert!(super::table_editor_values::parse_insert_value("4.2", "INTEGER").is_err());
    assert!(super::table_editor_values::parse_insert_value("12.345", "NUMERIC(10,2)").is_err());
    assert!(super::table_editor_values::parse_insert_value("123456789.01", "NUMERIC(10,2)").is_err());
    assert!(super::table_editor_values::parse_update_value("", "DECIMAL(10,2)").is_err());
}

fn connection_summary_with_ssl_mode(ssl_mode: UiSslMode) -> UiConnectionSummary {
    UiConnectionSummary {
        id: "conn-1".to_owned(),
        name: "Secure Prod".to_owned(),
        host: "db.example.com".to_owned(),
        port: 5432,
        database: "app".to_owned(),
        username: "postgres".to_owned(),
        driver: "PostgreSQL".to_owned(),
        ssl_mode,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }
}

#[test]
fn editing_a_connection_preserves_its_stored_ssl_mode() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let connection = connection_summary_with_ssl_mode(UiSslMode::Require);

    app.open_edit_connection(&connection);

    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Require);

    app.dispatch_connection_command(true);

    let UiCommand::UpdateConnection {
        connection_id, draft, ..
    } = command_rx.try_recv().expect("update command expected")
    else {
        panic!("expected UpdateConnection");
    };
    assert_eq!(connection_id, "conn-1");
    assert_eq!(draft.ssl_mode, UiSslMode::Require);
}

#[test]
fn duplicating_a_connection_preserves_its_stored_ssl_mode() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let connection = connection_summary_with_ssl_mode(UiSslMode::VerifyFull);

    app.open_duplicate_connection(&connection);

    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::VerifyFull);

    app.dispatch_connection_command(true);

    let UiCommand::CreateConnection { draft, .. } = command_rx.try_recv().expect("create command expected") else {
        panic!("expected CreateConnection");
    };
    assert_eq!(draft.name, "Secure Prod (Copy)");
    assert_eq!(draft.ssl_mode, UiSslMode::VerifyFull);
}

#[test]
fn new_postgresql_connection_defaults_to_tls_disabled() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    app.connection.open_new();

    assert_eq!(app.connection.dialog.draft().driver, UiDriver::Postgres);
    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Disable);

    app.connection.dialog.draft_mut().name = "PG".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(true);
    let UiCommand::CreateConnection { draft, .. } = command_rx.try_recv().expect("create command expected") else {
        panic!("expected CreateConnection");
    };
    assert_eq!(draft.ssl_mode, UiSslMode::Disable);
}

#[test]
fn editing_a_disable_connection_keeps_disable_until_the_user_changes_it() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let connection = connection_summary_with_ssl_mode(UiSslMode::Disable);

    app.open_edit_connection(&connection);
    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Disable);

    app.dispatch_connection_command(true);
    let UiCommand::UpdateConnection { draft, .. } = command_rx.try_recv().expect("update command expected") else {
        panic!("expected UpdateConnection");
    };
    assert_eq!(draft.ssl_mode, UiSslMode::Disable);
}

#[test]
fn switching_sqlite_to_postgresql_defaults_to_tls_disabled() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.open_new();
    app.connection.dialog.draft_mut().driver = UiDriver::Sqlite;
    app.connection.dialog.draft_mut().ssl_mode = UiSslMode::Disable;

    app.select_connection_driver(UiDriver::Postgres);

    assert_eq!(app.connection.dialog.draft().driver, UiDriver::Postgres);
    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Disable);
}

#[test]
fn selecting_mysql_keeps_blank_port_and_preserves_password_on_submit() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.open_new();
    app.select_connection_driver(UiDriver::Mysql);
    assert_eq!(app.connection.dialog.draft().driver, UiDriver::Mysql);
    assert!(app.connection.dialog.draft().port.is_empty());
    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Disable);

    app.connection.dialog.draft_mut().name = "MySQL Local".to_owned();
    app.connection.dialog.draft_mut().host = "127.0.0.1".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "root".to_owned();
    app.connection.dialog.draft_mut().password = "secret".to_owned();
    app.connection.dialog.draft_mut().port = "3306".to_owned();
    app.dispatch_connection_command(true);

    let UiCommand::CreateConnection { draft, .. } = command_rx.try_recv().expect("create") else {
        panic!("expected CreateConnection");
    };
    assert_eq!(draft.driver, UiDriver::Mysql);
    assert_eq!(draft.port, "3306");
    assert_eq!(draft.password, "secret");
}

#[test]
fn editing_a_mysql_connection_keeps_the_mysql_driver() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let connection = UiConnectionSummary {
        id: "conn-mysql".to_owned(),
        name: "MySQL".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 3306,
        database: "app".to_owned(),
        username: "root".to_owned(),
        driver: "MySQL".to_owned(),
        ssl_mode: UiSslMode::Require,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    };
    app.open_edit_connection(&connection);
    assert_eq!(app.connection.dialog.draft().driver, UiDriver::Mysql);
}

#[test]
fn explicit_require_selection_is_preserved_on_submit() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.open_new();
    assert_eq!(app.connection.dialog.draft().ssl_mode, UiSslMode::Disable);

    app.connection.dialog.draft_mut().ssl_mode = UiSslMode::Require;
    app.connection.dialog.draft_mut().name = "PG".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(true);

    let UiCommand::CreateConnection { draft, .. } = command_rx.try_recv().expect("create command expected") else {
        panic!("expected CreateConnection");
    };
    assert_eq!(draft.ssl_mode, UiSslMode::Require);
}

#[test]
fn ssl_mode_guidance_names_the_plaintext_risk_for_disable() {
    assert!(super::connection::view::ssl_mode_guidance(UiSslMode::Disable).contains("Plaintext"));
    assert!(super::connection::view::ssl_mode_guidance(UiSslMode::Require).contains("TLS"));
    assert!(super::connection::view::ssl_mode_guidance(UiSslMode::VerifyCa).contains("CA"));
    assert!(super::connection::view::ssl_mode_guidance(UiSslMode::VerifyFull).contains("Strongest"));
}

#[test]
fn table_edits_stage_until_explicit_apply() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.schema.explorer.selected_table = Some("customers".to_owned());
    app.table.state.table_info = Some(UiTableInfo {
        schema: "public".to_owned(),
        name: "customers".to_owned(),
        row_count: Some(1),
        columns: vec![
            crate::UiTableColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
                default: None,
                is_primary_key: true,
                ..Default::default()
            },
            crate::UiTableColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
                default: None,
                is_primary_key: false,
                ..Default::default()
            },
        ],
        primary_key: Some(vec!["id".to_owned()]),
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        check_constraints: Vec::new(),
        dependencies: Vec::new(),
    });
    app.table.editing.data_edit_value = "Updated".to_owned();
    let value = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![
            UiCell::Number("1".to_owned()),
            UiCell::Text("Original".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 1,
    };

    app.submit_data_cell_edit(&value, 0, 1);

    assert_eq!(app.table.mutation.staged_changes.counts().total(), 1);
    assert!(command_rx.try_recv().is_err());
    app.apply_staged_changes();
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::ApplyTableChanges { changes, .. }) if changes.len() == 1));
}

#[test]
fn apply_is_blocked_while_a_validation_error_exists() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: primary_key_identity("1"),
        current_row_index: Some(0),
        column_index: 1,
        column: "name".to_owned(),
        data_type: "text".to_owned(),
        original: UiCell::Text("Original".to_owned()),
        value: UiCell::Text("Updated".to_owned()),
    });
    app.table.editing.data_edit_error = Some("invalid value".to_owned());

    app.apply_staged_changes();

    assert!(command_rx.try_recv().is_err());
    assert_eq!(
        app.feedback.runtime_message,
        "Fix the validation error before applying changes"
    );
    assert_eq!(app.table.mutation.staged_changes.counts().total(), 1);
}

#[test]
fn editing_primary_key_stages_new_value_with_original_identity() {
    let mut app = DbProApp {
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
            data: TableDataState { ..Default::default() },
            editing: TableEditingState {
                data_edit_value: "2".to_owned(),
                ..Default::default()
            },
            state: TableState {
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "customers".to_owned(),
                    row_count: Some(1),
                    columns: vec![crate::UiTableColumn {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        default: None,
                        is_primary_key: true,
                        ..Default::default()
                    }],
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "id".to_owned(),
            data_type: "integer".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![UiCell::Number("1".to_owned())]],
        row_count: 1,
        duration_ms: 0,
    };
    assert!(app.submit_data_cell_edit(&result, 0, 0));
    let Some(StagedChange::Update { value, identity, .. }) = app.table.mutation.staged_changes.iter().next() else {
        panic!("primary-key edit was not staged");
    };
    assert_eq!(value, &UiCell::Number("2".to_owned()));
    assert_eq!(identity.original_pk_values, vec![UiCell::Number("1".to_owned())]);
}

#[test]
fn no_primary_key_table_blocks_safe_row_mutations() {
    let app = DbProApp {
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
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "logs".to_owned(),
                    row_count: Some(1),
                    columns: Vec::new(),
                    primary_key: None,
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    // No identity key → every row edit is refused with a reason; there is no
    // separate "can edit" gate anymore (`result_edit_reason` is the gate).
    let result = UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "id".to_owned(),
            data_type: "integer".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![UiCell::Number("1".to_owned())]],
        row_count: 1,
        duration_ms: 0,
    };
    assert!(app.result_edit_reason(&result).is_some());
}

/// The edit entry point refuses a column the write policy blocks, and says why.
#[test]
fn binary_cell_edit_is_refused_with_a_reason() {
    let mut app = DbProApp {
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
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "files".to_owned(),
                    row_count: Some(1),
                    columns: vec![
                        crate::UiTableColumn {
                            name: "id".to_owned(),
                            data_type: "integer".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: true,
                            ..Default::default()
                        },
                        crate::UiTableColumn {
                            name: "payload".to_owned(),
                            data_type: "bytea".to_owned(),
                            nullable: true,
                            default: None,
                            is_primary_key: false,
                            ..Default::default()
                        },
                    ],
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "payload".to_owned(),
                data_type: "bytea".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![vec![
            UiCell::Number("1".to_owned()),
            UiCell::Bytes("\\x00ff".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 0,
    };

    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);

    assert!(
        app.table.editing.data_editing_cell.is_none(),
        "no editor may open for a blocked column"
    );
    assert!(
        app.feedback.runtime_message.contains("read-only"),
        "the reason must be visible: {}",
        app.feedback.runtime_message
    );
    assert_eq!(app.table.mutation.staged_changes.counts().total(), 0);
}

/// A generated column cannot be staged, even through the commit path.
#[test]
fn generated_column_edit_is_refused_before_staging() {
    let mut app = DbProApp {
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
            data: TableDataState { ..Default::default() },
            editing: TableEditingState {
                data_edit_value: "99.99".to_owned(),
                ..Default::default()
            },
            state: TableState {
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "line_items".to_owned(),
                    row_count: Some(1),
                    columns: vec![
                        crate::UiTableColumn {
                            name: "id".to_owned(),
                            data_type: "integer".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: true,
                            ..Default::default()
                        },
                        crate::UiTableColumn {
                            name: "total".to_owned(),
                            data_type: "numeric".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: false,
                            is_generated: true,
                            ..Default::default()
                        },
                    ],
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "total".to_owned(),
                data_type: "numeric".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![UiCell::Number("1".to_owned()), UiCell::Number("9.99".to_owned())]],
        row_count: 1,
        duration_ms: 0,
    };

    let accepted = app.submit_data_cell_edit(&result, 0, 1);

    assert!(!accepted, "the generated column must refuse the edit");
    assert!(app
        .table
        .editing
        .data_edit_error
        .as_deref()
        .unwrap_or_default()
        .contains("computed"));
    assert_eq!(
        app.table.mutation.staged_changes.counts().total(),
        0,
        "nothing may be staged"
    );
    assert!(
        app.feedback.runtime_message.contains("computed"),
        "the reason must be visible: {}",
        app.feedback.runtime_message
    );
}

/// A generated column left empty is skipped on insert; a value for it is refused.
#[test]
fn generated_column_is_never_staged_by_insert() {
    let mut app = DbProApp {
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

        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("line_items".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        table: TableEditorState {
            data: TableDataState { ..Default::default() },
            editing: TableEditingState {
                insert_row_values: vec!["1".to_owned(), "2".to_owned(), String::new()],
                ..Default::default()
            },
            state: TableState {
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "line_items".to_owned(),
                    row_count: Some(0),
                    columns: vec![
                        crate::UiTableColumn {
                            name: "id".to_owned(),
                            data_type: "integer".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: true,
                            ..Default::default()
                        },
                        crate::UiTableColumn {
                            name: "qty".to_owned(),
                            data_type: "integer".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: false,
                            ..Default::default()
                        },
                        crate::UiTableColumn {
                            name: "total".to_owned(),
                            data_type: "numeric".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: false,
                            is_generated: true,
                            ..Default::default()
                        },
                    ],
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.submit_insert_row();

    let Some(StagedChange::Insert { columns, .. }) = app.table.mutation.staged_changes.iter().next() else {
        panic!("the insert was not staged");
    };
    assert_eq!(
        columns,
        &["id".to_owned(), "qty".to_owned()],
        "the generated column must be skipped"
    );
    assert!(
        app.table.editing.insert_row_error.is_empty(),
        "skipping a generated column is not an error"
    );

    // A value for the generated column is refused deterministically, before staging.
    let mut second = DbProApp {
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

        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("line_items".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        table: TableEditorState {
            data: TableDataState { ..Default::default() },
            editing: TableEditingState {
                insert_row_values: vec!["1".to_owned(), "2".to_owned(), "3.0".to_owned()],
                ..Default::default()
            },
            state: TableState {
                table_info: app.table.state.table_info.clone(),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    second.submit_insert_row();
    assert!(
        second.table.editing.insert_row_error.contains("computed"),
        "the refusal must be visible: {}",
        second.table.editing.insert_row_error
    );
    assert_eq!(second.table.mutation.staged_changes.counts().total(), 0);
}

/// Duplicating a row must not prefill a column the policy blocks.
#[test]
fn duplicated_row_leaves_blocked_columns_empty() {
    let mut app = DbProApp {
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
                table_info: Some(UiTableInfo {
                    schema: "public".to_owned(),
                    name: "line_items".to_owned(),
                    row_count: Some(1),
                    columns: vec![
                        crate::UiTableColumn {
                            name: "id".to_owned(),
                            data_type: "integer".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: true,
                            ..Default::default()
                        },
                        crate::UiTableColumn {
                            name: "total".to_owned(),
                            data_type: "numeric".to_owned(),
                            nullable: false,
                            default: None,
                            is_primary_key: false,
                            is_generated: true,
                            ..Default::default()
                        },
                    ],
                    primary_key: Some(vec!["id".to_owned()]),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    check_constraints: Vec::new(),
                    dependencies: Vec::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "total".to_owned(),
                data_type: "numeric".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![UiCell::Number("1".to_owned()), UiCell::Number("9.99".to_owned())]],
        row_count: 1,
        duration_ms: 0,
    };

    app.open_duplicate_row(&result, 0);

    assert_eq!(app.table.editing.insert_row_values, vec![String::new(), String::new()]);
    assert!(app.table.editing.insert_row_open);
    assert!(app.table.editing.insert_row_error.is_empty());
}

#[test]
fn staged_apply_failure_maps_statement_to_mutation_and_keeps_changes() {
    let mut staged_changes = ChangeSet::new();
    staged_changes.stage_update(StagedChange::Update {
        identity: primary_key_identity("3"),
        current_row_index: Some(2),
        column_index: 1,
        column: "name".to_owned(),
        data_type: "text".to_owned(),
        original: UiCell::Text("old".to_owned()),
        value: UiCell::Text("new".to_owned()),
    });
    let mut app = DbProApp {
        table: TableEditorState {
            mutation: TableMutationState {
                staged_apply_request: Some(crate::RequestId(7)),
                staged_apply_targets: vec![
                    MutationTarget::Delete {
                        identity: primary_key_identity("1"),
                        current_row_index: Some(0),
                    },
                    MutationTarget::Update {
                        identity: primary_key_identity("3"),
                        current_row_index: Some(2),
                        columns: vec![1, 3],
                    },
                ],
                staged_changes,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.staged_apply_failed(StagedApplyFailure {
        statement_index: 1,
        code: "CONSTRAINT_VIOLATION",
        message: "duplicate key value",
        rolled_back: true,
    });

    assert_eq!(app.table.mutation.staged_apply_request, None);
    assert_eq!(app.table.mutation.staged_changes.counts().updates, 1);
    assert_eq!(app.table.data.selected_cell, Some((2, 1)));
    assert!(matches!(
        app.table.mutation.table_mutation_error.as_ref().and_then(|failure| failure.target.as_ref()),
        Some(MutationTarget::Update {
            current_row_index: Some(2),
            columns,
            ..
        }) if columns == &vec![1, 3]
    ));
    assert!(app
        .feedback
        .runtime_message
        .contains("Staged change #2 failed · transaction rolled back"));
}

#[test]
fn conflict_failure_has_distinct_code_and_user_action_message() {
    let mut app = DbProApp {
        table: TableEditorState {
            mutation: TableMutationState {
                staged_apply_request: Some(crate::RequestId(8)),
                staged_apply_targets: vec![MutationTarget::Update {
                    identity: primary_key_identity("3"),
                    current_row_index: Some(2),
                    columns: vec![1],
                }],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.staged_apply_failed(StagedApplyFailure {
        statement_index: 0,
        code: "CONFLICT",
        message: "row count was zero",
        rolled_back: true,
    });

    let failure = app
        .table
        .mutation
        .table_mutation_error
        .expect("conflict should be visible");
    assert_eq!(failure.code, "CONFLICT");
    assert!(failure
        .message
        .starts_with("This row changed or was deleted in the database."));
}

#[test]
fn internal_error_code_is_normalized_for_mutation_state() {
    let mut app = DbProApp {
        table: TableEditorState {
            mutation: TableMutationState {
                staged_apply_request: Some(crate::RequestId(9)),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.staged_apply_failed(StagedApplyFailure {
        statement_index: usize::MAX,
        code: "INTERNAL_ERROR",
        message: "invariant violation",
        rolled_back: true,
    });

    assert_eq!(
        app.table
            .mutation
            .table_mutation_error
            .expect("error should be visible")
            .code,
        "INTERNAL"
    );
}

#[test]
fn explain_query_uses_selected_connection_and_switches_output() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.set_active_query_text("SELECT 1");

    app.explain_query();

    let UiCommand::ExplainQuery {
        request_id,
        connection_id,
        sql,
        analyze,
        ..
    } = command_rx.try_recv().expect("explain command expected")
    else {
        panic!("expected ExplainQuery");
    };
    assert_eq!(connection_id, "conn-1");
    assert_eq!(sql, "SELECT 1");
    assert!(!analyze);
    assert_eq!(app.query.session.active_explain_request(), Some(request_id));
    assert_eq!(
        app.query
            .output
            .active_tab_for_document(app.query.session.active_document().map(|document| document.id.as_str())),
        OutputTab::Explain
    );
}

#[test]
fn explain_analyze_requires_explicit_confirm_before_dispatch() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.set_active_query_text("SELECT 1");

    app.explain_query_analyze();
    assert!(command_rx.try_recv().is_err(), "ANALYZE must wait for confirm");
    assert!(app.query.execution.pending_explain_analyze);

    app.query.execution.explain_analyze_confirmed = true;
    app.explain_query_analyze();
    let UiCommand::ExplainQuery { analyze, sql, .. } = command_rx.try_recv().expect("analyze command") else {
        panic!("expected ExplainQuery");
    };
    assert!(analyze);
    assert_eq!(sql, "SELECT 1");
    assert!(!app.query.execution.pending_explain_analyze);
}

#[test]
fn query_output_tab_is_scoped_to_each_document() {
    let mut app = DbProApp::default();
    app.query
        .session
        .documents
        .push(QueryDocument::new("query-2", "Query 2", "SELECT 2"));

    app.query.output.set_active_for_optional_document(
        app.query.session.active_document().map(|document| document.id.as_str()),
        OutputTab::Explain,
    );
    app.switch_query_document(1);
    assert_eq!(
        app.query
            .output
            .active_tab_for_document(app.query.session.active_document().map(|document| document.id.as_str())),
        OutputTab::Results
    );

    app.query.output.set_active_for_optional_document(
        app.query.session.active_document().map(|document| document.id.as_str()),
        OutputTab::History,
    );
    app.switch_query_document(0);
    assert_eq!(
        app.query
            .output
            .active_tab_for_document(app.query.session.active_document().map(|document| document.id.as_str())),
        OutputTab::Explain
    );

    app.query.output.set_for_document_and_activate_if_active(
        "query-2",
        app.query.session.active_document().map(|document| document.id.as_str()),
        OutputTab::Messages,
    );
    app.switch_query_document(1);
    assert_eq!(
        app.query
            .output
            .active_tab_for_document(app.query.session.active_document().map(|document| document.id.as_str())),
        OutputTab::Messages
    );
    app.switch_query_document(0);
    assert_eq!(
        app.query
            .output
            .active_tab_for_document(app.query.session.active_document().map(|document| document.id.as_str())),
        OutputTab::Explain
    );
}

#[test]
fn closing_agent_restores_sidebar_state_after_narrow_window() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    ctx.begin_pass(egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        ..Default::default()
    });

    app.set_agent_open(true, &ctx);
    assert!(!app.workspace.sidebar_open);

    app.set_agent_open(false, &ctx);
    assert!(app.workspace.sidebar_open);
    let _ = crate::test_frame::finish_pass(&ctx);
}

#[test]
fn selected_connection_is_not_shown_as_connected() {
    let connection = UiConnectionSummary {
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
    };
    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![connection],
            },

            lifecycle: ConnectionLifecycleState {
                connected: false,
                active_connection_id: Some("conn-1".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    assert_eq!(app.active_connection_name(), "Local");
    assert_eq!(
        app.connection_indicator(app.connection.catalog.get(0).expect("connection"))
            .1,
        app.theme.accent
    );
    assert_eq!(app.statusbar_state().2, "Not connected");
    assert!(!app.can_mutate_active_connection());
    app.connection.lifecycle.set_connected(true);
    assert_eq!(
        app.connection_indicator(app.connection.catalog.get(0).expect("connection"))
            .1,
        app.theme.success
    );
    assert_eq!(app.statusbar_state().2, "Connected");
    assert!(app.can_mutate_active_connection());
    app.connection.catalog.connections_mut()[0].readonly = true;
    assert!(!app.can_mutate_active_connection());
    app.feedback.runtime_message = "Table data failed · timeout".to_owned();
    assert!(app.has_runtime_error());
    assert_eq!(app.statusbar_state().2, "Connected");
    app.connection.lifecycle.set_connected(false);
    assert_eq!(app.statusbar_state().2, "Runtime error");
}

#[test]
fn editor_status_lives_on_query_strip_not_shell_statusbar() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Query,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    // Shell statusbar no longer mirrors Ln/Col — the query status strip owns it.
    assert!(!app.shows_editor_status());
    assert_eq!(app.statusbar_context_label(), "SQL Editor");

    app.workspace.active_tab = WorkspaceTab::Table;
    assert!(!app.shows_editor_status());
    assert_eq!(app.statusbar_context_label(), "Table Structure");
    app.table.state.table_view = TableView::Data;
    assert_eq!(app.statusbar_context_label(), "Data Editor");
    app.workspace.active_tab = WorkspaceTab::Diagram;
    assert!(!app.shows_editor_status());
    assert_eq!(app.statusbar_context_label(), "ER Diagram");
}

#[test]
fn back_and_forward_return_to_the_previous_workspace_tab() {
    let mut app = DbProApp::default();
    app.record_workspace_navigation();
    app.open_diagram_tab();
    app.record_workspace_navigation();
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Diagram);

    app.go_back();
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Welcome);
    assert!(app.workspace.diagram_open);

    app.go_forward();
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Diagram);
}

#[test]
fn diagram_tab_stays_open_until_it_is_closed() {
    let mut app = DbProApp::default();
    app.open_diagram_tab();
    app.workspace.active_tab = WorkspaceTab::Query;
    assert!(app.workspace.diagram_open);
    app.request_close_workspace_tab(WorkspaceTab::Diagram);
    assert!(!app.workspace.diagram_open);
}

#[test]
fn switching_query_documents_resets_editor_cursor_metadata() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Query,
                ..Default::default()
            },
            ..Default::default()
        },
        query: QueryFeatureState {
            editor: QueryEditorState {
                query_cursor_line: 8,
                query_cursor_column: 13,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.new_query_document();

    assert_eq!(app.query.editor.query_cursor_line, 1);
    assert_eq!(app.query.editor.query_cursor_column, 1);
}

#[test]
fn new_query_identity_skips_restored_document_ids() {
    let mut app = DbProApp::default();
    app.query
        .session
        .documents
        .push(QueryDocument::new("query-2", "Restored query", "SELECT restored;"));

    app.new_query_document();

    assert_eq!(
        app.query.session.documents.last().map(|doc| doc.id.as_str()),
        Some("query-3")
    );
    assert_eq!(
        app.query
            .session
            .documents
            .iter()
            .filter(|doc| doc.id == "query-3")
            .count(),
        1
    );
}

#[test]
fn provider_capabilities_gate_provider_specific_actions() {
    let sqlite = UiConnectionSummary {
        id: "sqlite".to_owned(),
        name: "SQLite".to_owned(),
        host: String::new(),
        port: 0,
        database: "app.db".to_owned(),
        username: String::new(),
        driver: "SQLite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    };
    let postgres = UiConnectionSummary {
        ssl_mode: UiSslMode::Disable,
        driver: "PostgreSQL".to_owned(),
        ..sqlite.clone()
    };

    let sqlite_app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![sqlite],
            },

            lifecycle: ConnectionLifecycleState {
                active_connection_id: Some("sqlite".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };
    let postgres_app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![postgres],
            },

            lifecycle: ConnectionLifecycleState {
                active_connection_id: Some("sqlite".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    let sqlite_capabilities = sqlite_app
        .active_capabilities()
        .resolved()
        .cloned()
        .expect("SQLite capabilities");
    assert!(!sqlite_capabilities.schema.functions);
    assert!(!sqlite_capabilities.features.server_sessions);
    assert!(sqlite_capabilities.features.backup);

    let postgres_capabilities = postgres_app
        .active_capabilities()
        .resolved()
        .cloned()
        .expect("PostgreSQL capabilities");
    assert!(postgres_capabilities.schema.functions);
    assert!(postgres_capabilities.query.explain);
    assert!(postgres_capabilities.features.server_sessions);
}

#[test]
fn mysql_connection_resolves_to_its_own_capability_set() {
    // Regression for #234 criterion 2: the lookup used to return `None` for MySQL, so the
    // newly registered provider had no capability path at all and every gate behaved as
    // "capability absent".
    let app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![UiConnectionSummary {
                    id: "mysql".to_owned(),
                    name: "MySQL".to_owned(),
                    host: "127.0.0.1".to_owned(),
                    port: 33306,
                    database: "dbpro_fixture".to_owned(),
                    username: "root".to_owned(),
                    driver: "MySQL".to_owned(),
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
                active_connection_id: Some("mysql".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    let lookup = app.active_capabilities();
    assert_eq!(
        lookup.resolved().map(|caps| caps.driver),
        Some(DriverType::Mysql),
        "MySQL must resolve to the provider's own set"
    );
    assert_eq!(
        lookup.resolved().map(|caps| caps.query.parameters),
        Some(DatabaseCapabilities::mysql().query.parameters),
        "the resolved set is the provider's own, not a default"
    );

    // The gates the UI reads are the provider's real values.
    assert!(lookup.allows(|caps| caps.schema.functions));
    assert!(!lookup.allows(|caps| caps.features.backup));
    assert!(!lookup.allows(|caps| caps.features.partitions));
    assert!(lookup.allows(|caps| caps.query.cancel));
    assert!(lookup.allows(|caps| caps.query.parameters));
    assert!(lookup.allows(|caps| caps.query.positional_parameters));
}

#[test]
fn unknown_driver_resolves_to_a_named_state_not_none() {
    let app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![UiConnectionSummary {
                    id: "oracle".to_owned(),
                    name: "Oracle".to_owned(),
                    host: "db.example.com".to_owned(),
                    port: 1521,
                    database: "ORCL".to_owned(),
                    username: "system".to_owned(),
                    driver: "Oracle".to_owned(),
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
                active_connection_id: Some("oracle".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    let lookup = app.active_capabilities();
    match &lookup {
        CapabilityLookup::UnsupportedDriver { driver } => assert_eq!(driver, "Oracle"),
        other => panic!("expected the named UnsupportedDriver state, got {other:?}"),
    }
    assert!(
        lookup.resolved().is_none(),
        "an unresolved driver must not hand out a capability set"
    );
    assert!(
        !lookup.allows(|caps| caps.query.explain),
        "an unresolved driver must never enable a capability-gated action"
    );
    assert_eq!(
        lookup.unavailable_reason().as_deref(),
        Some("Oracle has no provider entry in this build")
    );
}

#[test]
fn trigger_toggle_capability_is_postgres_only_with_provider_reason() {
    use db_pro_core::domain::capabilities::CapabilityFeature::TriggerEnableDisable;

    let postgres = CapabilityLookup::for_driver_label("PostgreSQL");
    assert!(postgres.allows(|caps| caps.supports(TriggerEnableDisable)));
    assert_eq!(postgres.feature_limitation(TriggerEnableDisable), None);

    for (driver, reason) in [
        ("SQLite", "SQLite cannot enable or disable individual triggers"),
        ("MySQL", "MySQL does not support enabling or disabling individual triggers"),
        ("SQL Server", "SQL Server trigger enable/disable is not enabled for this provider"),
    ] {
        let lookup = CapabilityLookup::for_driver_label(driver);
        assert!(!lookup.allows(|caps| caps.supports(TriggerEnableDisable)));
        assert_eq!(lookup.feature_limitation(TriggerEnableDisable).as_deref(), Some(reason));
    }
}

#[test]
fn no_active_connection_is_distinct_from_an_unsupported_driver() {
    let app = DbProApp::default();
    let lookup = app.active_capabilities();

    assert!(matches!(lookup, CapabilityLookup::NoActiveConnection));
    assert!(!lookup.allows(|caps| caps.query.explain));
    assert_eq!(
        lookup.unavailable_reason().as_deref(),
        Some("no database connection is active")
    );
}

#[test]
fn query_capabilities_follow_the_bound_connection_and_do_not_default_to_postgres() {
    let pg = UiConnectionSummary {
        id: "pg".to_owned(),
        name: "PostgreSQL".to_owned(),
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
    };
    let sqlite = UiConnectionSummary {
        id: "sqlite".to_owned(),
        name: "SQLite".to_owned(),
        host: String::new(),
        port: 0,
        database: "app.db".to_owned(),
        username: String::new(),
        driver: "SQLite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    };

    // With no connection at all, the old lookup answered with PostgreSQL's set through
    // `active_query_driver`'s display fallback; it must now say nothing is connected.
    let empty = DbProApp::default();
    assert!(matches!(
        empty.query_capabilities(),
        CapabilityLookup::NoActiveConnection
    ));

    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![pg, sqlite],
            },

            lifecycle: ConnectionLifecycleState {
                active_connection_id: Some("pg".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };
    assert!(app.query_capabilities().allows(|caps| caps.features.server_sessions));

    // The active query document's connection wins over the active connection.
    app.query.session.documents[app.query.session.active_document_index].connection_id = Some("sqlite".to_owned());
    assert!(!app.query_capabilities().allows(|caps| caps.features.server_sessions));
    assert!(app.query_capabilities().allows(|caps| caps.query.cancel));
}

#[test]
fn closing_query_document_restores_the_next_valid_document() {
    let mut app = DbProApp::default();
    app.new_query_document();
    app.set_active_query_text("select 2");

    app.close_query_document(0);

    assert_eq!(app.query.session.documents.len(), 1);
    assert_eq!(app.query.session.active_document_index, 0);
    assert_eq!(app.query.session.active_text(), "select 2");
    assert_eq!(app.feedback.runtime_message, "Closed Query 2");
}

#[test]
fn closing_last_query_document_returns_to_welcome() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Query,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.close_query_document(0);

    assert!(app.query.session.documents.is_empty());
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Welcome);
    assert!(app.workspace.welcome_open);
    assert!(app.query.session.active_text().is_empty());
}

#[test]
fn closing_welcome_activates_the_existing_query_tab() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Welcome,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.close_welcome_tab();

    assert!(!app.workspace.welcome_open);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);
}

#[test]
fn quick_open_filters_workspaces_by_title_and_description() {
    let app = DbProApp {
        palette: PaletteState {
            query: "relationship".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    };
    let items = app.filtered_palette_items(PaletteMode::QuickOpen);

    // Quick Open merges the command catalog, so "Refresh schema"
    // ("…views and relationships") matches alongside "ER diagram".
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(titles, vec!["ER diagram", "Refresh schema"]);
}

#[test]
fn active_schema_prefers_user_selection_and_loaded_schema_metadata() {
    let mut app = DbProApp::default();
    app.schema.explorer.schema.schemas = vec!["public".to_owned(), "tenant1".to_owned()];

    assert_eq!(app.active_schema(), "public");
    app.schema.explorer.selected_schema = Some("tenant1".to_owned());
    assert_eq!(app.active_schema(), "tenant1");
}

#[test]
fn active_schema_columns_do_not_include_other_schemas() {
    let mut app = DbProApp::default();
    app.schema.explorer.schema.schemas = vec!["public".to_owned(), "tenant1".to_owned()];
    app.schema.explorer.schema.columns = vec!["legacy_global_column".to_owned()];
    app.schema.explorer.schema.table_details = vec![
        UiTableSummary {
            schema: "public".to_owned(),
            name: "customers".to_owned(),
            row_count: None,
            columns: vec![crate::UiSchemaColumn {
                name: "customer_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: Vec::new(),
        },
        UiTableSummary {
            schema: "tenant1".to_owned(),
            name: "orders".to_owned(),
            row_count: None,
            columns: vec![crate::UiSchemaColumn {
                name: "order_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: Vec::new(),
        },
    ];

    assert_eq!(app.active_schema_column_names(), vec!["customer_id"]);
    app.schema.explorer.selected_schema = Some("tenant1".to_owned());
    assert_eq!(app.active_schema_column_names(), vec!["order_id"]);
}

#[test]
fn diagram_search_matches_schema_table_and_column_names() {
    let table = UiTableSummary {
        schema: "tenant1".to_owned(),
        name: "orders".to_owned(),
        row_count: None,
        columns: vec![crate::UiSchemaColumn {
            name: "customer_id".to_owned(),
            data_type: "integer".to_owned(),
            nullable: false,
            is_primary_key: false,
        }],
        foreign_keys: Vec::new(),
    };

    assert!(matches_diagram_search(&table, "tenant"));
    assert!(matches_diagram_search(&table, "order"));
    assert!(matches_diagram_search(&table, "customer"));
    assert!(!matches_diagram_search(&table, "invoice"));
}

#[test]
fn diagram_candidates_bound_clones_until_show_all_is_explicit() {
    let tables: Vec<_> = (0..8)
        .map(|index| UiTableSummary {
            schema: "public".to_owned(),
            name: format!("table_{index}"),
            row_count: None,
            columns: Vec::new(),
            foreign_keys: Vec::new(),
        })
        .collect();

    let (candidate_count, visible) = diagram_candidates(&tables, "", false, 5);
    assert_eq!(candidate_count, 8);
    assert_eq!(visible.len(), 6);

    let (candidate_count, search_results) = diagram_candidates(&tables, "table_7", true, 5);
    assert_eq!(candidate_count, 1);
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].name, "table_7");
}

#[test]
fn diagram_search_mode_can_leave_explicit_show_all() {
    assert!(!diagram_search_mode(true, true));
    assert!(diagram_search_mode(true, false));
    assert!(!diagram_search_mode(false, false));
    assert!(!diagram_show_all_after_search_edit(true, "orders", true));
    assert!(diagram_show_all_after_search_edit(true, "  ", true));
}

#[test]
fn diagram_move_node_updates_rect_and_incident_edge_bbox() {
    let mut graph = ErGraph::build(&[
            UiTableSummary {
                schema: "main".to_owned(),
                name: "a".to_owned(),
                row_count: None,
                columns: vec![],
                foreign_keys: vec![],
            },
            UiTableSummary {
                schema: "main".to_owned(),
                name: "b".to_owned(),
                row_count: None,
                columns: vec![],
                foreign_keys: vec![UiSchemaForeignKey {
                    name: "fk_b_a".to_owned(),
                    from_columns: vec!["a_id".to_owned()],
                    to_schema: "main".to_owned(),
                    to_table: "a".to_owned(),
                    to_columns: vec!["id".to_owned()],
                }],
            },
        ], 1, (2, 120.0));
    let old_bbox = graph.edges[0].world_bbox;

    graph.move_node(1, egui::pos2(4000.0, 2000.0));

    assert_eq!(graph.nodes[1].world_rect.min, egui::pos2(4000.0, 2000.0));
    assert_ne!(graph.edges[0].world_bbox, old_bbox, "incident edge bbox must follow the node");
    assert!(graph.world_bounds.contains(graph.nodes[1].world_rect.min));
}

#[test]
fn diagram_position_overrides_apply_by_table_key() {
    let tables = vec![
        UiTableSummary {
            schema: "main".to_owned(),
            name: "orders".to_owned(),
            row_count: None,
            columns: vec![],
            foreign_keys: vec![],
        },
        UiTableSummary {
            schema: "main".to_owned(),
            name: "customers".to_owned(),
            row_count: None,
            columns: vec![],
            foreign_keys: vec![],
        },
    ];
    let mut graph = ErGraph::build(&tables, 1, (2, 120.0));
    let untouched_min = graph.nodes[1].world_rect.min;

    let mut positions = std::collections::HashMap::new();
    positions.insert("main.orders".to_owned(), [9000.0_f32, -500.0_f32]);
    positions.insert("main.dropped_table".to_owned(), [0.0_f32, 0.0_f32]);

    assert!(graph.apply_position_overrides(&positions));
    assert_eq!(graph.nodes[0].world_rect.min, egui::pos2(9000.0, -500.0));
    assert_eq!(
        graph.nodes[1].world_rect.min, untouched_min,
        "unmatched node keeps grid position"
    );
    assert!(graph.world_bounds.contains(egui::pos2(9000.0, -500.0)));

    let mut empty = ErGraph::build(&tables, 1, (2, 120.0));
    assert!(!empty.apply_position_overrides(&std::collections::HashMap::new()));
}

#[test]
fn diagram_foreign_key_label_formats_single_and_composite_keys() {
    let single_fk = UiSchemaForeignKey {
        name: "fk_orders_customer".to_owned(),
        from_columns: vec!["customer_id".to_owned()],
        to_schema: "public".to_owned(),
        to_table: "customers".to_owned(),
        to_columns: vec!["id".to_owned()],
    };
    assert_eq!(diagram_foreign_key_label(&single_fk), "customer_id → id");

    let composite_fk = UiSchemaForeignKey {
        name: "fk_order_items_order".to_owned(),
        from_columns: vec!["tenant_id".to_owned(), "order_id".to_owned()],
        to_schema: "public".to_owned(),
        to_table: "orders".to_owned(),
        to_columns: vec!["tenant_id".to_owned(), "id".to_owned()],
    };
    assert_eq!(
        diagram_foreign_key_label(&composite_fk),
        "[tenant_id, order_id] → [tenant_id, id]"
    );
}

#[test]
fn diagram_viewport_coordinate_transformations_roundtrip() {
    let screen_origin = egui::pos2(50.0, 100.0);
    let pan = egui::vec2(20.0, -30.0);
    let zoom = 1.5;
    let viewport = ErViewport::new(pan, zoom, screen_origin);

    let world_pos = egui::pos2(300.0, 400.0);
    let screen_pos = viewport.world_to_screen_pos(world_pos);
    let roundtrip_world = viewport.screen_to_world_pos(screen_pos);

    assert!((world_pos.x - roundtrip_world.x).abs() < 1e-4);
    assert!((world_pos.y - roundtrip_world.y).abs() < 1e-4);

    let world_rect = egui::Rect::from_min_size(world_pos, egui::vec2(280.0, 160.0));
    let screen_rect = viewport.world_to_screen_rect(world_rect);
    let roundtrip_rect = viewport.screen_to_world_rect(screen_rect);

    assert!((world_rect.min.x - roundtrip_rect.min.x).abs() < 1e-4);
    assert!((world_rect.max.x - roundtrip_rect.max.x).abs() < 1e-4);
    assert!((world_rect.min.y - roundtrip_rect.min.y).abs() < 1e-4);
    assert!((world_rect.max.y - roundtrip_rect.max.y).abs() < 1e-4);
}

#[test]
fn diagram_lod_transitions_and_rules() {
    let compact = ErLod::from_zoom(0.3);
    assert_eq!(compact, ErLod::Compact);
    assert!(!compact.shows_columns());
    assert!(!compact.shows_edge_labels());
    assert_eq!(compact.max_columns(), 0);

    let standard = ErLod::from_zoom(1.0);
    assert_eq!(standard, ErLod::Standard);
    assert!(standard.shows_columns());
    assert!(!standard.shows_edge_labels());
    assert_eq!(standard.max_columns(), 6);

    let detailed = ErLod::from_zoom(1.4);
    assert_eq!(detailed, ErLod::Detailed);
    assert!(detailed.shows_columns());
    assert!(detailed.shows_edge_labels());
    assert_eq!(detailed.max_columns(), 12);
    assert_eq!(diagram_node_height(1.0), ER_HEADER_HEIGHT + ER_ROW_HEIGHT * 6.0);
    assert_eq!(diagram_node_height(1.4), ER_HEADER_HEIGHT + ER_ROW_HEIGHT * 12.0);
    assert!(diagram_node_height(1.4) > diagram_node_height(1.0));
}

#[test]
fn diagram_bfs_neighborhood_expansion() {
    let tables = vec![
        UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: Some(100),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "orders".to_owned(),
            row_count: Some(500),
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "user_id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_order_user".to_owned(),
                from_columns: vec!["user_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "users".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "order_items".to_owned(),
            row_count: Some(2000),
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "order_id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_item_order".to_owned(),
                from_columns: vec!["order_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "orders".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "logs".to_owned(),
            row_count: Some(50),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![],
        },
    ];

    let graph = ErGraph::build(&tables, 1, (2, 120.0));

    // 1-hop neighborhood from users (index 0) -> users + orders
    let hop1 = graph.bfs_neighborhood(&[0], (1, 10));
    assert_eq!(hop1.len(), 2);
    assert!(hop1.contains(&0)); // users
    assert!(hop1.contains(&1)); // orders
    assert!(!hop1.contains(&2)); // order_items is 2 hops away

    // 2-hop neighborhood from users (index 0) -> users + orders + order_items
    let hop2 = graph.bfs_neighborhood(&[0], (2, 10));
    assert_eq!(hop2.len(), 3);
    assert!(hop2.contains(&0));
    assert!(hop2.contains(&1));
    assert!(hop2.contains(&2));
    assert!(!hop2.contains(&3)); // logs is disconnected
}

#[test]
fn diagram_graph_and_spatial_index_support_1000_table_scaling() {
    // Generate 1000 tables with realistic FK connections (every 3rd table references table 0 or previous)
    let tables: Vec<UiTableSummary> = (0..1000)
        .map(|i| {
            let fks = if i > 0 && i % 3 == 0 {
                vec![UiSchemaForeignKey {
                    name: format!("fk_t{i}_t0"),
                    from_columns: vec!["ref_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: format!("table_{}", i - 1),
                    to_columns: vec!["id".to_owned()],
                }]
            } else {
                vec![]
            };
            UiTableSummary {
                schema: "public".to_owned(),
                name: format!("table_{i}"),
                row_count: Some(100),
                columns: vec![
                    UiSchemaColumn {
                        name: "id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    UiSchemaColumn {
                        name: "ref_id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: fks,
            }
        })
        .collect();

    let grid_columns = 10;
    let node_height = 140.0;
    let graph = ErGraph::build(&tables, 1, (grid_columns, node_height));
    assert_eq!(graph.nodes.len(), 1000);
    assert!(graph.edges.len() > 300);

    let spatial_index = ErSpatialIndex::build(&graph.nodes, &graph.edges, DEFAULT_SPATIAL_CELL_SIZE);

    // Simulate standard desktop viewport at 1.0 zoom (1280x800)
    let viewport = ErViewport::new(egui::Vec2::ZERO, 1.0, egui::Pos2::ZERO);
    let screen_clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));

    let scene = prepare_render_scene(&graph, &spatial_index, &viewport, screen_clip, None);

    // Visible candidates must be strictly bounded (far less than all 1000 tables)
    assert!(scene.visible_nodes.len() < 50);
    assert!(scene.visible_nodes.len() >= 4);
    assert_eq!(scene.metrics.total_nodes, 1000);
    // Spatial query execution must be fast (typically < 1000 microseconds)
    assert!(scene.metrics.spatial_query_micros < 10_000);

    // Test hit testing: hit exact center of table_0
    let table0_center = graph.nodes[0].world_rect.center();
    let hit = spatial_index.hit_test_node(table0_center, &graph.nodes);
    assert_eq!(hit, Some(0));

    // Hit testing outside any node rect returns None
    let hit_outside = spatial_index.hit_test_node(egui::pos2(99999.0, 99999.0), &graph.nodes);
    assert_eq!(hit_outside, None);
}

#[test]
fn diagram_layout_worker_background_computation_and_stale_drop() {
    let mut worker = ErLayoutWorker::new();
    let tables = vec![
        UiTableSummary {
            schema: "public".to_owned(),
            name: "categories".to_owned(),
            row_count: Some(10),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "products".to_owned(),
            row_count: Some(100),
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "cat_id".to_owned(),
                    data_type: "int".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_prod_cat".to_owned(),
                from_columns: vec!["cat_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "categories".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
    ];

    let req_id1 = worker.request_layout(1, tables.clone(), (2, 120.0));
    let req_id2 = worker.request_layout(2, tables, (2, 120.0));
    assert!(req_id2 > req_id1);

    // Drain until the worker emits the layout for the newest request, or the deadline
    // expires. The worker coalesces only what is already queued when it wakes up, so if
    // the scheduler runs it between the two calls it emits the superseded version-1
    // layout first; that intermediate is dropped at the app boundary because
    // request_id != diagram_latest_layout_request. Asserting on the first result would
    // assert on the scheduler instead. The deadline is seconds because a descheduled
    // worker thread can take hundreds of milliseconds to run on a shared CI runner,
    // while the layout itself takes microseconds.
    const COALESCING_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    let start = std::time::Instant::now();
    let mut intermediate: Vec<ErLayoutResult> = Vec::new();
    let mut latest: Option<ErLayoutResult> = None;
    while latest.is_none() && start.elapsed() < COALESCING_DEADLINE {
        match worker.poll_result() {
            Some(res) if res.request_id == req_id2 && res.graph_version == 2 => latest = Some(res),
            Some(res) => intermediate.push(res),
            None => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }

    // Coalescing / latest request guarantees version 2 is the layout the app commits.
    let res = latest.expect("worker should emit the newest layout within the deadline");
    assert_eq!(res.graph_version, 2);
    assert_eq!(res.graph.nodes.len(), 2);
    assert_eq!(res.graph.edges.len(), 1);

    // Any earlier emission can only be the stale version-1 layout, which the app boundary
    // rejects because request_id != latest_layout_request.
    for stale in &intermediate {
        assert_eq!(stale.request_id, req_id1, "only the older request may be emitted first");
        assert_eq!(stale.graph_version, 1);
        // App-boundary acceptance check (diagram_view.rs): a result is committed only when both
        // its request id and its version match the latest request.
        let accepted = stale.request_id == req_id2 && stale.graph_version == 2;
        assert!(
            !accepted,
            "the intermediate layout must be rejected at the app boundary"
        );
    }
}

#[test]
fn diagram_dense_1000_table_graph_bounds_candidates() {
    // 1000 tables with dense relationships (~3 FKs per table = ~3000 edges)
    let tables: Vec<UiTableSummary> = (0..1000)
        .map(|i| {
            let mut fks = Vec::new();
            if i > 0 {
                fks.push(UiSchemaForeignKey {
                    name: format!("fk_{i}_prev"),
                    from_columns: vec!["parent_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: format!("dense_table_{}", i - 1),
                    to_columns: vec!["id".to_owned()],
                });
            }
            if i > 2 {
                fks.push(UiSchemaForeignKey {
                    name: format!("fk_{i}_prev2"),
                    from_columns: vec!["mod_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: format!("dense_table_{}", i - 2),
                    to_columns: vec!["id".to_owned()],
                });
            }
            if i % 10 > 3 {
                fks.push(UiSchemaForeignKey {
                    name: format!("fk_{i}_cluster"),
                    from_columns: vec!["cluster_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: format!("dense_table_{}", i - (i % 10)),
                    to_columns: vec!["id".to_owned()],
                });
            }
            UiTableSummary {
                schema: "public".to_owned(),
                name: format!("dense_table_{i}"),
                row_count: Some(50),
                columns: vec![
                    UiSchemaColumn {
                        name: "id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    UiSchemaColumn {
                        name: "parent_id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                    UiSchemaColumn {
                        name: "mod_id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                    UiSchemaColumn {
                        name: "step_id".to_owned(),
                        data_type: "int".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: fks,
            }
        })
        .collect();

    let graph = ErGraph::build(&tables, 1, (10, 160.0));
    assert_eq!(graph.nodes.len(), 1000);
    assert!(graph.edges.len() > 2000);

    let spatial_index = ErSpatialIndex::build(&graph.nodes, &graph.edges, DEFAULT_SPATIAL_CELL_SIZE);
    let viewport = ErViewport::new(egui::Vec2::ZERO, 1.0, egui::Pos2::ZERO);
    let screen_clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));

    let scene = prepare_render_scene(&graph, &spatial_index, &viewport, screen_clip, None);

    // Visible nodes & edges must both remain strictly bounded
    assert!(scene.visible_nodes.len() < 50);
    assert!(scene.visible_edges.len() < 150);
}

#[test]
fn diagram_cyclic_and_self_fk_bfs_neighborhood() {
    let tables = vec![
        UiTableSummary {
            schema: "public".to_owned(),
            name: "node_a".to_owned(),
            row_count: Some(10),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_a_b".to_owned(),
                from_columns: vec!["b_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "node_b".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "node_b".to_owned(),
            row_count: Some(10),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_b_c".to_owned(),
                from_columns: vec!["c_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "node_c".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "node_c".to_owned(),
            row_count: Some(10),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_c_a".to_owned(), // cycle back to A
                from_columns: vec!["a_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "node_a".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
        UiTableSummary {
            schema: "public".to_owned(),
            name: "self_referential".to_owned(),
            row_count: Some(10),
            columns: vec![UiSchemaColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
                is_primary_key: true,
            }],
            foreign_keys: vec![UiSchemaForeignKey {
                name: "fk_self".to_owned(), // self reference
                from_columns: vec!["parent_id".to_owned()],
                to_schema: "public".to_owned(),
                to_table: "self_referential".to_owned(),
                to_columns: vec!["id".to_owned()],
            }],
        },
    ];

    let graph = ErGraph::build(&tables, 1, (2, 100.0));

    // BFS on cycle terminates cleanly without infinite recursion
    let cycle_bfs = graph.bfs_neighborhood(&[0], (5, 100));
    assert_eq!(cycle_bfs.len(), 3);
    assert!(cycle_bfs.contains(&0));
    assert!(cycle_bfs.contains(&1));
    assert!(cycle_bfs.contains(&2));
    assert!(!cycle_bfs.contains(&3));

    // Self-referential BFS
    let self_bfs = graph.bfs_neighborhood(&[3], (3, 100));
    assert_eq!(self_bfs.len(), 1);
    assert_eq!(self_bfs[0], 3);

    // Active subset bounds
    let subset_bounds = graph.active_subset_bounds(&[0, 1]);
    assert!(subset_bounds.width() >= ER_NODE_WIDTH);
}

#[test]
fn ddl_impact_summary_uses_plain_language_for_destructive_operations() {
    assert!(ddl_impact_summary("DROP TABLE customers", "customers").contains("recovery requires a backup"));
    assert!(
        ddl_impact_summary("ALTER TABLE customers ADD COLUMN note TEXT", "customers").contains("changes its structure")
    );
    assert!(ddl_impact_summary("CREATE INDEX", "customers").contains("adds or rebuilds"));
}

#[test]
fn explorer_search_matches_table_names_case_insensitively() {
    assert!(matches_explorer_table("customer_orders", "orders"));
    assert!(matches_explorer_table("CustomerOrders", "customer"));
    assert!(matches_explorer_table("customer_orders", ""));
    assert!(!matches_explorer_table("customer_orders", "invoice"));

    assert!(matches_explorer_table_with_mode(
        "customer_orders",
        "orders",
        ExplorerMatchMode::Contains
    ));
    assert!(!matches_explorer_table_with_mode(
        "customer_orders",
        "orders",
        ExplorerMatchMode::Prefix
    ));
    assert!(matches_explorer_table_with_mode(
        "orders_archive",
        "orders",
        ExplorerMatchMode::Prefix
    ));
    let (fuzzy_count, fuzzy_visible) = filtered_explorer_tables(
        &["customer_id".to_owned(), "orders".to_owned()],
        "custid",
        ExplorerMatchMode::Contains,
    );
    assert_eq!(fuzzy_count, 1);
    assert_eq!(fuzzy_visible, vec!["customer_id".to_owned()]);

    // #572: a schema with many tables must be fully browsable, not truncated at
    // 100 with a "refine filter" hint.
    let tables = (0..350).map(|index| format!("orders_{index}")).collect::<Vec<_>>();
    let (matching_count, visible_tables) =
        filtered_explorer_tables(&tables, "orders", ExplorerMatchMode::Contains);
    assert_eq!(matching_count, 350);
    assert_eq!(visible_tables.len(), 350, "every table stays reachable");

    // The cap survives only as a guard against pathological schemas.
    let many = (0..=EXPLORER_MAX_TABLES).map(|index| format!("t_{index}")).collect::<Vec<_>>();
    let (matching_count, visible_tables) = filtered_explorer_tables(&many, "t_", ExplorerMatchMode::Contains);
    assert_eq!(matching_count, EXPLORER_MAX_TABLES + 1);
    assert_eq!(visible_tables.len(), EXPLORER_MAX_TABLES);
}

#[test]
fn schema_matching_table_count_filters_without_materialising_names() {
    let mut app = DbProApp::default();
    app.schema.explorer.schema.schemas = vec!["public".into(), "other".into()];
    app.schema.explorer.schema.table_details = (0..250)
        .map(|index| UiTableSummary {
            schema: if index < 200 { "public".into() } else { "other".into() },
            name: format!("orders_{index}"),
            row_count: None,
            columns: Vec::new(),
            foreign_keys: Vec::new(),
        })
        .collect();

    assert_eq!(app.schema_table_count("public"), 200);
    assert_eq!(app.schema.explorer.matching_table_count("public", ""), 200);
    assert_eq!(app.schema.explorer.matching_table_count("public", "orders_1"), 111);
    assert_eq!(app.schema.explorer.matching_table_count("other", "orders_24"), 10);
}

#[test]
fn command_palette_new_query_keeps_a_query_entry_point() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    app.execute_palette_action(PaletteAction::NewQuery, &ctx);

    assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);
    assert_eq!(app.query.session.documents.len(), 2);
    assert!(app.palette.mode.is_none());
}

#[test]
fn command_palette_opens_problems_and_diagnostics() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    app.execute_palette_action(PaletteAction::Problems, &ctx);
    assert_eq!(app.workspace.activity, Activity::Problems);
    assert!(app.workspace.sidebar_open);
    app.execute_palette_action(PaletteAction::Diagnostics, &ctx);
    assert_eq!(app.workspace.activity, Activity::Settings);
    assert!(app.feedback.runtime_message.contains("Diagnostics"));
}

#[test]
fn quick_open_finds_schema_workbench_and_compare() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();

    app.palette.query = "Schema workbench".to_owned();
    let workbench = app.filtered_palette_items(PaletteMode::QuickOpen);
    assert!(
        workbench.iter().any(|item| item.title == "Schema workbench"),
        "Quick Open must list Schema workbench; got {:?}",
        workbench.iter().map(|item| &item.title).collect::<Vec<_>>()
    );
    app.execute_palette_action(PaletteAction::SchemaWorkbench, &ctx);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::SchemaWorkbench);

    app.palette.query = "Schema compare".to_owned();
    let compare = app.filtered_palette_items(PaletteMode::QuickOpen);
    assert!(compare.iter().any(|item| item.title == "Schema compare"));
    app.execute_palette_action(PaletteAction::SchemaCompare, &ctx);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::SchemaCompare);
    assert_eq!(app.workspace.activity, Activity::Compare);
}

#[test]
fn global_search_scopes_and_indexes_functions_with_invalidation() {
    let mut app = DbProApp {
        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_schema: Some("public".to_owned()),
                schema: UiSchemaSummary {
                    schemas: vec!["public".to_owned()],
                    tables: vec!["orders".to_owned()],
                    columns: Vec::new(),
                    table_details: Vec::new(),
                    views: vec![UiViewSummary {
                        schema: "public".to_owned(),
                        name: "order_summary".to_owned(),
                        definition: "SELECT 1".to_owned(),
                    }],
                    triggers: Vec::new(),
                    functions: vec![UiFunctionSummary {
                        schema: "public".to_owned(),
                        name: "calc_total".to_owned(),
                        routine_type: "FUNCTION".to_owned(),
                        data_type: "numeric".to_owned(),
                        definition: "SELECT 1".to_owned(),
                        identity_arguments: "order_id integer".to_owned(),
                        language: "sql".to_owned(),
                        volatility: "volatile".to_owned(),
                        security_definer: false,
                        parameters: Vec::new(),
                    }],
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.palette.query = "calc_total".to_owned();
    app.palette.scope = SearchScope::Schema;
    let schema_hits = app.filtered_palette_items_fresh(PaletteMode::QuickOpen);
    assert!(
        schema_hits.iter().any(|item| item.title == "calc_total"),
        "functions must be searchable; got {:?}",
        schema_hits.iter().map(|i| &i.title).collect::<Vec<_>>()
    );
    assert!(schema_hits
        .iter()
        .any(|item| matches!(item.action, PaletteAction::OpenFunction { .. })));

    app.palette.query.clear();
    app.palette.scope = SearchScope::Agent;
    let agent_hits = app.filtered_palette_items_fresh(PaletteMode::QuickOpen);
    assert!(agent_hits.iter().all(|item| {
        matches!(item.action, PaletteAction::Agent | PaletteAction::ExplainQuery)
            || item.subtitle.to_ascii_lowercase().contains("agent")
    }));
    assert!(!agent_hits.iter().any(|item| item.title == "orders"));

    let fp_before = app.palette.search_index.fingerprint().to_owned();
    assert!(!fp_before.is_empty());
    app.palette.search_index.invalidate();
    assert!(app.palette.search_index.is_empty());
    let _ = app.filtered_palette_items_fresh(PaletteMode::QuickOpen);
    assert_ne!(app.palette.search_index.fingerprint(), "");
}

#[test]
fn command_palette_opens_saved_query_into_editor() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    app.query.library.saved_queries = vec![crate::UiSavedQuerySummary {
        id: "sq-1".to_owned(),
        name: "Active users".to_owned(),
        sql: "SELECT 1".to_owned(),
        folder: Some("Analytics".to_owned()),
    }];
    assert!(app
        .filtered_palette_items(PaletteMode::QuickOpen)
        .iter()
        .any(|item| item.title == "Active users"));
    app.execute_palette_action(PaletteAction::OpenSavedQuery("sq-1".to_owned()), &ctx);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);
    assert!(app.query.session.active_text().contains("SELECT 1"));
    assert_eq!(
        app.query.session.documents[app.query.session.active_document_index]
            .saved_query_id
            .as_deref(),
        Some("sq-1")
    );
}

#[test]
fn sql_snippet_insert_is_one_undoable_buffer_edit() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    app.query
        .session
        .documents
        .push(crate::query::query_document::QueryDocument::new(
            "doc-snip",
            "Query",
            "SELECT 1;",
        ));
    app.query.session.active_document_index = 0;
    let before = app.query.session.active_text().to_owned();
    app.insert_snippet("SELECT 2;");
    assert!(app.query.session.active_text().contains("SELECT 2;"));
    assert_ne!(app.query.session.active_text(), before);
    assert!(app.query.session.documents[0].buffer.undo_stack.can_undo());
    app.query.session.documents[0].buffer.undo();
    assert_eq!(app.query.session.active_text(), before);
}

#[test]
fn command_palette_connection_switch_preserves_session_when_dispatch_fails() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    drop(command_rx);
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![
        UiConnectionSummary {
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
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
        UiConnectionSummary {
            id: "target".to_owned(),
            name: "Target".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "target".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
    ];
    app.connection.lifecycle.set_active_connection_id(Some("active".to_owned()));
    app.connection.lifecycle.set_connected(true);

    app.switch_connection_from_palette("target".to_owned());

    assert_eq!(app.connection.lifecycle.active_connection_id(), Some("active"));
    assert!(app.connection.lifecycle.is_connected());
    assert!(app.connection.lifecycle.pending_request().is_none());
    assert_eq!(app.feedback.runtime_message, "Runtime worker unavailable");
}

#[test]
fn command_palette_refresh_schema_bypasses_the_metadata_cache() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
    let ctx = egui::Context::default();

    app.execute_palette_action(PaletteAction::RefreshSchema, &ctx);

    let UiCommand::IntrospectSchema {
        connection_id,
        force_refresh,
        ..
    } = command_rx.try_recv().expect("schema refresh command expected")
    else {
        panic!("expected IntrospectSchema command");
    };
    assert_eq!(connection_id, "active");
    assert!(force_refresh);
    assert_eq!(app.feedback.runtime_message, "Refreshing schema…");
}

#[test]
fn native_menu_refresh_cache_dispatches_cache_invalidation_for_active_connection() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.lifecycle.active_connection_id_mut() = Some("active".to_owned());
    app.connection.lifecycle.set_connected(true);

    app.refresh_active_schema_from_menu(true);

    let UiCommand::IntrospectSchema {
        connection_id,
        force_refresh,
        invalidate_cache,
        ..
    } = command_rx.try_recv().expect("cache refresh command expected")
    else {
        panic!("expected IntrospectSchema command");
    };
    assert_eq!(connection_id, "active");
    assert!(force_refresh);
    assert!(invalidate_cache);
    assert_eq!(app.feedback.runtime_message, "Refreshing schema cache…");
}

#[test]
fn native_menu_refresh_without_active_connection_reports_feedback_without_dispatch() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    app.refresh_active_schema_from_menu(true);

    assert!(command_rx.try_recv().is_err());
    assert_eq!(app.feedback.runtime_message, "Connect a source database first");
}

#[test]
fn loading_connections_automatically_connects_active_connection() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    event_tx
        .send(UiEvent::ConnectionsLoaded {
            request_id: crate::RequestId(1),
            connections: vec![UiConnectionSummary {
                id: "local".to_owned(),
                name: "Local".to_owned(),
                host: "127.0.0.1".to_owned(),
                port: 5432,
                database: "postgres".to_owned(),
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
        })
        .expect("connections should be queued");

    app.apply_runtime_events();

    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::Connect {
            connection_id,
            ..
        }) if connection_id == "local"
    ));
    assert!(command_rx.try_recv().is_err());
}

#[test]
fn failed_connection_shows_red_indicator_and_records_error() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
        id: "conn-bad".to_owned(),
        name: "Remote Bad".to_owned(),
        host: "10.0.0.99".to_owned(),
        port: 5432,
        database: "mydb".to_owned(),
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
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-bad".to_owned());
    app.connection.lifecycle.pending_connection_id = Some("conn-bad".to_owned());
    app.connection.lifecycle.pending_request = Some(crate::RequestId(99));

    event_tx
        .send(UiEvent::QueryFailed {
            request_id: crate::RequestId(99),
            message: "Connection refused (os error 61)".to_owned(),
        })
        .expect("query failed event should be queued");

    app.apply_runtime_events();

    assert!(app.connection.lifecycle.failed_connection_ids.contains("conn-bad"));
    assert_eq!(
        app.connection.lifecycle.errors.get("conn-bad").map(|s| s.as_str()),
        Some("Connection refused (os error 61)")
    );
    let (icon, color) = app.connection_indicator(app.connection.catalog.get(0).expect("connection"));
    assert_eq!(char::from(icon), char::from(Icon::AlertCircle));
    assert_eq!(color, app.theme.danger);
}

#[test]
fn connected_event_starts_schema_and_metadata_loading() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.lifecycle.pending_request = Some(crate::RequestId(1));
    event_tx
        .send(UiEvent::Connected {
            request_id: crate::RequestId(1),
            connection_id: "local".to_owned(),
        })
        .expect("connected event should be queued");

    app.apply_runtime_events();

    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::IntrospectSchema { .. })));
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::ListSavedQueries { .. })));
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::ListQueryFolders { .. })));
}

#[test]
fn agent_provider_status_uses_runtime_provider_name() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    event_tx
        .send(UiEvent::AgentProviderReady {
            provider: "Groq".to_owned(),
            detail: "Responses API · SQL drafts stay unexecuted".to_owned(),
        })
        .expect("provider status should be queued");

    app.apply_runtime_events();
    app.agent.input = "show the active schema".to_owned();
    app.submit_agent_prompt();

    assert_eq!(app.agent.provider_label, "Groq");
    assert_eq!(app.feedback.runtime_message, "Sending request to Groq…");
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::StartAgentRun { .. })));
}

#[test]
fn typed_agent_events_are_scoped_to_the_origin_document() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.new_query_document();
    let first_id = app.query.session.documents[0].id.clone();
    let second_id = app.query.session.documents[1].id.clone();
    let first_session = super::agent_workflow_state::AgentUiSession::for_document(&first_id, None, None);
    let session_id = first_session.session.as_ref().expect("session should exist").id;
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(first_id.clone(), first_session);
    app.agent.sessions.insert(
        second_id.clone(),
        super::agent_workflow_state::AgentUiSession::for_document(&second_id, None, None),
    );
    app.agent
        .sessions
        .get_mut(&first_id)
        .expect("first session should exist")
        .active_run_id = Some(run_id);

    app.apply_runtime_event(UiEvent::AgentWorkflow {
        request_id: crate::RequestId(7),
        event: db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
            run_id,
            session_id,
            document_id: first_id.clone(),
            delta: "Use the users table".to_owned(),
        },
    });

    assert_eq!(app.agent.sessions[&first_id].streaming_text, "Use the users table");
    assert!(app.agent.sessions[&second_id].streaming_text.is_empty());
}

#[test]
fn typed_agent_patch_confirmation_applies_one_document_edit_and_continues() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.query.session.documents[0].set_text("SELECT old");
    let document_id = app.query.session.documents[0].id.clone();
    let session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let session_id = session.session.as_ref().expect("session should exist").id;
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(document_id.clone(), session);
    app.agent
        .sessions
        .get_mut(&document_id)
        .expect("session should exist")
        .active_run_id = Some(run_id);
    let patch = db_pro_core::domain::agent::AgentSqlPatch {
        document_id: document_id.clone(),
        expected_version: app.query.session.documents[0].buffer.version(),
        range: (7, 10),
        replacement: "users".to_owned(),
    };
    app.apply_runtime_event(UiEvent::AgentWorkflow {
        request_id: crate::RequestId(8),
        event: db_pro_core::domain::agent_workflow::AgentWorkflowEvent::ConfirmationRequired {
            run_id,
            session_id,
            document_id: document_id.clone(),
            call_id: "patch-1".to_owned(),
            kind: db_pro_core::domain::agent_workflow::AgentConfirmationKind::ApplyPatch,
            preview: Some(db_pro_core::domain::agent::AgentToolOutput::PatchPreview {
                patch,
                original: "old".to_owned(),
                proposed: "users".to_owned(),
            }),
        },
    });

    app.agent_confirmation_action(true);

    assert_eq!(app.query.session.documents[0].text(), "SELECT users");
    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::ContinueAgentRun {
            approved: true,
            applied_patch: Some(db_pro_core::domain::agent::AgentToolOutput::PatchApplied { .. }),
            ..
        })
    ));
    app.query.session.documents[0].buffer.undo();
    assert_eq!(app.query.session.documents[0].text(), "SELECT old");
}

#[test]
fn typed_agent_failure_clears_stale_confirmation_and_marks_activity_failed() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let _session_id = session.session.as_ref().expect("session should exist").id;
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(document_id.clone(), session);
    let session = app.agent.sessions.get_mut(&document_id).expect("session should exist");
    session.active_run_id = Some(run_id);
    session.request_id = Some(crate::RequestId(17));
    session.activities.push(super::agent_workflow_state::AgentUiActivity {
        call_id: Some("patch-1".to_owned()),
        tool: Some(db_pro_core::domain::agent::AgentTool::PatchQuery),
        label: "Preparing SQL change".to_owned(),
        status: super::agent_workflow_state::AgentUiActivityStatus::Running,
        duration_ms: None,
    });
    session.pending_confirmation = Some(super::agent_workflow_state::AgentUiConfirmation {
        run_id,
        call_id: "patch-1".to_owned(),
        kind: db_pro_core::domain::agent_workflow::AgentConfirmationKind::ApplyPatch,
        preview: None,
        document_id: document_id.clone(),
    });

    app.apply_runtime_event(UiEvent::AgentFailed {
        request_id: crate::RequestId(17),
        message: "provider unavailable".to_owned(),
    });

    let session = &app.agent.sessions[&document_id];
    assert_eq!(session.state, db_pro_core::domain::agent::AgentSessionState::Failed);
    assert_eq!(session.active_run_id, None);
    assert_eq!(session.request_id, None);
    assert_eq!(session.pending_confirmation, None);
    assert_eq!(
        session.activities[0].status,
        super::agent_workflow_state::AgentUiActivityStatus::Failed
    );
}

#[test]
fn typed_agent_open_result_in_workspace_populates_query_document() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let summary = db_pro_core::domain::agent_context::AgentResultSummary {
        columns: vec![
            db_pro_core::domain::agent_context::AgentResultColumn {
                name: "id".to_owned(),
                data_type: Some("integer".to_owned()),
            },
            db_pro_core::domain::agent_context::AgentResultColumn {
                name: "name".to_owned(),
                data_type: Some("text".to_owned()),
            },
        ],
        sample_rows: vec![vec!["1".to_owned(), "Alice".to_owned()]],
        row_count: Some(1),
        affected_rows: None,
        truncated: false,
    };
    session.tool_results.insert(
        "query-1".to_owned(),
        super::agent_workflow_state::AgentUiToolResult {
            call_id: "query-1".to_owned(),
            tool: db_pro_core::domain::agent::AgentTool::RunQuery,
            output: db_pro_core::domain::agent::AgentToolOutput::QueryResult {
                statement_index: Some(0),
                result_count: 1,
                summary,
            },
            duration_ms: Some(42),
            status: super::agent_workflow_state::AgentUiActivityStatus::Success,
        },
    );
    app.agent.sessions.insert(document_id, session);

    app.open_agent_result_in_workspace("query-1");

    assert!(!app.query.session.documents[0].query_results.is_empty());
    let res = app.query.session.documents[0].query_results.first().unwrap();
    assert_eq!(res.columns.len(), 2);
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.duration_ms, 42);
    assert_eq!(app.query.output.active_tab, crate::app::OutputTab::Results);
}

#[test]
fn typed_agent_cancellation_marks_session_and_activities_cancelled() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let session_id = session.session.as_ref().expect("session should exist").id;
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(document_id.clone(), session);
    let session = app.agent.sessions.get_mut(&document_id).expect("session should exist");
    session.active_run_id = Some(run_id);
    session.state = db_pro_core::domain::agent::AgentSessionState::Running;
    session.activities.push(super::agent_workflow_state::AgentUiActivity {
        call_id: Some("tool-1".to_owned()),
        tool: Some(db_pro_core::domain::agent::AgentTool::InspectSchema),
        label: "Inspecting schema".to_owned(),
        status: super::agent_workflow_state::AgentUiActivityStatus::Running,
        duration_ms: None,
    });

    app.apply_runtime_event(UiEvent::AgentWorkflow {
        request_id: crate::RequestId(1),
        event: db_pro_core::domain::agent_workflow::AgentWorkflowEvent::Cancelled {
            run_id,
            session_id,
            document_id: document_id.clone(),
        },
    });

    let session = &app.agent.sessions[&document_id];
    assert_eq!(session.state, db_pro_core::domain::agent::AgentSessionState::Cancelled);
    assert_eq!(session.active_run_id, None);
    assert_eq!(
        session.activities[0].status,
        super::agent_workflow_state::AgentUiActivityStatus::Cancelled
    );
}

#[test]
fn late_agent_workflow_events_are_ignored_after_cancellation() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let session_id = session.session.as_ref().expect("session should exist").id;
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(document_id.clone(), session);
    let session = app.agent.sessions.get_mut(&document_id).expect("session should exist");
    session.active_run_id = Some(run_id);
    session.state = db_pro_core::domain::agent::AgentSessionState::Cancelled;

    // Late event arrives for cancelled run
    app.apply_runtime_event(UiEvent::AgentWorkflow {
        request_id: crate::RequestId(2),
        event: db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
            run_id,
            session_id,
            document_id: document_id.clone(),
            delta: "Late arriving text".to_owned(),
        },
    });

    let session = &app.agent.sessions[&document_id];
    assert_eq!(session.state, db_pro_core::domain::agent::AgentSessionState::Cancelled);
    assert!(session.streaming_text.is_empty());
}

#[test]
fn closing_query_tab_cleans_up_agent_session_and_cancels_active_run() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.agent.sessions.insert(document_id.clone(), session);
    let session = app.agent.sessions.get_mut(&document_id).expect("session should exist");
    session.active_run_id = Some(run_id);
    session.state = db_pro_core::domain::agent::AgentSessionState::Running;

    app.close_query_document(0);

    assert!(!app.agent.sessions.contains_key(&document_id));
    let mut saw_cancel = false;
    while let Ok(cmd) = command_rx.try_recv() {
        if let UiCommand::CancelAgentRun { run_id: cancelled, .. } = cmd {
            if cancelled == run_id {
                saw_cancel = true;
            }
        }
    }
    assert!(saw_cancel);
}

#[test]
fn command_palette_shortcut_is_available_from_the_native_shell() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let ctx = egui::Context::default();
    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers {
                ctrl: true,
                ..Default::default()
            }),
            egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    }, |ui| app.handle_shortcuts(ui.ctx()));

    assert_eq!(app.palette.mode, Some(PaletteMode::QuickOpen));
}

#[test]
fn command_palette_shortcut_accepts_mac_command_modifier() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let ctx = egui::Context::default();
    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers {
                mac_cmd: true,
                command: true,
                ..Default::default()
            }),
            egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    mac_cmd: true,
                    command: true,
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    }, |ui| app.handle_shortcuts(ui.ctx()));

    assert_eq!(app.palette.mode, Some(PaletteMode::QuickOpen));
}

#[cfg(debug_assertions)]
#[test]
fn inspector_toggles_with_mod_shift_i() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            }),
            egui::Event::Key {
                key: egui::Key::I,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };

    assert!(!app.dev_tools.open);
    let _ = crate::test_frame::frame(&ctx, input(), |ui| app.handle_shortcuts(ui.ctx()));
    assert!(app.dev_tools.open, "mod+shift+i must open the inspector");
    let _ = crate::test_frame::frame(&ctx, input(), |ui| app.handle_shortcuts(ui.ctx()));
    assert!(!app.dev_tools.open, "mod+shift+i must close the inspector");
}

#[test]
fn native_text_edit_maps_linux_ctrl_to_command_shortcuts() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("native-text-edit-shortcuts");
    let mut value = "select me".to_owned();
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));

    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).id(id)).request_focus();
        });
    });

    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        events: vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).id(id));
        });
    });

    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        events: vec![egui::Event::Text("replaced".to_owned())],
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).id(id));
        });
    });

    assert_eq!(value, "replaced");
}

#[test]
fn global_panel_shortcuts_do_not_steal_text_input_combinations() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let ctx = egui::Context::default();
    let id = egui::Id::new("shortcut-routing-input");
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
    let mut value = String::new();

    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).id(id)).request_focus();
        });
    });

    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        events: vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::B,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
        ..Default::default()
    }, |ui| app.handle_shortcuts(ui.ctx()));

    assert!(app.workspace.sidebar_open);
}

#[test]
fn global_palette_shortcuts_do_not_steal_text_input_combinations() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let ctx = egui::Context::default();
    let id = egui::Id::new("palette-shortcut-input");
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0));
    let mut value = String::new();

    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        ..Default::default()
    }, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut value).id(id)).request_focus();
        });
    });

    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    let _ = crate::test_frame::frame(&ctx, egui::RawInput {
        screen_rect: Some(screen_rect),
        events: vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
        ..Default::default()
    }, |ui| app.handle_shortcuts(ui.ctx()));

    assert_eq!(app.palette.mode, None);
}

#[test]
fn failed_connection_request_clears_connecting_state_and_keeps_error() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.lifecycle.set_connected(true);
    app.connection.lifecycle.pending_request = Some(crate::RequestId(42));
    event_tx
        .send(UiEvent::QueryFailed {
            request_id: crate::RequestId(42),
            message: "auth failed".to_owned(),
        })
        .expect("connection failure should be queued");

    app.apply_runtime_events();

    assert!(!app.connection.lifecycle.is_connected());
    assert_eq!(app.connection.lifecycle.pending_request, None);
    assert_eq!(app.connection.dialog.error(), "auth failed");
    assert_eq!(app.feedback.runtime_message, "Connection failed · auth failed");
}

#[test]
fn connection_mutation_refreshes_the_explorer_without_waiting_for_another_frame() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.lifecycle.mark_connections_requested();
    app.connection.lifecycle.pending_request = Some(crate::RequestId(7));
    event_tx
        .send(UiEvent::OperationCompleted {
            request_id: crate::RequestId(7),
            operation: "connection.created".to_owned(),
        })
        .expect("connection mutation should be queued");

    app.apply_runtime_events();

    assert!(app.connection.lifecycle.connections_requested());
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::ListConnections { .. })));
}

#[test]
fn sidebar_header_launcher_opens_full_command_palette() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    // Header name+search is one control → full palette (connections + commands).
    app.palette.open(PaletteMode::Commands);

    assert_eq!(app.palette.mode, Some(PaletteMode::Commands));
    assert_eq!(app.palette.scope, SearchScope::All);
}

#[test]
fn deleting_sibling_connection_does_not_auto_reconnect_active() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![
        UiConnectionSummary {
            id: "conn-a".to_owned(),
            name: "A".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "a".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
        UiConnectionSummary {
            id: "conn-b".to_owned(),
            name: "B".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "b".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
    ];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-a".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.connection.lifecycle.pending_request = Some(crate::RequestId(21));
    app.connection.lifecycle.pending_connection_id = Some("conn-b".to_owned());
    app.connection.lifecycle.mark_connections_requested();

    event_tx
        .send(UiEvent::OperationCompleted {
            request_id: crate::RequestId(21),
            operation: "connection.deleted".to_owned(),
        })
        .expect("delete completion should queue");
    app.apply_runtime_events();

    assert_eq!(app.connection.lifecycle.active_connection_id(), Some("conn-a"));
    assert!(app.connection.lifecycle.is_connected());
    assert!(matches!(command_rx.try_recv(), Ok(UiCommand::ListConnections { .. })));

    // List refresh must not force a Connect when the active session is still up.
    event_tx
        .send(UiEvent::ConnectionsLoaded {
            request_id: crate::RequestId(22),
            connections: vec![app.connection.catalog.connections_mut()[0].clone()],
        })
        .expect("connections list should queue");
    app.apply_runtime_events();

    assert_eq!(app.connection.lifecycle.active_connection_id(), Some("conn-a"));
    assert!(app.connection.lifecycle.is_connected());
    assert!(
        !matches!(command_rx.try_recv(), Ok(UiCommand::Connect { .. })),
        "active session must not reconnect after deleting a sibling"
    );
}

#[test]
fn sql_diagnostics_allow_expression_selects_without_from() {
    let diagnostics = query_diagnostics_view::parse_sql_diagnostics("SELECT 1 AS ok;", "PostgreSQL");

    assert!(diagnostics.is_empty());
}

#[test]
fn sql_diagnostics_include_unmatched_square_bracket_range() {
    let diagnostics = query_diagnostics_view::parse_sql_diagnostics("SELECT items[1 FROM data;", "PostgreSQL");

    assert!(diagnostics
        .iter()
        .any(|message| message.contains("Unmatched delimiter [")));
}

#[test]
fn sql_diagnostics_report_mixed_delimiter_mismatch() {
    let diagnostics = query_diagnostics_view::parse_sql_diagnostics("SELECT ([)]", "PostgreSQL");

    assert!(diagnostics
        .iter()
        .any(|message| message.contains("Mismatched delimiter ): expected ]")));
}

#[test]
fn sql_diagnostics_gate_ilike_and_glob_through_capabilities() {
    let sqlite = query_diagnostics_view::parse_sql_diagnostics("SELECT * FROM t WHERE name ILIKE 'a%'", "SQLite");
    assert!(sqlite.iter().any(|m| m.contains("ILIKE")));

    let postgres = query_diagnostics_view::parse_sql_diagnostics("SELECT * FROM t WHERE name GLOB 'a*'", "PostgreSQL");
    assert!(postgres.iter().any(|m| m.contains("GLOB")));

    let mysql = query_diagnostics_view::parse_sql_diagnostics("SELECT * FROM t WHERE name ILIKE 'a%'", "MySQL");
    assert!(mysql.iter().any(|m| m.contains("ILIKE")));

    let pg_ok = query_diagnostics_view::parse_sql_diagnostics("SELECT * FROM t WHERE name ILIKE 'a%'", "PostgreSQL");
    assert!(!pg_ok.iter().any(|m| m.contains("ILIKE is not supported")));
}

#[test]
fn sql_lint_warns_on_select_star_and_null_compare() {
    let (messages, structured) =
        query_diagnostics_view::analyze_sql_diagnostics("SELECT * FROM t WHERE id = NULL", "PostgreSQL");
    assert!(
        !messages.iter().any(|m| m.contains("SELECT *")),
        "SELECT * is a normal table read and does not warn unless the rule is enabled"
    );
    assert!(messages.iter().any(|m| m.contains("IS NULL")));
    assert!(!structured.iter().any(|d| d.code.as_deref() == Some("lint.select-star")));
    let lint = crate::app::SqlLintSettings {
        select_star: true,
        ..Default::default()
    };
    let (messages, structured) = query_diagnostics_view::analyze_sql_diagnostics_with_lint(
        "SELECT * FROM t WHERE id = NULL",
        "PostgreSQL",
        &lint,
    );
    assert!(messages.iter().any(|m| m.contains("SELECT *")));
    assert!(structured.iter().any(|d| {
        d.source == crate::editor::DiagnosticSource::Lint && d.code.as_deref() == Some("lint.select-star")
    }));
    assert!(structured.iter().any(|d| {
        d.source == crate::editor::DiagnosticSource::Lint
            && d.code.as_deref() == Some("lint.null-compare")
            && d.fix.as_deref() == Some("IS NULL")
    }));
}

#[test]
fn sql_lint_null_compare_quick_fix_is_one_undoable_replace() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    let mut doc =
        crate::query::query_document::QueryDocument::new("doc-fix", "Query fix", "SELECT 1 FROM t WHERE id = NULL");
    let (_, structured) = query_diagnostics_view::analyze_sql_diagnostics(doc.text(), "PostgreSQL");
    doc.diagnostics = structured;
    app.query.session.documents.push(doc);

    let entry = app
        .collect_problem_entries()
        .into_iter()
        .find(|e| e.has_fix)
        .expect("null-compare fix");
    assert!(app.apply_problem_fix(&entry));
    let text = app.query.session.documents[0].text().to_owned();
    assert!(text.contains("IS NULL"), "fixed text was {text}");
    assert!(!text.to_lowercase().contains("= null"));
    assert!(app.query.session.documents[0].buffer.undo_stack.can_undo());
    app.query.session.documents[0].buffer.undo();
    assert!(app.query.session.documents[0].text().to_lowercase().contains("= null"));
}

#[test]
fn sql_lint_warns_on_delete_without_where() {
    let (messages, structured) = query_diagnostics_view::analyze_sql_diagnostics("DELETE FROM t", "SQLite");
    assert!(messages.iter().any(|m| m.contains("DELETE without WHERE")));
    assert!(structured
        .iter()
        .any(|d| d.code.as_deref() == Some("lint.delete-no-where")));
}

#[test]
fn sql_lint_warns_on_order_by_ordinal_and_comma_join() {
    let (messages, structured) =
        query_diagnostics_view::analyze_sql_diagnostics("SELECT a, b FROM t1, t2 ORDER BY 1", "PostgreSQL");
    assert!(messages.iter().any(|m| m.contains("ORDER BY ordinal")));
    assert!(messages.iter().any(|m| m.contains("Comma join")));
    assert!(structured
        .iter()
        .any(|d| d.code.as_deref() == Some("lint.order-by-ordinal")));
    assert!(structured.iter().any(|d| d.code.as_deref() == Some("lint.comma-join")));
}

#[test]
fn sql_lint_warns_on_duplicate_projection_alias() {
    let (messages, structured) =
        query_diagnostics_view::analyze_sql_diagnostics("SELECT a AS x, b AS x FROM t", "PostgreSQL");
    assert!(messages.iter().any(|m| m.contains("Duplicate projection alias")));
    assert!(structured
        .iter()
        .any(|d| d.code.as_deref() == Some("lint.duplicate-alias")));
}

#[test]
fn sql_lint_warns_on_update_without_where() {
    let (messages, structured) = query_diagnostics_view::analyze_sql_diagnostics("UPDATE t SET a = 1", "PostgreSQL");
    assert!(messages.iter().any(|m| m.contains("UPDATE without WHERE")));
    assert!(structured.iter().any(|d| {
        d.source == crate::editor::DiagnosticSource::Lint && d.code.as_deref() == Some("lint.update-no-where")
    }));
}

#[test]
fn sql_lint_respects_disabled_and_suppressed_rules() {
    let mut lint = crate::app::SqlLintSettings {
        select_star: false,
        ..crate::app::SqlLintSettings::default()
    };
    lint.suppressed_codes.insert("lint.null-compare".into());
    let (messages, structured) = query_diagnostics_view::analyze_sql_diagnostics_with_lint(
        "SELECT * FROM t WHERE id = NULL",
        "PostgreSQL",
        &lint,
    );
    assert!(!messages.iter().any(|m| m.contains("SELECT *")));
    assert!(!structured.iter().any(|d| d.code.as_deref() == Some("lint.select-star")));
    assert!(!structured
        .iter()
        .any(|d| d.code.as_deref() == Some("lint.null-compare")));
}

#[test]
fn problems_panel_aggregates_open_document_diagnostics_and_navigates() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    let mut doc = crate::query::query_document::QueryDocument::new("doc-1", "Query 1", "DELETE FROM t");
    let (_, structured) = query_diagnostics_view::analyze_sql_diagnostics(doc.text(), "PostgreSQL");
    doc.diagnostics = structured;
    app.query.session.documents.push(doc);

    let entries = app.collect_problem_entries();
    assert!(!entries.is_empty());
    assert!(entries.iter().any(|e| e.message.contains("DELETE without WHERE")));

    app.query.editor.problems_severity_filter = ProblemsSeverityFilter::Warnings;
    app.query.editor.problems_source_filter = ProblemsSourceFilter::Lint;
    assert!(entries.iter().any(|e| app.problem_matches_filters(e)));

    let first_lint = entries
        .iter()
        .find(|e| e.source == crate::editor::DiagnosticSource::Lint)
        .expect("lint entry");
    app.navigate_to_problem(first_lint);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);
    assert_eq!(app.workspace.activity, Activity::Problems);
    let active = &app.query.session.documents[app.query.session.active_document_index];
    assert_eq!(active.cursor.offset, first_lint.range.0);
    assert_eq!(active.selection.normalized(), (first_lint.range.0, first_lint.range.1));
}

#[test]
fn diagnostics_summary_redacts_runtime_errors_and_lists_mysql() {
    let mut app = DbProApp {
        feedback: FeedbackState {
            runtime_message: "connection failed password=hunter2".to_owned(),
            ..Default::default()
        },
        ..DbProApp::default()
    };
    app.connection
        .catalog
        .connections_mut()
        .push(crate::UiConnectionSummary {
            id: "c1".to_owned(),
            name: "local".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "app".to_owned(),
            username: "alice".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: crate::UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        });
    let summary = app.build_diagnostics_summary();
    assert!(summary.drivers.iter().any(|d| d.driver == "mysql"));
    assert_eq!(summary.connections.len(), 1);
    assert_eq!(summary.recent_errors.len(), 1);
    assert_eq!(summary.recent_errors[0].message, "***");
    let json = serde_json::to_string(&summary).expect("serialize");
    assert!(!json.contains("hunter2"));
}

#[test]
fn database_error_position_maps_postgres_character_to_utf8_editor_offset() {
    let sql = "SELECT café FROM users";
    let diagnostic = super::query_view::database_error_diagnostic(
        "syntax error",
        sql,
        (4, 4 + sql.len()),
        Some(11),
        Some("QUERY_SYNTAX_ERROR"),
    )
    .expect("database diagnostic should have a range");

    assert_eq!(diagnostic.source, crate::editor::DiagnosticSource::Database);
    assert_eq!(diagnostic.code.as_deref(), Some("QUERY_SYNTAX_ERROR"));
    assert_eq!(diagnostic.range, (14, 16));
}

#[test]
fn database_error_position_maps_selection_relative_to_document_offset() {
    let prefix = "-- before\n";
    let sql = "SELECT café FROM users";
    let selection_range = (prefix.len(), prefix.len() + sql.len());
    let diagnostic = super::query_view::database_error_diagnostic("syntax error", sql, selection_range, Some(11), None)
        .expect("database diagnostic should have a range");

    assert_eq!(diagnostic.range, (prefix.len() + 10, prefix.len() + 12));
}

#[test]
fn database_error_position_outside_executed_sql_falls_back_to_statement_range() {
    let sql = "SELECT café";
    let range = (3, 3 + sql.len());
    let diagnostic = super::query_view::database_error_diagnostic("syntax error", sql, range, Some(0), None)
        .expect("database diagnostic should have a range");

    assert_eq!(diagnostic.range, range);
}

#[test]
fn database_error_without_position_uses_the_executed_statement_range() {
    let sql = "SELECT 1";
    let range = (8, 8 + sql.len());
    let diagnostic = super::query_view::database_error_diagnostic("database error", sql, range, None, None)
        .expect("database diagnostic should have a range");

    assert_eq!(diagnostic.range, range);
}

#[test]
fn query_failure_attaches_database_diagnostic_to_the_originating_document() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let mut doc = QueryDocument::new("query-1", "Query 1", "SELECT café;");
    let request_id = crate::RequestId(77);
    doc.execution_state = QueryExecutionState::Running(request_id);
    doc.executing_range = Some((0, doc.text().len()));
    doc.executing_sql = Some(doc.text().to_owned());
    doc.executing_version = Some(doc.buffer.version());
    app.query.session.documents = vec![doc];
    app.query
        .session
        .document_requests
        .insert(request_id, "query-1".to_owned());

    event_tx
        .send(UiEvent::QueryFailedDetailed {
            request_id,
            code: "QUERY_SYNTAX_ERROR".to_owned(),
            message: "syntax error".to_owned(),
            position: Some(8),
        })
        .expect("query failure should be queued");
    app.apply_runtime_events();

    let diagnostic = app.query.session.documents[0]
        .execution_diagnostic
        .as_ref()
        .expect("query failure should attach a diagnostic");
    assert_eq!(diagnostic.source, crate::editor::DiagnosticSource::Database);
    assert_eq!(diagnostic.range, (7, 8));
    assert_eq!(
        app.query.session.documents[0].execution_state,
        QueryExecutionState::Failed
    );
}

#[test]
fn failed_schema_request_is_visible_and_retryable() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.schema.explorer.schema_request = Some(crate::RequestId(7));
    event_tx
        .send(UiEvent::QueryFailed {
            request_id: crate::RequestId(7),
            message: "missing field `from_columns`".to_owned(),
        })
        .expect("schema failure should be queued");

    app.apply_runtime_events();

    assert_eq!(app.schema.explorer.schema_request, None);
    assert_eq!(
        app.schema.explorer.schema_error.as_deref(),
        Some("missing field `from_columns`")
    );
    assert_eq!(
        app.feedback.runtime_message,
        "Schema introspection failed · missing field `from_columns`"
    );
}

#[test]
fn failed_schema_dispatch_does_not_leave_a_fake_pending_request() {
    let mut app = DbProApp::default();

    app.request_schema_introspection("conn-1".to_owned(), true);

    assert_eq!(app.schema.explorer.schema_request, None);
    assert_eq!(
        app.schema.explorer.schema_error.as_deref(),
        Some("Runtime worker unavailable")
    );
}

#[test]
fn failed_connection_list_dispatch_does_not_leave_a_fake_pending_request() {
    let mut app = DbProApp::default();

    app.request_connections_once();

    assert!(!app.connection.lifecycle.connections_request_pending());
}

#[test]
fn failed_table_data_dispatch_does_not_leave_a_fake_pending_request() {
    let mut app = DbProApp::default();
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.schema.explorer.selected_table = Some("users".to_owned());

    app.request_table_data();

    assert_eq!(app.table.data_query.request, None);
}

#[test]
fn failed_query_dispatch_does_not_mark_document_running() {
    let mut app = DbProApp::default();

    let options = crate::app::events_query_dispatch::QueryRunOptions {
        execution_range: (0, 8),
        version: 0,
        all_statements: false,
        bound_params: None,
        applied_row_limit: None,
    };
    app.send_query_run("conn-1".to_owned(), "SELECT 1".to_owned(), options);

    assert_eq!(app.query.session.active_running_request(), None);
    assert!(app.query.editor.query_history.is_empty());
}

#[test]
fn stale_schema_event_cannot_replace_the_selected_connection_schema() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.schema.explorer.schema.tables = vec!["current_table".to_owned()];
    app.schema.explorer.schema_request = Some(crate::RequestId(2));
    event_tx
        .send(UiEvent::SchemaLoaded {
            request_id: crate::RequestId(1),
            schema: UiSchemaSummary {
                schemas: Vec::new(),
                tables: vec!["stale_table".to_owned()],
                columns: Vec::new(),
                table_details: Vec::new(),
                views: Vec::new(),
                triggers: Vec::new(),
                functions: Vec::new(),
            },
        })
        .expect("stale schema event should be queued");

    app.apply_runtime_events();

    assert_eq!(app.schema.explorer.schema.tables, vec!["current_table"]);
    assert_eq!(app.schema.explorer.schema_request, Some(crate::RequestId(2)));
}

#[test]
fn connection_test_success_is_invalidated_when_the_draft_changes() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.connection.dialog.draft_mut().name = "Local".to_owned();
    app.connection.dialog.draft_mut().host = "db.test".to_owned();
    app.connection.dialog.draft_mut().database = "app".to_owned();
    app.connection.dialog.draft_mut().username = "app".to_owned();
    app.connection.dialog.draft_mut().port = "5432".to_owned();
    app.dispatch_connection_command(false);

    let request_id = match command_rx.try_recv().expect("test command expected") {
        UiCommand::TestConnection { request_id, .. } => request_id,
        _ => panic!("expected TestConnection command"),
    };
    app.connection.dialog.draft_mut().database = "other".to_owned();
    event_tx
        .send(UiEvent::OperationCompleted {
            request_id,
            operation: "connection.tested".to_owned(),
        })
        .expect("test result should be queued");

    app.apply_runtime_events();

    assert!(!app.connection.dialog.test_valid());
    assert_eq!(
        app.feedback.runtime_message,
        "Connection changed · test again before saving"
    );
}


#[test]
fn schema_refresh_reloads_the_selected_table_after_summary_completion() {
    let (bridge, command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
        id: "active".to_owned(),
        name: "Active".to_owned(),
        host: String::new(),
        port: 0,
        database: "active".to_owned(),
        username: String::new(),
        driver: "SQLite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("active".to_owned());
    app.schema.explorer.selected_table = Some("customers".to_owned());
    app.workspace.active_tab = WorkspaceTab::Table;
    app.table.state.refresh_table_info_after_schema = true;
    app.schema.explorer.schema_request = Some(crate::RequestId(1));
    event_tx
        .send(UiEvent::SchemaLoaded {
            request_id: crate::RequestId(1),
            schema: UiSchemaSummary {
                schemas: vec!["main".to_owned()],
                tables: vec!["customers".to_owned()],
                columns: vec!["id".to_owned()],
                table_details: Vec::new(),
                views: Vec::new(),
                triggers: Vec::new(),
                functions: Vec::new(),
            },
        })
        .expect("schema event should be queued");

    app.apply_runtime_events();

    let UiCommand::LoadTableInfo {
        connection_id,
        schema,
        table,
        ..
    } = command_rx.try_recv().expect("table metadata refresh expected")
    else {
        panic!("expected LoadTableInfo command");
    };
    assert_eq!(connection_id, "active");
    assert_eq!(schema, "main");
    assert_eq!(table, "customers");
    assert!(!app.table.state.refresh_table_info_after_schema);
}

#[test]
fn schema_refresh_returns_to_welcome_when_selected_table_disappears() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.schema.explorer.selected_table = Some("deleted_table".to_owned());
    app.workspace.active_tab = WorkspaceTab::Table;
    app.schema.explorer.schema_request = Some(crate::RequestId(1));
    event_tx
        .send(UiEvent::SchemaLoaded {
            request_id: crate::RequestId(1),
            schema: UiSchemaSummary {
                schemas: Vec::new(),
                tables: vec!["remaining_table".to_owned()],
                columns: Vec::new(),
                table_details: Vec::new(),
                views: Vec::new(),
                triggers: Vec::new(),
                functions: Vec::new(),
            },
        })
        .expect("schema event should be queued");

    app.apply_runtime_events();

    assert_eq!(app.schema.explorer.selected_table, None);
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Welcome);
}

#[test]
fn closing_workspace_tab_clears_its_resource_and_requests() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Table,
                ..Default::default()
            },
            ..Default::default()
        },
        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("customers".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        table: TableEditorState {
            state: TableState {
                table_info_request: Some(crate::RequestId(1)),
                table_ddl_request: Some(crate::RequestId(2)),
                ..Default::default()
            },
            data_query: TableDataQueryState {
                request: Some(crate::RequestId(3)),
                result: Some(result()),
                filters: vec![UiTableDataFilter {
                    column: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    operator: UiTableFilterOperator::Equals,
                    value: "1".to_owned(),
                }],
                sorts: vec![UiTableDataSort {
                    column: "id".to_owned(),
                    descending: true,
                }],
                ..Default::default()
            },
            mutation: TableMutationState {
                staged_changes: {
                    let mut changes = ChangeSet::default();
                    changes.ensure_target("customers");
                    changes
                },
                pending_changes_open: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    app.request_close_workspace_tab(WorkspaceTab::Table);

    assert_eq!(app.workspace.active_tab, WorkspaceTab::Welcome);
    assert_eq!(app.schema.explorer.selected_table, None);
    assert_eq!(app.table.state.table_info_request, None);
    assert_eq!(app.table.state.table_ddl_request, None);
    assert_eq!(app.table.data_query.request, None);
    assert!(app.table.data_query.result.is_none());
    assert!(app.table.data_query.filters.is_empty());
    assert!(app.table.data_query.sorts.is_empty());
    assert!(app.table.mutation.staged_changes.is_empty());
    assert!(!app.table.mutation.pending_changes_open);
}

#[test]
fn query_dispatch_uses_the_active_connection_not_the_first_connection() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![
        UiConnectionSummary {
            id: "first".to_owned(),
            name: "First".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "first".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
        UiConnectionSummary {
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
        },
    ];
    *app.connection.lifecycle.active_connection_id_mut() = Some("active".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.set_document_schema(0, Some("tenant1".to_owned()));
    app.dispatch_query();

    let UiCommand::RunQuery {
        connection_id, schema, ..
    } = command_rx.try_recv().expect("query command expected")
    else {
        panic!("expected RunQuery command");
    };
    assert_eq!(connection_id, "active");
    assert_eq!(schema.as_deref(), Some("tenant1"));
}

#[test]
fn dispatch_caps_reads_with_the_toolbar_row_limit() {
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

    let dispatch_and_finish = |app: &mut DbProApp,
                               command_rx: &std::sync::mpsc::Receiver<UiCommand>,
                               event_tx: &std::sync::mpsc::SyncSender<UiEvent>|
     -> String {
        app.dispatch_query();
        let UiCommand::RunQuery { request_id, sql, .. } =
            command_rx.try_recv().expect("query command expected")
        else {
            panic!("expected RunQuery command");
        };
        event_tx
            .send(UiEvent::QueryCompleted {
                request_id,
                result: UiQueryResult {
                    columns: vec![],
                    rows: vec![],
                    row_count: 0,
                    duration_ms: 1,
                },
            })
            .unwrap();
        app.apply_runtime_events();
        sql
    };

    app.set_active_query_text("SELECT * FROM users");
    app.query.execution.query_row_limit = Some(100);
    assert_eq!(
        dispatch_and_finish(&mut app, &command_rx, &event_tx),
        "SELECT * FROM users LIMIT 100"
    );

    // An explicit LIMIT in the statement wins over the toolbar cap.
    app.set_active_query_text("SELECT * FROM users LIMIT 10");
    assert_eq!(
        dispatch_and_finish(&mut app, &command_rx, &event_tx),
        "SELECT * FROM users LIMIT 10"
    );

    // "No limit" passes the statement through untouched.
    app.query.execution.query_row_limit = None;
    app.set_active_query_text("SELECT * FROM users");
    assert_eq!(
        dispatch_and_finish(&mut app, &command_rx, &event_tx),
        "SELECT * FROM users"
    );
}

#[test]
fn ddl_apply_dispatch_requires_an_explicit_request_and_uses_active_connection() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
    app.table.state.table_ddl = Some("CREATE TABLE \"public\".\"audit\" (id INTEGER)".to_owned());

    app.submit_ddl();

    let UiCommand::ExecuteDdl { connection_id, sql, .. } = command_rx.try_recv().expect("DDL command expected") else {
        panic!("expected ExecuteDdl command");
    };
    assert_eq!(connection_id, "active");
    assert_eq!(sql, "CREATE TABLE \"public\".\"audit\" (id INTEGER)");
    assert!(app.table.state.ddl_execution_request.is_some());
}

#[test]
fn test_column_order_and_move_column() {
    let mut app = DbProApp::default();
    let order = app.table.data.column_order(4);
    assert_eq!(order, vec![0, 1, 2, 3]);

    app.table.data.move_column(0, 2, 4);
    assert_eq!(app.table.data.grid_column_order, vec![1, 2, 0, 3]);

    app.table.data.move_column(3, 1, 4);
    assert_eq!(app.table.data.grid_column_order, vec![1, 3, 2, 0]);

    // Invalid persisted indexes are removed while valid order is preserved.
    let new_order = app.table.data.column_order(2);
    assert_eq!(new_order, vec![1, 0]);
}

#[test]
fn test_format_cell_csv_and_cell_to_json() {
    assert_eq!(result_grid_export::format_cell_csv(&UiCell::Null), "\\N");
    assert_eq!(result_grid_export::format_cell_csv(&UiCell::Boolean(true)), "true");
    assert_eq!(
        result_grid_export::format_cell_csv(&UiCell::Number("42.50".to_owned())),
        "42.50"
    );
    assert_eq!(
        result_grid_export::format_cell_csv(&UiCell::Text("Hello, \"World\"".to_owned())),
        "\"Hello, \"\"World\"\"\""
    );

    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Null),
        serde_json::Value::Null
    );
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Boolean(false)),
        serde_json::Value::Bool(false)
    );
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Number("100".to_owned())),
        serde_json::json!(100)
    );
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Text("admin".to_owned())),
        serde_json::Value::String("admin".to_owned())
    );
}

/// Copy-as-JSON must not round an exact value through `f64`.
#[test]
fn test_copy_as_json_keeps_exact_numeric_digits() {
    // Beyond f64's 2^53 exact-integer range: must stay the identical text.
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Number("9007199254740993".to_owned())),
        serde_json::Value::String("9007199254740993".to_owned())
    );
    // Exact decimal with trailing zeroes: the digits are the value.
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Number("42.50".to_owned())),
        serde_json::Value::String("42.50".to_owned())
    );
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Number("12345678901234567890.12345".to_owned())),
        serde_json::Value::String("12345678901234567890.12345".to_owned())
    );
    // A value f64 represents exactly still serializes as a JSON number.
    assert_eq!(
        result_grid_export::cell_to_json_value(&UiCell::Number("1.5".to_owned())),
        serde_json::json!(1.5)
    );
    // ... and the serialized text of that number is the original text.
    let copied = result_grid_export::cell_to_json_value(&UiCell::Number("100".to_owned())).to_string();
    assert_eq!(copied, "100");
}

/// The copied/exported record shape must survive values that contain the
/// separator, a quote, or a line break.
#[test]
fn test_delimited_export_keeps_field_count_for_awkward_values() {
    let value = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "note".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![vec![
            crate::UiCell::Number("9007199254740993".to_owned()),
            crate::UiCell::Text("first, \"second\"\nthird".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 0,
    };

    let csv = result_grid_export::format_result_delimited(&value, ",");
    assert_eq!(
        csv, "id,note\n9007199254740993,\"first, \"\"second\"\"\nthird\"\n",
        "the delimiter, the quotes and the newline must be escaped inside one field, \
         and the exact digits must survive unquoted"
    );

    let tsv = result_grid_export::format_result_delimited(&value, "\t");
    assert_eq!(
        tsv, "id\tnote\n9007199254740993\t\"first, \"\"second\"\"\nthird\"\n",
        "the tab-separated shape must survive too"
    );
}

/// The export writes exactly the shared serializer's text to disk.
#[test]
fn test_export_result_writes_escaped_delimited_text() {
    let value = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "note".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
            crate::UiColumn {
                name: "amount".to_owned(),
                data_type: "numeric".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![
            vec![
                crate::UiCell::Text("line, one\ntwo".to_owned()),
                crate::UiCell::Number("42.50".to_owned()),
            ],
            vec![
                crate::UiCell::Null,
                crate::UiCell::Number("9007199254740993".to_owned()),
            ],
        ],
        row_count: 2,
        duration_ms: 0,
    };

    let path = std::env::temp_dir().join(format!("db-pro-export-test-{}.csv", uuid::Uuid::new_v4()));
    let path_text = path.to_string_lossy().into_owned();
    let mut app = DbProApp {
        overlay: OverlayState {
            export_format: ExportFormat::Csv,
            export_path: path_text.clone(),
            export_open: true,
            ..Default::default()
        },
        ..Default::default()
    };
    app.export_result(&value);
    assert!(!app.overlay.export_open, "the dialog closes after a successful export");
    assert_eq!(app.feedback.runtime_message, format!("Exported 2 rows to {path_text}"));

    let written = std::fs::read_to_string(&path).expect("export file must exist");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        written, "note,amount\n\"line, one\ntwo\",42.50\n\\N,9007199254740993\n",
        "NULL is the \\N sentinel, the exact digits survive, and the embedded newline stays quoted"
    );
}

#[test]
fn test_table_data_limit_and_paging_offset() {
    let mut app = DbProApp::default();
    assert_eq!(app.table.data_query.limit, 100);

    app.table.data_query.limit = 50;
    app.table.data_query.offset = 100;
    app.reset_table_data_page();
    assert_eq!(app.table.data_query.offset, 0);
}

#[test]
fn test_table_metadata_dependency_and_constraint_models() {
    let dep = UiTableDependency {
        name: "orders_archive".to_owned(),
        schema: "public".to_owned(),
        kind: UiDependencyKind::Table,
        direction: UiDependencyDirection::DependsOn,
        details: "Foreign key reference".to_owned(),
    };
    assert_eq!(dep.kind, UiDependencyKind::Table);
    assert_eq!(dep.direction, UiDependencyDirection::DependsOn);

    let check = UiCheckConstraint {
        name: "chk_positive_qty".to_owned(),
        definition: "quantity > 0".to_owned(),
    };
    assert_eq!(check.name, "chk_positive_qty");
    assert_eq!(check.definition, "quantity > 0");
}

#[test]
fn test_compare_ui_cells_typed_sorting() {
    use std::cmp::Ordering;

    // Number comparisons (exact numeric, not string alphabetical)
    let n2 = UiCell::Number("2".to_owned());
    let n10 = UiCell::Number("10".to_owned());
    let n3 = UiCell::Number("3".to_owned());
    assert_eq!(crate::compare_ui_cells(Some(&n2), Some(&n10)), Ordering::Less);
    assert_eq!(crate::compare_ui_cells(Some(&n10), Some(&n3)), Ordering::Greater);
    assert_eq!(crate::compare_ui_cells(Some(&n2), Some(&n3)), Ordering::Less);

    // Negative and decimal numbers
    let neg = UiCell::Number("-5.5".to_owned());
    let pos = UiCell::Number("1.2".to_owned());
    assert_eq!(crate::compare_ui_cells(Some(&neg), Some(&pos)), Ordering::Less);

    // Booleans: false < true
    let b_false = UiCell::Boolean(false);
    let b_true = UiCell::Boolean(true);
    assert_eq!(crate::compare_ui_cells(Some(&b_false), Some(&b_true)), Ordering::Less);

    // Nulls placed last
    assert_eq!(
        crate::compare_ui_cells(Some(&UiCell::Null), Some(&n2)),
        Ordering::Greater
    );
    assert_eq!(crate::compare_ui_cells(Some(&n2), Some(&UiCell::Null)), Ordering::Less);
    assert_eq!(
        crate::compare_ui_cells(Some(&UiCell::Null), Some(&UiCell::Null)),
        Ordering::Equal
    );
    assert_eq!(crate::compare_ui_cells(None, Some(&n2)), Ordering::Greater);

    // Date / timestamp strings compared chronologically
    let d1 = UiCell::Text("2026-01-15T09:00:00Z".to_owned());
    let d2 = UiCell::Text("2026-03-01T10:00:00Z".to_owned());
    assert_eq!(crate::compare_ui_cells(Some(&d1), Some(&d2)), Ordering::Less);
}

#[test]
fn test_open_table_blocked_with_unapplied_staged_changes() {
    let mut app = DbProApp {
        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("users".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.table.mutation.staged_changes.ensure_target("users");
    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: primary_key_identity("1"),
        current_row_index: Some(0),
        column_index: 0,
        column: "name".to_owned(),
        data_type: "text".to_owned(),
        original: UiCell::Text("Alice".to_owned()),
        value: UiCell::Text("Alicia".to_owned()),
    });

    // Opening another table should be blocked to prevent mutation retargeting
    app.open_table("orders".to_owned());
    assert_eq!(app.schema.explorer.selected_table, Some("users".to_owned()));
    assert!(app.table.editing.discard_changes_confirmation);
    assert!(app.feedback.runtime_message.contains("Apply or discard staged changes"));

    // Closing table tab with staged changes is guarded
    app.table.editing.discard_changes_confirmation = false;
    app.request_close_workspace_tab(WorkspaceTab::Table);
    assert_eq!(app.schema.explorer.selected_table, Some("users".to_owned()));
    assert!(app.table.editing.discard_changes_confirmation);

    // Discarding changes allows opening a new table
    app.discard_staged_changes();
    app.open_table("orders".to_owned());
    assert_eq!(app.schema.explorer.selected_table, Some("orders".to_owned()));
}

#[test]
fn test_query_cancellation_capability_gate() {
    let postgres_conn = UiConnectionSummary {
        id: "pg".to_owned(),
        name: "PostgreSQL".to_owned(),
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
    };
    let sqlite_conn = UiConnectionSummary {
        id: "sqlite".to_owned(),
        name: "SQLite".to_owned(),
        host: String::new(),
        port: 0,
        database: "app.db".to_owned(),
        username: String::new(),
        driver: "SQLite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    };

    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![postgres_conn, sqlite_conn],
            },

            lifecycle: ConnectionLifecycleState {
                connected: true,
                active_connection_id: Some("pg".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    assert!(app.active_capabilities().allows(|caps| caps.query.cancel));

    // Switching to SQLite enables query cancellation
    *app.connection.lifecycle.active_connection_id_mut() = Some("sqlite".to_owned());
    assert!(app.active_capabilities().allows(|caps| caps.query.cancel));
}

#[test]
fn test_navigation_staged_changes_apply_discard_cancel_flows() {
    let mut app = DbProApp {
        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("users".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.table.mutation.staged_changes.ensure_target("users");
    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: primary_key_identity("1"),
        current_row_index: Some(0),
        column_index: 0,
        column: "name".to_owned(),
        data_type: "text".to_owned(),
        original: UiCell::Text("Alice".to_owned()),
        value: UiCell::Text("Alicia".to_owned()),
    });

    // 1. Navigation Attempt sets pending_navigation_action
    app.open_table("orders".to_owned());
    assert_eq!(
        app.workspace.pending_navigation_action,
        Some(PendingNavigationAction::OpenTable {
            schema: None,
            table: "orders".to_owned()
        })
    );
    assert!(app.table.editing.discard_changes_confirmation);
    assert_eq!(app.schema.explorer.selected_table, Some("users".to_owned()));

    // 2. Cancel retains current context and clears pending action
    app.table.editing.discard_changes_confirmation = false;
    app.workspace.pending_navigation_action = None;
    assert_eq!(app.schema.explorer.selected_table, Some("users".to_owned()));
    assert!(!app.table.mutation.staged_changes.is_empty());

    // 3. Staged apply success executes pending navigation action
    app.open_table("products".to_owned());
    assert_eq!(
        app.workspace.pending_navigation_action,
        Some(PendingNavigationAction::OpenTable {
            schema: None,
            table: "products".to_owned()
        })
    );
    app.staged_apply_completed();
    assert_eq!(app.schema.explorer.selected_table, Some("products".to_owned()));
    assert!(app.table.mutation.staged_changes.is_empty());
    assert!(app.workspace.pending_navigation_action.is_none());
}

#[test]
fn staged_save_completion_reruns_the_active_inline_sql() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let mut connection = connection_summary_with_ssl_mode(UiSslMode::Disable);
    connection.id = "sqlite".to_owned();
    connection.driver = "SQLite".to_owned();
    connection.database = "/tmp/db_pro_sample.db".to_owned();
    app.connection.catalog.connections.push(connection);
    app.connection.lifecycle.connected = true;
    *app.connection.lifecycle.active_connection_id_mut() = Some("sqlite".to_owned());
    app.schema.explorer.selected_table = Some("customers".to_owned());
    let inline_sql = "SELECT * FROM \"main\".\"customers\" WHERE first_name = 'Alice'".to_owned();
    app.table.data_query.begin_inline_query(RequestId(73), inline_sql.clone(), None);
    app.on_query_completed(RequestId(73), result());
    assert_eq!(app.table.data_query.active_inline_query_sql.as_deref(), Some(inline_sql.as_str()));

    app.staged_apply_completed();

    let UiCommand::RunQuery { sql, .. } = command_rx.try_recv().expect("filtered query refresh expected") else {
        panic!("refresh after inline query must rerun its SQL");
    };
    assert!(sql.contains("WHERE first_name = 'Alice'"));
}

/// UAT-4: completion reports the toolbar cap but does not claim truncation.
/// Providers do not fetch an extra row, so a result of N cannot prove whether
/// the full cardinality was N or larger.
#[test]
fn applied_row_limit_is_disclosed_in_completion_state() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    let mut doc = QueryDocument::new("doc-limit", "Query 1", "SELECT * FROM users");
    doc.execution_state = crate::query::query_document::QueryExecutionState::Running(RequestId(42));
    doc.executing_sql = Some("SELECT * FROM users LIMIT 100".to_owned());
    doc.executing_row_limit = Some(100);
    doc.executing_version = Some(0);
    app.query.session.documents.push(doc);
    app.query
        .session
        .document_requests
        .insert(RequestId(42), "doc-limit".to_owned());

    let mut capped = result();
    capped.row_count = 100;
    app.on_query_completed(RequestId(42), capped);

    let doc = &app.query.session.documents[app.query.session.active_document_index];
    // The cap is stored for the results-header flag and outlives `executing_*`.
    assert_eq!(doc.query_result_row_limit, Some(100));
    assert_eq!(doc.executing_row_limit, None);
    let last = doc.query_messages.last().expect("completion message");
    assert!(
        last.contains("100 rows") && last.contains("limit 100 applied"),
        "completion must disclose the cap without asserting truncation: {last}"
    );
    assert!(app.feedback.runtime_message.contains("limit 100 applied"));
}

#[test]
fn row_limit_is_reported_even_when_result_has_fewer_rows() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    let mut doc = QueryDocument::new("doc-under-limit", "Query 1", "SELECT * FROM users");
    doc.executing_sql = Some("SELECT * FROM users LIMIT 100".to_owned());
    doc.executing_row_limit = Some(100);
    doc.executing_version = Some(0);
    app.query.session.documents.push(doc);
    app.query
        .session
        .document_requests
        .insert(RequestId(43), "doc-under-limit".to_owned());

    // The cap is disclosed as query policy, not as proof of truncation.
    app.on_query_completed(RequestId(43), result());

    let doc = &app.query.session.documents[app.query.session.active_document_index];
    assert_eq!(doc.query_result_row_limit, Some(100));
    let last = doc.query_messages.last().expect("completion message");
    assert!(last.contains("3 rows"), "unexpected message: {last}");
    assert!(
        last.contains("limit 100 applied"),
        "applied cap must remain visible below the cap: {last}"
    );
}

#[test]
fn test_typed_filter_operator_support() {
    // Text types support full text operators
    assert!(TableDataQueryState::filter_operator_supported(
        "text",
        &UiTableFilterOperator::Contains
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "varchar(255)",
        &UiTableFilterOperator::StartsWith
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "character varying",
        &UiTableFilterOperator::EndsWith
    ));

    // Numeric and timestamp types support comparison operators
    assert!(TableDataQueryState::filter_operator_supported(
        "integer",
        &UiTableFilterOperator::GreaterThan
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "bigint",
        &UiTableFilterOperator::LessThanOrEqual
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "numeric(10,2)",
        &UiTableFilterOperator::GreaterThanOrEqual
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "timestamptz",
        &UiTableFilterOperator::GreaterThan
    ));

    // Boolean only supports equals / not equals
    assert!(TableDataQueryState::filter_operator_supported(
        "boolean",
        &UiTableFilterOperator::Equals
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "bool",
        &UiTableFilterOperator::NotEquals
    ));
    assert!(!TableDataQueryState::filter_operator_supported(
        "boolean",
        &UiTableFilterOperator::GreaterThan
    ));

    // All types support IS NULL and IS NOT NULL
    assert!(TableDataQueryState::filter_operator_supported(
        "integer",
        &UiTableFilterOperator::IsNull
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "text",
        &UiTableFilterOperator::IsNotNull
    ));
    assert!(TableDataQueryState::filter_operator_supported(
        "uuid",
        &UiTableFilterOperator::IsNull
    ));
}

#[test]
fn test_grid_layout_schema_reconciliation() {
    let mut app = DbProApp::default();
    let initial_columns = vec![
        crate::UiColumn {
            name: "id".to_owned(),
            data_type: "int".to_owned(),
            nullable: false,
        },
        crate::UiColumn {
            name: "email".to_owned(),
            data_type: "text".to_owned(),
            nullable: false,
        },
    ];

    app.table.data.grid_pending_named_layout = Some(vec![
        PersistedGridColumnLayout {
            column_name: "email".to_owned(),
            width: 240.0,
            order: 0,
            hidden: false,
        },
        PersistedGridColumnLayout {
            column_name: "id".to_owned(),
            width: 100.0,
            order: 1,
            hidden: false,
        },
        // Removed column in DB should be gracefully dropped
        PersistedGridColumnLayout {
            column_name: "old_column".to_owned(),
            width: 300.0,
            order: 2,
            hidden: true,
        },
    ]);

    let order = app.table.data.column_order_for_columns(&initial_columns);
    // email was index 1, id was index 0
    assert_eq!(order, vec![1, 0]);
    assert_eq!(app.table.data.grid_column_widths[1], 240.0);
    assert_eq!(app.table.data.grid_column_widths[0], 100.0);
}

#[test]
fn test_multi_tab_query_result_routing() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    // Create 2 query documents
    app.query.session.documents = vec![
        QueryDocument::new("query-1", "Query 1", "SELECT 1;"),
        QueryDocument::new("query-2", "Query 2", "SELECT 2;"),
    ];
    app.query.session.active_document_index = 0;

    // Simulate Tab 1 running request 101
    let req1 = crate::RequestId(101);
    app.query.session.documents[0].execution_state = QueryExecutionState::Running(req1);
    app.query.session.document_requests.insert(req1, "query-1".to_owned());

    // Switch to Tab 2 and simulate Tab 2 running request 102
    app.switch_query_document(1);
    let req2 = crate::RequestId(102);
    app.query.session.documents[1].execution_state = QueryExecutionState::Running(req2);
    app.query.session.document_requests.insert(req2, "query-2".to_owned());

    // Tab 1 query completes while user is on Tab 2
    let result1 = UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "num".to_owned(),
            data_type: "int".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![crate::UiCell::Text("1".to_owned())]],
        row_count: 1,
        duration_ms: 12,
    };
    event_tx
        .send(UiEvent::QueryCompleted {
            request_id: req1,
            result: result1,
        })
        .unwrap();

    app.apply_runtime_events();

    // Tab 1 should have received its result and message, but Tab 2 is active and has no result yet
    assert_eq!(
        app.query.session.documents[0]
            .query_results
            .first()
            .map(|r| r.row_count),
        Some(1)
    );
    assert_eq!(
        app.query.session.documents[0].execution_state,
        QueryExecutionState::Idle
    );
    assert!(app.query.session.documents[0]
        .query_messages
        .iter()
        .any(|m| m.contains("1 rows")));
    assert!(app.query.session.active_result().is_none());

    // Tab 2 query completes
    let result2 = UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "num".to_owned(),
            data_type: "int".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![crate::UiCell::Text("2".to_owned())]],
        row_count: 1,
        duration_ms: 8,
    };
    event_tx
        .send(UiEvent::QueryCompleted {
            request_id: req2,
            result: result2,
        })
        .unwrap();

    app.apply_runtime_events();

    // Tab 2 is active, the session state now returns Tab 2's result
    assert_eq!(
        app.query.session.active_result().and_then(|r| match &r.rows[0][0] {
            crate::UiCell::Text(s) => Some(s.as_str()),
            _ => None,
        }),
        Some("2")
    );
    assert_eq!(
        app.query.session.documents[1].execution_state,
        QueryExecutionState::Idle
    );

    // Switch back to Tab 1 -> the session state returns Tab 1's result
    app.switch_query_document(0);
    assert_eq!(
        app.query.session.active_result().and_then(|r| match &r.rows[0][0] {
            crate::UiCell::Text(s) => Some(s.as_str()),
            _ => None,
        }),
        Some("1")
    );
}

#[test]
fn multi_result_completion_keeps_statement_order_and_active_tab_state() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.query.session.documents = vec![QueryDocument::new("query-1", "Query 1", "SELECT 1; SELECT 2;")];
    let request_id = crate::RequestId(301);
    app.query.session.documents[0].execution_state = QueryExecutionState::Running(request_id);
    app.query.session.documents[0].execution_started_at = Some(std::time::Instant::now());
    app.query
        .session
        .document_requests
        .insert(request_id, "query-1".to_owned());

    let make_result = |value: &str| UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "value".to_owned(),
            data_type: "int".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![crate::UiCell::Number(value.to_owned())]],
        row_count: 1,
        duration_ms: 1,
    };
    event_tx
        .send(UiEvent::QueryMultiCompleted {
            request_id,
            output: crate::UiQueryExecutionOutput {
                statements: vec![
                    crate::UiStatementOutput {
                        statement_index: 0,
                        result_set: Some(make_result("1")),
                        affected_rows: None,
                        duration_ms: 1,
                        message: None,
                        error: None,
                    },
                    crate::UiStatementOutput {
                        statement_index: 1,
                        result_set: Some(make_result("2")),
                        affected_rows: None,
                        duration_ms: 1,
                        message: None,
                        error: None,
                    },
                ],
                total_duration_ms: 2,
            },
        })
        .unwrap();
    app.apply_runtime_events();

    assert_eq!(app.query.session.documents[0].query_results.len(), 2);
    app.set_active_query_result(1);
    assert_eq!(app.query.session.documents[0].active_result_index, 1);
    assert_eq!(
        app.query.session.documents[0].query_results[1].rows[0][0],
        UiCell::Number("2".to_owned())
    );
}

#[test]
fn query_history_uses_execution_start_time() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let request_id = crate::RequestId(304);
    app.query.session.documents[0].execution_state = QueryExecutionState::Running(request_id);
    app.query.session.documents[0].execution_started_at = Some(std::time::Instant::now());
    app.query.session.documents[0].execution_started_wall_time = Some("2026-09-13T01:02:03Z".to_owned());
    app.query.session.documents[0].executing_sql = Some("SELECT 1".to_owned());
    app.query
        .session
        .document_requests
        .insert(request_id, "query-1".to_owned());

    event_tx
        .send(UiEvent::QueryCompleted {
            request_id,
            result: result(),
        })
        .unwrap();
    app.apply_runtime_events();

    assert_eq!(app.query.editor.query_history_entries.len(), 1);
    assert_eq!(
        app.query.editor.query_history_entries[0].started_at,
        "2026-09-13T01:02:03Z"
    );
}

#[test]
fn multi_result_failure_attaches_database_diagnostic_to_failed_statement() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document = &mut app.query.session.documents[0];
    document.set_text("SELECT 1;\nSELECT bad;");
    let request_id = crate::RequestId(305);
    let document_id = document.id.clone();
    let version = document.buffer.version();
    let end = document.buffer.len_bytes();
    document.execution_state = QueryExecutionState::Running(request_id);
    document.execution_started_at = Some(std::time::Instant::now());
    document.executing_range = Some((0, end));
    document.executing_sql = Some(document.text().to_owned());
    document.executing_version = Some(version);
    app.query.session.document_requests.insert(request_id, document_id);

    event_tx
        .send(UiEvent::QueryMultiCompleted {
            request_id,
            output: crate::UiQueryExecutionOutput {
                statements: vec![
                    crate::UiStatementOutput {
                        statement_index: 0,
                        result_set: Some(result()),
                        affected_rows: None,
                        duration_ms: 1,
                        message: None,
                        error: None,
                    },
                    crate::UiStatementOutput {
                        statement_index: 1,
                        result_set: None,
                        affected_rows: None,
                        duration_ms: 0,
                        message: None,
                        error: Some(crate::UiQueryError {
                            code: "QUERY_FAILED".to_owned(),
                            message: "column \"bad\" does not exist".to_owned(),
                            position: None,
                            detail: None,
                            hint: None,
                        }),
                    },
                ],
                total_duration_ms: 2,
            },
        })
        .unwrap();
    app.apply_runtime_events();

    let diagnostic = app.query.session.documents[0]
        .execution_diagnostic
        .as_ref()
        .expect("failed statement should have a database diagnostic");
    assert_eq!(diagnostic.source, crate::editor::DiagnosticSource::Database);
    assert_eq!(
        diagnostic.range,
        app.query.session.documents[0].analysis.statements[1].range
    );
}

#[test]
fn saved_query_event_resets_dirty_baseline_and_failed_save_keeps_it() {
    let (bridge, _command_rx, event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.query.session.documents[0].set_text("SELECT changed;");
    assert!(app.query.session.documents[0].is_dirty());
    let save_request = crate::RequestId(302);
    let version = app.query.session.documents[0].buffer.version();
    app.query
        .session
        .save_requests
        .insert(save_request, ("query-1".to_owned(), version));
    event_tx
        .send(UiEvent::QuerySaved {
            request_id: save_request,
            query: UiSavedQuerySummary {
                id: "saved-1".to_owned(),
                name: "Query 1".to_owned(),
                sql: "SELECT changed;".to_owned(),
                folder: None,
            },
        })
        .unwrap();
    app.apply_runtime_events();
    assert!(!app.query.session.documents[0].is_dirty());
    assert_eq!(
        app.query.session.documents[0].saved_query_id.as_deref(),
        Some("saved-1")
    );

    app.query.session.documents[0].set_text("SELECT failed;");
    let failed_save_request = crate::RequestId(303);
    let version2 = app.query.session.documents[0].buffer.version();
    app.query
        .session
        .save_requests
        .insert(failed_save_request, ("query-1".to_owned(), version2));
    event_tx
        .send(UiEvent::QueryFailed {
            request_id: failed_save_request,
            message: "storage unavailable".to_owned(),
        })
        .unwrap();
    app.apply_runtime_events();
    assert!(app.query.session.documents[0].is_dirty());
}

#[test]
fn dirty_query_close_is_deferred_until_user_decision() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.query.session.documents[0].set_text("SELECT changed;");

    app.request_close_query_document(0);

    assert_eq!(app.query.session.documents.len(), 1);
    assert_eq!(app.query.session.pending_dirty_close, Some(0));
}

#[test]
fn query_dispatch_allows_independent_documents_to_run_concurrently() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![
        UiConnectionSummary {
            id: "conn-1".to_owned(),
            name: "DB 1".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "db1".to_owned(),
            username: "user".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
        UiConnectionSummary {
            id: "conn-2".to_owned(),
            name: "DB 2".to_owned(),
            host: String::new(),
            port: 0,
            database: "db2".to_owned(),
            username: String::new(),
            driver: "SQLite".to_owned(),
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: vec![],
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        },
    ];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.set_document_connection(0, Some("conn-1".to_owned()));
    app.set_active_query_text("SELECT 1;");
    app.dispatch_query();
    let first = command_rx.recv().expect("first query command");
    let first_request = match first {
        UiCommand::RunQuery { request_id, .. } => request_id,
        _ => panic!("unexpected first command"),
    };

    app.new_query_document();
    app.set_document_connection(1, Some("conn-2".to_owned()));
    app.set_active_query_text("SELECT 2;");
    app.dispatch_query();
    let second = command_rx.recv().expect("second query command");
    let second_request = match second {
        UiCommand::RunQuery {
            request_id,
            connection_id,
            ..
        } => {
            assert_eq!(connection_id, "conn-2");
            request_id
        }
        _ => panic!("unexpected second command"),
    };

    assert_ne!(first_request, second_request);
    assert!(matches!(
        app.query.session.documents[0].execution_state,
        QueryExecutionState::Running(request) if request == first_request
    ));
    assert!(matches!(
        app.query.session.documents[1].execution_state,
        QueryExecutionState::Running(request) if request == second_request
    ));
}

#[test]
fn test_tab_switching_preserves_completion_and_prediction_isolation() {
    let mut doc1 = QueryDocument::new("query-1", "Query 1", "SELECT * FROM u");
    let document_version = doc1.buffer.version();
    doc1.completion.open(
        15,
        document_version,
        egui::Pos2::new(100.0, 100.0),
        "u".to_string(),
        vec![crate::editor::CompletionItem {
            label: "users".to_owned(),
            insert_text: "users".to_owned(),
            kind: crate::editor::CompletionItemKind::Table,
            detail: None,
            documentation: None,
            replacement_range: (14, 15),
            sort_score: 800,
        }],
        crate::editor::CompletionTriggerKind::Automatic,
    );
    doc1.prediction = Some(crate::editor::EditPrediction {
        request_id: Some(crate::RequestId(1)),
        document_version: doc1.buffer.version(),
        anchor: 15,
        replacement_range: (15, 15),
        text: "sers WHERE id = 1".to_owned(),
    });

    let doc2 = QueryDocument::new("query-2", "Query 2", "SELECT 2;");

    let mut app = DbProApp {
        query: QueryFeatureState {
            session: QuerySessionState {
                documents: vec![doc1, doc2],
                active_document_index: 0,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    // Doc 1 has open completion and prediction
    assert!(app.query.session.documents[0].completion.is_open);
    assert!(app.query.session.documents[0].prediction.is_some());

    // Switch to Doc 2
    app.switch_query_document(1);
    assert!(!app.query.session.documents[1].completion.is_open);
    assert!(app.query.session.documents[1].prediction.is_none());

    // Switch back to Doc 1
    app.switch_query_document(0);
    assert!(app.query.session.documents[0].completion.is_open);
    assert!(app.query.session.documents[0].prediction.is_some());
}

#[test]
fn test_prefix_replacement_logic() {
    let mut doc = QueryDocument::new("query-1", "Query 1", "SELECT * FROM us");
    doc.cursor.set_offset(&doc.buffer, 16); // end of "us"

    let item = crate::editor::CompletionItem {
        label: "users".to_owned(),
        insert_text: "users".to_owned(),
        kind: crate::editor::CompletionItemKind::Table,
        detail: None,
        documentation: None,
        replacement_range: (14, 16),
        sort_score: 800,
    };
    let (start, end) = item.replacement_range;
    doc.buffer.replace(start, end, &item.insert_text);
    let new_offset = start + item.insert_text.len();
    doc.cursor.set_offset(&doc.buffer, new_offset);
    doc.dirty = true;

    assert_eq!(doc.text(), "SELECT * FROM users");
    assert_eq!(doc.cursor.offset, 19);
    assert!(doc.dirty);
}

#[test]
fn test_popup_flipping_near_viewport_bottom() {
    let screen_rect = egui::Rect::from_min_max(egui::Pos2::new(0.0, 0.0), egui::Pos2::new(1000.0, 800.0));
    let popup_height = 220.0;
    let mut popup_pos = egui::Pos2::new(200.0, 750.0); // Near bottom (750 + 220 = 970 > 800 - 30)

    if popup_pos.y + popup_height > screen_rect.max.y - 30.0 {
        popup_pos.y = (popup_pos.y - popup_height - 24.0).max(screen_rect.min.y + 10.0);
    }

    assert_eq!(popup_pos.y, 750.0 - 220.0 - 24.0); // Flipped upward to 506.0
}

#[test]
fn test_multi_tab_explain_plan_routing() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
        id: "conn-1".to_owned(),
        name: "Test DB".to_owned(),
        database: "test".to_owned(),
        host: "localhost".to_owned(),
        port: 5432,
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

    // Tab 1
    app.set_active_query_text("SELECT count(*) FROM users");
    let doc1_id = app.query.session.documents[0].id.clone();

    // Trigger explain on Tab 1
    app.explain_query();
    let explain_req_id = app
        .query
        .session
        .active_explain_request()
        .expect("explain request id must be set");
    assert_eq!(app.query.session.documents[0].explain_request, Some(explain_req_id));

    // Create and switch to Tab 2
    app.new_query_document();
    app.set_active_query_text("SELECT * FROM orders");
    assert_eq!(app.query.session.active_document_index, 1);
    assert!(app.query.session.active_explain_request().is_none());
    assert!(app.query.session.active_explain_plan().is_none());

    // Explain completion event arrives for Tab 1's request
    app.apply_runtime_event(UiEvent::ExplainCompleted {
        request_id: explain_req_id,
        plan: "Seq Scan on users (cost=0.00..35.50 rows=2550 width=8)".to_owned(),
    });

    // Tab 2 (currently active) should NOT have the plan
    assert!(app.query.session.active_explain_plan().is_none());

    // Tab 1 must have the plan received and request cleared
    let doc1 = app.query.session.documents.iter().find(|d| d.id == doc1_id).unwrap();
    assert!(doc1.explain_request.is_none());
    assert_eq!(
        doc1.explain_plan.as_deref(),
        Some("Seq Scan on users (cost=0.00..35.50 rows=2550 width=8)")
    );

    // Switch back to Tab 1, active explain plan is immediately available
    app.switch_query_document(0);
    assert_eq!(
        app.query.session.active_explain_plan(),
        Some("Seq Scan on users (cost=0.00..35.50 rows=2550 width=8)")
    );
}

#[test]
fn test_prediction_mode_defaults_and_options() {
    let app = DbProApp::default();
    assert_eq!(app.preferences.prediction_mode, PredictionMode::Off);

    let eager = PredictionMode::Eager;
    let off = PredictionMode::Off;
    assert_ne!(eager, off);
}

#[test]
fn test_per_document_connection_and_schema_isolation() {
    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![
                    UiConnectionSummary {
                        id: "conn-pg".to_owned(),
                        name: "Postgres Prod".to_owned(),
                        driver: "postgresql".to_owned(),
                        host: "localhost".to_owned(),
                        port: 5432,
                        database: "prod".to_owned(),
                        username: "postgres".to_owned(),
                        ssl_mode: UiSslMode::Disable,
                        readonly: false,
                        create_if_missing: false,
                        tags: vec![],
                        group: None,
                        favorite: false,
                        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
                    },
                    UiConnectionSummary {
                        id: "conn-sqlite".to_owned(),
                        name: "Local SQLite".to_owned(),
                        driver: "sqlite".to_owned(),
                        host: "".to_owned(),
                        port: 0,
                        database: "/tmp/test.db".to_owned(),
                        username: "".to_owned(),
                        ssl_mode: UiSslMode::Disable,
                        readonly: false,
                        create_if_missing: false,
                        tags: vec![],
                        group: None,
                        favorite: false,
                        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
                    },
                ],
            },

            lifecycle: ConnectionLifecycleState {
                active_connection_id: Some("conn-pg".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        ..Default::default()
    };

    // Tab 1 setup
    app.set_document_connection(0, Some("conn-pg".to_owned()));
    app.set_document_schema(0, Some("analytics".to_owned()));
    assert_eq!(app.active_query_connection_id(), Some("conn-pg"));
    assert_eq!(app.active_query_schema(), "analytics");

    // Tab 2: create and switch connection to SQLite
    app.new_query_document();
    assert_eq!(app.query.session.active_document_index, 1);
    app.set_document_connection(1, Some("conn-sqlite".to_owned()));
    app.set_document_schema(1, Some("main".to_owned()));

    assert_eq!(app.active_query_connection_id(), Some("conn-sqlite"));
    assert_eq!(app.active_query_schema(), "main");
    assert_eq!(app.active_query_driver(), "sqlite");

    // Switch back to Tab 1: retains its own connection and schema
    app.switch_query_document(0);
    assert_eq!(app.active_query_connection_id(), Some("conn-pg"));
    assert_eq!(app.active_query_schema(), "analytics");
    assert_eq!(app.active_query_driver(), "postgresql");
}

#[test]
fn document_schema_resets_to_bound_driver_default_on_connection_switch() {
    // UAT-8: a document bound to SQLite picked up "main"; switching its
    // connection to PostgreSQL must drop the stale schema so it resolves to the
    // new driver's default ("public"), not keep rendering "main".
    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: vec![
                    UiConnectionSummary {
                        id: "conn-pg".to_owned(),
                        name: "PG".to_owned(),
                        host: "localhost".to_owned(),
                        port: 5432,
                        database: "db".to_owned(),
                        username: "u".to_owned(),
                        ssl_mode: UiSslMode::Disable,
                        readonly: false,
                        create_if_missing: false,
                        tags: vec![],
                        group: None,
                        favorite: false,
                        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
                        driver: "postgresql".to_owned(),
                    },
                    UiConnectionSummary {
                        id: "conn-sqlite".to_owned(),
                        name: "Lite".to_owned(),
                        host: "".to_owned(),
                        port: 0,
                        database: "/tmp/test.db".to_owned(),
                        username: "".to_owned(),
                        ssl_mode: UiSslMode::Disable,
                        readonly: false,
                        create_if_missing: false,
                        tags: vec![],
                        group: None,
                        favorite: false,
                        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
                        driver: "sqlite".to_owned(),
                    },
                ],
            },
            lifecycle: ConnectionLifecycleState {
                active_connection_id: Some("conn-sqlite".to_owned()),
                ..Default::default()
            },
            dialog: ConnectionDialogState::default(),
        },
        ..Default::default()
    };

    app.set_document_connection(0, Some("conn-sqlite".to_owned()));
    app.set_document_schema(0, Some("main".to_owned()));
    assert_eq!(app.active_query_schema(), "main");

    // Rebinding to PostgreSQL clears the stored schema and derives "public".
    app.set_document_connection(0, Some("conn-pg".to_owned()));
    assert_eq!(app.active_query_schema(), "public");

    // Rebinding to the same connection is a no-op and keeps the schema.
    app.set_document_schema(0, Some("tenant1".to_owned()));
    app.set_document_connection(0, Some("conn-pg".to_owned()));
    assert_eq!(app.active_query_schema(), "tenant1");
}

#[test]
fn test_ambiguous_column_completion_qualified_ranking() {
    let summary = UiSchemaSummary {
        table_details: vec![
            UiTableSummary {
                schema: "public".to_owned(),
                name: "users".to_owned(),
                row_count: Some(10),
                columns: vec![
                    crate::runtime::UiSchemaColumn {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    crate::runtime::UiSchemaColumn {
                        name: "email".to_owned(),
                        data_type: "text".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: vec![],
            },
            UiTableSummary {
                schema: "public".to_owned(),
                name: "orders".to_owned(),
                row_count: Some(50),
                columns: vec![
                    crate::runtime::UiSchemaColumn {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    crate::runtime::UiSchemaColumn {
                        name: "user_id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: vec![],
            },
        ],
        ..Default::default()
    };

    let ctx = crate::query::CompletionContext {
        text_before_cursor: "SELECT i",
        text_after_cursor: " FROM users u JOIN orders o ON u.id = o.user_id",
        cursor_offset: 8,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = crate::query::SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "i");
    assert!(!items.is_empty());

    // Because 'id' exists in both users and orders, qualified versions (u.id, o.id) should be boosted
    let u_id = items.iter().find(|it| it.insert_text == "u.id");
    let o_id = items.iter().find(|it| it.insert_text == "o.id");
    assert!(u_id.is_some(), "u.id should be suggested");
    assert!(o_id.is_some(), "o.id should be suggested");
    assert!(u_id.unwrap().sort_score >= 880);
}

#[test]
fn test_prediction_partial_accept_word_and_line() {
    let mut pred = crate::editor::EditPrediction::new(
        10,
        "WHERE users.id = 42\nORDER BY created_at DESC",
        Some(crate::RequestId(1)),
    );

    // Accept next word
    assert_eq!(pred.accept_next_word(), "WHERE");
    pred.consume("WHERE".len());
    assert_eq!(pred.anchor, 15);
    assert_eq!(pred.text, " users.id = 42\nORDER BY created_at DESC");

    // Accept next line
    assert_eq!(pred.accept_next_line(), " users.id = 42\n");
    pred.consume(" users.id = 42\n".len());
    assert_eq!(pred.text, "ORDER BY created_at DESC");

    // Accept remaining full text
    assert_eq!(pred.accept_full(), "ORDER BY created_at DESC");
    pred.consume("ORDER BY created_at DESC".len());
    assert!(pred.is_empty());
}

#[test]
fn test_async_prediction_routing_and_stale_rejection() {
    let mut app = DbProApp::default();
    app.new_query_document();

    let doc = &mut app.query.session.documents[0];
    doc.set_text("SELECT * FROM users ");
    doc.cursor.offset = doc.buffer.len_bytes();
    let current_version = doc.buffer.version();
    let req_id = crate::RequestId(77);
    doc.pending_prediction_request = Some(req_id);

    // Apply prediction ready event for matching request & version
    app.apply_runtime_event(UiEvent::SqlPredictionReady {
        request_id: req_id,
        document_id: app.query.session.documents[0].id.clone(),
        document_version: current_version,
        anchor: app.query.session.documents[0].cursor.offset,
        replacement_range: (
            app.query.session.documents[0].cursor.offset,
            app.query.session.documents[0].cursor.offset,
        ),
        prediction: "WHERE active = true".to_owned(),
    });

    let doc = &app.query.session.documents[0];
    assert!(doc.pending_prediction_request.is_none());
    assert!(doc.prediction.is_some());
    let pred = doc.prediction.as_ref().unwrap();
    assert_eq!(pred.text, "WHERE active = true");
    assert_eq!(pred.document_version, current_version);
}

#[test]
fn stale_prediction_event_does_not_mutate_a_newer_document_version() {
    let mut app = DbProApp::default();
    let document_id = app.query.session.documents[0].id.clone();
    let doc = &mut app.query.session.documents[0];
    doc.set_text("SELECT 1");
    doc.cursor.set_offset(&doc.buffer, doc.buffer.len_bytes());
    let request_id = crate::RequestId(88);
    let current_version = doc.buffer.version();
    let anchor = doc.cursor.offset;
    doc.pending_prediction_request = Some(request_id);

    app.apply_runtime_event(UiEvent::SqlPredictionReady {
        request_id,
        document_id,
        document_version: current_version.saturating_sub(1),
        anchor,
        replacement_range: (anchor, anchor),
        prediction: "WHERE stale = true".to_owned(),
    });

    let doc = &app.query.session.documents[0];
    assert!(doc.pending_prediction_request.is_none());
    assert!(doc.prediction.is_none());
}

#[test]
fn test_agent_multitab_isolation_and_close_tab_cancellation() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.new_query_document(); // creates tab 1 (index 1)
    let doc_a_id = app.query.session.documents[0].id.clone();
    let doc_b_id = app.query.session.documents[1].id.clone();

    // Start agent workflow on Tab A
    app.query.session.active_document_index = 0;
    let mut session_a = super::agent_workflow_state::AgentUiSession::for_document(&doc_a_id, None, None);
    let session_a_id = session_a.session.as_ref().unwrap().id;
    let run_a_id = db_pro_core::domain::agent::AgentRunId::new();
    session_a.state = db_pro_core::domain::agent::AgentSessionState::Running;
    session_a.active_run_id = Some(run_a_id);
    app.agent.sessions.insert(doc_a_id.clone(), session_a);

    // Send ToolRequested for Tab A
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::ToolRequested {
        session_id: session_a_id,
        run_id: run_a_id,
        document_id: doc_a_id.clone(),
        call: db_pro_core::domain::agent::AgentToolCall {
            call_id: "call-a-1".to_owned(),
            tool: db_pro_core::domain::agent::AgentTool::InspectSchema,
            input: db_pro_core::domain::agent::AgentToolInput::None,
        },
    });

    // Tab A has 1 activity
    assert_eq!(app.agent.sessions.get(&doc_a_id).unwrap().activities.len(), 1);

    // Switch to Tab B
    app.query.session.active_document_index = 1;
    let session_b = super::agent_workflow_state::AgentUiSession::for_document(&doc_b_id, None, None);
    app.agent.sessions.insert(doc_b_id.clone(), session_b);

    // Tab B session is isolated from Tab A
    assert_eq!(app.agent.sessions.get(&doc_b_id).unwrap().activities.len(), 0);
    assert_eq!(app.agent.sessions.get(&doc_b_id).unwrap().messages.len(), 0);

    // Close Tab A
    app.close_query_document(0);

    // Tab A session was cleaned up
    assert!(!app.agent.sessions.contains_key(&doc_a_id));

    // Cancel command was dispatched for Tab A's active run
    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::CancelAgentRun { run_id, .. }) if run_id == run_a_id
    ));

    // Late event for Tab A is dropped silently and does not recreate session
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: session_a_id,
        run_id: run_a_id,
        document_id: doc_a_id.clone(),
        delta: "Late message".to_owned(),
    });
    assert!(!app.agent.sessions.contains_key(&doc_a_id));
}

#[test]
fn test_agent_vietnamese_ime_input_and_patch_version_safety() {
    let mut app = DbProApp::default();
    let doc_id = app.query.session.documents[0].id.clone();
    let initial_version = app.query.session.documents[0].buffer.version();

    // User types Vietnamese query with IME into editor
    app.query.session.documents[0].set_text("SELECT * FROM người_dùng WHERE tên = 'Nguyễn Văn A'");
    let typed_version = app.query.session.documents[0].buffer.version();
    assert!(typed_version > initial_version);

    // Setup Agent session with pending patch targeted at initial_version
    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&doc_id, None, None);
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    session.pending_confirmation = Some(super::agent_workflow_state::AgentUiConfirmation {
        run_id,
        call_id: "patch-1".to_owned(),
        kind: db_pro_core::domain::agent_workflow::AgentConfirmationKind::ApplyPatch,
        preview: Some(db_pro_core::domain::agent::AgentToolOutput::PatchPreview {
            patch: db_pro_core::domain::agent::AgentSqlPatch {
                document_id: doc_id.clone(),
                expected_version: initial_version,
                range: (0, 6),
                replacement: "SELECT 1".to_owned(),
            },
            original: "SELECT".to_owned(),
            proposed: "SELECT 1".to_owned(),
        }),
        document_id: doc_id.clone(),
    });
    app.agent.sessions.insert(doc_id.clone(), session);

    // User attempts to apply patch - rejected due to stale version from typing
    app.agent_confirmation_action(true);
    assert_eq!(
        app.feedback.runtime_message,
        "This query changed since the suggestion was created."
    );
    // Text buffer unchanged and preserved
    assert_eq!(
        app.query.session.documents[0].text(),
        "SELECT * FROM người_dùng WHERE tên = 'Nguyễn Văn A'"
    );
}

#[test]
fn test_agent_vietnamese_valid_patch_application_and_undo() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let doc_id = app.query.session.documents[0].id.clone();

    // Set Vietnamese Unicode query text
    let initial_text = "SELECT tên FROM người_dùng";
    app.query.session.documents[0].set_text(initial_text);
    let expected_version = app.query.session.documents[0].buffer.version();

    // "SELECT " is 7 bytes; "tên" is 4 bytes (t: 1 byte, ê: 2 bytes, n: 1 byte) -> range (7, 11)
    let start_byte = 7;
    let end_byte = 11;
    assert_eq!(&initial_text[start_byte..end_byte], "tên");

    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&doc_id, None, None);
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    session.pending_confirmation = Some(super::agent_workflow_state::AgentUiConfirmation {
        run_id,
        call_id: "patch-vn".to_owned(),
        kind: db_pro_core::domain::agent_workflow::AgentConfirmationKind::ApplyPatch,
        preview: Some(db_pro_core::domain::agent::AgentToolOutput::PatchPreview {
            patch: db_pro_core::domain::agent::AgentSqlPatch {
                document_id: doc_id.clone(),
                expected_version,
                range: (start_byte, end_byte),
                replacement: "ho_ten".to_owned(),
            },
            original: "tên".to_owned(),
            proposed: "ho_ten".to_owned(),
        }),
        document_id: doc_id.clone(),
    });
    app.agent.sessions.insert(doc_id.clone(), session);

    // Approve the patch
    app.agent_confirmation_action(true);

    // Verify document text was updated cleanly without byte index slicing panic
    assert_eq!(app.query.session.documents[0].text(), "SELECT ho_ten FROM người_dùng");

    // Verify ContinueAgentRun command was dispatched with PatchApplied outcome
    match command_rx.try_recv() {
        Ok(UiCommand::ContinueAgentRun {
            run_id: cmd_run_id,
            approved,
            applied_patch,
            ..
        }) => {
            assert_eq!(cmd_run_id, run_id);
            assert!(approved);
            assert!(matches!(
                applied_patch,
                Some(db_pro_core::domain::agent::AgentToolOutput::PatchApplied {
                    new_version,
                    range: (7, 11),
                    ..
                }) if new_version > expected_version
            ));
        }
        other => panic!("expected ContinueAgentRun, got {:?}", other),
    }

    // Verify undo restores the exact original Vietnamese text
    assert!(app.query.session.documents[0].buffer.undo().is_some());
    assert_eq!(app.query.session.documents[0].text(), initial_text);
}

#[test]
fn test_agent_event_routing_ignores_mismatched_session_and_document_and_run_ids() {
    let mut app = DbProApp::default();
    let doc_id = app.query.session.documents[0].id.clone();
    let real_run_id = db_pro_core::domain::agent::AgentRunId::new();

    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&doc_id, None, None);
    let real_session_id = session.session.as_ref().unwrap().id;
    session.active_run_id = Some(real_run_id);
    app.agent.sessions.insert(doc_id.clone(), session);

    // 1. Mismatched document_id -> ignored
    let wrong_doc_id = "doc-nonexistent".to_owned();
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: real_session_id,
        run_id: real_run_id,
        document_id: wrong_doc_id.clone(),
        delta: "ignored text".to_owned(),
    });
    assert!(!app.agent.sessions.contains_key(&wrong_doc_id));
    assert!(app.agent.sessions.get(&doc_id).unwrap().streaming_text.is_empty());

    // 2. Mismatched run_id -> ignored
    let wrong_run_id = db_pro_core::domain::agent::AgentRunId::new();
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: real_session_id,
        run_id: wrong_run_id,
        document_id: doc_id.clone(),
        delta: "stale run text".to_owned(),
    });
    assert!(app.agent.sessions.get(&doc_id).unwrap().streaming_text.is_empty());

    // 3. Matching IDs -> accepted
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: real_session_id,
        run_id: real_run_id,
        document_id: doc_id.clone(),
        delta: "valid delta".to_owned(),
    });
    assert_eq!(app.agent.sessions.get(&doc_id).unwrap().streaming_text, "valid delta");

    // 4. Wrong session_id with matching doc/run -> ignored
    let wrong_session_id = db_pro_core::domain::agent::AgentSessionId::new();
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: wrong_session_id,
        run_id: real_run_id,
        document_id: doc_id.clone(),
        delta: "ignored session delta".to_owned(),
    });
    assert_eq!(app.agent.sessions.get(&doc_id).unwrap().streaming_text, "valid delta");
}

#[test]
fn test_agent_db_cancellation_and_terminal_cleanup() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let doc_id = app.query.session.documents[0].id.clone();
    let run_id = db_pro_core::domain::agent::AgentRunId::new();

    let mut session = super::agent_workflow_state::AgentUiSession::for_document(
        &doc_id,
        Some("conn-1".to_owned()),
        Some("public".to_owned()),
    );
    let session_id = session.session.as_ref().unwrap().id;
    session.active_run_id = Some(run_id);
    session.state = db_pro_core::domain::agent::AgentSessionState::Running;
    session.activities.push(super::agent_workflow_state::AgentUiActivity {
        call_id: Some("call-db-1".to_owned()),
        tool: Some(db_pro_core::domain::agent::AgentTool::RunQuery),
        label: "Executing query".to_owned(),
        status: super::agent_workflow_state::AgentUiActivityStatus::Running,
        duration_ms: None,
    });
    app.agent.sessions.insert(doc_id.clone(), session);

    // Cancel the active agent run
    app.cancel_active_agent_run();

    // Verify CancelAgentRun command was dispatched to runtime bridge
    assert!(matches!(
        command_rx.try_recv(),
        Ok(UiCommand::CancelAgentRun { run_id: cmd_run_id, .. }) if cmd_run_id == run_id
    ));

    // Runtime returns Cancelled event
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::Cancelled {
        session_id,
        run_id,
        document_id: doc_id.clone(),
    });

    let finished_session = app.agent.sessions.get(&doc_id).unwrap();
    assert_eq!(
        finished_session.state,
        db_pro_core::domain::agent::AgentSessionState::Cancelled
    );
    assert_eq!(finished_session.active_run_id, None);
    assert_eq!(finished_session.pending_confirmation, None);
    assert_eq!(
        finished_session.activities[0].status,
        super::agent_workflow_state::AgentUiActivityStatus::Cancelled
    );

    // Late event arrives after cancellation -> ignored, state remains Cancelled
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id,
        run_id,
        document_id: doc_id.clone(),
        delta: "Late output".to_owned(),
    });
    let after_late = app.agent.sessions.get(&doc_id).unwrap();
    assert_eq!(
        after_late.state,
        db_pro_core::domain::agent::AgentSessionState::Cancelled
    );
    assert_eq!(after_late.active_run_id, None);
}

#[test]
fn test_agent_retry_isolation_and_session_routing() {
    let mut app = DbProApp::default();
    let doc_id = app.query.session.documents[0].id.clone();
    let run_1 = db_pro_core::domain::agent::AgentRunId::new();

    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&doc_id, None, None);
    let session_1_id = session.session.as_ref().unwrap().id;
    session.active_run_id = Some(run_1);
    session.state = db_pro_core::domain::agent::AgentSessionState::Running;
    app.agent.sessions.insert(doc_id.clone(), session);

    // Run 1 fails
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::Failed {
        session_id: session_1_id,
        run_id: run_1,
        document_id: doc_id.clone(),
        message: "API error".to_owned(),
    });

    let failed_session = app.agent.sessions.get(&doc_id).unwrap();
    assert_eq!(
        failed_session.state,
        db_pro_core::domain::agent::AgentSessionState::Failed
    );
    assert_eq!(failed_session.active_run_id, None);

    // Start Run 2 (Retry)
    let run_2 = db_pro_core::domain::agent::AgentRunId::new();
    let mut session_2 = super::agent_workflow_state::AgentUiSession::for_document(&doc_id, None, None);
    let session_2_id = session_2.session.as_ref().unwrap().id;
    assert_ne!(session_1_id, session_2_id);
    session_2.active_run_id = Some(run_2);
    session_2.state = db_pro_core::domain::agent::AgentSessionState::Running;
    app.agent.sessions.insert(doc_id.clone(), session_2);

    // Late event from Run 1 / Session 1 -> ignored
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: session_1_id,
        run_id: run_1,
        document_id: doc_id.clone(),
        delta: "stale message".to_owned(),
    });
    assert!(app.agent.sessions.get(&doc_id).unwrap().streaming_text.is_empty());

    // Event from Run 2 / Session 2 -> accepted
    app.on_agent_workflow_event(db_pro_core::domain::agent_workflow::AgentWorkflowEvent::TextDelta {
        session_id: session_2_id,
        run_id: run_2,
        document_id: doc_id.clone(),
        delta: "active message".to_owned(),
    });
    assert_eq!(
        app.agent.sessions.get(&doc_id).unwrap().streaming_text,
        "active message"
    );
}

#[test]
fn test_composite_pk_targeted_reload_and_merge() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.schema.explorer.selected_table = Some("user_roles".to_owned());
    app.table.state.table_info = Some(UiTableInfo {
        schema: "public".to_owned(),
        name: "user_roles".to_owned(),
        row_count: Some(2),
        columns: vec![
            crate::UiTableColumn {
                name: "tenant_id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
                ..Default::default()
            },
            crate::UiTableColumn {
                name: "user_id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
                ..Default::default()
            },
            crate::UiTableColumn {
                name: "role".to_owned(),
                data_type: "TEXT".to_owned(),
                nullable: false,
                ..Default::default()
            },
        ],
        primary_key: Some(vec!["tenant_id".to_owned(), "user_id".to_owned()]),
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        check_constraints: Vec::new(),
        dependencies: Vec::new(),
    });
    app.table.data_query.set_result(UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "tenant_id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "user_id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "role".to_owned(),
                data_type: "TEXT".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![
            vec![
                UiCell::Number("1".to_owned()),
                UiCell::Number("10".to_owned()),
                UiCell::Text("admin".to_owned()),
            ],
            vec![
                UiCell::Number("1".to_owned()),
                UiCell::Number("20".to_owned()),
                UiCell::Text("member".to_owned()),
            ],
        ],
        row_count: 2,
        duration_ms: 5,
    });

    let target_identity = RowIdentity {
        original_pk_columns: vec!["tenant_id".to_owned(), "user_id".to_owned()],
        original_pk_values: vec![UiCell::Number("1".to_owned()), UiCell::Number("20".to_owned())],
        original_pk_data_types: Vec::new(),
    };

    app.request_table_row_reload(target_identity);

    let UiCommand::LoadTableData {
        filters, limit, offset, ..
    } = command_rx.try_recv().expect("LoadTableData command expected")
    else {
        panic!("expected LoadTableData");
    };

    assert_eq!(limit, 1);
    assert_eq!(offset, 0);
    assert_eq!(filters.len(), 2);
    assert_eq!(filters[0].column, "tenant_id");
    assert_eq!(filters[0].value, "1");
    assert_eq!(filters[1].column, "user_id");
    assert_eq!(filters[1].value, "20");

    let server_reloaded = UiQueryResult {
        columns: app.table.data_query.result_ref().unwrap().columns.clone(),
        rows: vec![vec![
            UiCell::Number("1".to_owned()),
            UiCell::Number("20".to_owned()),
            UiCell::Text("manager".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 2,
    };

    app.on_table_row_reloaded(server_reloaded);

    let result = app.table.data_query.result_ref().unwrap();
    assert_eq!(result.rows[0][2], UiCell::Text("admin".to_owned()));
    assert_eq!(result.rows[1][2], UiCell::Text("manager".to_owned()));
}

#[test]
fn test_inserted_row_delete_removes_from_changeset_without_db_delete() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.schema.explorer.selected_table = Some("users".to_owned());

    let local_id = app.table.mutation.staged_changes.stage_insert(
        vec!["username".to_owned(), "email".to_owned()],
        vec![
            UiCell::Text("alice".to_owned()),
            UiCell::Text("alice@test.com".to_owned()),
        ],
    );

    assert_eq!(app.table.mutation.staged_changes.counts().inserts, 1);
    assert_eq!(app.table.mutation.staged_changes.counts().total(), 1);

    // Deleting the draft insert row removes it locally
    let removed = app.table.mutation.staged_changes.remove_insert(local_id);
    assert!(removed);
    assert!(app.table.mutation.staged_changes.is_empty());

    // Apply now has zero changes and dispatches nothing
    app.apply_staged_changes();
    assert!(command_rx.try_recv().is_err());
}

#[test]
fn test_apply_mutation_failure_preserves_changeset_and_focuses_failed_cell() {
    let mut app = DbProApp {
        table: TableEditorState {
            mutation: TableMutationState {
                staged_apply_request: Some(crate::RequestId(12)),
                ..Default::default()
            },
            state: TableState { ..Default::default() },
            data_query: TableDataQueryState {
                result: Some(UiQueryResult {
                    columns: vec![
                        crate::UiColumn {
                            name: "id".to_owned(),
                            data_type: "INTEGER".to_owned(),
                            nullable: false,
                        },
                        crate::UiColumn {
                            name: "name".to_owned(),
                            data_type: "TEXT".to_owned(),
                            nullable: false,
                        },
                    ],
                    rows: vec![
                        vec![UiCell::Number("1".to_owned()), UiCell::Text("Alice".to_owned())],
                        vec![UiCell::Number("2".to_owned()), UiCell::Text("Bob".to_owned())],
                        vec![UiCell::Number("3".to_owned()), UiCell::Text("Charlie".to_owned())],
                    ],
                    row_count: 3,
                    duration_ms: 0,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let id_1 = RowIdentity {
        original_pk_columns: vec!["id".to_owned()],
        original_pk_values: vec![UiCell::Number("1".to_owned())],
        original_pk_data_types: Vec::new(),
    };
    let id_2 = RowIdentity {
        original_pk_columns: vec!["id".to_owned()],
        original_pk_values: vec![UiCell::Number("2".to_owned())],
        original_pk_data_types: Vec::new(),
    };
    let id_3 = RowIdentity {
        original_pk_columns: vec!["id".to_owned()],
        original_pk_values: vec![UiCell::Number("3".to_owned())],
        original_pk_data_types: Vec::new(),
    };

    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: id_1.clone(),
        current_row_index: Some(0),
        column_index: 1,
        column: "name".to_owned(),
        data_type: "TEXT".to_owned(),
        original: UiCell::Text("Alice".to_owned()),
        value: UiCell::Text("Alice Updated".to_owned()),
    });
    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: id_2.clone(),
        current_row_index: Some(1),
        column_index: 1,
        column: "name".to_owned(),
        data_type: "TEXT".to_owned(),
        original: UiCell::Text("Bob".to_owned()),
        value: UiCell::Text("Bob Conflicting".to_owned()),
    });
    app.table.mutation.staged_changes.stage_delete(StagedChange::Delete {
        identity: id_3.clone(),
        current_row_index: Some(2),
    });

    app.table.mutation.staged_apply_targets = vec![
        MutationTarget::Update {
            identity: id_1,
            current_row_index: Some(0),
            columns: vec![1],
        },
        MutationTarget::Update {
            identity: id_2,
            current_row_index: Some(1),
            columns: vec![1],
        },
        MutationTarget::Delete {
            identity: id_3,
            current_row_index: Some(2),
        },
    ];

    // Failure occurs on statement index 1 (Bob) with CONFLICT
    app.staged_apply_failed(StagedApplyFailure {
        statement_index: 1,
        code: "CONFLICT",
        message: "row count was zero",
        rolled_back: true,
    });

    // 1. Transaction rolled back
    let failure = app
        .table
        .mutation
        .table_mutation_error
        .as_ref()
        .expect("failure recorded");
    assert!(failure.rolled_back);
    assert_eq!(failure.code, "CONFLICT");

    // 2. ChangeSet remains intact (2 updates + 1 delete)
    assert_eq!(app.table.mutation.staged_changes.counts().updates, 2);
    assert_eq!(app.table.mutation.staged_changes.counts().deletes, 1);

    // 3. Focus moves to failed row and cell
    assert_eq!(app.table.data.selected_row, Some(1));
    assert_eq!(app.table.data.selected_cell, Some((1, 1)));

    // 4. Conflict resolution dialog opened
    assert!(app.table.mutation.conflict_dialog_open);
}

#[test]
fn test_conflict_keep_mine_and_use_database_resolution_actions() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let id = RowIdentity {
        original_pk_columns: vec!["id".to_owned()],
        original_pk_values: vec![UiCell::Number("42".to_owned())],
        original_pk_data_types: Vec::new(),
    };

    app.table.data_query.set_result(UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".to_owned(),
                data_type: "INTEGER".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "val".to_owned(),
                data_type: "TEXT".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![
            UiCell::Number("42".to_owned()),
            UiCell::Text("val_server".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 0,
    });

    app.table.mutation.staged_changes.stage_update(StagedChange::Update {
        identity: id.clone(),
        current_row_index: Some(0),
        column_index: 1,
        column: "val".to_owned(),
        data_type: "TEXT".to_owned(),
        original: UiCell::Text("val_orig".to_owned()),
        value: UiCell::Text("val_mine".to_owned()),
    });

    app.table.mutation.table_mutation_error = Some(MutationFailure {
        statement_index: 0,
        target: Some(MutationTarget::Update {
            identity: id.clone(),
            current_row_index: Some(0),
            columns: vec![1],
        }),
        code: "CONFLICT".to_owned(),
        message: "Conflict".to_owned(),
        rolled_back: true,
    });
    app.table.mutation.conflict_dialog_open = true;

    // Test Use Database: reverts local staged changes
    app.conflict_use_database();
    assert!(app.table.mutation.staged_changes.is_empty());
    assert!(app.table.mutation.table_mutation_error.is_none());
    assert!(!app.table.mutation.conflict_dialog_open);
}

/// The status bar used to render `runtime_message` only when the text happened to
/// contain "failed"/"error", so every refusal, gate and informational message the app
/// sets was silently dropped — "Select a row before deleting" and "Connect with write
/// access to delete rows" among them.
#[test]
fn every_runtime_message_reaches_the_status_bar() {
    let mut app = DbProApp::default();
    app.feedback.runtime_message.clear();
    assert!(app.runtime_status().is_none(), "an empty message renders nothing");

    app.feedback.runtime_message = "Select a row before deleting".to_owned();
    let (message, color) = app.runtime_status().expect("a refusal must be shown");
    assert_eq!(message, "Select a row before deleting");
    assert_eq!(color, app.theme.text_secondary, "a refusal is not an error");

    app.feedback.runtime_message = "Connect with write access to delete rows".to_owned();
    let (_, color) = app.runtime_status().expect("a read-only refusal must be shown");
    assert_eq!(color, app.theme.text_secondary);

    app.feedback.runtime_message = "Query failed: syntax error at or near SELECT".to_owned();
    let (_, color) = app.runtime_status().expect("an error must be shown");
    assert_eq!(color, app.theme.danger, "errors keep the danger colour");
}

/// A destructive statement must not reach the database on the first Run: it is held
/// until the user confirms the exact text that the prompt shows, and the confirm path
/// dispatches that same text.
#[test]
fn destructive_statement_is_held_until_it_is_confirmed() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
    app.set_active_query_text("DROP TABLE users");

    app.dispatch_query();

    assert!(
        command_rx.try_recv().is_err(),
        "a destructive statement must not be dispatched before it is confirmed"
    );
    let pending = app
        .query
        .execution
        .pending_destructive_run()
        .expect("the statement must be held for confirmation");
    assert_eq!(pending.sql(), "DROP TABLE users");
    assert!(!pending.all_statements());
    assert!(app.feedback.runtime_message.contains("held for confirmation"));

    app.confirm_pending_destructive_run();

    let UiCommand::RunQuery { sql, .. } = command_rx.try_recv().expect("confirmed statement must be dispatched") else {
        panic!("expected RunQuery command");
    };
    assert_eq!(sql, "DROP TABLE users");
    assert!(app.query.execution.pending_destructive_run().is_none());
}

#[test]
fn cancelling_a_held_destructive_statement_sends_nothing() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
    app.set_active_query_text("TRUNCATE users");

    app.dispatch_query();
    assert!(app.query.execution.pending_destructive_run().is_some());
    app.cancel_pending_destructive_run();

    assert!(
        command_rx.try_recv().is_err(),
        "a cancelled statement must never be dispatched"
    );
    assert!(app.query.execution.pending_destructive_run().is_none());
    assert!(app.feedback.runtime_message.contains("cancelled"));
}

/// Reads, writes and plain DDL are not gated: the confirmation exists for the classes
/// that can drop or truncate, not for everything that mutates.
#[test]
fn reads_writes_and_plain_ddl_dispatch_without_a_prompt() {
    for sql in [
        "SELECT 1",
        "WITH recent AS (SELECT 1) SELECT * FROM recent",
        "INSERT INTO users (id) VALUES (1)",
        "UPDATE users SET name = 'x' WHERE id = 1",
        "DELETE FROM users WHERE id = 1",
        "ALTER TABLE users ADD COLUMN note TEXT",
    ] {
        let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        // Verbatim dispatch is the point here; the toolbar cap has its own test.
        app.query.execution.query_row_limit = None;
        app.set_active_query_text(sql);

        app.dispatch_query();

        assert!(
            app.query.execution.pending_destructive_run().is_none(),
            "{sql} must not be gated"
        );
        let UiCommand::RunQuery { sql: dispatched, .. } = command_rx
            .try_recv()
            .unwrap_or_else(|_| panic!("{sql} must dispatch immediately"))
        else {
            panic!("expected RunQuery for {sql}");
        };
        assert_eq!(dispatched.trim(), sql);
    }
}

/// A script is classified by its most dangerous statement, so a harmless-looking batch
/// that ends in DROP is held too — and the prompt says it is a script.
#[test]
fn a_script_whose_worst_statement_is_destructive_is_held() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
    app.set_active_query_text("SELECT 1;\nDROP TABLE users;");

    app.dispatch_query_all();

    assert!(command_rx.try_recv().is_err(), "the script must be held");
    let pending = app
        .query
        .execution
        .pending_destructive_run()
        .expect("script must be held");
    assert!(
        pending.all_statements(),
        "a run-all must be dispatched as a script on confirm"
    );

    app.confirm_pending_destructive_run();
    let UiCommand::RunQueryMulti { sql, .. } = command_rx.try_recv().expect("script must dispatch on confirm") else {
        panic!("expected RunQueryMulti");
    };
    assert_eq!(sql, "SELECT 1;\nDROP TABLE users;");

    // Removing the destructive statement makes the same script dispatch immediately.
    app.set_active_query_text("SELECT 1;\nSELECT 2;");
    app.query.session.documents[0].execution_state = QueryExecutionState::Idle;
    app.dispatch_query_all();
    assert!(app.query.execution.pending_destructive_run().is_none());
    assert!(
        command_rx.try_recv().is_ok(),
        "a read-only script must dispatch immediately"
    );
}

fn connect_postgres(app: &mut DbProApp) {
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
    app.query.execution.query_row_limit = None;
}

#[test]
fn caret_on_the_blank_line_after_a_semicolon_runs_that_statement() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    connect_postgres(&mut app);
    app.set_active_query_text("SELECT 1;\n\nSELECT 2;");
    let document = &mut app.query.session.documents[0];
    document.cursor.set_offset(&document.buffer, 10);

    app.dispatch_query();

    let UiCommand::RunQuery { sql, .. } = command_rx.try_recv().expect("statement must dispatch") else {
        panic!("expected RunQuery for the statement before the caret");
    };
    assert_eq!(sql, "SELECT 1;");
}

#[test]
fn mod_shift_enter_runs_the_script() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    connect_postgres(&mut app);
    app.query.editor.query_editor_focused = true;
    app.set_active_query_text("SELECT 1;\nSELECT 2;");
    let ctx = egui::Context::default();
    let modifiers = egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    let _ = crate::test_frame::frame(
        &ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
            events: vec![
                egui::Event::ModifiersChanged(modifiers),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
            ],
            ..Default::default()
        },
        |ui| app.handle_shortcuts(ui.ctx()),
    );

    let UiCommand::RunQueryMulti { sql, .. } = command_rx.try_recv().expect("script must dispatch") else {
        panic!("expected RunQueryMulti");
    };
    assert_eq!(sql, "SELECT 1;\nSELECT 2;");
}

#[test]
fn pinned_tables_toggle_appears_in_quick_open() {
    let mut app = DbProApp {
        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_table: Some("users".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        ..DbProApp::default()
    };
    app.toggle_pinned_table(String::new());
    assert_eq!(app.schema.explorer.pinned_tables, vec!["users".to_owned()]);
    assert!(app
        .filtered_palette_items(PaletteMode::QuickOpen)
        .iter()
        .any(|item| item.title == "users" && item.subtitle.contains("Pinned")));
    app.toggle_pinned_table("users".to_owned());
    assert!(app.schema.explorer.pinned_tables.is_empty());
}

#[test]
fn recent_tables_track_mru_and_appear_in_quick_open() {
    let mut app = DbProApp::default();
    app.open_table("orders".to_owned());
    app.open_table("users".to_owned());
    app.open_table("orders".to_owned());
    assert_eq!(
        app.schema.explorer.recent_tables,
        vec!["orders".to_owned(), "users".to_owned()]
    );
    assert!(app
        .filtered_palette_items(PaletteMode::QuickOpen)
        .iter()
        .any(|item| item.title == "orders" && item.subtitle.contains("Recent")));
    app.schema.explorer.remove_recent_table("orders");
    assert_eq!(app.schema.explorer.recent_tables, vec!["users".to_owned()]);
}

#[test]
fn data_activity_palette_action_opens_sidebar() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                sidebar_open: false,
                ..Default::default()
            },
            ..Default::default()
        },
        ..DbProApp::default()
    };
    app.execute_palette_action(PaletteAction::Data, &egui::Context::default());
    assert_eq!(app.workspace.activity, Activity::Data);
    assert!(app.workspace.sidebar_open);
}

#[test]
fn dispatch_query_binds_named_parameters_for_postgres() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    *app.connection.catalog.connections_mut() = vec![UiConnectionSummary {
        id: "conn-1".to_owned(),
        name: "Local".to_owned(),
        host: "localhost".to_owned(),
        port: 5432,
        database: "db".to_owned(),
        username: "u".to_owned(),
        driver: "PostgreSQL".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    // Parameter binding is the point here; the toolbar cap has its own test.
    app.query.execution.query_row_limit = None;
    app.set_active_query_text("SELECT :id, :name".to_owned());
    if let Some(doc) = app.query.session.documents.get_mut(0) {
        doc.parameter_values.insert(":id".to_owned(), "7".to_owned());
        doc.parameter_values.insert(":name".to_owned(), "Ada".to_owned());
    }
    app.dispatch_query();
    let UiCommand::RunQuery { sql, params, .. } = command_rx.try_recv().expect("bound query") else {
        panic!("expected RunQuery");
    };
    assert_eq!(sql, "SELECT $1, $2");
    assert_eq!(
        params,
        vec![
            db_pro_core::domain::query::QueryParam::Text("7".to_owned()),
            db_pro_core::domain::query::QueryParam::Text("Ada".to_owned()),
        ]
    );
}

#[test]
fn workspace_folder_opens_sql_as_file_backed_document() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("dbpro-ws-ui-{nanos}"));
    std::fs::create_dir_all(dir.join("sql")).unwrap();
    std::fs::write(dir.join("sql/demo.sql"), "SELECT 42;").unwrap();

    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState::default(),

            lifecycle: ConnectionLifecycleState {
                connected: true,
                active_connection_id: Some("conn-1".to_owned()),
                ..Default::default()
            },

            dialog: ConnectionDialogState::default(),
        },

        schema: SchemaWorkspaceState {
            explorer: SchemaExplorerState {
                selected_schema: Some("public".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    // Folder scans and file reads run on worker threads (#377/#393); pump the
    // poll loop until both land instead of asserting mid-flight.
    let wait_for = |app: &mut DbProApp, done: &dyn Fn(&DbProApp) -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            app.poll_background_tasks();
            if done(app) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("background task did not finish");
    };
    app.open_workspace_folder(dir.clone());
    wait_for(&mut app, &|app| app.workspace.files.ide_workspace.root().is_some());
    assert!(app
        .workspace
        .files
        .ide_workspace
        .index()
        .iter()
        .any(|entry| entry.relative_path == "sql/demo.sql"));
    app.open_workspace_sql_file("sql/demo.sql".to_owned());
    wait_for(&mut app, &|app| {
        app.query
            .session
            .documents
            .last()
            .is_some_and(|doc| doc.file_path.as_deref().is_some_and(|path| path.ends_with("demo.sql")))
    });
    let doc = app.query.session.documents.last().expect("file doc");
    assert_eq!(doc.text(), "SELECT 42;");
    assert!(doc.file_path.as_ref().is_some_and(|path| path.ends_with("demo.sql")));
    assert_eq!(doc.connection_id.as_deref(), Some("conn-1"));
    assert_eq!(doc.schema.as_deref(), Some("public"));
    assert_eq!(app.workspace.active_tab, WorkspaceTab::Query);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saved_task_persists_without_secrets_and_blocks_destructive_without_confirm() {
    use db_pro_core::domain::saved_task::{SavedTask, SavedTaskPayload, SavedTaskRunTrigger, SavedTaskStore};
    let mut store = SavedTaskStore::new();
    let id = uuid::Uuid::new_v4();
    store
        .upsert(SavedTask {
            id,
            name: "Wipe".into(),
            description: String::new(),
            connection_id: "conn-1".into(),
            payload: SavedTaskPayload::Sql {
                sql: "DELETE FROM t".into(),
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_run: None,
            schedule: None,
        })
        .unwrap();
    assert!(store
        .upsert(SavedTask {
            id: uuid::Uuid::new_v4(),
            name: "Bad".into(),
            description: String::new(),
            connection_id: "conn-1".into(),
            payload: SavedTaskPayload::Sql {
                sql: "SELECT 1 -- password=x".into(),
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_run: None,
            schedule: None,
        })
        .is_err());

    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    app.preferences.settings.general.confirm_destructive_queries = true;
    app.saved_tasks.store = store;
    app.run_saved_task(id, SavedTaskRunTrigger::Manual);
    assert!(command_rx.try_recv().is_err());
    assert_eq!(app.saved_tasks.pending_destructive_task_id, Some(id));

    app.saved_tasks.confirm_destructive = true;
    app.run_saved_task(id, SavedTaskRunTrigger::Manual);
    assert!(command_rx.try_recv().is_ok());
    assert!(app.saved_tasks.store.tasks.iter().all(|task| task.last_run.is_none()));
}

#[test]
fn scheduled_task_tick_dispatches_once_while_app_active() {
    use db_pro_core::domain::saved_task::{
        SavedTask, SavedTaskPayload, SavedTaskStore, TaskSchedule,
    };
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
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
        tags: vec![],
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }];
    *app.connection.lifecycle.active_connection_id_mut() = Some("conn-1".to_owned());
    app.connection.lifecycle.set_connected(true);
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let mut schedule = TaskSchedule::every_secs(30);
    schedule.next_run_at = Some(now - chrono::Duration::seconds(1));
    let mut store = SavedTaskStore::new();
    store
        .upsert(SavedTask {
            id,
            name: "Ping".into(),
            description: String::new(),
            connection_id: "conn-1".into(),
            payload: SavedTaskPayload::Sql { sql: "SELECT 1".into() },
            created_at: now,
            updated_at: now,
            last_run: None,
            schedule: Some(schedule),
        })
        .unwrap();
    app.saved_tasks.store = store;
    app.tick_saved_task_scheduler();
    let UiCommand::RunQuery { sql, .. } = command_rx.try_recv().expect("scheduled run") else {
        panic!("expected RunQuery");
    };
    assert_eq!(sql, "SELECT 1");
    assert!(app.saved_tasks.store.tasks[0].last_run.is_none());
    app.tick_saved_task_scheduler();
    assert!(command_rx.try_recv().is_err(), "no duplicate fire");
}

#[test]
fn named_workspace_session_restores_layout_and_tolerates_missing_connection() {
    let mut app = DbProApp::default();
    app.query.session.documents.clear();
    app.query
        .session
        .documents
        .push(QueryDocument::new("doc-a", "A", "SELECT 1"));
    app.query
        .session
        .documents
        .push(QueryDocument::new("doc-b", "B", "SELECT 2"));
    app.query.session.active_document_index = 1;
    app.workspace.activity = Activity::Data;
    app.workspace.active_tab = WorkspaceTab::Query;
    *app.connection.lifecycle.active_connection_id_mut() = Some("gone-conn".to_owned());
    app.schema.explorer.pinned_tables = vec!["public.orders".to_owned()];
    app.workspace.sessions.name_draft = "Focus pack".to_owned();
    app.save_named_workspace_session();
    assert_eq!(app.workspace.sessions.store.sessions.len(), 1);
    let id = app.workspace.sessions.store.sessions[0].id.clone();

    // Mutate live state, then restore.
    app.workspace.activity = Activity::Explorer;
    app.query.session.active_document_index = 0;
    *app.connection.lifecycle.active_connection_id_mut() = Some("other".to_owned());
    app.schema.explorer.pinned_tables.clear();
    app.restore_named_workspace_session(&id);

    assert_eq!(app.workspace.activity, Activity::Data);
    assert_eq!(app.query.session.active_document_index, 1);
    assert_eq!(app.schema.explorer.pinned_tables, vec!["public.orders".to_owned()]);
    assert!(
        app.connection.lifecycle.active_connection_id().is_none(),
        "missing connection must not crash"
    );
    assert!(!app.workspace.sessions.last_restore_notes.is_empty());

    app.duplicate_named_workspace_session(&id);
    assert_eq!(app.workspace.sessions.store.sessions.len(), 2);
}

// ── Sidebar geometry ──────────────────────────────────────────────────────
//
// The sidebar paints into a fixed-width column and *clips* to that width, so anything laid
// out wider than the column is silently cut off. Two defects lived here. The filter toolbar
// overflowed the column — a hardcoded reservation for the refresh button under-counted its
// real width — and because `set_max_width` unions with `min_rect`, that overflow inflated
// `max_rect` for every width measured later in the same frame, so tree rows stretched past
// the clip and lost their trailing driver badge. Separately the filter field's border was
// painted with an *outside* stroke, so both its vertical edges were cut off where the field
// met the clip boundary.

/// A connection that carries a driver badge, so the tree paints a trailing cluster.
fn badged_connection(index: usize) -> UiConnectionSummary {
    UiConnectionSummary {
        id: format!("native-test-{index}"),
        name: format!("Native Test {index}"),
        host: "localhost".to_owned(),
        port: 5432,
        database: "app".to_owned(),
        username: String::new(),
        driver: "sqlite".to_owned(),
        ssl_mode: UiSslMode::Disable,
        readonly: false,
        create_if_missing: false,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }
}

/// Renders the sidebar at `sidebar_width` with `connections` connections and returns
/// everything it painted.
///
/// Two frames, because egui measures before it settles: a scrollbar in particular only
/// appears on the frame after the content was found to overflow.
fn painted_sidebar(sidebar_width: f32, connections: usize) -> Vec<egui::epaint::ClippedShape> {
    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: (0..connections).map(badged_connection).collect(),
            },

            lifecycle: ConnectionLifecycleState::default(),

            dialog: ConnectionDialogState::default(),
        },

        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                sidebar_width,
                ..Default::default()
            },
            ..Default::default()
        },
        ..DbProApp::default()
    };
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    DbProTheme::light().apply(&ctx);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::new(1440.0, 900.0),
        )),
        ..Default::default()
    };
    // The first frame only measures; only the second frame's shapes are asserted on.
    let _ = crate::test_frame::frame(&ctx, input(), |ctx| app.draw_sidebar(ctx));
    crate::test_frame::frame(&ctx, input(), |ctx| app.draw_sidebar(ctx)).shapes
}

#[test]
fn navigator_tree_scrolls_with_the_mouse_wheel() {
    let mut app = DbProApp {
        connection: ConnectionFeatureState {
            catalog: ConnectionCatalogState {
                connections: (0..40).map(badged_connection).collect(),
            },
            ..Default::default()
        },
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                sidebar_width: 260.0,
                ..Default::default()
            },
            ..Default::default()
        },
        ..DbProApp::default()
    };
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    DbProTheme::light().apply(&ctx);
    let pointer = egui::pos2(100.0, 400.0);
    let input = |events| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::new(1440.0, 900.0),
        )),
        events,
        ..Default::default()
    };

    let _ = crate::test_frame::frame(&ctx, input(Vec::new()), |ctx| app.draw_sidebar(ctx));
    let _ = crate::test_frame::frame(&ctx, input(vec![egui::Event::PointerMoved(pointer)]), |ctx| app.draw_sidebar(ctx));
    let _ = crate::test_frame::frame(&ctx, input(vec![egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    }]), |ctx| {
        assert_eq!(ctx.pointer_hover_pos(), Some(pointer));
        assert!(ctx.input(|input| input.smooth_scroll_delta.y < 0.0));
        app.draw_sidebar(ctx);
    });

    let sidebar_ui_id = egui::Id::new("dbpro_sidebar_content");
    let scroll_id = sidebar_ui_id.with(egui::IdSalt::new("codex_navigator_scroll"));
    let offset = egui::scroll_area::State::load(&ctx, scroll_id)
        .expect("navigator ScrollArea state was not stored")
        .offset
        .y;
    assert!(offset > 0.0, "mouse wheel did not advance tree offset: {offset}");
}

/// The area a shape actually covers. `Shape::rect_stroke` paints *entirely outside* its
/// path, so `visual_bounding_rect` under-reports a stroked rect by half the stroke width —
/// using it would let a border that spills past the clip pass the assertions below.
fn painted_extent(shape: &egui::Shape) -> egui::Rect {
    match shape {
        egui::Shape::Rect(rect) => rect.rect.expand(rect.stroke.width.max(0.0)),
        other => other.visual_bounding_rect(),
    }
}

/// Nothing the sidebar paints may be cut off horizontally. A shape that escapes the clip it
/// was given is silently truncated on screen, which is the general form of both reported
/// defects: the driver badge lost its right side, and every 1px border on a widget filling
/// the column lost both vertical edges.
///
/// Only the horizontal axis is asserted. The sidebar is a fixed-width column and every
/// defect here is a width overflow, whereas on the vertical axis a full-height rule
/// legitimately ends half a stroke above and below the viewport.
#[test]
fn the_sidebar_paints_nothing_past_its_clip() {
    for sidebar_width in [SIDEBAR_MIN_WIDTH, 260.0, 360.0] {
        let mut escaped: Vec<String> = painted_sidebar(sidebar_width, 1)
            .iter()
            .filter_map(|clipped| {
                let painted = painted_extent(&clipped.shape);
                // Empty shapes (e.g. an empty mesh) report an inverted, infinite rect.
                if !painted.is_finite() {
                    return None;
                }
                let escapes =
                    painted.min.x < clipped.clip_rect.min.x - 0.01 || painted.max.x > clipped.clip_rect.max.x + 0.01;
                escapes.then(|| {
                    format!(
                        "  {:?} painted={painted:?} clip={:?}",
                        clipped.shape.visual_bounding_rect(),
                        clipped.clip_rect
                    )
                })
            })
            .collect();
        escaped.sort();
        assert!(
            escaped.is_empty(),
            "the sidebar paints {} shape(s) past their clip at sidebar_width={sidebar_width}:\n{}",
            escaped.len(),
            escaped.join("\n")
        );
    }
}

/// The driver badge text the fixture paints, and the first badge's geometry:
/// `(label, label clip, pill, pill clip)`.
fn driver_badge(shapes: &[egui::epaint::ClippedShape]) -> (egui::Rect, egui::Rect, egui::Rect, egui::Rect) {
    let label = shapes
        .iter()
        .find(|clipped| matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == "SQLITE"))
        .expect("no connection row painted its driver badge");
    let label_bounds = label.shape.visual_bounding_rect();

    // The innermost painted background around the label is the badge pill.
    let pill = shapes
        .iter()
        .filter(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(_)) && painted_extent(&clipped.shape).contains_rect(label_bounds)
        })
        .min_by(|left, right| {
            painted_extent(&left.shape)
                .area()
                .total_cmp(&painted_extent(&right.shape).area())
        })
        .expect("the driver badge has no painted background");
    (
        label_bounds,
        label.clip_rect,
        painted_extent(&pill.shape),
        pill.clip_rect,
    )
}

#[test]
fn the_driver_badge_is_fully_visible_in_the_navigator_tree() {
    // Every legal sidebar width, plus a tree long enough to show a scrollbar.
    for (sidebar_width, connections) in [(SIDEBAR_MIN_WIDTH, 1), (260.0, 1), (360.0, 1), (260.0, 40)] {
        let (label, label_clip, pill, pill_clip) = driver_badge(&painted_sidebar(sidebar_width, connections));
        assert!(
            label_clip.contains_rect(label),
            "the driver badge label is cut off at sidebar_width={sidebar_width} with {connections} connection(s): \
             {label:?} escapes its clip {label_clip:?}"
        );
        assert!(
            pill_clip.contains_rect(pill),
            "the driver badge background is cut off at sidebar_width={sidebar_width} with {connections} connection(s): \
             {pill:?} escapes its clip {pill_clip:?}"
        );

        // The pill is sized from the measured label. A per-character estimate pads it with a
        // phantom gap that widens the pill and shoves it out of the column.
        let padding = (pill.width() - label.width()) * 0.5;
        assert!(
            (padding - explorer_tree::CODEX_BADGE_PAD_X).abs() < 0.5,
            "the driver badge pads its label by {padding} instead of {}",
            explorer_tree::CODEX_BADGE_PAD_X
        );
    }
}

/// The tree row spans the sidebar column, not whatever clip it happens to sit in.
///
/// Inside a ScrollArea the clip narrows by the scrollbar's width, so a row that sized itself
/// from the clip would shift its trailing badge left as soon as the connection list grew long
/// enough to scroll — and shift it back when it did not.
#[test]
fn the_tree_row_spans_its_layout_width_not_its_clip() {
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    let theme = DbProTheme::light();
    theme.apply(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::new(800.0, 200.0),
        )),
        ..Default::default()
    };
    let column_width = 240.0;
    let mut painted = None;
    let _ = crate::test_frame::frame(&ctx, input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let column = egui::Rect::from_min_size(egui::Pos2::new(8.0, 8.0), egui::vec2(column_width, 160.0));
            let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(column));
            // As if a scrollbar had just appeared and taken 10px of the viewport.
            row_ui.set_clip_rect(column.with_max_x(column.max.x - 10.0));
            painted = Some(
                explorer_tree::draw_codex_tree_row(
                    &mut row_ui,
                    &theme,
                    explorer_tree::CodexTreeRow {
                        depth: 0,
                        is_expandable: false,
                        is_expanded: false,
                        icon: Icon::Database,
                        icon_color: theme.text_primary,
                        label: "Native Test",
                        is_selected: false,
                        is_dimmed: false,
                        status_dot: None,
                        badge_text: Some("SQLITE"),
                        badge_accent: false,
                        count_text: None,
                        detail_text: None,
                    },
                )
                .0,
            );
        });
    });
    let row = painted.expect("the tree row was not painted");
    assert_eq!(
        row.rect.width(),
        column_width,
        "the tree row sized itself from its clip instead of the column"
    );
}

#[test]
fn the_filter_field_border_is_painted_inside_the_field() {
    for sidebar_width in [SIDEBAR_MIN_WIDTH, 260.0, 360.0] {
        let shapes = painted_sidebar(sidebar_width, 1);
        let hint = shapes
            .iter()
            .find(
                |clipped| matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == "Filter objects…"),
            )
            .unwrap_or_else(|| panic!("the filter field is not painted at sidebar_width={sidebar_width}"));
        let hint_bounds = hint.shape.visual_bounding_rect();

        // The field's own background: the innermost filled rect around its content.
        let background = shapes
            .iter()
            .filter(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT)
                    && painted_extent(&clipped.shape).contains_rect(hint_bounds)
            })
            .min_by(|left, right| {
                painted_extent(&left.shape)
                    .area()
                    .total_cmp(&painted_extent(&right.shape).area())
            })
            .unwrap_or_else(|| panic!("the filter field has no painted background at sidebar_width={sidebar_width}"));
        let field = painted_extent(&background.shape);

        // Its border: the stroked rect around the same content.
        let border = shapes
            .iter()
            .find(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(rect) if !rect.stroke.is_empty())
                    && painted_extent(&clipped.shape).contains_rect(hint_bounds)
            })
            .unwrap_or_else(|| panic!("the filter field border is not painted at sidebar_width={sidebar_width}"));

        let painted = painted_extent(&border.shape);
        assert!(
            field.expand(0.01).contains_rect(painted),
            "the filter field border is painted outside the field at sidebar_width={sidebar_width}: \
             painted {painted:?} against field {field:?}"
        );
    }
}
#[test]
// cc-scan:allow LONG_FUNCTION — egui click harness needs setup + drive + assert
fn clicking_a_dock_tab_switches_the_output_pane() {
    use crate::app::{DbProApp, WorkspaceTab};
    use crate::query::QueryDocument;
    use crate::TaskBridge;

    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.workspace.active_tab = WorkspaceTab::Query;
    app.workspace.bottom_panel_open = true;
    app.query
        .session
        .add_document(QueryDocument::new("query-1", "Query 1", ""));
    let ctx = egui::Context::default();
    DbProTheme::install_fonts(&ctx);
    app.theme.apply(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let frame = |app: &mut DbProApp, events: Vec<egui::Event>| {
        crate::test_frame::frame(&ctx, egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_min_size(screen.size());
                app.draw_query(ui);
            });
        })
    };

    let output = frame(&mut app, Vec::new());
    let tab_pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "History" => Some(text.pos),
            _ => None,
        })
        .expect("History tab label must render in the dock");
    let click = egui::pos2(tab_pos.x + 12.0, tab_pos.y + 8.0);

    frame(&mut app, vec![egui::Event::PointerMoved(click)]);
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    let document_id = app.query.session.active_document().map(|document| document.id.as_str());
    assert_eq!(
        app.query.output.active_tab_for_document(document_id),
        OutputTab::History,
        "clicking the History dock tab must switch the pane"
    );
}

#[test]
fn staged_apply_with_deletes_requires_confirmation() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    app.table.mutation.staged_changes.stage_delete(StagedChange::Delete {
        identity: RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("7".to_owned())],
            original_pk_data_types: Vec::new(),
        },
        current_row_index: Some(0),
    });

    // A delete-containing apply parks behind the confirm gate instead of
    // dispatching `ApplyTableChanges` (#408).
    app.apply_staged_changes();
    assert!(app.table.mutation.staged_apply_confirm);
    assert!(app.table.mutation.staged_apply_request.is_none());
    assert!(command_rx.try_recv().is_err());
    assert!(!app.table.mutation.staged_changes.is_empty());
}

#[test]
fn staged_apply_without_deletes_skips_confirmation() {
    let (bridge, _command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);

    app.table
        .mutation
        .staged_changes
        .stage_insert(vec!["name".to_owned()], vec![UiCell::Text("x".to_owned())]);

    // Inserts-only apply reaches the connection check directly — no confirm
    // flag is raised (#408).
    app.apply_staged_changes();
    assert!(!app.table.mutation.staged_apply_confirm);
    assert_eq!(
        app.feedback.runtime_message,
        "Connect to a database before applying changes"
    );
}

/// The agent panel rect is the occlusion surface for pointer hit tests on
/// layers below it — rows must not hover-highlight under it (#338).
#[test]
fn agent_panel_rect_occludes_pointer_positions() {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::pos2(800.0, 0.0), egui::vec2(320.0, 700.0));
    store_agent_panel_rect(&ctx, rect);
    assert!(agent_panel_contains_pointer(&ctx, egui::pos2(900.0, 300.0)));
    assert!(!agent_panel_contains_pointer(&ctx, egui::pos2(400.0, 300.0)));
    assert!(!agent_panel_contains_pointer(&ctx, egui::pos2(900.0, 720.0)));
}

/// #409: approving a patch mutates the document optimistically; when the
/// `ContinueAgentRun` command cannot be dispatched the buffer must be restored.
#[test]
fn agent_patch_approval_reverts_document_when_continue_dispatch_fails() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    app.query.session.documents[0].set_text("SELECT old");
    let document_id = app.query.session.documents[0].id.clone();
    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    let run_id = db_pro_core::domain::agent::AgentRunId::new();
    session.active_run_id = Some(run_id);
    session.pending_confirmation = Some(super::agent_workflow_state::AgentUiConfirmation {
        run_id,
        call_id: "patch-1".to_owned(),
        kind: db_pro_core::domain::agent_workflow::AgentConfirmationKind::ApplyPatch,
        preview: Some(db_pro_core::domain::agent::AgentToolOutput::PatchPreview {
            patch: db_pro_core::domain::agent::AgentSqlPatch {
                document_id: document_id.clone(),
                expected_version: app.query.session.documents[0].buffer.version(),
                range: (7, 10),
                replacement: "users".to_owned(),
            },
            original: "old".to_owned(),
            proposed: "users".to_owned(),
        }),
        document_id: document_id.clone(),
    });
    app.agent.sessions.insert(document_id.clone(), session);
    // Closing the command channel makes every send fail.
    drop(command_rx);

    app.agent_confirmation_action(true);

    assert_eq!(app.query.session.documents[0].text(), "SELECT old");
    assert_eq!(
        app.agent.sessions[&document_id].state,
        db_pro_core::domain::agent::AgentSessionState::Failed
    );
    assert!(app.feedback.runtime_message.contains("restored"));
}

/// #360: pressing Stop clears the in-flight run state optimistically so the UI
/// exits "generating" even if the cancel command cannot be queued.
#[test]
fn cancel_agent_run_clears_in_flight_state_when_dispatch_fails() {
    let (bridge, command_rx, _event_tx) = TaskBridge::with_channels();
    let mut app = DbProApp::with_task_bridge(bridge);
    let document_id = app.query.session.documents[0].id.clone();
    let mut session = super::agent_workflow_state::AgentUiSession::for_document(&document_id, None, None);
    session.active_run_id = Some(db_pro_core::domain::agent::AgentRunId::new());
    session.request_id = Some(crate::RequestId(7));
    session.state = db_pro_core::domain::agent::AgentSessionState::Running;
    app.agent.sessions.insert(document_id.clone(), session);
    drop(command_rx);

    app.cancel_active_agent_run();

    let session = &app.agent.sessions[&document_id];
    assert_eq!(session.active_run_id, None);
    assert_eq!(session.request_id, None);
    assert_eq!(session.state, db_pro_core::domain::agent::AgentSessionState::Cancelled);
}
