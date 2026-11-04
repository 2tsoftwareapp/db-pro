//! Live MySQL 8 verification for the #235 provider.
//!
//! These tests are marked `#[ignore]` so they only run when `DATABASE_URL` is set
//! to a MySQL connection string (e.g. `mysql://user:pass@host:3306/db`).
//!
//! `#[ignore]` alone is not enough: the CI command is
//! `cargo test --all -- --include-ignored` with a *PostgreSQL* `DATABASE_URL`, so every test here
//! also runs in an environment with no MySQL server. `setup()` returns `None` there and the test
//! skips with a reason, instead of failing the run on a fixture it cannot have.
//!
//! Run with: `cargo test --package db-pro-infrastructure --test mysql_integration -- --ignored`
//! The read-only #452 regressions instead use separate `DB_PRO_MYSQL_452_*` fields so
//! fixture credentials never pass through this file's legacy `DATABASE_URL` parser.
//! See `docs/plans/active/backend-four-issue-wave/MYSQL_FINDINGS.md` for the fixture.

use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use db_pro_core::ports::{DbConnector, TransactionFailureOutcome, TransactionFailurePhase};
use db_pro_infrastructure::mysql::connector::MySqlConnector;

fn mysql_config() -> Option<ConnectionConfig> {
    let url = std::env::var("DATABASE_URL").ok()?;
    if !url.starts_with("mysql://") {
        return None;
    }
    // Parse mysql://user:pass@host:port/db
    let without_prefix = url.strip_prefix("mysql://")?;
    let (auth, rest) = without_prefix.split_once('@')?;
    let (username, _password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host_port, database) = rest.split_once('/')?;
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, "3306"));
    Some(ConnectionConfig {
        name: "mysql-test".into(),
        host: host.into(),
        port: port.parse().ok().unwrap_or(3306),
        database: database.split('?').next().unwrap_or(database).into(),
        username: username.into(),
        driver: DriverType::Mysql,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 10_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    })
}

