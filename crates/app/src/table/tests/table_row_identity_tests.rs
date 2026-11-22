use super::*;
use crate::app::table_mutation_state::TableApplyPlan;
use crate::{UiCell, UiColumn, UiQueryResult, UiSslMode, UiTableColumn, UiTableIndex, UiTableInfo};
use db_pro_core::application::sql_builder::{build_delete, build_update};
use db_pro_core::application::DriverDialect;
use db_pro_core::domain::query::CellValue;

fn column(name: &str, data_type: &str) -> UiTableColumn {
    UiTableColumn {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable: name != "email" && name != "code",
        ..UiTableColumn::default()
    }
}

fn index(name: &str, columns: &[&str], unique: bool, predicate: Option<&str>) -> UiTableIndex {
    UiTableIndex {
        name: name.to_owned(),
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        unique,
        method: "btree".to_owned(),
        primary: false,
        include_columns: Vec::new(),
        predicate: predicate.map(str::to_owned),
        definition: String::new(),
    }
}

fn people_info(indexes: Vec<UiTableIndex>) -> UiTableInfo {
    UiTableInfo {
        schema: "public".to_owned(),
        name: "people".to_owned(),
        row_count: Some(1),
        columns: vec![
            column("email", "text"),
            column("name", "text"),
            column("nick", "text"),
            column("code", "text"),
        ],
        primary_key: None,
        indexes,
        foreign_keys: Vec::new(),
        check_constraints: Vec::new(),
        dependencies: Vec::new(),
    }
}

fn people_result() -> UiQueryResult {
    UiQueryResult {
        columns: vec![
            UiColumn {
                name: "email".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
            UiColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
            UiColumn {
                name: "nick".to_owned(),
                data_type: "text".to_owned(),
                nullable: true,
            },
            UiColumn {
                name: "code".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            },
        ],
        rows: vec![vec![
            UiCell::Text("ada@x".to_owned()),
            UiCell::Text("Ada".to_owned()),
            UiCell::Text("a".to_owned()),
            UiCell::Text("A1".to_owned()),
        ]],
        row_count: 1,
        duration_ms: 1,
    }
}

fn people_app(indexes: Vec<UiTableIndex>) -> DbProApp {
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
                table_info: Some(people_info(indexes)),
                table_view: TableView::Data,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    app.workspace.active_tab = WorkspaceTab::Table;
    app.schema.explorer.selected_table = Some("people".to_owned());
    app
}

fn domain_text(cell: &UiCell) -> CellValue {
    match cell {
        UiCell::Text(value) => CellValue::Text(value.clone()),
        other => panic!("expected text cell, got {other:?}"),
    }
}

fn where_sql(plan: &TableApplyPlan) -> (Option<String>, Option<String>) {
    let dialect = DriverDialect::from_name("PostgreSQL");
    let mut update_sql = None;
    let mut delete_sql = None;
    for change in &plan.changes {
        match change {
            UiTableMutation::Update {
                columns,
                values,
                pk_columns,
                pk_values,
                ..
            } => {
                let (sql, _) = build_update(
                    &dialect,
                    "public",
                    "people",
                    columns,
                    &values.iter().map(domain_text).collect::<Vec<_>>(),
                    pk_columns,
                    &pk_values.iter().map(domain_text).collect::<Vec<_>>(),
                )
                .expect("update sql");
                update_sql = Some(sql);
            }
            UiTableMutation::Delete {
                pk_columns,
                pk_values,
                ..
            } => {
                let (sql, _) = build_delete(
                    &dialect,
                    "public",
                    "people",
                    pk_columns,
                    &pk_values.iter().map(domain_text).collect::<Vec<_>>(),
                )
                .expect("delete sql");
                delete_sql = Some(sql);
            }
            UiTableMutation::Insert { .. } => {}
        }
    }
    (update_sql, delete_sql)
}

/// A non-partial UNIQUE, or columns the user names, identify UPDATE and DELETE.
#[test]
fn unique_or_chosen_columns_identify_row_mutations() {
    let partial = index("people_nick_partial", &["nick"], true, Some("nick IS NOT NULL"));
    let full = index("people_email_key", &["email"], true, None);
    let mut app = people_app(vec![partial.clone(), full]);
    let result = people_result();

    assert_eq!(app.result_edit_reason(&result), None);
    app.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    app.table.editing.data_edit_value = "Grace".to_owned();
    assert!(app.submit_data_cell_edit(&result, 0, 1));
    let update_plan = app.table.mutation.build_apply_plan();
    let update_sql = where_sql(&update_plan).0.expect("staged update");
    assert!(update_sql.contains(r#"WHERE "email" ="#), "{update_sql}");
    assert!(!update_sql.contains("nick"), "{update_sql}");
    app.table.data.selected_rows.insert(0);
    app.request_delete_selected_data_rows(&result);
    let delete_plan = app.table.mutation.build_apply_plan();
    let delete_sql = where_sql(&delete_plan).1.expect("staged delete");
    assert!(delete_sql.contains(r#"WHERE "email" ="#), "{delete_sql}");
    assert!(!delete_sql.contains("nick"), "{delete_sql}");

    let mut partial_only = people_app(vec![partial]);
    let blocked = partial_only.result_edit_reason(&result).expect("partial unique is not an id");
    assert!(blocked.contains("no primary key"), "{blocked}");
    partial_only.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    assert!(partial_only.table.editing.data_editing_cell.is_none());
    assert_eq!(partial_only.table.mutation.staged_changes.counts().total(), 0);

    partial_only.choose_row_identity_column("code".to_owned());
    partial_only.choose_row_identity_column("email".to_owned());
    assert_eq!(partial_only.result_edit_reason(&result), None);
    partial_only.begin_data_cell_edit(&result, 0, 1, &result.rows[0][1]);
    partial_only.table.editing.data_edit_value = "Grace".to_owned();
    assert!(partial_only.submit_data_cell_edit(&result, 0, 1));
    let update_plan = partial_only.table.mutation.build_apply_plan();
    let update_sql = where_sql(&update_plan).0.expect("staged chosen-column update");
    partial_only.table.data.selected_rows.insert(0);
    partial_only.request_delete_selected_data_rows(&result);
    let delete_plan = partial_only.table.mutation.build_apply_plan();
    let delete_sql = where_sql(&delete_plan).1.expect("staged chosen-column delete");
    assert!(
        update_sql.contains(r#""code" = $"#) && update_sql.contains(r#""email" = $"#),
        "{update_sql}"
    );
    assert!(
        delete_sql.contains(r#"WHERE "code" = $"#) && delete_sql.contains(r#""email" = $"#),
        "{delete_sql}"
    );
    assert!(!update_sql.contains("nick"), "{update_sql}");
}
