use super::*;
use crate::domain::connection::ConnectionHandle;
use crate::domain::query::{CellValue, ColumnMeta, QueryResult, Row};
use crate::domain::routine_debug::RoutineDebugState;
use crate::domain::schema::*;
use crate::ports::MockIntrospectionCache;
use crate::ports::{MockConnectionRepository, MockDbConnector};

fn connections_for(driver: DriverType) -> MockConnectionRepository {
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(move |_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "test".into(),
            host: "localhost".into(),
            port: 5432,
            database: "testdb".into(),
            username: "user".into(),
            driver,
            ssl_mode: crate::domain::connection::SslMode::Disable,
            ssh_tunnel: None,
            ssh_profile_id: None,
            ssl_root_cert_path: None,
            ssl_client_cert_path: None,
            ssl_client_key_path: None,
            query_timeout_ms: 30_000,
            max_rows: 500,
            color: None,
            tags: vec![],
            group: None,
            favorite: false,
            environment: Default::default(),
            readonly: false,
            create_if_missing: false,
}))
    });
    repo
}

fn mock_connections() -> MockConnectionRepository {
    connections_for(DriverType::Postgres)
}

fn sqlite_connections() -> MockConnectionRepository {
    connections_for(DriverType::SQLite)
}

fn test_introspect_result() -> IntrospectResult {
    IntrospectResult {
        schemas: vec![Schema { name: "public".into() }],
        tables: vec![Table {
            name: "users".into(),
            schema: "public".into(),
            row_count: Some(100),
        }],
        columns: vec![
            Column {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                default: None,
                is_primary_key: true,
                table_name: "users".into(),
                schema: "public".into(),
                ..Default::default()
            },
            Column {
                name: "email".into(),
                data_type: "TEXT".into(),
                nullable: false,
                default: None,
                is_primary_key: false,
                table_name: "users".into(),
                schema: "public".into(),
                ..Default::default()
            },
        ],
        primary_keys: vec![PrimaryKey {
            constraint_name: "users_pk".into(),
            columns: vec!["id".into()],
            table_name: "users".into(),
            schema: "public".into(),
        }],
        indexes: vec![Index {
            name: "idx_email".into(),
            columns: vec!["email".into()],
            unique: true,
            origin: IndexOrigin::User,
            table_name: "users".into(),
            schema: "public".into(),
            ..Default::default()
        }],
        foreign_keys: vec![],
        views: vec![View {
            name: "active_users".into(),
            schema: "public".into(),
            definition: "SELECT id, email FROM users WHERE active = true".into(),
            kind: Default::default(),
        }],
        triggers: vec![Trigger {
            name: "audit_insert".into(),
            table_name: "users".into(),
            schema: "public".into(),
            timing: "AFTER".into(),
            event: "INSERT".into(),
            definition: "EXECUTE FUNCTION audit_fn()".into(),
            function_def: String::new(),
            enabled: true,
        }],
        check_constraints: vec![],
        functions: vec![],
        table_definition_sql: vec![],
    }
}

#[tokio::test]
async fn introspect_cache_miss() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(None));
    cache.expect_save().returning(|_, _| Ok(()));

    let mut connector = MockDbConnector::new();
    connector
        .expect_introspect()
        .returning(|_| Ok(test_introspect_result()));

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let result = svc.introspect(&conn_id, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn introspect_cache_hit() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));

    let connector = MockDbConnector::new();

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let result = svc.introspect(&conn_id, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn introspect_discards_sqlite_cache_when_database_file_is_empty() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let empty_db = std::env::temp_dir().join(format!("db-pro-empty-schema-cache-{}.sqlite", std::process::id()));
    std::fs::write(&empty_db, []).expect("create empty sqlite file");

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));
    cache.expect_invalidate().returning(|_| Ok(()));
    cache.expect_save().returning(|_, _| Ok(()));

    let mut connector = MockDbConnector::new();
    connector.expect_introspect().returning(|_| {
        Ok(IntrospectResult {
            schemas: vec![Schema { name: "main".into() }],
            tables: vec![],
            columns: vec![],
            primary_keys: vec![],
            indexes: vec![],
            foreign_keys: vec![],
            views: vec![],
            triggers: vec![],
            check_constraints: vec![],
            functions: vec![],
            table_definition_sql: vec![],
        })
    });

    let path = empty_db.to_string_lossy().into_owned();
    let mut repo = MockConnectionRepository::new();
    repo.expect_get_config().returning(move |_id| {
        Ok(Some(crate::domain::connection::ConnectionConfig {
            name: "empty-sqlite".into(),
            host: "localhost".into(),
            port: 0,
            database: path.clone(),
            username: String::new(),
            driver: DriverType::SQLite,
            ssl_mode: crate::domain::connection::SslMode::Disable,
            ssh_tunnel: None,
            ssh_profile_id: None,
            ssl_root_cert_path: None,
            ssl_client_cert_path: None,
            ssl_client_key_path: None,
            query_timeout_ms: 30_000,
            max_rows: 500,
            color: None,
            tags: vec![],
            group: None,
            favorite: false,
            environment: Default::default(),
            readonly: false,
            create_if_missing: false,
}))
    });

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(repo),
    );

    let result = svc.introspect(&conn_id, false).await.expect("refresh");
    assert!(result.tables.is_empty(), "live empty DB must replace stale cache");
    let _ = std::fs::remove_file(&empty_db);
}

