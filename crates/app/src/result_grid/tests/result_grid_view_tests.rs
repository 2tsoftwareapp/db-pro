use super::*;
use crate::app::result_grid_projection::GridSelectionCache;

/// The criterion bench (`benches/result_grid_benchmarks.rs`) is a separate crate, so it reaches
/// this type through the crate-root re-export only. Resolving it the same way here — instead of
/// through `super::` — makes a dropped re-export fail `cargo test`, not just the bench build
/// under `cargo clippy --all-targets`.
#[test]
fn selection_lookup_is_reachable_from_the_crate_root() {
    let lookup: crate::GridSelectionLookup = crate::GridSelectionLookup::new(&[4, 1], &[1, 0]);

    assert_eq!(lookup.row_positions.get(&4), Some(&0));
    assert_eq!(lookup.row_positions.get(&1), Some(&1));
    assert_eq!(lookup.column_positions.get(&0), Some(&1));
}

#[test]
fn row_range_selection_follows_filtered_sort_order() {
    let mut app = DbProApp::default();
    let indexes = [4, 1, 7, 2];
    let lookup = GridSelectionLookup::new(&indexes, &[]);
    app.table
        .data
        .select_visible_row(&indexes, &lookup.row_positions, 1, false, false);
    app.table
        .data
        .select_visible_row(&indexes, &lookup.row_positions, 3, true, false);

    assert_eq!(
        app.table.data.selected_rows.into_iter().collect::<Vec<_>>(),
        vec![1, 2, 7]
    );
    assert_eq!(app.table.data.selected_row, Some(2));
    assert_eq!(app.table.data.selection_anchor_row, Some(1));
}

#[test]
fn toggling_last_row_keeps_a_non_empty_selection() {
    let mut app = DbProApp::default();
    let indexes = [3];
    let lookup = GridSelectionLookup::new(&indexes, &[]);
    app.table
        .data
        .select_visible_row(&indexes, &lookup.row_positions, 0, false, false);
    app.table
        .data
        .select_visible_row(&indexes, &lookup.row_positions, 0, false, true);

    assert_eq!(app.table.data.selected_rows.into_iter().collect::<Vec<_>>(), vec![3]);
    assert_eq!(app.table.data.selected_row, Some(3));
}

#[test]
fn cell_range_selection_uses_visible_row_and_column_order() {
    let mut app = DbProApp::default();
    let indexes = [4, 1, 7, 2];
    let order = [2, 0, 1];
    let lookup = GridSelectionLookup::new(&indexes, &order);

    app.table.data.select_single_cell((1, 0));
    app.table
        .data
        .select_cell_range(&indexes, &lookup.row_positions, (2, 1), true);

    assert_eq!(app.table.data.selected_cell, Some((2, 1)));
    assert_eq!(
        app.table.data.selected_rows.iter().copied().collect::<Vec<_>>(),
        vec![1, 2, 7]
    );
    assert!(app.table.data.is_cell_selected(&lookup, (1, 0)));
    assert!(app.table.data.is_cell_selected(&lookup, (7, 0)));
    assert!(app.table.data.is_cell_selected(&lookup, (2, 1)));
    assert!(!app.table.data.is_cell_selected(&lookup, (1, 2)));
    assert!(!app.table.data.is_cell_selected(&lookup, (4, 0)));
}

#[test]
fn select_all_visible_cells_covers_current_grid() {
    let mut app = DbProApp::default();
    let indexes = [5, 2, 9];
    let order = [1, 0, 3];
    let lookup = GridSelectionLookup::new(&indexes, &order);
    app.table.data.select_all_visible_cells(&indexes, &order);

    assert_eq!(app.table.data.selected_cell, Some((9, 3)));
    assert_eq!(app.table.data.selection_anchor_cell, Some((5, 1)));
    assert!([5, 2, 9].iter().all(|row| app.table.data.selected_rows.contains(row)));
    assert!(app.table.data.is_cell_selected(&lookup, (2, 0)));
}

