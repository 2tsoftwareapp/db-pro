//! Tests for `../table_mutation_state.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn default_table_mutation_state_is_idle_and_has_no_staged_changes() {
        let state = TableMutationState::default();

        assert!(state.staged_changes.is_empty());
        assert!(state.table_mutation_request.is_none());
        assert!(state.staged_apply_request.is_none());
        assert!(!state.pending_changes_open);
        assert!(!state.conflict_dialog_open);
    }

    #[test]
    fn clearing_matching_mutation_error_is_scoped_to_the_target_cell_or_row() {
        let identity = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("7".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        let mut state = TableMutationState {
            table_mutation_error: Some(MutationFailure {
                statement_index: 0,
                target: Some(MutationTarget::Update {
                    identity: identity.clone(),
                    current_row_index: Some(2),
                    columns: vec![1],
                }),
                code: "23505".to_owned(),
                message: "duplicate".to_owned(),
                rolled_back: true,
            }),
            ..Default::default()
        };

        assert!(!state.clear_error_for_identity(&identity, Some(0)));
        assert!(state.table_mutation_error.is_some());
        assert!(state.clear_error_for_identity(&identity, Some(1)));
        assert!(state.table_mutation_error.is_none());
    }

    #[test]
    fn apply_plan_groups_updates_and_preserves_delete_update_insert_order() {
        let identity = RowIdentity {
            original_pk_columns: vec!["id".to_owned()],
            original_pk_values: vec![UiCell::Number("7".to_owned())],
            original_pk_data_types: Vec::new(),
        };
        let mut state = TableMutationState::default();
        state.staged_changes.stage_update(StagedChange::Update {
            identity: identity.clone(),
            current_row_index: Some(2),
            column_index: 1,
            column: "name".to_owned(),
            data_type: "text".to_owned(),
            original: UiCell::Text("old".to_owned()),
            value: UiCell::Text("new".to_owned()),
        });
        state.staged_changes.stage_delete(StagedChange::Delete {
            identity: RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("8".to_owned())],
                original_pk_data_types: Vec::new(),
            },
            current_row_index: Some(3),
        });
        state
            .staged_changes
            .stage_insert(vec!["name".to_owned()], vec![UiCell::Text("inserted".to_owned())]);

        let plan = state.build_apply_plan();

        assert_eq!(plan.changes.len(), 3);
        assert_eq!(plan.targets.len(), 3);
        assert!(matches!(plan.changes[0], UiTableMutation::Delete { .. }));
        assert!(matches!(plan.changes[1], UiTableMutation::Update { .. }));
        assert!(matches!(plan.changes[2], UiTableMutation::Insert { .. }));
    }

    #[test]
    fn row_reload_filters_are_built_from_primary_key_metadata() {
        let info = UiTableInfo {
            schema: "public".to_owned(),
            name: "customers".to_owned(),
            row_count: None,
            columns: vec![
                crate::UiTableColumn {
                    name: "tenant_id".to_owned(),
                    data_type: "uuid".to_owned(),
                    ..Default::default()
                },
                crate::UiTableColumn {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    ..Default::default()
                },
            ],
            primary_key: Some(vec!["tenant_id".to_owned(), "id".to_owned()]),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
            dependencies: Vec::new(),
        };
        let identity = RowIdentity {
            original_pk_columns: vec!["tenant_id".to_owned(), "id".to_owned()],
            original_pk_values: vec![UiCell::Text("tenant-a".to_owned()), UiCell::Number("7".to_owned())],
            original_pk_data_types: Vec::new(),
        };

        let filters = TableMutationState::row_reload_filters(&info, &identity).expect("filters");

        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].column, "tenant_id");
        assert_eq!(filters[0].data_type, "uuid");
        assert_eq!(filters[1].column, "id");
        assert_eq!(filters[1].value, "7");
    }