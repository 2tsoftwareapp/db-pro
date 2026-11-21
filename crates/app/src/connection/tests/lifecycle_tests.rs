//! Tests for `../lifecycle.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn clearing_connection_error_removes_failure_marker_and_message() {
        let mut state = ConnectionLifecycleState {
            errors: [("conn-1".to_owned(), "failed".to_owned())].into_iter().collect(),
            failed_connection_ids: ["conn-1".to_owned()].into_iter().collect(),
            ..Default::default()
        };

        state.clear_connection_error("conn-1");

        assert!(state.errors.is_empty());
        assert!(state.failed_connection_ids.is_empty());
    }

    #[test]
    fn default_lifecycle_starts_disconnected_with_a_local_fallback_name() {
        let state = ConnectionLifecycleState {
            fallback_name: "Local PostgreSQL".to_owned(),
            ..Default::default()
        };

        assert!(!state.connected);
        assert_eq!(state.fallback_name, "Local PostgreSQL");
    }

    #[test]
    fn live_sessions_track_open_and_close() {
        let mut state = ConnectionLifecycleState::default();
        assert!(!state.has_live_session("pg-1"));
        state.note_session_opened("pg-1");
        state.note_session_opened("sqlite-1");
        assert!(state.has_live_session("pg-1"));
        assert!(state.has_live_session("sqlite-1"));
        state.note_session_closed("pg-1");
        assert!(!state.has_live_session("pg-1"));
        assert!(state.has_live_session("sqlite-1"));
    }