#[test]
fn table_sort_cycles_and_shift_adds_prioritized_clauses() {
    let mut app = DbProApp {
        workspace: WorkspaceFeatureState {
            shell: WorkspaceShellState {
                active_tab: WorkspaceTab::Table,
                ..Default::default()
            },
            ..Default::default()
        },
        table: TableEditorState {
            state: TableState {
                table_view: TableView::Data,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "tenant_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
            crate::UiColumn {
                name: "item_id".to_owned(),
                data_type: "integer".to_owned(),
                nullable: false,
            },
        ],
        rows: Vec::new(),
        row_count: 0,
        duration_ms: 0,
    };

    app.cycle_table_data_sort(&result, 0, false);
    assert_eq!(app.table.data_query.sorts[0].column, "tenant_id");
    assert!(!app.table.data_query.sorts[0].descending);
    app.cycle_table_data_sort(&result, 0, false);
    assert!(app.table.data_query.sorts[0].descending);
    app.cycle_table_data_sort(&result, 1, true);
    assert_eq!(
        app.table
            .data_query
            .sorts
            .iter()
            .map(|sort| sort.column.as_str())
            .collect::<Vec<_>>(),
        vec!["tenant_id", "item_id"]
    );
    app.cycle_table_data_sort(&result, 0, true);
    assert_eq!(app.table.data_query.sorts.len(), 1);
    assert_eq!(app.table.data_query.sorts[0].column, "item_id");
}

#[test]
fn named_layout_drops_removed_columns_and_appends_new_columns() {
    let mut app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_pending_named_layout: Some(vec![
                    PersistedGridColumnLayout {
                        column_name: "id".to_owned(),
                        width: 240.0,
                        order: 1,
                        hidden: true,
                    },
                    PersistedGridColumnLayout {
                        column_name: "removed".to_owned(),
                        width: 500.0,
                        order: 0,
                        hidden: true,
                    },
                ]),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let columns = vec![
        crate::UiColumn {
            name: "id".to_owned(),
            data_type: "integer".to_owned(),
            nullable: false,
        },
        crate::UiColumn {
            name: "name".to_owned(),
            data_type: "text".to_owned(),
            nullable: true,
        },
    ];

    assert_eq!(app.table.data.column_order_for_columns(&columns), vec![1]);
    assert_eq!(app.table.data.grid_column_order, vec![0, 1]);
    assert_eq!(app.table.data.grid_column_widths, vec![240.0, 180.0]);
    assert_eq!(app.table.data.grid_hidden_columns, [0].into_iter().collect());
}

#[test]
fn named_layout_does_not_map_renamed_column_state() {
    let mut app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_pending_named_layout: Some(vec![PersistedGridColumnLayout {
                    column_name: "old_name".to_owned(),
                    width: 420.0,
                    order: 0,
                    hidden: true,
                }]),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let columns = vec![crate::UiColumn {
        name: "new_name".to_owned(),
        data_type: "text".to_owned(),
        nullable: true,
    }];

    assert_eq!(app.table.data.column_order_for_columns(&columns), vec![0]);
    assert_eq!(app.table.data.grid_column_widths, vec![180.0]);
    assert!(app.table.data.grid_hidden_columns.is_empty());
}

#[test]
fn legacy_layout_is_discarded_when_schema_shape_changes() {
    let mut app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_column_order: vec![1, 0],
                grid_column_widths: vec![300.0, 300.0],
                grid_legacy_layout_pending: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let columns = vec![crate::UiColumn {
        name: "only_column".to_owned(),
        data_type: "text".to_owned(),
        nullable: true,
    }];

    assert_eq!(app.table.data.column_order_for_columns(&columns), vec![0]);
    assert!(app.table.data.grid_column_widths.is_empty());
    assert!(app.table.data.grid_hidden_columns.is_empty());
}

#[test]
fn persisted_layout_is_normalized_when_schema_changes() {
    let mut app = DbProApp {
        table: TableEditorState {
            data: TableDataState {
                grid_column_order: vec![4, 1, 1, 99],
                grid_hidden_columns: [4, 88].into_iter().collect(),
                grid_column_widths: vec![40.0, 120.0, 2000.0, 240.0],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    assert_eq!(app.table.data.column_order(3), vec![1, 0, 2]);
    assert!(app.table.data.grid_hidden_columns.is_empty());
    assert_eq!(app.table.data.grid_column_widths, vec![60.0, 120.0, 1000.0]);
}

#[test]
fn sorting_is_blocked_while_staged_changes_are_present() {
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
                staged_changes: ChangeSet::from(vec![StagedChange::Insert {
                    local_id: 1,
                    columns: vec!["name".to_owned()],
                    data_types: Vec::new(),
                    values: vec![UiCell::Text("draft".to_owned())],
                }]),
                ..Default::default()
            },
            state: TableState {
                table_view: TableView::Data,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let result = UiQueryResult {
        columns: vec![crate::UiColumn {
            name: "name".to_owned(),
            data_type: "text".to_owned(),
            nullable: true,
        }],
        rows: Vec::new(),
        row_count: 0,
        duration_ms: 0,
    };

    app.cycle_table_data_sort(&result, 0, false);

    assert!(app.table.data_query.sorts.is_empty());
    assert!(app.feedback.runtime_message.contains("staged changes"));
}

fn projection_test_result() -> UiQueryResult {
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
            vec![UiCell::Number("2".to_owned()), UiCell::Text("Beta".to_owned())],
            vec![UiCell::Number("1".to_owned()), UiCell::Text("Alpha".to_owned())],
            vec![UiCell::Number("3".to_owned()), UiCell::Text("Gamma".to_owned())],
        ],
        row_count: 3,
        duration_ms: 1,
    }
}

/// Draw the result grid for one frame, the way the app does.
fn draw_grid_frame(app: &mut DbProApp, ctx: &egui::Context, result: &UiQueryResult) {
    DbProTheme::install_fonts(ctx);
    let _ = crate::test_frame::frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 640.0))),
            ..Default::default()
        },
        |ui| {
            app.draw_result_grid(ui, result);
        },
    );
}