#[test]
fn empty_sqlite_file_does_not_match_nonempty_cache() {
    let empty_db = std::env::temp_dir().join(format!("db-pro-fingerprint-{}.sqlite", std::process::id()));
    std::fs::write(&empty_db, []).unwrap();
    let cached = test_introspect_result();
    assert!(!sqlite_file_matches_cached_schema(empty_db.to_str().unwrap(), &cached));
    let _ = std::fs::remove_file(&empty_db);
}

#[tokio::test]
async fn introspect_invalid_cache_is_discarded_and_rebuilt() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_get()
        .returning(|_| Err(DbError::Internal("invalid cached schema".into())));
    cache.expect_invalidate().returning(|_| Ok(()));
    cache.expect_save().returning(|_, _| Ok(()));

    let mut connector = MockDbConnector::new();
    connector
        .expect_introspect()
        .returning(|_| Ok(test_introspect_result()));

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let result = svc.introspect(&conn_id, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn introspect_force_refresh() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));
    cache.expect_save().returning(|_, _| Ok(()));

    let mut connector = MockDbConnector::new();
    connector
        .expect_introspect()
        .returning(|_| Ok(test_introspect_result()));

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let result = svc.introspect(&conn_id, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_table_info_found() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));

    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let info = svc.get_table_info(&conn_id, "public", "users").await;
    assert!(info.is_ok());
    let info = info.unwrap();
    assert_eq!(info.columns.len(), 2);
    assert!(info.primary_key.is_some());
    assert_eq!(info.indexes.len(), 1);
}

#[tokio::test]
async fn get_table_info_not_found() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));

    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let result = svc.get_table_info(&conn_id, "public", "nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_table_ddl_view() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));

    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let ddl = svc.get_table_ddl(&conn_id, "public", "active_users").await.unwrap();
    assert!(ddl.contains("SELECT id, email FROM users WHERE active = true"));
}

#[tokio::test]
async fn get_table_ddl_basic() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(|_| Ok(Some(test_introspect_result())));

    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let ddl = svc.get_table_ddl(&conn_id, "public", "users").await.unwrap();
    assert!(ddl.contains("CREATE TABLE \"public\".\"users\""));
    assert!(ddl.contains("\"id\" INTEGER NOT NULL"));
    assert!(ddl.contains("PRIMARY KEY (\"id\")"));
    assert!(ddl.contains("CREATE UNIQUE INDEX \"idx_email\" ON \"public\".\"users\""));
    assert!(ddl.contains("CREATE TRIGGER"));
    assert!(ddl.contains("audit_insert"));
    assert!(ddl.contains("AFTER INSERT"));
}

