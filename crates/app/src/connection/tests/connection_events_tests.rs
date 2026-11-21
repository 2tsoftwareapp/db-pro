//! Tests for `../connection_events.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn request_failure_classification_uses_typed_pending_operation() {
        let mut lifecycle = ConnectionLifecycleState::default();
        lifecycle.set_pending_request(Some(RequestId(7)));
        lifecycle.set_pending_operation(Some(PendingConnectionOperation::Delete));
        lifecycle.set_pending_connection_id(Some("conn-1".to_owned()));
        let mut dialog = ConnectionDialogState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut feedback = FeedbackState::default();
        feedback.set_runtime_message("Connecting to Local…");

        assert!(handle_connection_request_failure(
            &mut lifecycle,
            &mut dialog,
            &mut schema_explorer,
            &mut feedback,
            RequestId(7),
            "permission denied",
        ));

        assert_eq!(feedback.runtime_message, "Delete failed · permission denied");
        assert!(lifecycle.pending_operation().is_none());
    }

    #[test]
    fn test_failure_for_draft_does_not_mark_active_connection() {
        let mut lifecycle = ConnectionLifecycleState::default();
        lifecycle.set_pending_request(Some(RequestId(9)));
        lifecycle.set_pending_operation(Some(PendingConnectionOperation::Test));
        // Draft test: no pending_connection_id is set (workspace_actions only
        // sets one for Connect/Delete).
        lifecycle.set_active_connection_id(Some("sample-sqlite".to_owned()));
        lifecycle.set_connected(true);
        let mut dialog = ConnectionDialogState::default();
        dialog.set_open(true);
        let mut schema_explorer = SchemaExplorerState::default();
        let mut feedback = FeedbackState::default();

        assert!(handle_connection_request_failure(
            &mut lifecycle,
            &mut dialog,
            &mut schema_explorer,
            &mut feedback,
            RequestId(9),
            "password authentication failed for user \"dbpro\"",
        ));

        assert!(!lifecycle.has_failed_connection("sample-sqlite"));
        assert_eq!(lifecycle.connection_error("sample-sqlite"), None);
        assert!(lifecycle.is_connected(), "active connection must stay connected");
        assert_eq!(
            feedback.runtime_message,
            "Connection test failed · password authentication failed for user \"dbpro\""
        );
    }

    #[test]
    fn save_failure_for_new_connection_does_not_mark_active_connection() {
        let mut lifecycle = ConnectionLifecycleState::default();
        lifecycle.set_pending_request(Some(RequestId(10)));
        lifecycle.set_pending_operation(Some(PendingConnectionOperation::Save));
        lifecycle.set_active_connection_id(Some("sample-sqlite".to_owned()));
        lifecycle.set_connected(true);
        let mut dialog = ConnectionDialogState::default();
        dialog.set_open(true);
        let mut schema_explorer = SchemaExplorerState::default();
        let mut feedback = FeedbackState::default();

        assert!(handle_connection_request_failure(
            &mut lifecycle,
            &mut dialog,
            &mut schema_explorer,
            &mut feedback,
            RequestId(10),
            "duplicate key value",
        ));

        assert!(!lifecycle.has_failed_connection("sample-sqlite"));
        assert!(lifecycle.is_connected());
        assert_eq!(feedback.runtime_message, "Save failed · duplicate key value");
    }

    #[test]
    fn connect_failure_still_marks_active_connection() {
        let mut lifecycle = ConnectionLifecycleState::default();
        lifecycle.set_pending_request(Some(RequestId(11)));
        lifecycle.set_pending_operation(Some(PendingConnectionOperation::Connect));
        lifecycle.set_active_connection_id(Some("pg-shop".to_owned()));
        lifecycle.set_connected(true);
        let mut dialog = ConnectionDialogState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut feedback = FeedbackState::default();

        assert!(handle_connection_request_failure(
            &mut lifecycle,
            &mut dialog,
            &mut schema_explorer,
            &mut feedback,
            RequestId(11),
            "connection refused",
        ));

        assert!(lifecycle.has_failed_connection("pg-shop"));
        assert!(!lifecycle.is_connected());
        assert_eq!(feedback.runtime_message, "Connection failed · connection refused");
    }