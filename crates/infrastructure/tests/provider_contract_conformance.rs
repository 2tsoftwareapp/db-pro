use db_pro_core::domain::capabilities::DatabaseCapabilities;
use db_pro_core::domain::cloud_presets::parse_connection_snippet;
use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{CellValue, QueryResult};
use db_pro_core::ports::{DbConnector, ProviderFactory, TransactionFailureOutcome, TransactionFailurePhase};
use db_pro_infrastructure::connector::CompositeConnector;

fn sqlite_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "test".into(),
        host: String::new(),
        port: 0,
        database: ":memory:".into(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: Default::default(),
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
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. CompositeConnector dispatches connect/query/execute to the right factory
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn composite_connects_to_sqlite_via_factory() {
    let connector = CompositeConnector::new();
    let handle = connector.connect(&sqlite_config(), "").await.expect("connect");
    let result = connector.query(&handle, "SELECT 1 + 1", &[]).await.expect("query");
    assert!(!result.rows.is_empty());
    connector.disconnect(&handle).await.expect("disconnect");
}

#[tokio::test]
async fn composite_handles_mysql_capabilities() {
    let connector = CompositeConnector::new();
    let caps = connector.capabilities(&ConnectionConfig {
        driver: DriverType::Mysql,
        ..sqlite_config()
    });
    assert!(caps.schema.schemas, "MySQL should support schemas");
    assert!(
        !caps.features.tablespaces,
        "MySQL connector reports no tablespaces support"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. capabilities() dispatches via factory
// ═══════════════════════════════════════════════════════════════════════════

/// The advertised capability set must not promise what the shipping provider code
/// refuses. Each assertion names the path it was measured against; a provider that
/// starts serving one of them should flip the flag and this test together.
#[test]
fn mysql_advertisement_matches_the_shipping_code_paths() {
    let connector = CompositeConnector::new();
    let caps = connector.capabilities(&ConnectionConfig {
        driver: DriverType::Mysql,
        ..sqlite_config()
    });

    // `MySqlConnector::query`/`execute` bind positional `?` parameters.
    assert!(caps.query.parameters);
    assert!(caps.query.positional_parameters);
    assert!(!caps.query.numbered_parameters);
    // `UserService` / `BackupService` / `PostgresApi::partitions` reject MySQL.
    // Keyed data-diff shares the dialect-neutral path and is advertised.
    assert!(!caps.features.server_sessions);
    assert!(!caps.features.partitions);
    assert!(!caps.features.backup);
    assert!(caps.features.data_diff);

    // Still served, so still advertised: query/execute/DDL through the generic
    // connector methods, EXPLAIN, and introspection-backed schema diff.
    assert!(caps.query.explain);
    assert!(caps.query.multi_statement);
    assert!(caps.schema.schemas);
    assert!(caps.schema.functions);
    assert!(caps.features.schema_diff);
}

#[test]
fn capabilities_match_driver() {
    let connector = CompositeConnector::new();
    let pg_caps = connector.capabilities(&ConnectionConfig {
        driver: DriverType::Postgres,
        ..sqlite_config()
    });
    assert!(pg_caps.schema.schemas);
    assert!(pg_caps.query.numbered_parameters);

    let sqlite_caps = connector.capabilities(&sqlite_config());
    assert!(!sqlite_caps.schema.schemas);
    assert!(sqlite_caps.query.positional_parameters);

    let mysql_caps = connector.capabilities(&ConnectionConfig {
        driver: DriverType::Mysql,
        ..sqlite_config()
    });
    assert!(mysql_caps.schema.schemas);
    assert!(!mysql_caps.features.tablespaces);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. SQLite execution path: query returns rows, execute returns affected count
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sqlite_execute_returns_affected_rows() {
    let connector = CompositeConnector::new();
    let handle = connector.connect(&sqlite_config(), "").await.expect("connect");
    connector
        .execute(&handle, "CREATE TABLE t (id INTEGER)", &[])
        .await
        .expect("create");
    let affected = connector
        .execute(&handle, "INSERT INTO t VALUES (1)", &[])
        .await
        .expect("insert");
    assert_eq!(affected, 1);
    connector.disconnect(&handle).await.expect("disconnect");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Transaction path: all-or-nothing rollback on failure
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sqlite_transaction_rolls_back_on_failure() {
    let connector = CompositeConnector::new();
    let handle = connector.connect(&sqlite_config(), "").await.expect("connect");
    connector
        .execute(&handle, "CREATE TABLE txprobe (id INTEGER)", &[])
        .await
        .expect("create");

    let failure = connector
        .execute_transaction(
            &handle,
            &[
                "INSERT INTO txprobe VALUES (1)".to_owned(),
                "INVALID STATEMENT".to_owned(),
            ],
            &[false, true],
        )
        .await
        .expect_err("must fail");

    assert_eq!(failure.phase, TransactionFailurePhase::Statement);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);

    let count = connector
        .query(&handle, "SELECT count(*) FROM txprobe", &[])
        .await
        .expect("count");
    // The count should be 0 because the insert was rolled back.
    let cnt: i64 = match &count.rows[0].0[0] {
        db_pro_core::domain::query::CellValue::Int64(n) => *n,
        _ => panic!("unexpected cell"),
    };
    assert_eq!(cnt, 0, "the failed transaction must have rolled back");
    connector.disconnect(&handle).await.expect("disconnect");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Introspection returns canonical schema model
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sqlite_introspect_returns_canonical_model() {
    let connector = CompositeConnector::new();
    let handle = connector.connect(&sqlite_config(), "").await.expect("connect");
    connector
        .execute(&handle, "CREATE TABLE probe1 (id INTEGER PRIMARY KEY, name TEXT)", &[])
        .await
        .expect("create");
    let intro = connector.introspect(&handle).await.expect("introspect");
    assert!(!intro.tables.is_empty());
    assert!(intro.tables.iter().any(|t| t.name == "probe1"));
    connector.disconnect(&handle).await.expect("disconnect");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Cancel is a no-op on SQLite (not supported at connector level for simple queries)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sqlite_cancel_is_safe() {
    let connector = CompositeConnector::new();
    let handle = connector.connect(&sqlite_config(), "").await.expect("connect");
    // Cancel on a handle with no running query should not panic or error.
    connector.cancel(&handle).await.expect("cancel is safe");
    connector.disconnect(&handle).await.expect("disconnect");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Register factory at runtime
// ═══════════════════════════════════════════════════════════════════════════

struct StubFactory;
#[async_trait::async_trait]
impl ProviderFactory for StubFactory {
    fn driver(&self) -> DriverType {
        DriverType::Mysql
    }
    fn capabilities(&self, _config: &ConnectionConfig) -> DatabaseCapabilities {
        DatabaseCapabilities::mysql()
    }
    fn build(&self) -> Box<dyn DbConnector> {
        // This stub factory is never asked to build a connector because we only test capability dispatch.
        panic!("stub factory build should not be called in this test")
    }
    async fn test_connection(&self, _config: &ConnectionConfig, _password: &str) -> Result<(), DbError> {
        Err(DbError::Validation("stub".into()))
    }
}

#[test]
fn register_factory_replaces_existing_factory() {
    let mut connector = CompositeConnector::new();
    // MySQL is now registered by default, so replacing it returns the old factory.
    let previous = connector.register_factory(Box::new(StubFactory));
    assert!(previous.is_some(), "MySQL factory is registered by default");

    let caps = connector.capabilities(&ConnectionConfig {
        driver: DriverType::Mysql,
        ..sqlite_config()
    });
    assert!(!caps.features.tablespaces);
    assert!(caps.schema.schemas);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Value/decoder contract, driven against whichever provider DATABASE_URL names
// ═══════════════════════════════════════════════════════════════════════════
//
// Sections 3–6 above exercise SQLite in memory, so nothing in this file read a value from
// a real server — which is why the MySQL mapper could serve `count(*)` as a boolean and
// `DECIMAL`/`DATE` as the literal text `unsupported` with every test here green (#235;
// `docs/release/evidence/v01-runtime/providers/60-mysql-live-fixture-and-mapper.md`).
// This section reads the fixture's `decoder_matrix` — the table both fixtures ship for
// exactly this purpose (`fixtures/postgres/001_schema.sql`, `fixtures/mysql/001_schema.sql`)
// — through the composite connector, so the same contract is asserted against whichever
// provider `DATABASE_URL` points at. SQL Server drives the capability-appropriate subset
// of the same matrix (`fixtures/mssql/001_schema.sql`, #260): classes whose TDS decoder
// arm exists (`query_mapper.rs::decode_cell`) assert unchanged; the Json arm has no SQL
// Server counterpart and is driver-gated to a parse-and-members check, not weakened.
// With no `postgres://`/`mysql://`/`sqlserver://` URL the tests skip
// with a reason instead of failing, which keeps a PostgreSQL-only CI job green.

/// A fixture this suite can drive: the parsed connection plus the two per-fixture details
/// the shared `decoder_matrix` contract needs.
struct FixtureTarget {
    config: ConnectionConfig,
    password: String,
    /// The fixture's byte column — the contract needs a binary cell, and the two fixtures
    /// name theirs differently (`blob` vs `blob_data`).
    binary_column: &'static str,
    /// What that column holds, so "stays bytes" is asserted against the fixture.
    binary_hex: &'static str,
}

/// Parses `DATABASE_URL` the way `pg_integration.rs` and `mysql_fixture_matrix.rs` already do.
/// `sqlserver://` (and `mssql://`) go through the product parser instead of the manual split,
/// because TDS fixture passwords are percent-encoded in the URL (`…%21` must arrive as `…!`)
/// and `parse_connection_snippet` is the only decoder of that form (#260).
fn fixture_target() -> Option<FixtureTarget> {
    let url = std::env::var("DATABASE_URL").ok()?;
    if url.starts_with("sqlserver://") || url.starts_with("mssql://") || url.starts_with("jdbc:sqlserver:") {
        return sqlserver_target(&url);
    }
    let (driver, rest, fallback_port) = if let Some(rest) = url.strip_prefix("postgres://") {
        (DriverType::Postgres, rest, 5432u16)
    } else {
        let rest = url.strip_prefix("mysql://")?;
        (DriverType::Mysql, rest, 3306u16)
    };
    let (binary_column, binary_hex) = match driver {
        DriverType::Postgres => ("blob", "deadbeef"),
        DriverType::Mysql => ("blob_data", "ff00fe01"),
        // SQLite has no fixture server; SQL Server is handled by `sqlserver_target` above.
        DriverType::SQLite | DriverType::SqlServer => return None,
    };

    let (without_query, _query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, database) = without_query.rsplit_once('/')?;
    let (auth, host_port) = authority.rsplit_once('@').unwrap_or(("", authority));
    let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, ""));

    Some(FixtureTarget {
        config: ConnectionConfig {
            name: "conformance-fixture".into(),
            host: host.to_string(),
            port: port.parse().unwrap_or(fallback_port),
            database: database.to_string(),
            username: username.to_string(),
            driver,
            ssl_mode: Default::default(),
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
            create_if_missing: false,
},
        password: password.to_string(),
        binary_column,
        binary_hex,
    })
}

/// Build the SQL Server fixture target from a parsed `sqlserver://` URL. The TDS
/// matrix (`fixtures/mssql/001_schema.sql`) names its binary column like the MySQL
/// fixture; its seed bytes are distinct so a cross-fixture mix-up is visible.
fn sqlserver_target(url: &str) -> Option<FixtureTarget> {
    let parsed = parse_connection_snippet(url).ok()?;
    if parsed.driver != DriverType::SqlServer {
        return None;
    }
    let password = parsed.password.unwrap_or_default();
    Some(FixtureTarget {
        config: ConnectionConfig {
            name: "conformance-fixture".into(),
            host: parsed.host,
            port: parsed.port,
            database: parsed.database,
            username: parsed.username,
            driver: DriverType::SqlServer,
            // Mirrors `sqlserver_integration.rs`: live fixture runs with TLS required.
            ssl_mode: parsed.ssl_mode.unwrap_or(SslMode::Require),
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
            create_if_missing: false,
},
        password,
        binary_column: "blob_data",
        binary_hex: "deadbeef2024",
    })
}

fn cell<'a>(result: &'a QueryResult, row: usize, column: &str) -> &'a CellValue {
    let index = result
        .columns
        .iter()
        .position(|meta| meta.name == column)
        .unwrap_or_else(|| panic!("decoder_matrix has no column {column}"));
    &result.rows[row].0[index]
}

/// `CellValue` deliberately has no `PartialEq` (two `Decimal("1.0")` and `Decimal("1.00")`
/// are different representations of the same number), so each class is destructured here
/// and the mismatch panic prints the value that actually arrived.
fn expect_int64(cell: &CellValue, column: &str) -> i64 {
    match cell {
        CellValue::Int64(value) => *value,
        other => panic!("{column} must decode to Int64, got {other:?}"),
    }
}

fn expect_bool(cell: &CellValue, column: &str) -> bool {
    match cell {
        CellValue::Bool(value) => *value,
        other => panic!("{column} must decode to Bool, got {other:?}"),
    }
}

fn expect_decimal(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::Decimal(digits) => digits.clone(),
        other => panic!("{column} must decode to Decimal, got {other:?}"),
    }
}

fn expect_date(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::Date(value) => value.clone(),
        other => panic!("{column} must decode to Date, got {other:?}"),
    }
}

fn expect_text(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::Text(value) => value.clone(),
        other => panic!("{column} must decode to Text, got {other:?}"),
    }
}

fn expect_timestamp(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::Timestamp(value) => value.clone(),
        other => panic!("{column} must decode to Timestamp, got {other:?}"),
    }
}