#[tokio::test]
async fn get_table_ddl_sqlite_uses_inline_foreign_keys_and_unqualified_names() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut introspection = test_introspect_result();
    introspection.schemas = vec![Schema { name: "main".into() }];
    introspection.tables[0].schema = "main".into();
    for column in &mut introspection.columns {
        column.schema = "main".into();
    }
    introspection.primary_keys[0].schema = "main".into();
    introspection.indexes[0].schema = "main".into();
    introspection.indexes.push(Index {
        name: "sqlite_autoindex_users_1".into(),
        columns: vec!["email".into()],
        unique: true,
        origin: IndexOrigin::UniqueConstraint,
        table_name: "users".into(),
        schema: "main".into(),
        ..Default::default()
    });
    introspection.check_constraints = vec![CheckConstraint {
        name: "users_check_0".into(),
        table_name: "users".into(),
        schema: "main".into(),
        definition: "id > 0".into(),
    }];
    introspection.foreign_keys = vec![ForeignKey {
        name: "users_fk_0".into(),
        from_table: "users".into(),
        from_columns: vec!["id".into()],
        to_table: "parents".into(),
        to_columns: vec!["id".into()],
        schema: "main".into(),
        to_schema: "main".into(),
        ..Default::default()
    }];

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(move |_| Ok(Some(introspection.clone())));

    let service = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(sqlite_connections()),
    );

    let ddl = service.get_table_ddl(&conn_id, "main", "users").await.unwrap();

    assert!(ddl.contains("CREATE TABLE \"users\""));
    assert!(ddl.contains("CONSTRAINT \"users_fk_0\" FOREIGN KEY (\"id\") REFERENCES \"parents\" (\"id\")"));
    assert!(ddl.contains("CONSTRAINT \"users_check_0\" CHECK (id > 0)"));
    assert!(ddl.contains("CREATE UNIQUE INDEX \"idx_email\" ON \"users\""));
    assert!(ddl.contains("UNIQUE (\"email\")"));
    assert!(!ddl.contains("sqlite_autoindex_users_1"));
    assert!(!ddl.contains("ADD CONSTRAINT"));
    assert!(!ddl.contains("\"main\"."));
}

#[test]
fn empty_schema_ddl_generation_omits_prefix() {
    let info = TableInfo {
        table: Table {
            name: "users".into(),
            schema: "".into(),
            row_count: None,
        },
        columns: vec![Column {
            name: "id".into(),
            data_type: "INTEGER".into(),
            nullable: false,
            default: None,
            is_primary_key: true,
            table_name: "users".into(),
            schema: "".into(),
            ..Default::default()
        }],
        primary_key: None,
        indexes: vec![],
        foreign_keys: vec![ForeignKey {
            name: "fk_parent".into(),
            from_table: "users".into(),
            from_columns: vec!["parent_id".into()],
            to_table: "parents".into(),
            to_columns: vec!["id".into()],
            schema: "".into(),
            to_schema: "".into(),
            ..Default::default()
        }],
        check_constraints: vec![],
        dependencies: vec![],
    };

    let ddl = build_create_table_ddl(&info, DriverType::Postgres, &[]);
    assert!(ddl.contains("CREATE TABLE \"users\""));
    assert!(ddl.contains("REFERENCES \"parents\""));

    let trigger = Trigger {
        name: "tr_test".into(),
        table_name: "users".into(),
        schema: "".into(),
        timing: "AFTER".into(),
        event: "INSERT".into(),
        definition: "EXECUTE FUNCTION test()".into(),
        function_def: String::new(),
        enabled: true,
    };
    let trigger_ddl = format_trigger_ddl(&trigger);
    assert!(trigger_ddl.contains("AFTER INSERT ON \"users\""));
}

#[test]
fn ddl_generation_with_fk() {
    let info = TableInfo {
        table: Table {
            name: "orders".into(),
            schema: "public".into(),
            row_count: None,
        },
        columns: vec![Column {
            name: "user_id".into(),
            data_type: "INTEGER".into(),
            nullable: false,
            default: None,
            is_primary_key: false,
            table_name: "orders".into(),
            schema: "public".into(),
            ..Default::default()
        }],
        primary_key: None,
        indexes: vec![],
        foreign_keys: vec![ForeignKey {
            name: "fk_user".into(),
            from_table: "orders".into(),
            from_columns: vec!["user_id".into()],
            to_table: "users".into(),
            to_columns: vec!["id".into()],
            schema: "public".into(),
            to_schema: "public".into(),
            ..Default::default()
        }],
        check_constraints: vec![],
        dependencies: vec![],
    };

    let ddl = build_create_table_ddl(&info, DriverType::Postgres, &[]);
    assert!(ddl.contains("FOREIGN KEY (\"user_id\") REFERENCES \"public\".\"users\" (\"id\")"));
}