#[test]
fn result_grid_does_not_rebuild_the_projection_every_frame() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    let result = projection_test_result();

    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 1);

    // Two more frames with the same result, filter and sort: the projection is reused. This is
    // before the projection cache, every one of these frames paid the full
    // filter/sort pass (seconds per frame on a large temporal-text column).
    draw_grid_frame(&mut app, &ctx, &result);
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 1);
}

#[test]
fn result_grid_rebuilds_the_projection_when_an_input_changes() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    let result = projection_test_result();

    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 1);

    app.table.data.grid_sort_column = Some(1);
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 2);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 2);

    app.table.data.grid_sort_desc = true;
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 3);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 3);

    app.table.data.grid_filter = "alpha".to_owned();
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 4);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 4);

    // A new result set behind the same filter and sort: only the epoch differs.
    app.table.data.invalidate_grid_projection();
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 5);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 5);

    // And a frame that changes nothing reuses that one.
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 5);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 5);
}

/// A projection key shaped like the draw path's, for the selection-cache unit tests.
fn selection_test_key(epoch: u64, sort_column: Option<usize>) -> GridProjectionKey {
    let result = projection_test_result();
    GridProjectionKey {
        epoch,
        filter: String::new(),
        sort_column,
        sort_desc: false,
        row_count: result.row_count,
        column_count: result.columns.len(),
    }
}

/// One frame of the draw path's cache protocol: take, build on a miss, hand the lookup back.
fn selection_cache_frame(cache: &mut GridSelectionCache, key: &GridProjectionKey, order: &[usize], indexes: &[usize]) {
    let lookup = cache
        .take(key, order)
        .unwrap_or_else(|| GridSelectionLookup::new(indexes, order));
    cache.restore(key.clone(), order.to_vec(), lookup);
}

#[test]
fn selection_cache_reuses_the_lookup_across_frames() {
    let mut cache = GridSelectionCache::default();
    let key = selection_test_key(0, None);
    let order = vec![1, 0];
    let indexes = [2usize, 0, 1];

    selection_cache_frame(&mut cache, &key, &order, &indexes);
    assert_eq!(cache.rebuilds(), 1);

    // The next frame takes the lookup out instead of building it again, and what it gets is the
    // map for this projection and this visual order — not the default row or column order.
    let reused = cache.take(&key, &order).expect("the frame's lookup is reused");
    assert_eq!(reused.row_positions.get(&2), Some(&0));
    assert_eq!(reused.row_positions.get(&0), Some(&1));
    assert_eq!(reused.row_positions.get(&1), Some(&2));
    assert_eq!(reused.column_positions.get(&1), Some(&0));
    assert_eq!(reused.column_positions.get(&0), Some(&1));
    cache.restore(key.clone(), order.clone(), reused);

    selection_cache_frame(&mut cache, &key, &order, &indexes);
    assert_eq!(cache.rebuilds(), 1);
}

#[test]
fn selection_cache_rebuilds_when_the_projection_or_the_column_order_changes() {
    let mut cache = GridSelectionCache::default();
    let order = vec![0, 1];
    let indexes = [0usize, 1, 2];

    selection_cache_frame(&mut cache, &selection_test_key(0, None), &order, &indexes);
    assert_eq!(cache.rebuilds(), 1);

    // The rows did not move, but the visual column positions the lookup resolves did.
    selection_cache_frame(&mut cache, &selection_test_key(0, None), &[1, 0], &indexes);
    assert_eq!(cache.rebuilds(), 2);

    // A sort moves the rows, so the cached positions are stale.
    selection_cache_frame(&mut cache, &selection_test_key(0, Some(0)), &order, &indexes);
    assert_eq!(cache.rebuilds(), 3);

    // A new result set behind the same filter and sort: only the epoch differs.
    selection_cache_frame(&mut cache, &selection_test_key(1, Some(0)), &order, &indexes);
    assert_eq!(cache.rebuilds(), 4);

    // And an unchanged frame reuses the entry the previous one handed back.
    selection_cache_frame(&mut cache, &selection_test_key(1, Some(0)), &order, &indexes);
    assert_eq!(cache.rebuilds(), 4);
}