async fn setup() -> Option<(MySqlConnector, ConnectionHandle, String)> {
    let config = mysql_config()?;
    let password = std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("mysql://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default();
    let database = config.database.clone();

    let connector = MySqlConnector::new();
    let handle = connector
        .connect(&config, &password)
        .await
        .expect("MySQL connect failed");
    Some((connector, handle, database))
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_connects_and_executes_query() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let result = connector
        .query(&handle, "SELECT 1 + 1 AS two", &[])
        .await
        .expect("query");
    assert!(!result.rows.is_empty());
    let col_names: Vec<&str> = result.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(col_names, vec!["two"]);

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_execute_returns_affected_rows() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_probe", &[])
        .await
        .ok();
    connector
        .execute(&handle, "CREATE TABLE mysql_probe (id INT, name VARCHAR(100))", &[])
        .await
        .expect("create");
    let affected = connector
        .execute(&handle, "INSERT INTO mysql_probe VALUES (1, 'hello')", &[])
        .await
        .expect("insert");
    assert_eq!(affected, 1);

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_transaction_rolls_back_on_failure() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_tx_probe", &[])
        .await
        .ok();
    connector
        .execute(&handle, "CREATE TABLE mysql_tx_probe (id INT)", &[])
        .await
        .expect("create");

    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "INSERT INTO mysql_tx_probe VALUES (1)".to_owned(),
                "INVALID STATEMENT".to_owned(),
            ],
            &[false, true],
        )
        .await
        .expect_err("must fail");

    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);

    let count = connector
        .query(&handle, "SELECT count(*) FROM mysql_tx_probe", &[])
        .await
        .expect("count");
    let cnt: i64 = match &count.rows[0].0[0] {
        db_pro_core::domain::query::CellValue::Int64(n) => *n,
        _ => panic!("unexpected cell"),
    };
    assert_eq!(cnt, 0, "the failed transaction must have rolled back");

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_tx_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_introspect_returns_canonical_model() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_intro_probe", &[])
        .await
        .ok();
    connector
        .execute(
            &handle,
            "CREATE TABLE mysql_intro_probe (id INT PRIMARY KEY, name VARCHAR(100))",
            &[],
        )
        .await
        .expect("create");

    let intro = connector.introspect(&handle).await.expect("introspect");
    assert!(intro.tables.iter().any(|t| t.name == "mysql_intro_probe"));

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_intro_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_explain_returns_json() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let plan = connector
        .explain(&handle, "SELECT 1", &[], false)
        .await
        .expect("explain");
    assert!(plan.is_array());

    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_positional_parameters_bind_and_round_trip() {
    use db_pro_core::domain::query::{CellValue, QueryParam};
    use db_pro_infrastructure::connector::CompositeConnector;

    let Some((_connector, _handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };
    let config = mysql_config().expect("mysql config");
    let password = std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("mysql://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default();

    let connector = CompositeConnector::new();
    let handle = connector.connect(&config, &password).await.expect("connect");

    // Dialect must resolve for an active MySQL connection (#235 criterion 5 path).
    let dialect = connector.dialect(&handle).expect("mysql dialect");
    assert_eq!(dialect.placeholder(1), "?");
    assert_eq!(dialect.quote_identifier("order"), "`order`");

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_bind_probe", &[])
        .await
        .ok();
    connector
        .execute(
            &handle,
            "CREATE TABLE mysql_bind_probe (
                id BIGINT NOT NULL,
                label VARCHAR(64) NOT NULL,
                amount DECIMAL(20,4) NOT NULL,
                stamp DATETIME(6) NOT NULL,
                flag BOOLEAN NOT NULL,
                payload JSON NOT NULL
            )",
            &[],
        )
        .await
        .expect("create");

    let affected = connector
        .execute(
            &handle,
            "INSERT INTO mysql_bind_probe (id, label, amount, stamp, flag, payload)
             VALUES (?, ?, ?, ?, ?, ?)",
            &[
                QueryParam::Int64(7),
                QueryParam::Text("bound".into()),
                QueryParam::Decimal("123456789012345.6789".into()),
                QueryParam::DateTime("2024-03-15T10:20:30.123456".into()),
                QueryParam::Bool(true),
                QueryParam::Json(serde_json::json!({"k": "v"})),
            ],
        )
        .await
        .expect("bound insert");
    assert_eq!(affected, 1);

    let result = connector
        .query(
            &handle,
            "SELECT id, label, amount, stamp, flag, payload
             FROM mysql_bind_probe WHERE id = ? AND label = ?",
            &[QueryParam::Int64(7), QueryParam::Text("bound".into())],
        )
        .await
        .expect("bound select");
    assert_eq!(result.row_count, 1);
    let row = &result.rows[0].0;
    assert!(matches!(row[0], CellValue::Int64(7)), "id: {:?}", row[0]);
    assert!(
        matches!(&row[1], CellValue::Text(text) if text == "bound"),
        "label: {:?}",
        row[1]
    );
    match &row[2] {
        CellValue::Decimal(digits) => assert_eq!(digits, "123456789012345.6789"),
        other => panic!("expected Decimal, got {other:?}"),
    }
    match &row[3] {
        CellValue::Timestamp(value) => assert!(value.starts_with("2024-03-15T10:20:30.123456")),
        other => panic!("expected Timestamp, got {other:?}"),
    }
    assert!(matches!(row[4], CellValue::Bool(true)), "flag: {:?}", row[4]);
    match &row[5] {
        CellValue::Json(value) => assert_eq!(value, &serde_json::json!({"k": "v"})),
        other => panic!("expected Json, got {other:?}"),
    }

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_bind_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_introspects_named_check_constraints() {
    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_check_probe", &[])
        .await
        .ok();
    connector
        .execute(
            &handle,
            "CREATE TABLE mysql_check_probe (
                id BIGINT NOT NULL,
                amount DECIMAL(10,2) NOT NULL,
                CONSTRAINT chk_amount_nonneg CHECK (amount >= 0)
            )",
            &[],
        )
        .await
        .expect("create with CHECK");

    let schema = connector.introspect(&handle).await.expect("introspect");
    let checks: Vec<_> = schema
        .check_constraints
        .iter()
        .filter(|c| c.table_name == "mysql_check_probe")
        .collect();
    assert_eq!(
        checks.len(),
        1,
        "expected one CHECK on mysql_check_probe, got {checks:?}"
    );
    assert_eq!(checks[0].name, "chk_amount_nonneg");
    assert!(
        checks[0].definition.to_lowercase().contains("amount"),
        "definition should mention amount: {}",
        checks[0].definition
    );

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_check_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_table_data_change_set_round_trips() {
    use db_pro_core::application::sql_builder::{self, FilterOp, TableFilter};
    use db_pro_core::domain::query::CellValue;
    use db_pro_core::ports::{DbConnector, ParameterizedTransactionStatement};
    use db_pro_infrastructure::connector::CompositeConnector;

    let Some((_connector, _handle, database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };
    let config = mysql_config().expect("mysql config");
    let password = std::env::var("DATABASE_URL")
        .ok()
        .and_then(|url| {
            url.strip_prefix("mysql://")
                .and_then(|s| s.split_once('@').map(|(auth, _)| auth))
                .and_then(|auth| auth.split_once(':').map(|(_, p)| p.to_string()))
        })
        .unwrap_or_default();

    let connector = CompositeConnector::new();
    let handle = connector.connect(&config, &password).await.expect("connect");
    let dialect = connector.dialect(&handle).expect("mysql dialect");

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_changeset_probe", &[])
        .await
        .ok();
    connector
        .execute(
            &handle,
            "CREATE TABLE mysql_changeset_probe (
                id BIGINT NOT NULL PRIMARY KEY,
                label VARCHAR(64) NOT NULL
            )",
            &[],
        )
        .await
        .expect("create");

    let (insert_sql, insert_params) = sql_builder::build_insert(
        dialect.as_ref(),
        &database,
        "mysql_changeset_probe",
        &["id".into(), "label".into()],
        &[CellValue::Int64(1), CellValue::Text("alpha".into())],
    )
    .expect("build insert");
    connector
        .execute_parameterized_transaction(
            &handle,
            &[ParameterizedTransactionStatement {
                sql: insert_sql,
                params: insert_params,
                expect_affected_rows: true,
                max_affected_rows: Some(1),
            }],
        )
        .await
        .expect("insert change-set");

    let (select_sql, select_params) = sql_builder::build_select(
        dialect.as_ref(),
        &database,
        "mysql_changeset_probe",
        &[TableFilter {
            column: "id".into(),
            op: FilterOp::Eq,
            value: CellValue::Int64(1),
        }],
        &[],
        10,
        0,
    )
    .expect("build select");
    let selected = connector
        .query(&handle, &select_sql, &select_params)
        .await
        .expect("select");
    assert_eq!(selected.row_count, 1);
    assert!(matches!(&selected.rows[0].0[1], CellValue::Text(t) if t == "alpha"));

    let (update_sql, update_params) = sql_builder::build_update(
        dialect.as_ref(),
        &database,
        "mysql_changeset_probe",
        &["label".into()],
        &[CellValue::Text("beta".into())],
        &["id".into()],
        &[CellValue::Int64(1)],
    )
    .expect("build update");
    connector
        .execute_parameterized_transaction(
            &handle,
            &[ParameterizedTransactionStatement {
                sql: update_sql,
                params: update_params,
                expect_affected_rows: true,
                max_affected_rows: Some(1),
            }],
        )
        .await
        .expect("update change-set");

    let (delete_sql, delete_params) = sql_builder::build_delete(
        dialect.as_ref(),
        &database,
        "mysql_changeset_probe",
        &["id".into()],
        &[CellValue::Int64(1)],
    )
    .expect("build delete");
    connector
        .execute_parameterized_transaction(
            &handle,
            &[ParameterizedTransactionStatement {
                sql: delete_sql,
                params: delete_params,
                expect_affected_rows: true,
                max_affected_rows: Some(1),
            }],
        )
        .await
        .expect("delete change-set");

    let (count_sql, count_params) = sql_builder::build_count(dialect.as_ref(), &database, "mysql_changeset_probe", &[]);
    let count = connector
        .query(&handle, &count_sql, &count_params)
        .await
        .expect("count");
    assert_eq!(count.row_count, 1);
    assert!(matches!(count.rows[0].0[0], CellValue::Int64(0)));

    connector
        .execute(&handle, "DROP TABLE IF EXISTS mysql_changeset_probe", &[])
        .await
        .ok();
    connector.disconnect(&handle).await.unwrap();
}

#[tokio::test]
#[ignore = "requires DATABASE_URL=mysql://... and MYSQL_REFERENCE_DATABASE with table creation privileges"]
async fn mysql_cross_database_foreign_key_ddl_round_trips() {
    use db_pro_core::application::{sql_builder::qualify, ConnectionRegistry, SchemaService};
    use db_pro_core::domain::connection::Connection;
    use db_pro_core::ports::ConnectionRepository;
    use db_pro_infrastructure::meta::store::SQLiteMetaStore;
    use std::sync::Arc;

    let Some((connector, handle, database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };
    let Ok(reference_database) = std::env::var("MYSQL_REFERENCE_DATABASE") else {
        eprintln!("skipping MySQL cross-database test: MYSQL_REFERENCE_DATABASE is not set");
        return;
    };
    assert_ne!(database, reference_database, "the referenced database must be distinct");
    let connector = Arc::new(connector);
    let dialect = connector.dialect(&handle).unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let parent_name = format!("dbpro442_parent_{suffix}");
    let child_name = format!("dbpro442_child_{suffix}");
    let constraint_name = format!("dbpro442_fk_{suffix}");
    let local_parent = qualify(dialect.as_ref(), &database, &parent_name);
    let remote_parent = qualify(dialect.as_ref(), &reference_database, &parent_name);
    let local_child = qualify(dialect.as_ref(), &database, &child_name);
    let remote_child = qualify(dialect.as_ref(), &reference_database, &child_name);
    let constraint = dialect.quote_identifier(&constraint_name);
    for target in [&local_parent, &remote_parent] {
        connector
            .execute(
                &handle,
                &format!(
                    "CREATE TABLE {target} (tenant_id INT NOT NULL, id INT NOT NULL, PRIMARY KEY (tenant_id, id))"
                ),
                &[],
            )
            .await
            .expect("create same-named parent in each database");
    }
    // Constraint and table names deliberately collide across databases. Catalog joins
    // must scope every side to its database instead of multiplying matching names.
    for target in [&local_child, &remote_child] {
        connector
            .execute(
                &handle,
                &format!(
                    "CREATE TABLE {target} (id INT PRIMARY KEY, tenant_id INT NOT NULL, parent_id INT NOT NULL, \
                     CONSTRAINT {constraint} FOREIGN KEY (tenant_id, parent_id) \
                     REFERENCES {remote_parent} (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE)"
                ),
                &[],
            )
            .await
            .expect("create cross-database and same-database constraints");
    }
    connector
        .execute(&handle, &format!("INSERT INTO {local_parent} VALUES (10, 10)"), &[])
        .await
        .unwrap();
    connector
        .execute(&handle, &format!("INSERT INTO {remote_parent} VALUES (10, 20)"), &[])
        .await
        .unwrap();

    let model = connector
        .introspect(&handle)
        .await
        .expect("introspect cross-database FK");
    let foreign_keys: Vec<_> = model
        .foreign_keys
        .iter()
        .filter(|foreign_key| foreign_key.from_table == child_name && foreign_key.schema == database)
        .collect();
    assert_eq!(foreign_keys.len(), 1);
    assert_eq!(foreign_keys[0].to_schema, reference_database);
    assert_eq!(foreign_keys[0].to_table, parent_name);
    assert_eq!(foreign_keys[0].from_columns, vec!["tenant_id", "parent_id"]);
    assert_eq!(foreign_keys[0].to_columns, vec!["tenant_id", "id"]);

    let connection = Connection::new(mysql_config().unwrap());
    let store = SQLiteMetaStore::new(":memory:").await.unwrap();
    ConnectionRepository::save(&store, &connection).await.unwrap();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(connection.id, handle);
    let service = SchemaService::new(
        Box::new(Arc::clone(&connector)),
        Box::new(store.clone()),
        registry,
        Box::new(store),
    );
    let ddl = service
        .get_table_ddl(&connection.id, &database, &child_name)
        .await
        .unwrap();
    assert!(ddl.contains(&format!("REFERENCES {remote_parent} (")));
    assert!(!ddl.contains(&format!("REFERENCES {local_parent} (")));
    connector
        .execute(&handle, &format!("DROP TABLE {local_child}"), &[])
        .await
        .unwrap();
    service
        .execute_ddl(&connection.id, &ddl)
        .await
        .expect("recreate FK plus its supporting index without a duplicate-index error");
    connector
        .execute(&handle, &format!("INSERT INTO {local_child} VALUES (1, 10, 20)"), &[])
        .await
        .expect("reconstructed FK accepts the remote parent's key, absent from the local parent");
    assert!(
        connector
            .execute(&handle, &format!("INSERT INTO {local_child} VALUES (2, 10, 10)"), &[])
            .await
            .is_err(),
        "a key only present in the local parent must not satisfy the remote FK"
    );
    for target in [&local_child, &remote_child, &local_parent, &remote_parent] {
        connector
            .execute(&handle, &format!("DROP TABLE {target}"), &[])
            .await
            .unwrap();
    }
    connector.disconnect(&handle).await.unwrap();
}

fn mysql_reserved_fields_fixture() -> Option<(ConnectionConfig, String, String)> {
    let Ok(host) = std::env::var("DB_PRO_MYSQL_452_HOST") else {
        eprintln!("skipping #452 MySQL regression: DB_PRO_MYSQL_452_HOST is not set");
        return None;
    };
    let required = |name| std::env::var(name).expect("all DB_PRO_MYSQL_452_* fixture fields must be set");
    let config = ConnectionConfig {
        name: "mysql-reserved-fields-test".into(),
        host,
        port: required("DB_PRO_MYSQL_452_PORT").parse().expect("fixture port"),
        database: required("DB_PRO_MYSQL_452_DATABASE"),
        username: required("DB_PRO_MYSQL_452_USERNAME"),
        driver: DriverType::Mysql,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 10_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
    };
    Some((
        config,
        required("DB_PRO_MYSQL_452_PASSWORD"),
        required("DB_PRO_MYSQL_452_CURRENT_USER"),
    ))
}

#[tokio::test]
#[ignore] // Requires the reserved/Unicode account and database fixture.
async fn mysql_reserved_fields_preserve_authenticated_user_and_database() {
    use db_pro_core::domain::query::CellValue;

    let Some((config, password, current_user)) = mysql_reserved_fields_fixture() else {
        return;
    };
    let connector = MySqlConnector::new();
    let handle = connector
        .connect(&config, &password)
        .await
        .expect("reserved fields connect");
    let result = connector.query(&handle, "SELECT CURRENT_USER(), DATABASE()", &[]).await;
    connector.disconnect(&handle).await.expect("close identity probe pool");
    let result = result.expect("query authenticated identity");
    assert_eq!(result.rows.len(), 1);
    let [CellValue::Text(actual_user), CellValue::Text(actual_database)] = result.rows[0].0.as_slice() else {
        panic!("identity query must return two text cells");
    };
    assert_eq!(actual_user, &current_user);
    assert_eq!(actual_database, &config.database);
    connector
        .test_connection(&config, &password)
        .await
        .expect("test_connection preserves reserved fields too");
}

#[tokio::test]
#[ignore] // Requires the reserved/Unicode account and database fixture.
async fn mysql_reserved_fields_still_reject_wrong_password() {
    use db_pro_core::domain::error::DbError;

    let Some((config, password, _)) = mysql_reserved_fields_fixture() else {
        return;
    };
    let wrong_password = "db-pro-452-invalid-password";
    assert!(
        password != wrong_password,
        "fixture must not use the denial probe password"
    );
    let connector = MySqlConnector::new();
    connector
        .test_connection(&config, &password)
        .await
        .expect("fixture accepts the correct password");

    let result = connector.connect(&config, wrong_password).await;
    if let Ok(handle) = &result {
        connector
            .disconnect(handle)
            .await
            .expect("close unexpected successful pool");
    }
    let connect_error = result.expect_err("connect must reject the wrong password");
    let test_error = connector
        .test_connection(&config, wrong_password)
        .await
        .expect_err("test_connection must reject the wrong password");
    for error in [connect_error, test_error] {
        assert!(
            matches!(&error, DbError::ConnectionFailed(message) if message.contains("1045")),
            "the server must reject authentication, not fail URL parsing: {error}"
        );
        assert!(
            !error.to_string().contains(wrong_password),
            "error must not disclose the password"
        );
    }
}

#[tokio::test]
#[ignore] // Requires the reserved-fields fixture on MySQL started with --tls-version=.
async fn mysql_strict_ssl_modes_reject_a_server_without_tls() {
    use db_pro_core::domain::error::DbError;

    let Some((mut config, password, _)) = mysql_reserved_fields_fixture() else {
        return;
    };
    let connector = MySqlConnector::new();
    connector
        .test_connection(&config, &password)
        .await
        .expect("Disable must connect to the plaintext fixture");

    for mode in [SslMode::Require, SslMode::VerifyCa, SslMode::VerifyFull] {
        config.ssl_mode = mode;
        let result = connector.connect(&config, &password).await;
        if let Ok(handle) = &result {
            connector
                .disconnect(handle)
                .await
                .expect("close unexpected plaintext pool");
        }
        assert!(
            matches!(result, Err(DbError::ConnectionFailed(_))),
            "strict TLS connect must not downgrade to plaintext"
        );
        assert!(
            matches!(
                connector.test_connection(&config, &password).await,
                Err(DbError::ConnectionFailed(_))
            ),
            "strict TLS test_connection must not downgrade to plaintext"
        );
    }
}

/// A run staged while the tab holds `BEGIN` belongs to the tab's transaction.
/// The tab's `ROLLBACK` must undo the run's writes. On a separate pool connection
/// the run would commit alone and survive the tab's rollback.
#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_joined_parameterized_transaction_rolls_back_with_the_tab() {
    use db_pro_core::application::with_query_session;
    use db_pro_core::domain::query::{CellValue, QueryParam};
    use db_pro_core::ports::{ParameterizedTransactionStatement, TransactionStatementResult};

    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let table = format!("dbpro_join_tx_{}_{}", std::process::id(), suffix);

    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {table} (id VARCHAR(64) PRIMARY KEY, name VARCHAR(100) NOT NULL)"),
            &[],
        )
        .await
        .expect("create joined fixture");

    let ids = |rows: &db_pro_core::domain::query::QueryResult| -> Vec<String> {
        rows.rows
            .iter()
            .map(|row| match &row.0[0] {
                CellValue::Text(value) => value.clone(),
                other => panic!("unexpected id cell: {other:?}"),
            })
            .collect()
    };

    with_query_session(Some("mysql-join-tab"), async {
        connector
            .execute(&handle, "BEGIN", &[])
            .await
            .expect("BEGIN opens the tab transaction");
        connector
            .execute(
                &handle,
                &format!("INSERT INTO {table} (id, name) VALUES (?, ?)"),
                &[QueryParam::Text("tab".into()), QueryParam::Text("Tab work".into())],
            )
            .await
            .expect("tab statement inside its own transaction");

        let results = connector
            .execute_parameterized_transaction(
                &handle,
                &[ParameterizedTransactionStatement {
                    sql: format!("INSERT INTO {table} (id, name) VALUES (?, ?)"),
                    params: vec![QueryParam::Text("joined".into()), QueryParam::Text("Joined".into())],
                    expect_affected_rows: true,
                    max_affected_rows: Some(1),
                }],
            )
            .await
            .expect("the joined run succeeds");
        assert!(
            matches!(
                results.as_slice(),
                [TransactionStatementResult::Affected {
                    row_count: 1,
                    duration_ms: _
                }]
            ),
            "the run reports its own affected row, got {results:?}"
        );

        let inside = connector
            .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
            .await
            .expect("read inside the tab transaction");
        assert_eq!(
            ids(&inside),
            vec!["joined".to_string(), "tab".to_string()],
            "the run's write must be visible to the tab that owns the transaction"
        );

        connector
            .execute(&handle, "ROLLBACK", &[])
            .await
            .expect("rollback the tab transaction");
    })
    .await;

    let after = connector
        .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
        .await
        .expect("read after rollback");
    assert!(
        ids(&after).is_empty(),
        "the tab's rollback must undo the joined write too, got {:?}",
        ids(&after)
    );

    connector
        .execute(&handle, &format!("DROP TABLE {table}"), &[])
        .await
        .expect("drop joined fixture");
    connector.disconnect(&handle).await.expect("disconnect fixture");
}

/// Failed statements in the joined run roll back only that run's savepoint.
/// The tab's already-open work must survive, and the connection must still accept SQL.
#[tokio::test]
#[ignore] // Requires DATABASE_URL=mysql://...
async fn mysql_joined_parameterized_transaction_rolls_back_only_its_own_statements() {
    use db_pro_core::application::with_query_session;
    use db_pro_core::domain::query::{CellValue, QueryParam};
    use db_pro_core::ports::ParameterizedTransactionStatement;

    let Some((connector, handle, _database)) = setup().await else {
        eprintln!("skipping MySQL integration test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let table = format!("dbpro_join_sp_{}_{}", std::process::id(), suffix);

    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {table} (id VARCHAR(64) PRIMARY KEY, name VARCHAR(100) NOT NULL)"),
            &[],
        )
        .await
        .expect("create joined-run fixture");

    let ids = |rows: &db_pro_core::domain::query::QueryResult| -> Vec<String> {
        rows.rows
            .iter()
            .map(|row| match &row.0[0] {
                CellValue::Text(value) => value.clone(),
                other => panic!("unexpected id cell: {other:?}"),
            })
            .collect()
    };

    with_query_session(Some("mysql-join-tab"), async {
        connector
            .execute(&handle, "BEGIN", &[])
            .await
            .expect("BEGIN opens the tab transaction");
        connector
            .execute(
                &handle,
                &format!("INSERT INTO {table} (id, name) VALUES (?, ?)"),
                &[QueryParam::Text("tab".into()), QueryParam::Text("Tab work".into())],
            )
            .await
            .expect("tab statement inside its own transaction");

        let failure = connector
            .execute_parameterized_transaction(
                &handle,
                &[
                    ParameterizedTransactionStatement {
                        sql: format!("INSERT INTO {table} (id, name) VALUES (?, ?)"),
                        params: vec![QueryParam::Text("joined".into()), QueryParam::Text("Joined".into())],
                        expect_affected_rows: true,
                        max_affected_rows: Some(1),
                    },
                    ParameterizedTransactionStatement {
                        sql: "THIS IS NOT VALID SQL".into(),
                        params: Vec::new(),
                        expect_affected_rows: false,
                        max_affected_rows: None,
                    },
                ],
            )
            .await
            .expect_err("the invalid statement must fail the joined run");

        assert_eq!(failure.phase, TransactionFailurePhase::Statement);
        assert_eq!(failure.statement_index, 1);
        assert_eq!(failure.results.len(), 1, "the statement before the failure did run");
        assert_eq!(
            failure.outcome,
            TransactionFailureOutcome::LeftOpen,
            "a confirmed savepoint undo leaves the tab's transaction open"
        );

        let inside = connector
            .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
            .await
            .expect("the tab transaction must still accept statements after the joined failure");
        assert_eq!(
            ids(&inside),
            vec!["tab".to_string()],
            "the joined write must be undone while the tab's own work is kept"
        );

        connector
            .execute(&handle, "COMMIT", &[])
            .await
            .expect("commit the tab transaction");
    })
    .await;

    let after = connector
        .query(&handle, &format!("SELECT id FROM {table} ORDER BY id"), &[])
        .await
        .expect("read after commit");
    assert_eq!(
        ids(&after),
        vec!["tab".to_string()],
        "the joined run's write must not survive the tab's commit"
    );

    connector
        .execute(&handle, &format!("DROP TABLE {table}"), &[])
        .await
        .expect("drop joined-run fixture");
    connector.disconnect(&handle).await.expect("disconnect fixture");
}