#[test]
fn format_trigger_ddl_reconstructs_from_parts() {
    let trigger = Trigger {
        name: "tr_audit".into(),
        table_name: "users".into(),
        schema: "public".into(),
        timing: "AFTER".into(),
        event: "INSERT".into(),
        definition: "EXECUTE FUNCTION audit_fn()".into(),
        function_def: String::new(),
        enabled: true,
    };
    let ddl = format_trigger_ddl(&trigger);
    assert!(ddl.contains("CREATE TRIGGER"));
    assert!(ddl.contains("\"tr_audit\""));
    assert!(ddl.contains("AFTER INSERT ON"));
    assert!(ddl.contains("\"public\".\"users\""));
    assert!(ddl.contains("EXECUTE FUNCTION audit_fn()"));
}

#[test]
fn format_trigger_ddl_includes_function_def_when_present() {
    // PostgreSQL's `pg_get_triggerdef` output, so the definition is replayed as-is.
    let full_sql = "CREATE TRIGGER tr_audit AFTER INSERT ON public.users FOR EACH ROW EXECUTE FUNCTION audit_fn()";
    let func_body = "CREATE FUNCTION audit_fn() RETURNS trigger AS $$ BEGIN RETURN NEW; END; $$ LANGUAGE plpgsql";
    let trigger = Trigger {
        name: "tr_audit".into(),
        table_name: "users".into(),
        schema: "public".into(),
        timing: "AFTER".into(),
        event: "INSERT".into(),
        definition: full_sql.into(),
        function_def: func_body.into(),
        enabled: true,
    };
    let ddl = format_trigger_ddl(&trigger);
    assert!(ddl.contains(full_sql), "{ddl}");
    // The function body is emitted first, and both statements end with `;`.
    let func_pos = ddl.find("CREATE FUNCTION").unwrap();
    let trigger_pos = ddl.find("CREATE TRIGGER").unwrap();
    assert!(func_pos < trigger_pos, "function_def must precede CREATE TRIGGER");
    assert_eq!(ddl.matches(';').count(), 2, "{ddl}");
}

#[test]
fn format_trigger_ddl_never_drops_row_level_clauses() {
    let full_sql = concat!(
        "CREATE TRIGGER tr_touch BEFORE INSERT OR UPDATE OF name ON public.t ",
        "FOR EACH ROW WHEN ((new.name IS NOT NULL)) EXECUTE FUNCTION touch()"
    );
    let trigger = Trigger {
        name: "tr_touch".into(),
        table_name: "t".into(),
        schema: "public".into(),
        timing: "BEFORE".into(),
        event: "INSERT OR UPDATE".into(),
        definition: full_sql.into(),
        function_def: String::new(),
        enabled: true,
    };
    assert_eq!(format_trigger_ddl(&trigger), format!("{full_sql};\n"));
}

#[test]
fn format_trigger_ddl_adds_the_missing_semicolon_after_a_function_body() {
    // Summary-only definition (the pre-#594 shape) must still rebuild with a
    // terminator after the function body, so the trigger parses on replay.
    let func_body = "CREATE FUNCTION touch() RETURNS trigger AS $$ BEGIN RETURN NEW; END; $$ LANGUAGE plpgsql";
    let trigger = Trigger {
        name: "tr_touch".into(),
        table_name: "t".into(),
        schema: "public".into(),
        timing: "BEFORE".into(),
        event: "UPDATE".into(),
        definition: "EXECUTE FUNCTION touch()".into(),
        function_def: func_body.into(),
        enabled: true,
    };
    let ddl = format_trigger_ddl(&trigger);
    assert!(ddl.contains("LANGUAGE plpgsql"), "{ddl}");
    // `;` closes the function, `;` closes the rebuilt CREATE TRIGGER.
    assert_eq!(ddl.matches(';').count(), 2, "{ddl}");
    assert!(ddl.trim_end().ends_with(';'), "{ddl}");
}

#[test]
fn format_trigger_ddl_uses_full_definition_when_available() {
    let full_sql = "CREATE TRIGGER tr_audit AFTER INSERT ON users BEGIN SELECT 1; END";
    let trigger = Trigger {
        name: "tr_audit".into(),
        table_name: "users".into(),
        schema: "main".into(),
        timing: "AFTER".into(),
        event: "INSERT".into(),
        definition: full_sql.into(),
        function_def: String::new(),
        enabled: true,
    };
    let ddl = format_trigger_ddl(&trigger);
    assert!(ddl.starts_with(full_sql));
    assert!(ddl.ends_with(";\n"));
}