fn expect_timestamp_tz(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::TimestampTz(value) => value.clone(),
        other => panic!("{column} must decode to TimestampTz, got {other:?}"),
    }
}

fn expect_bytes_hex(cell: &CellValue, column: &str) -> String {
    match cell {
        CellValue::Bytes(bytes) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        other => panic!("{column} must stay Bytes, got {other:?}"),
    }
}

fn assert_null(cell: &CellValue, column: &str) {
    assert!(matches!(cell, CellValue::Null), "{column} must stay NULL, got {cell:?}");
}

/// The class-aware decode contract, asserted cell by cell against a live server.
#[tokio::test]
async fn provider_value_decoder_contract_holds_against_the_fixture() {
    let Some(target) = fixture_target() else {
        eprintln!(
            "skipping value/decoder conformance: DATABASE_URL is not a postgres://, mysql:// or sqlserver:// URL"
        );
        return;
    };
    let connector = CompositeConnector::new();
    let handle = connector
        .connect(&target.config, &target.password)
        .await
        .expect("fixture connect must succeed");
    let sql = format!(
        "SELECT id, flag, big_count, ratio, precise_ratio, amount, calendar_date, wall_time, \
         local_stamp, instant, doc, status, missing, {} FROM decoder_matrix ORDER BY id",
        target.binary_column
    );
    let result = connector
        .query(&handle, &sql, &[])
        .await
        .expect("decoder_matrix must exist: load the fixture (001_schema.sql, 002_seed.sql)");
    connector.disconnect(&handle).await.expect("disconnect");

    assert_eq!(result.row_count, 2, "one populated row and one all-NULL row");

    // ── Integers stay integers, and are never booleans ──────────────────────
    // The MySQL mapper's old probe chain decoded every integer that fit an `i8` through the
    // driver's `bool` decoder, so `SELECT count(*)` arrived as `Bool(true)`.
    assert_eq!(
        expect_int64(cell(&result, 0, "id"), "id"),
        1,
        "an integer primary key is Int64, not Bool"
    );
    assert_eq!(
        expect_int64(cell(&result, 0, "big_count"), "big_count"),
        i64::MAX,
        "the widest integer keeps its exact value"
    );
    // The one genuinely boolean column still decodes as a boolean: the rule is class-aware,
    // not "every value is an integer".
    assert!(expect_bool(cell(&result, 0, "flag"), "flag"));

    // ── Floating classes stay Float64 ───────────────────────────────────────
    assert!(matches!(cell(&result, 0, "ratio"), CellValue::Float64(_)));
    assert!(matches!(cell(&result, 0, "precise_ratio"), CellValue::Float64(_)));

    // ── DECIMAL-class values are exact: no f64 hop, no rounding, scale kept ─
    let amount = expect_decimal(cell(&result, 0, "amount"), "amount");
    let (integer_part, fraction) = amount.split_once('.').expect("Decimal keeps its scale");
    assert_eq!(
        integer_part, "12345678901234567890",
        "all 20 integer digits survive; an f64 round-trip would have destroyed them"
    );
    assert!(!fraction.is_empty(), "the declared scale survives");
    let through_f64: f64 = integer_part.parse().expect("parses as a float");
    assert_ne!(
        format!("{through_f64:.0}"),
        integer_part,
        "proof this column is not served through an f64"
    );

    // ── Temporal classes keep their real semantics ──────────────────────────
    let date = expect_date(cell(&result, 0, "calendar_date"), "calendar_date");
    assert_eq!(date.len(), 10, "a DATE carries no time and no offset: {date}");
    assert!(matches!(cell(&result, 0, "wall_time"), CellValue::Time(_)));

    // A naive wall-clock value must not gain an invented offset. Both fixtures seed
    // `2024-03-15 10:20:30.123456`; MySQL used to serve this column as unusable text.
    let naive = expect_timestamp(cell(&result, 0, "local_stamp"), "local_stamp");
    assert!(
        naive.starts_with("2024-03-15T10:20:30.123456"),
        "the wall-clock reading is kept verbatim: {naive}"
    );
    assert!(
        !naive.ends_with('Z') && !naive.contains('+'),
        "a timestamp without time zone must not gain an offset: {naive}"
    );

    // An instant keeps its marker.
    let instant = expect_timestamp_tz(cell(&result, 0, "instant"), "instant");
    assert!(
        instant.ends_with('Z') || instant.contains('+'),
        "an instant keeps an explicit offset marker: {instant}"
    );

    // ── Structured classes ─────────────────────────────────────────────────
    match cell(&result, 0, "doc") {
        CellValue::Json(value) => assert_eq!(value["a"], serde_json::json!(1)),
        // SQL Server has no JSON decoder arm in `query_mapper::decode_cell`: its JSON
        // is NVARCHAR(MAX) behind an ISJSON CHECK, so Text is the honest class for this
        // driver. Gated by driver instead of downgrading the Json assertion for everyone:
        // the payload must still parse as JSON and carry the same member.
        CellValue::Text(value) if target.config.driver == DriverType::SqlServer => {
            let parsed: serde_json::Value =
                serde_json::from_str(value).unwrap_or_else(|error| panic!("doc must be valid JSON text: {error}"));
            assert_eq!(parsed["a"], serde_json::json!(1));
        }
        other => panic!("doc must decode to Json, got {other:?}"),
    }
    expect_text(cell(&result, 0, "status"), "status");

    // ── Binary classes stay bytes, byte-exact ──────────────────────────────
    assert_eq!(
        expect_bytes_hex(cell(&result, 0, target.binary_column), target.binary_column),
        target.binary_hex,
        "{} must arrive byte-exact",
        target.binary_column
    );

    // ── NULL is preserved per family, on the all-NULL row ──────────────────
    // `flag` is excluded: PostgreSQL and SQL Server declare it NOT NULL (and seed FALSE/0),
    // MySQL declares it nullable and seeds NULL, so that one column differs by provider
    // on the NULL row.
    for column in [
        "big_count",
        "ratio",
        "precise_ratio",
        "amount",
        "calendar_date",
        "wall_time",
        "local_stamp",
        "instant",
        "doc",
        "status",
        "missing",
        target.binary_column,
    ] {
        assert_null(cell(&result, 1, column), column);
    }
}

