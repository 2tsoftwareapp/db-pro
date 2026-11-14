use super::*;
use crate::{UiCell, UiColumn, UiQueryResult, UiSslMode, UiTableColumn, UiTableForeignKey, UiTableInfo};

fn orders_info() -> UiTableInfo {
    UiTableInfo {
        schema: "public".to_owned(),
        name: "orders".to_owned(),
        row_count: Some(1),
        columns: vec![
            UiTableColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
                is_primary_key: true,
                ..UiTableColumn::default()
            },
            UiTableColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
                ..UiTableColumn::default()
            },
            UiTableColumn {
                name: "customer_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: true,
                ..UiTableColumn::default()
            },
        ],
        primary_key: Some(vec!["id".to_owned()]),
        indexes: Vec::new(),
        foreign_keys: vec![UiTableForeignKey {
            name: "orders_customer_fk".to_owned(),
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

fn result_with(id: UiCell) -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            UiColumn {
                name: "id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            UiColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
            UiColumn {
                name: "customer_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: true,
            },
        ],
        rows: vec![vec![id, UiCell::Text("Ada".to_owned()), UiCell::Number("3".to_owned())]],
        row_count: 1,
        duration_ms: 1,
    }
}

fn writable_orders_app() -> DbProApp {
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
                table_info: Some(orders_info()),
                table_view: TableView::Data,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.workspace.active_tab = WorkspaceTab::Query;
    app.schema.explorer.selected_table = Some("orders".to_owned());
    app
}

/// Query results edit when every primary-key column is present and non-NULL.
/// A view and inline SQL stay read-only and say why. A foreign key does not block the edit.
#[test]
fn query_result_edits_when_primary_key_remains() {
    let mut app = writable_orders_app();
    assert!(
        !app.table.state.table_info.as_ref().unwrap().foreign_keys.is_empty(),
        "the fixture must carry a foreign key"
    );
    let result = result_with(UiCell::Number("7".to_owned()));

    assert_eq!(app.result_edit_reason(&result), None);
    app.workspace.active_tab = WorkspaceTab::Table;
    assert_eq!(app.result_edit_reason(&result), None, "table data stays editable");
    app.workspace.active_tab = WorkspaceTab::Query;

    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    assert_eq!(app.table.editing.data_editing_cell, Some((0, 1)));
    app.table.editing.data_edit_value = "Grace".to_owned();
    assert!(app.submit_data_cell_edit(&result, 0, 1));
    let identity = TableDataState::row_identity(
        &result,
        app.table.state.table_info.as_ref().unwrap(),
        None,
        0,
    )
    .unwrap();
    assert_eq!(
        app.table.mutation.staged_changes.cell_value(&identity, 1),
        Some(UiCell::Text("Grace".to_owned()))
    );

    let missing_key = UiQueryResult {
        columns: vec![UiColumn {
            name: "name".to_owned(),
            data_type: "text".to_owned(),
            nullable: false,
        }],
        rows: vec![vec![UiCell::Text("Ada".to_owned())]],
        row_count: 1,
        duration_ms: 1,
    };
    let missing = app.result_edit_reason(&missing_key).expect("missing key blocks edit");
    assert!(missing.contains("not present in this result"), "{missing}");
    let missing_notice = app
        .result_read_only_notice(Some(&missing))
        .expect("missing identity column has a read-only notice");
    assert!(missing_notice.contains("id"), "{missing_notice}");
    app.begin_data_cell_edit(&missing_key, 0, 0, &missing_key.rows[0][0]);
    assert!(app.table.editing.data_editing_cell.is_none());
    assert!(app.feedback.runtime_message.contains("not present in this result"));

    let null_key = result_with(UiCell::Null);
    app.begin_data_cell_edit(&null_key, 0, 1, &null_key.rows[0][1]);
    assert!(app.table.editing.data_editing_cell.is_none());
    assert!(
        app.feedback.runtime_message.contains("NULL primary key"),
        "{}",
        app.feedback.runtime_message
    );

    app.workspace.active_tab = WorkspaceTab::SchemaObject;
    app.schema.explorer.selected_schema_object = Some(SchemaObjectSelection::View("orders_v".to_owned()));
    app.table.state.table_info = None;
    let view_reason = app.result_edit_reason(&result).expect("a view is read-only");
    assert_eq!(view_reason, table_editor_context::VIEW_ROWS_READ_ONLY);
    assert!(!view_reason.to_ascii_lowercase().contains("structure is still loading"));
    app.open_insert_row();
    assert_eq!(app.feedback.runtime_message, table_editor_context::VIEW_ROWS_READ_ONLY);
    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    assert_eq!(app.feedback.runtime_message, table_editor_context::VIEW_ROWS_READ_ONLY);

    app.workspace.active_tab = WorkspaceTab::Table;
    app.schema.explorer.selected_schema_object = None;
    app.table.state.table_info = Some(orders_info());
    app.table.state.table_view = TableView::Data;
    app.table.data_query.inline_query_result = true;
    let inline_reason = app.result_edit_reason(&result).expect("inline SQL is read-only");
    assert_eq!(inline_reason, table_editor_context::INLINE_SQL_READ_ONLY);
    assert!(!inline_reason.to_ascii_lowercase().contains("structure is still loading"));
    assert!(!inline_reason.contains("Connect with write access"));
    assert!(app.result_read_only_notice(Some(&inline_reason)).is_some());
    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    assert!(app.table.editing.data_editing_cell.is_none());
    assert_eq!(app.feedback.runtime_message, table_editor_context::INLINE_SQL_READ_ONLY);
}

fn truncated_text_cell() -> UiCell {
    let mut value = "x".repeat(db_pro_ui::MAX_CELL_TEXT_BYTES);
    value.push('…');
    UiCell::Text(value)
}

// #537: the grid only holds a display prefix of an oversized value. Opening an
// editor on it must not be possible, because Apply would write the prefix back.
#[test]
fn truncated_cell_never_opens_an_editor() {
    let mut app = writable_orders_app();
    let mut result = result_with(UiCell::Number("7".to_owned()));
    result.rows[0][1] = truncated_text_cell();

    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);

    assert_eq!(app.table.editing.data_editing_cell, None, "the cell must stay read-only");
    assert!(
        app.feedback.runtime_message.to_ascii_lowercase().contains("truncated"),
        "the user needs to know why: {}",
        app.feedback.runtime_message
    );
    assert!(app.table.mutation.staged_changes.counts().total() == 0);
}

// #537: the write boundary refuses a truncated cell even when the editor state
// was populated by another path.
#[test]
fn truncated_cell_is_rejected_at_the_write_boundary() {
    let mut app = writable_orders_app();
    let mut result = result_with(UiCell::Number("7".to_owned()));
    result.rows[0][1] = truncated_text_cell();
    app.table.editing.data_edit_value = "Grace".to_owned();

    assert!(!app.submit_data_cell_edit(&result, 0, 1));

    assert_eq!(app.table.mutation.staged_changes.counts().total(), 0);
    assert!(
        app.feedback.runtime_message.to_ascii_lowercase().contains("truncated"),
        "{}",
        app.feedback.runtime_message
    );
}