#[test]
fn ddl_generation_groups_composite_fk_rows_into_one_constraint() {
    let info = TableInfo {
        table: Table {
            name: "child".into(),
            schema: "public".into(),
            row_count: None,
        },
        columns: vec![
            Column {
                name: "tenant_id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                default: None,
                is_primary_key: false,
                table_name: "child".into(),
                schema: "public".into(),
                ..Default::default()
            },
            Column {
                name: "parent_id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                default: None,
                is_primary_key: false,
                table_name: "child".into(),
                schema: "public".into(),
                ..Default::default()
            },
        ],
        primary_key: None,
        indexes: vec![],
        foreign_keys: vec![ForeignKey {
            name: "fk_parent".into(),
            from_table: "child".into(),
            from_columns: vec!["tenant_id".into(), "parent_id".into()],
            to_table: "parent".into(),
            to_columns: vec!["tenant_id".into(), "id".into()],
            schema: "public".into(),
            to_schema: "public".into(),
            ..Default::default()
        }],
        check_constraints: vec![],
        dependencies: vec![],
    };

    let ddl = build_create_table_ddl(&info, DriverType::Postgres, &[]);
    assert_eq!(ddl.matches("ADD CONSTRAINT \"fk_parent\"").count(), 1);
    assert!(ddl.contains(
        "FOREIGN KEY (\"tenant_id\", \"parent_id\") REFERENCES \"public\".\"parent\" (\"tenant_id\", \"id\")"
    ));
}

fn special_identifier_introspection() -> IntrospectResult {
    let mut result = test_introspect_result();
    let schema = "app`\"]zone";
    let table = "select";
    result.tables[0].schema = schema.into();
    result.tables[0].name = table.into();
    for (column, name) in result.columns.iter_mut().zip(["order", "part`\"]key"]) {
        column.schema = schema.into();
        column.table_name = table.into();
        column.name = name.into();
        column.data_type = "INTEGER".into();
        column.default = Some("1".into());
    }
    result.primary_keys[0].schema = schema.into();
    result.primary_keys[0].table_name = table.into();
    result.primary_keys[0].columns = vec!["order".into(), "part`\"]key".into()];
    result.indexes[0].schema = schema.into();
    result.indexes[0].table_name = table.into();
    result.indexes[0].name = "index`\"]key".into();
    result.indexes[0].columns = vec!["part`\"]key".into(), "order".into()];
    result.check_constraints = vec![CheckConstraint {
        name: "check`\"]rule".into(),
        table_name: table.into(),
        schema: schema.into(),
        definition: "CHECK (\"order\" > 0)".into(),
    }];
    result.foreign_keys = vec![ForeignKey {
        name: "foreign`\"]rule".into(),
        schema: schema.into(),
        from_table: table.into(),
        from_columns: vec!["order".into(), "part`\"]key".into()],
        to_schema: schema.into(),
        to_table: "parent`\"]table".into(),
        to_columns: vec!["order".into(), "part`\"]key".into()],
        on_update: "CASCADE".into(),
        on_delete: "CASCADE".into(),
        ..Default::default()
    }];
    result.triggers.clear();
    result
}