#[test]
fn selection_cache_keeps_a_newer_entry_over_the_frame_copy() {
    let mut cache = GridSelectionCache::default();
    let stale_key = selection_test_key(0, None);
    let newer_key = selection_test_key(0, Some(1));
    let order = vec![0, 1];
    let indexes = [0usize, 1, 2];
    let frame_lookup = cache
        .take(&stale_key, &order)
        .unwrap_or_else(|| GridSelectionLookup::new(&indexes, &order));
    assert_eq!(cache.rebuilds(), 1);

    // Something rebuilt during the same frame (a toolbar action changed the sort).
    cache.restore(
        newer_key.clone(),
        order.clone(),
        GridSelectionLookup::new(&indexes, &[1, 0]),
    );

    // The frame's own copy must not overwrite the newer entry.
    cache.restore(stale_key, order.clone(), frame_lookup);
    let kept = cache.take(&newer_key, &order).expect("the newer entry survives");
    assert_eq!(kept.column_positions.get(&0), Some(&1));
    assert_eq!(cache.rebuilds(), 1);
}

#[test]
fn result_grid_rebuilds_the_selection_lookup_when_the_column_order_changes() {
    let mut app = DbProApp::default();
    let ctx = egui::Context::default();
    let result = projection_test_result();

    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 1);

    // Reordering the columns leaves the rows and their order alone, so the projection is reused,
    // while the visual positions the selection lookup resolves are now different.
    app.table.data.grid_column_order = vec![1, 0];
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 2);

    // And a frame that changes neither reuses both.
    draw_grid_frame(&mut app, &ctx, &result);
    assert_eq!(app.table.data.grid_projection_cache.rebuilds(), 1);
    assert_eq!(app.table.data.grid_selection_cache.rebuilds(), 2);
}

#[test]
fn sql_insert_and_copy_export_preserve_null_and_quotes() {
    let result = crate::UiQueryResult {
        columns: vec![
            crate::UiColumn {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
            },
            crate::UiColumn {
                name: "name".into(),
                data_type: "text".into(),
                nullable: true,
            },
        ],
        rows: vec![
            vec![crate::UiCell::Number("1".into()), crate::UiCell::Text("O'Brien".into())],
            vec![crate::UiCell::Number("2".into()), crate::UiCell::Null],
        ],
        row_count: 2,
        duration_ms: 0,
    };
    let insert = result_grid_export::format_result_sql_insert(&result, "people", "postgres");
    assert!(insert.contains("INSERT INTO \"people\""));
    assert!(insert.contains("'O''Brien'"));
    assert!(insert.contains("NULL"));
    let copy = result_grid_export::format_result_copy(&result, "people", "postgres");
    assert!(copy.contains("COPY \"people\""));
    assert!(copy.contains("\\N"));
    assert!(copy.contains("\\."));

    let md = result_grid_export::format_result_markdown(&result);
    assert!(md.contains("| id | name |"));
    assert!(md.contains("| 1 | O'Brien |"));
    assert!(md.contains("| 2 | NULL |"));

    let json = result_grid_export::format_result_json(&result);
    assert!(json.contains("\"id\": 1"));
    assert!(json.contains("\"name\": \"O'Brien\""));
    assert!(json.contains("\"name\": null"));
}

/// #UAT-2: the grid focus claim used one global `Id::new("db-pro-result-grid")`,
/// so a table grid and the output-dock grid in the same frame collided on it.
/// The focus id must be distinct per grid `Ui`.
#[test]
fn result_grid_focus_id_is_scoped_per_grid_ui() {
    let ctx = egui::Context::default();
    let mut ids = Vec::new();
    let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
        ui.horizontal(|ui| {
            ids.push(result_grid_keyboard_view::result_grid_focus_id(ui));
        });
        ui.horizontal(|ui| {
            ids.push(result_grid_keyboard_view::result_grid_focus_id(ui));
        });
    });
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "sibling grids must not share a focus id");
}

/// The docked query grid publishes its ui-scoped focus id so `handle_shortcuts`
/// can request focus without a `Ui`; the table grid does not claim that slot.
#[test]
fn query_result_grid_focus_id_records_only_query_grids() {
    let ctx = egui::Context::default();
    assert!(result_grid_keyboard_view::query_result_grid_focus_id(&ctx).is_none());
    let _ = crate::test_frame::frame(&ctx, egui::RawInput::default(), |ui| {
        ui.horizontal(|ui| {
            result_grid_keyboard_view::record_query_result_grid_focus_id(ui);
        });
    });
    let recorded = result_grid_keyboard_view::query_result_grid_focus_id(&ctx);
    assert!(recorded.is_some());
    assert_ne!(recorded, Some(egui::Id::new("db-pro-result-grid")));
}