/// Criterion 1 of #234 asserted "no regressions" for the new factory dispatch without
/// demonstrating it: `pg_integration.rs` builds `PostgresConnector` directly, so no test
/// drove PostgreSQL *through* the composite. This one does, against the live fixture.
#[tokio::test]
async fn factory_dispatch_drives_postgres_against_the_live_fixture() {
    let Some(target) = fixture_target() else {
        eprintln!("skipping factory-dispatch test: DATABASE_URL is not a postgres://, mysql:// or sqlserver:// URL");
        return;
    };
    if target.config.driver != DriverType::Postgres {
        eprintln!("skipping factory-dispatch test: DATABASE_URL is not a postgres:// URL");
        return;
    }

    let connector = CompositeConnector::new();
    // The capability set must come from the registered factory, not from a default.
    let caps = connector.capabilities(&target.config);
    assert_eq!(caps.driver, DriverType::Postgres);
    assert!(caps.schema.schemas, "the PostgreSQL factory's own set is served");
    assert!(caps.features.server_sessions);

    let handle = connector
        .connect(&target.config, &target.password)
        .await
        .expect("the factory-built PostgreSQL connector must connect");
    let result = connector
        .query(&handle, "SELECT count(*) AS total FROM decoder_matrix", &[])
        .await
        .expect("query through the composite");
    assert_eq!(
        expect_int64(cell(&result, 0, "total"), "total"),
        2,
        "the fixture seeds two rows"
    );
    let introspected = connector
        .introspect(&handle)
        .await
        .expect("introspect through the composite");
    assert!(introspected.tables.iter().any(|table| table.name == "decoder_matrix"));
    connector.disconnect(&handle).await.expect("disconnect");
}

