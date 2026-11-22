//! Tests for `../catalog.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn replace_replaces_the_saved_connection_read_model() {
        let mut catalog = ConnectionCatalogState::default();
        catalog.replace(vec![UiConnectionSummary {
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
            id: "conn-1".to_owned(),
            name: "Local".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: "app".to_owned(),
            username: "postgres".to_owned(),
            driver: "PostgreSQL".to_owned(),
            ssl_mode: crate::UiSslMode::Require,
            readonly: false,
            create_if_missing: false,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        }]);

        assert_eq!(
            catalog.find("conn-1").map(|connection| connection.name.as_str()),
            Some("Local")
        );
        assert!(catalog.find("missing").is_none());
    }