#[tokio::test]
async fn get_table_ddl_quotes_special_identifiers_for_each_provider() {
    // Literal expectations are independent of the production dialect formatter.
    for (driver, schema, column, check, foreign_key, parent, index) in [
        (
            DriverType::Postgres,
            r#""app`""]zone""#,
            r#""part`""]key""#,
            r#""check`""]rule""#,
            r#""foreign`""]rule""#,
            r#""parent`""]table""#,
            r#""index`""]key""#,
        ),
        (
            DriverType::SQLite,
            "",
            r#""part`""]key""#,
            r#""check`""]rule""#,
            r#""foreign`""]rule""#,
            r#""parent`""]table""#,
            r#""index`""]key""#,
        ),
        (
            DriverType::Mysql,
            r#"`app``"]zone`"#,
            r#"`part``"]key`"#,
            r#"`check``"]rule`"#,
            r#"`foreign``"]rule`"#,
            r#"`parent``"]table`"#,
            r#"`index``"]key`"#,
        ),
        (
            DriverType::SqlServer,
            r#"[app`"]]zone]"#,
            r#"[part`"]]key]"#,
            r#"[check`"]]rule]"#,
            r#"[foreign`"]]rule]"#,
            r#"[parent`"]]table]"#,
            r#"[index`"]]key]"#,
        ),
    ] {
        let (table, order) = match driver {
            DriverType::Mysql => ("`select`", "`order`"),
            DriverType::SqlServer => ("[select]", "[order]"),
            DriverType::Postgres | DriverType::SQLite => ("\"select\"", "\"order\""),
        };
        let mut introspection = special_identifier_introspection();
        introspection.check_constraints[0].definition = format!("CHECK ({order} > 0 AND {column} > 0)");
        let conn_id = ConnectionId::new();
        let registry = Arc::new(ConnectionRegistry::new());
        let mut cache = MockIntrospectionCache::new();
        cache.expect_get().returning(move |_| Ok(Some(introspection.clone())));
        let service = SchemaService::new(
            Box::new(MockDbConnector::new()),
            Box::new(cache),
            registry,
            Box::new(connections_for(driver)),
        );
        let ddl = service.get_table_ddl(&conn_id, "app`\"]zone", "select").await.unwrap();
        let prefix = if schema.is_empty() {
            String::new()
        } else {
            format!("{schema}.")
        };
        let target = format!("{prefix}{table}");
        let foreign_key_clause = format!(
            "CONSTRAINT {foreign_key} FOREIGN KEY ({order}, {column}) REFERENCES \
             {prefix}{parent} ({order}, {column}) ON UPDATE CASCADE ON DELETE CASCADE"
        );
        let inline_index = if driver == DriverType::Mysql {
            format!(",\n    UNIQUE INDEX {index} ({column}, {order})")
        } else {
            String::new()
        };
        let inline_fk = if driver == DriverType::Postgres {
            String::new()
        } else {
            format!(",\n    {foreign_key_clause}")
        };
        let mut expected = format!(
            "CREATE TABLE {target} (\n    {order} INTEGER NOT NULL DEFAULT 1,\n    \
             {column} INTEGER NOT NULL DEFAULT 1,\n    PRIMARY KEY ({order}, {column}),\n    \
             CONSTRAINT {check} CHECK ({order} > 0 AND {column} > 0){inline_index}{inline_fk}\n);\n"
        );
        if driver != DriverType::Mysql {
            expected.push_str(&format!(
                "CREATE UNIQUE INDEX {index} ON {target} ({column}, {order});\n"
            ));
        }
        if driver == DriverType::Postgres {
            expected.push_str(&format!("ALTER TABLE {target} ADD {foreign_key_clause};\n"));
        }
        assert_eq!(ddl, expected, "{driver:?}");
    }
}

#[test]
fn ddl_preserves_postgres_native_index_and_deferred_composite_fk() {
    let mut introspection = special_identifier_introspection();
    let definition = concat!(
        "CREATE UNIQUE INDEX native_idx ON \"app`\"\"]zone\".\"select\" ",
        "USING btree (\"order\" DESC) WHERE \"order\" > 0"
    );
    introspection.indexes[0].definition = definition.into();
    let foreign_key = &mut introspection.foreign_keys[0];
    foreign_key.match_option = "FULL".into();
    foreign_key.deferrable = true;
    foreign_key.initially_deferred = true;
    let info = TableInfo {
        table: introspection.tables.remove(0),
        columns: introspection.columns,
        primary_key: introspection.primary_keys.pop(),
        indexes: introspection.indexes,
        foreign_keys: introspection.foreign_keys,
        check_constraints: introspection.check_constraints,
        dependencies: vec![],
    };
    let ddl = build_create_table_ddl(&info, DriverType::Postgres, &info.check_constraints);
    assert!(ddl.contains(&format!("{definition};\n")));
    assert_eq!(ddl.matches("CREATE UNIQUE INDEX").count(), 1);
    assert!(ddl.ends_with("MATCH FULL ON UPDATE CASCADE ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;\n"));
}