/// Issue #260: the same factory-dispatch evidence PostgreSQL got in #235, for the
/// TDS provider — the composite must serve the SQL Server factory's own capability
/// set (including the honesty flips: no partitions / dependency catalog / sequences)
/// and drive connect/query/introspect through it against the live fixture.
#[tokio::test]
async fn factory_dispatch_drives_sql_server_against_the_live_fixture() {
    let Some(target) = fixture_target() else {
        eprintln!("skipping SQL Server factory-dispatch test: DATABASE_URL is not a sqlserver:// URL");
        return;
    };
    if target.config.driver != DriverType::SqlServer {
        eprintln!("skipping SQL Server factory-dispatch test: DATABASE_URL is not a sqlserver:// URL");
        return;
    }

    let connector = CompositeConnector::new();
    let caps = connector.capabilities(&target.config);
    assert_eq!(caps.driver, DriverType::SqlServer);
    assert!(caps.schema.schemas, "the SQL Server factory's own set is served");
    // #260 capability honesty: paths the TDS provider does not implement stay false.
    assert!(!caps.features.partitions);
    assert!(!caps.features.object_dependencies);
    assert!(!caps.schema.sequences);
    assert!(!caps.features.server_sessions);

    let handle = connector
        .connect(&target.config, &target.password)
        .await
        .expect("the factory-built SQL Server connector must connect");
    let result = connector
        .query(&handle, "SELECT COUNT(*) AS total FROM decoder_matrix", &[])
        .await
        .expect("query through the composite");
    assert_eq!(
        expect_int64(cell(&result, 0, "total"), "total"),
        2,
        "the fixture seeds two rows"
    );
    let introspected = connector
        .introspect(&handle)
        .await
        .expect("introspect through the composite");
    assert!(introspected.tables.iter().any(|table| table.name == "decoder_matrix"));
    connector.disconnect(&handle).await.expect("disconnect");
}

