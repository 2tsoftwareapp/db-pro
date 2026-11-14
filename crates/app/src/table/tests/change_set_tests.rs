//! Tests for `../change_set.rs` (moved out of the source file).
    use super::*;

    fn update(value: &str) -> StagedChange {
        StagedChange::Update {
            identity: RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("1".to_owned())],
                original_pk_data_types: Vec::new(),
            },
            current_row_index: Some(1),
            column_index: 2,
            column: "name".to_owned(),
            data_type: "TEXT".to_owned(),
            original: UiCell::Text("old".to_owned()),
            value: UiCell::Text(value.to_owned()),
        }
    }

    #[test]
    fn remove_applied_targets_keeps_unrelated_staged_rows() {
        let id_a = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("1".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        let id_b = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("2".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        let mut changes = ChangeSet::new();
        changes.stage_update(StagedChange::Update {
            identity: id_a.clone(),
            current_row_index: Some(0),
            column_index: 1,
            column: "name".to_owned(),
            data_type: "TEXT".to_owned(),
            original: UiCell::Text("a".to_owned()),
            value: UiCell::Text("a1".to_owned()),
        });
        changes.stage_update(StagedChange::Update {
            identity: id_b.clone(),
            current_row_index: Some(1),
            column_index: 1,
            column: "name".to_owned(),
            data_type: "TEXT".to_owned(),
            original: UiCell::Text("b".to_owned()),
            value: UiCell::Text("b1".to_owned()),
        });
        let targets = vec![MutationTarget::Update {
            identity: id_b,
            current_row_index: Some(1),
            columns: vec![1],
        }];
        changes.remove_applied_targets(&targets);
        assert_eq!(changes.counts().updates, 1);
        assert!(changes.iter().any(|entry| matches!(
            entry,
            StagedChange::Update { identity, .. } if *identity == id_a
        )));
    }

    #[test]
    fn repeated_cell_updates_keep_only_final_value() {
        let mut changes = ChangeSet::new();
        changes.stage_update(update("one"));
        changes.stage_update(update("two"));

        assert_eq!(changes.counts().updates, 1);
        assert!(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            matches!(changes.iter().next(), Some(StagedChange::Update { value: UiCell::Text(value), .. }) if value == "two")
        );
    }

    #[test]
    fn updates_with_same_original_identity_merge_across_row_coordinates() {
        let mut changes = ChangeSet::new();
        changes.stage_update(update("one"));
        changes.stage_update(StagedChange::Update {
            identity: RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("1".to_owned())],
                original_pk_data_types: Vec::new(),
            },
            current_row_index: Some(99),
            column_index: 3,
            column: "active".to_owned(),
            data_type: "BOOLEAN".to_owned(),
            original: UiCell::Boolean(false),
            value: UiCell::Boolean(true),
        });

        assert_eq!(changes.counts().updates, 1);
        assert_eq!(changes.iter().count(), 2);
    }

    #[test]
    fn composite_identity_keeps_rows_with_shared_pk_prefix_separate() {
        let mut changes = ChangeSet::new();
        let first = RowIdentity {
            original_pk_columns: vec!["tenant_id".to_owned(), "item_id".to_owned()],
            original_pk_values: vec![UiCell::Number("1".to_owned()), UiCell::Number("10".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        let second = RowIdentity {
            original_pk_columns: first.original_pk_columns.clone(),
            original_pk_values: vec![UiCell::Number("1".to_owned()), UiCell::Number("11".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        changes.stage_update(StagedChange::Update {
            identity: first.clone(),
            current_row_index: Some(4),
            column_index: 2,
            column: "name".to_owned(),
            data_type: "text".to_owned(),
            original: UiCell::Text("old-a".to_owned()),
            value: UiCell::Text("new-a".to_owned()),
        });
        changes.stage_update(StagedChange::Update {
            identity: second,
            current_row_index: Some(1),
            column_index: 2,
            column: "name".to_owned(),
            data_type: "text".to_owned(),
            original: UiCell::Text("old-b".to_owned()),
            value: UiCell::Text("new-b".to_owned()),
        });

        assert_eq!(changes.counts().updates, 2);
        assert_eq!(changes.cell_value(&first, 2), Some(UiCell::Text("new-a".to_owned())));
    }

    #[test]
    fn staged_value_follows_identity_when_visual_row_changes() {
        let mut changes = ChangeSet::new();
        let identity = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("7".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        changes.stage_update(StagedChange::Update {
            identity: identity.clone(),
            current_row_index: Some(9),
            column_index: 1,
            column: "name".to_owned(),
            data_type: "text".to_owned(),
            original: UiCell::Text("old".to_owned()),
            value: UiCell::Text("new".to_owned()),
        });

        assert_eq!(changes.cell_value(&identity, 1), Some(UiCell::Text("new".to_owned())));
        assert!(changes.row_has_changes(&identity));
    }

    #[test]
    fn reverting_cell_removes_the_staged_update() {
        let mut changes = ChangeSet::new();
        changes.stage_update(update("one"));
        let identity = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("1".to_owned())],
            original_pk_data_types: Vec::new(),
        };

        assert!(changes.revert_cell(&identity, 2));
        assert!(changes.is_empty());
    }

    #[test]
    fn delete_supersedes_all_updates_for_the_same_row() {
        let mut changes = ChangeSet::new();
        changes.stage_update(update("one"));
        changes.stage_update(StagedChange::Update {
            identity: RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("1".to_owned())],
                original_pk_data_types: Vec::new(),
            },
            current_row_index: Some(1),
            column_index: 3,
            column: "active".to_owned(),
            data_type: "BOOLEAN".to_owned(),
            original: UiCell::Boolean(false),
            value: UiCell::Boolean(true),
        });

        changes.stage_delete(StagedChange::Delete {
            identity: RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("1".to_owned())],
                original_pk_data_types: Vec::new(),
            },
            current_row_index: Some(42),
        });

        assert_eq!(changes.counts().updates, 0);
        assert_eq!(changes.counts().deletes, 1);
        assert!(matches!(changes.iter().next(), Some(StagedChange::Delete { .. })));
    }

    #[test]
    fn insert_delete_removes_the_local_insert_before_apply() {
        let mut changes = ChangeSet::new();
        let local_id = changes.stage_insert(vec!["name".to_owned()], vec![UiCell::Text("draft".to_owned())]);

        assert!(changes.remove_insert(local_id));
        assert!(changes.is_empty());
    }

    #[test]
    fn staging_original_value_reverts_noop_without_a_pending_change() {
        let mut changes = ChangeSet::new();
        changes.stage_update(update("one"));
        changes.stage_update(update("old"));

        assert!(changes.is_empty());
        assert_eq!(changes.counts().total(), 0);
    }