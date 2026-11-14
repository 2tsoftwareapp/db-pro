//! Tests for `../events.rs` (moved out of the source file).
    use super::*;
    use crate::{UiColumn, UiTableColumn};

    fn row_result(name: &str) -> UiQueryResult {
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
            ],
            rows: vec![vec![UiCell::Number("7".to_owned()), UiCell::Text(name.to_owned())]],
            row_count: 1,
            duration_ms: 0,
        }
    }

    fn row_reload_app() -> DbProApp {
        DbProApp {
            table: TableEditorState {
                state: TableState {
                    table_info: Some(UiTableInfo {
                        schema: "public".to_owned(),
                        name: "customers".to_owned(),
                        row_count: Some(1),
                        columns: vec![
                            UiTableColumn {
                                name: "id".to_owned(),
                                data_type: "integer".to_owned(),
                                nullable: false,
                                default: None,
                                is_primary_key: true,
                                ..Default::default()
                            },
                            UiTableColumn {
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
                    }),
                    ..Default::default()
                },
                data_query: TableDataQueryState {
                    result: Some(row_result("local server value")),
                    row_reload_request: Some(RequestId(9)),
                    row_reload_identity: Some(RowIdentity {
                        original_pk_columns: vec!["id".to_owned()],
                        original_pk_values: vec![UiCell::Number("7".to_owned())],
                        original_pk_data_types: Vec::new(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn row_reload_merges_server_values_by_original_identity() {
        let mut app = row_reload_app();
        app.on_table_data_loaded(RequestId(9), row_result("fresh server value"), 1);

        let result = app
            .table
            .data_query
            .result
            .clone()
            .expect("table result should remain visible");
        assert_eq!(result.rows[0][1], UiCell::Text("fresh server value".to_owned()));
        assert!(app.table.data_query.row_reload_request.is_none());
    }

    #[test]
    fn row_reload_reports_deleted_row_without_dropping_local_result() {
        let mut app = row_reload_app();
        let empty = UiQueryResult {
            columns: row_result("unused").columns,
            rows: Vec::new(),
            row_count: 0,
            duration_ms: 0,
        };
        app.on_table_data_loaded(RequestId(9), empty, 0);

        assert_eq!(app.feedback.runtime_message, "Row was deleted");
        assert!(app.table.data_query.result.is_some());
    }

    #[test]
    fn row_reload_reloads_grid_when_server_columns_drifted() {
        let mut app = row_reload_app();
        // Simulate an ALTER TABLE landing between the edit and the row reload: the reload answer
        // carries a different column layout than the visible grid, so patching by position would
        // write cells under the wrong headers.
        let drifted = UiQueryResult {
            columns: vec![
                UiColumn { name: "id".to_owned(), data_type: "integer".to_owned(), nullable: false },
                UiColumn { name: "email".to_owned(), data_type: "text".to_owned(), nullable: true },
                UiColumn { name: "name".to_owned(), data_type: "text".to_owned(), nullable: false },
            ],
            rows: vec![vec![
                UiCell::Number("7".to_owned()),
                UiCell::Text("a@b.c".to_owned()),
                UiCell::Text("fresh".to_owned()),
            ]],
            row_count: 1,
            duration_ms: 0,
        };
        app.on_table_data_loaded(RequestId(9), drifted, 1);

        assert_eq!(app.feedback.runtime_message, "Table columns changed — reloading table data");
        assert!(app.table.data_query.result.is_none(), "stale grid must be discarded");
    }

    #[test]
    fn row_reload_reloads_grid_when_identity_matches_multiple_rows() {
        let mut app = row_reload_app();
        let mut duplicate = row_result("other");
        duplicate.rows.push(vec![UiCell::Number("7".to_owned()), UiCell::Text("second".to_owned())]);
        duplicate.row_count = 2;
        app.on_table_data_loaded(RequestId(9), duplicate, 2);

        assert_eq!(app.feedback.runtime_message, "Row reload matched multiple rows — reloading table data");
        assert!(app.table.data_query.result.is_none());
    }

    #[test]
    fn deleting_non_active_connection_preserves_active_session() {
        let mut app = DbProApp {
            connection: ConnectionFeatureState {
                catalog: ConnectionCatalogState::default(),

                lifecycle: ConnectionLifecycleState {
                    connected: true,
                    active_connection_id: Some("conn-a".to_owned()),
                    pending_request: Some(RequestId(11)),
                    pending_connection_id: Some("conn-b".to_owned()),
                    failed_connection_ids: ["conn-a".to_owned(), "conn-b".to_owned()].into_iter().collect(),
                    errors: [
                        ("conn-a".to_owned(), "stale".to_owned()),
                        ("conn-b".to_owned(), "gone".to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                },

                dialog: ConnectionDialogState::default(),
            },

            ..Default::default()
        };

        app.on_operation_completed(RequestId(11), "connection.deleted".to_owned());

        assert_eq!(app.connection.lifecycle.active_connection_id(), Some("conn-a"));
        assert!(app.connection.lifecycle.is_connected());
        assert!(app.connection.lifecycle.pending_connection_id.is_none());
        assert!(app.connection.lifecycle.failed_connection_ids.contains("conn-a"));
        assert!(!app.connection.lifecycle.failed_connection_ids.contains("conn-b"));
        assert_eq!(
            app.connection.lifecycle.errors.get("conn-a").map(String::as_str),
            Some("stale")
        );
        assert!(!app.connection.lifecycle.errors.contains_key("conn-b"));
    }

    #[test]
    fn deleting_active_connection_clears_session() {
        let mut app = DbProApp {
            connection: ConnectionFeatureState {
                catalog: ConnectionCatalogState::default(),

                lifecycle: ConnectionLifecycleState {
                    connected: true,
                    active_connection_id: Some("conn-a".to_owned()),
                    pending_request: Some(RequestId(12)),
                    pending_connection_id: Some("conn-a".to_owned()),
                    failed_connection_ids: ["conn-a".to_owned()].into_iter().collect(),
                    ..Default::default()
                },

                dialog: ConnectionDialogState::default(),
            },

            ..Default::default()
        };

        app.on_operation_completed(RequestId(12), "connection.deleted".to_owned());

        assert!(app.connection.lifecycle.active_connection_id().is_none());
        assert!(!app.connection.lifecycle.is_connected());
        assert!(app.connection.lifecycle.pending_connection_id.is_none());
        assert!(app.connection.lifecycle.failed_connection_ids.is_empty());
    }