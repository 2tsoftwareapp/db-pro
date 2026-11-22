//! Tests for `../table_editor_state.rs` (moved out of the source file).
    use super::super::{RowIdentity, UiCell};
    use crate::RequestId;

    use super::*;

    #[test]
    fn reset_workspace_clears_table_requests_selection_and_staged_mutations() {
        let mut state = TableEditorState::default();
        state.state.table_info_request = Some(RequestId(7));
        state.state.table_ddl_request = Some(RequestId(8));
        state.data.select_single_row(2);
        state.data.grid_row_identity_cache_ready = true;
        state.data.grid_row_identity_cache.insert(
            0,
            RowIdentity {
                original_pk_columns: vec!["id".to_owned()],
                original_pk_values: vec![UiCell::Number("1".to_owned())],
                original_pk_data_types: Vec::new(),
            },
        );
        state.mutation.pending_changes_open = true;
        state.editing.data_edit_value = "draft".to_owned();
        state.editing.data_delete_confirmation = true;
        state.editing.discard_changes_confirmation = true;

        state.reset_workspace();

        assert!(state.state.table_info_request.is_none());
        assert!(state.state.table_ddl_request.is_none());
        assert!(state.data.selected_rows.is_empty());
        assert!(!state.data.grid_row_identity_cache_ready);
        assert!(state.data.grid_row_identity_cache.is_empty());
        assert!(state.mutation.staged_changes.is_empty());
        assert!(!state.mutation.pending_changes_open);
        assert!(state.editing.data_edit_value.is_empty());
        assert!(!state.editing.data_delete_confirmation);
        assert!(!state.editing.discard_changes_confirmation);
        assert_eq!(state.state.table_view, TableView::Data);
    }