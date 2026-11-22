//! Tests for `../mapper.rs` (moved out of the source file).
    use super::*;
    use crate::UiDriver;

    #[test]
    fn apply_snippet_clears_cloud_snippet_after_success() {
        let snippet = "postgresql://alice:s3cret@db.example.com:5432/appdb?sslmode=require";
        let mut draft = UiConnectionDraft {
            cloud_snippet: snippet.to_owned(),
            ..UiConnectionDraft::default()
};

        assert_eq!(apply_connection_snippet(&mut draft, snippet), Ok(()));
        assert_eq!(draft.host, "db.example.com");
        assert_eq!(draft.port, "5432");
        assert_eq!(draft.database, "appdb");
        assert_eq!(draft.username, "alice");
        assert_eq!(draft.password, "s3cret");
        assert_eq!(draft.driver, UiDriver::Postgres);
        assert_eq!(draft.ssl_mode, UiSslMode::Require);
        assert_eq!(draft.cloud_snippet, "");
    }

    #[test]
    fn apply_snippet_keeps_cloud_snippet_on_parse_error() {
        let snippet = "not-a-uri";
        let mut draft = UiConnectionDraft {
            cloud_snippet: snippet.to_owned(),
            ..UiConnectionDraft::default()
};

        assert!(apply_connection_snippet(&mut draft, snippet).is_err());
        assert_eq!(draft.cloud_snippet, snippet);
    }