#[tokio::test]
async fn test_dependency_deduplication() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut introspection = test_introspect_result();
    // Add two foreign keys referencing the same parent table
    introspection.foreign_keys.push(ForeignKey {
        name: "fk_users_parent1".into(),
        from_table: "users".into(),
        from_columns: vec!["parent_id".into()],
        to_table: "parents".into(),
        to_columns: vec!["id".into()],
        schema: "public".into(),
        to_schema: "public".into(),
        ..Default::default()
    });
    introspection.foreign_keys.push(ForeignKey {
        name: "fk_users_parent2".into(),
        from_table: "users".into(),
        from_columns: vec!["alt_parent_id".into()],
        to_table: "parents".into(),
        to_columns: vec!["id".into()],
        schema: "public".into(),
        to_schema: "public".into(),
        ..Default::default()
    });

    let mut cache = MockIntrospectionCache::new();
    cache.expect_get().returning(move |_| Ok(Some(introspection.clone())));

    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(cache),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let info = svc.get_table_info(&conn_id, "public", "users").await.unwrap();
    let parent_table_deps: Vec<_> = info
        .dependencies
        .iter()
        .filter(|d| d.name == "parents" && d.kind == DependencyKind::Table)
        .collect();
    assert_eq!(
        parent_table_deps.len(),
        1,
        "Duplicate table dependency should be deduplicated"
    );
}

fn ddl_service(connector: MockDbConnector, registry: Arc<ConnectionRegistry>) -> SchemaService {
    let mut cache = MockIntrospectionCache::new();
    cache.expect_invalidate().returning(|_| Ok(()));
    SchemaService::new(
        Box::new(connector),
        Box::new(cache),
        registry,
        Box::new(mock_connections()),
    )
}

#[tokio::test]
async fn execute_ddl_refuses_placeholder() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));
    let svc = ddl_service(MockDbConnector::new(), registry);

    let error = svc
        .execute_ddl(
            &conn_id,
            "CREATE TABLE t (id INTEGER PRIMARY KEY /* placeholder — expand from introspection */)",
        )
        .await
        .expect_err("a placeholder must not be applied");

    assert!(matches!(error, DbError::QueryFailed(_)));
    assert!(error.to_string().to_ascii_lowercase().contains("placeholder"));
}

#[tokio::test]
async fn execute_ddl_runs_multiple_statements() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_execute_batch()
        .withf(|handle, statements| {
            handle.0 == 1
                && statements.len() == 2
                && statements[0] == "CREATE TABLE a (id int)"
                && statements[1] == "CREATE TABLE b (id int)"
        })
        .returning(|_, _| Ok(0));
    let svc = ddl_service(connector, registry);

    let affected = svc
        .execute_ddl(&conn_id, "CREATE TABLE a (id int); CREATE TABLE b (id int);")
        .await
        .expect("multi-statement apply runs");
    assert_eq!(affected, 0);
}

#[test]
fn sqlserver_ddl_preserves_canonical_foreign_key_actions() {
    for (on_update, on_delete, expected_actions) in [
        ("NO ACTION", "NO ACTION", ""),
        ("SET NULL", "SET DEFAULT", " ON UPDATE SET NULL ON DELETE SET DEFAULT"),
        ("SET DEFAULT", "SET NULL", " ON UPDATE SET DEFAULT ON DELETE SET NULL"),
    ] {
        let mut introspection = special_identifier_introspection();
        introspection.foreign_keys[0].on_update = on_update.into();
        introspection.foreign_keys[0].on_delete = on_delete.into();
        let info = TableInfo {
            table: introspection.tables.remove(0),
            columns: introspection.columns,
            primary_key: introspection.primary_keys.pop(),
            indexes: vec![],
            foreign_keys: introspection.foreign_keys,
            check_constraints: vec![],
            dependencies: vec![],
        };
        let ddl = build_create_table_ddl(&info, DriverType::SqlServer, &[]);
        let foreign_key_line = ddl.lines().find(|line| line.contains("FOREIGN KEY")).unwrap();
        assert!(foreign_key_line.ends_with(&format!("([order], [part`\"]]key]){expected_actions}")));
        if expected_actions.is_empty() {
            assert!(!foreign_key_line.contains("ON UPDATE"));
            assert!(!foreign_key_line.contains("ON DELETE"));
        }
    }
}