/// Issue #260: the classes the shared matrix cannot express but SQL Server ships —
/// `uniqueidentifier` — must arrive losslessly through the composite, and the
/// datetimeoffset arm must keep its offset marker (the shared test asserts this
/// shape; this one pins the concrete value end to end).
#[tokio::test]
async fn sqlserver_native_classes_decode_losslessly_through_the_composite() {
    let Some(target) = fixture_target() else {
        eprintln!("skipping SQL Server native-class test: DATABASE_URL is not a sqlserver:// URL");
        return;
    };
    if target.config.driver != DriverType::SqlServer {
        eprintln!("skipping SQL Server native-class test: DATABASE_URL is not a sqlserver:// URL");
        return;
    }

    let connector = CompositeConnector::new();
    let handle = connector
        .connect(&target.config, &target.password)
        .await
        .expect("fixture connect must succeed");
    let result = connector
        .query(
            &handle,
            "SELECT token, instant, blob_data FROM decoder_matrix WHERE id = 1",
            &[],
        )
        .await
        .expect("the mssql decoder_matrix must exist: load fixtures/mssql (001_schema.sql, 002_seed.sql)");
    connector.disconnect(&handle).await.expect("disconnect");

    match cell(&result, 0, "token") {
        CellValue::Uuid(value) => {
            let parsed = uuid::Uuid::parse_str(value).unwrap_or_else(|error| panic!("token must be a UUID: {error}"));
            assert_eq!(
                parsed.simple().to_string(),
                "a0eebc999c0b4ef8bb6d6bb9bd380a11",
                "the uniqueidentifier arrives without byte or ordering loss"
            );
        }
        other => panic!("token must decode to Uuid, got {other:?}"),
    }
    let instant = expect_timestamp_tz(cell(&result, 0, "instant"), "instant");
    assert!(
        instant.starts_with("2024-03-15T10:20:30.123456") && (instant.ends_with('Z') || instant.contains('+')),
        "datetimeoffset keeps its wall reading and an explicit offset: {instant}"
    );
    assert_eq!(
        expect_bytes_hex(cell(&result, 0, target.binary_column), target.binary_column),
        target.binary_hex,
        "varbinary(max) stays byte-exact"
    );
}