#[test]
fn mysql_ddl_declares_supporting_indexes_before_foreign_keys_once() {
    let mut introspection = special_identifier_introspection();
    let mut supporting_index = introspection.indexes[0].clone();
    supporting_index.name = introspection.foreign_keys[0].name.clone();
    supporting_index.columns = introspection.foreign_keys[0].from_columns.clone();
    supporting_index.unique = false;
    introspection.indexes.push(supporting_index);
    introspection.indexes.push(Index {
        name: "PRIMARY".into(),
        columns: vec!["order".into(), "part`\"]key".into()],
        primary: true,
        ..Default::default()
    });
    let info = TableInfo {
        table: introspection.tables.remove(0),
        columns: introspection.columns,
        primary_key: introspection.primary_keys.pop(),
        indexes: introspection.indexes,
        foreign_keys: introspection.foreign_keys,
        check_constraints: vec![],
        dependencies: vec![],
    };
    let ddl = build_create_table_ddl(&info, DriverType::Mysql, &[]);
    let supporting_index = "INDEX `foreign``\"]rule` (`order`, `part``\"]key`)";
    assert_eq!(ddl.matches(supporting_index).count(), 1);
    assert!(ddl.find(supporting_index).unwrap() < ddl.find("FOREIGN KEY").unwrap());
    assert!(ddl.contains("UNIQUE INDEX `index``\"]key` (`part``\"]key`, `order`)"));
    assert_eq!(ddl.matches("PRIMARY KEY").count(), 1);
    assert!(!ddl.contains("INDEX `PRIMARY`"));
    assert!(!ddl.contains("CREATE INDEX"));
    assert!(!ddl.contains("CREATE UNIQUE INDEX"));
}

fn debug_probe_result(installed: bool, available: bool, preloaded: bool, version: &str) -> QueryResult {
    let cells = vec![
        CellValue::Bool(installed),
        CellValue::Bool(available),
        CellValue::Bool(preloaded),
        CellValue::Text(version.into()),
    ];
    QueryResult {
        columns: (0..4)
            .map(|i| ColumnMeta {
                name: format!("col{i}"),
                data_type: "bool".into(),
                nullable: true,
            })
            .collect(),
        rows: vec![Row(cells)],
        row_count: 1,
        duration_ms: 0,
    }
}

#[tokio::test]
async fn routine_debug_probe_sends_the_catalog_probe_sql_verbatim() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .withf(|handle, sql, params| handle.0 == 1 && sql == DEBUG_CAPABILITY_PROBE_SQL && params.is_empty())
        .once()
        .returning(|_, _, _| Ok(debug_probe_result(false, true, false, "18.0")));

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(MockIntrospectionCache::new()),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let probe = svc.routine_debug_probe(&conn_id).await.expect("probe runs");
    assert_eq!(probe.state, RoutineDebugState::ExtensionMissing);
    assert!(probe.extension_available, "installable is not the same as installed");
    assert_eq!(probe.server_version, "18.0");
}

#[tokio::test]
async fn routine_debug_probe_keeps_query_failures_as_explainable_state() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector
        .expect_query()
        .returning(|_, _, _| Err(DbError::Internal("permission denied for pg_extension".into())));

    let svc = SchemaService::new(
        Box::new(connector),
        Box::new(MockIntrospectionCache::new()),
        Arc::clone(&registry),
        Box::new(mock_connections()),
    );

    let probe = svc.routine_debug_probe(&conn_id).await.expect("probe outcome is data");
    assert_eq!(probe.state, RoutineDebugState::ProbeFailed);
    assert!(probe.probe_error.unwrap().contains("permission denied"));
}

#[tokio::test]
async fn routine_debug_probe_answers_non_postgres_without_a_round_trip() {
    let conn_id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(conn_id, ConnectionHandle(1));

    // No `expect_query` is set up: any query attempt panics the mock, proving the shortcut.
    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(MockIntrospectionCache::new()),
        Arc::clone(&registry),
        Box::new(sqlite_connections()),
    );

    let probe = svc.routine_debug_probe(&conn_id).await.expect("answered locally");
    assert_eq!(probe.state, RoutineDebugState::UnsupportedDriver);
}

#[tokio::test]
async fn routine_debug_probe_reports_inactive_connection_honestly() {
    let conn_id = ConnectionId::new();
    let svc = SchemaService::new(
        Box::new(MockDbConnector::new()),
        Box::new(MockIntrospectionCache::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(mock_connections()),
    );

    let probe = svc.routine_debug_probe(&conn_id).await.expect("still a probe result");
    assert_eq!(probe.state, RoutineDebugState::ProbeFailed);
    assert!(probe.probe_error.unwrap().contains("not active"));
}
