//! Live MySQL fixture matrix for the #235 provider.
//!
//! Runs against a MySQL 8 server whose `DATABASE_URL` is a `mysql://` URL and the fixture
//! in `fixtures/mysql/` is loaded (`001_schema.sql` then `002_seed.sql`; `003_verify.sql`
//! self-checks the load). Without a MySQL URL the tests skip with a reason instead of
//! failing, so the CI-mirroring run (`cargo test --all -- --include-ignored`) stays green
//! in the PostgreSQL-only job.
//!
//! Unlike `mysql_integration.rs`, these tests are **not** `#[ignore]`d: a plain
//! `cargo test --workspace` with a `mysql://` `DATABASE_URL` exercises them, which is what
//! makes the live path reproducible on a workstation. See
//! `docs/release/evidence/v01-runtime/providers/60-mysql-live-fixture-and-mapper.md` for the
//! CI change that would make them run in CI as well.
//!
//! Reproduce locally:
//! ```text
//! docker run -d --name dbpro-v01-mysql-fixture -e MYSQL_ROOT_PASSWORD=<pw> \
//!   -e MYSQL_DATABASE=dbpro_fixture -p 33306:3306 \
//!   -v "$PWD/fixtures/mysql/001_schema.sql:/docker-entrypoint-initdb.d/001_schema.sql:ro" \
//!   -v "$PWD/fixtures/mysql/002_seed.sql:/docker-entrypoint-initdb.d/002_seed.sql:ro" \
//!   -v "$PWD/fixtures/mysql/003_verify.sql:/docker-entrypoint-initdb.d/003_verify.sql:ro" \
//!   mysql:8.4 --default-time-zone=+00:00
//! DATABASE_URL=mysql://root:<pw>@127.0.0.1:33306/dbpro_fixture \
//!   cargo test -p db-pro-infrastructure --test mysql_fixture_matrix
//! ```

use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use db_pro_core::domain::query::CellValue;
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::mysql::connector::MySqlConnector;

fn mysql_config() -> Option<(ConnectionConfig, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    if !url.starts_with("mysql://") {
        return None;
    }
    let without_prefix = url.strip_prefix("mysql://")?;
    let (auth, rest) = without_prefix.split_once('@')?;
    let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host_port, database) = rest.split_once('/')?;
    let (host, port) = host_port.split_once(':').unwrap_or((host_port, "3306"));
    Some((
        ConnectionConfig {
            name: "mysql-fixture".into(),
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
            create_if_missing: false,
},
        password.to_string(),
    ))
}

async fn setup() -> Option<(MySqlConnector, ConnectionHandle)> {
    let (config, password) = mysql_config()?;
    let connector = MySqlConnector::new();
    let handle = connector
        .connect(&config, &password)
        .await
        .expect("MySQL connect failed");
    Some((connector, handle))
}

/// The decoder matrix: one row with a value for every class the provider-value contract
/// distinguishes, one row NULL in every nullable family. Asserted expected-vs-actual
/// against a live server, so a class that silently changes representation fails here
/// rather than in a consumer.
#[tokio::test]
// cc-scan:allow HARD_COMPLEXITY — decoder matrix test: one assertion per value class, table-flat by design
async fn mysql_fixture_decoder_matrix_covers_every_value_class() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping MySQL fixture test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let result = connector
        .query(&handle, "SELECT * FROM decoder_matrix ORDER BY id", &[])
        .await
        .expect("decoder_matrix must exist: load fixtures/mysql/001_schema.sql and 002_seed.sql");

    assert_eq!(result.row_count, 2, "one populated row and one NULL row");
    let row = &result.rows[0].0;
    assert_eq!(row.len(), 41, "one cell per decoder_matrix column");

    let checks: Vec<(&str, bool)> = vec![
        ("id smallint", matches!(&row[0], CellValue::Int64(1))),
        ("flag tinyint(1)/boolean", matches!(&row[1], CellValue::Bool(true))),
        ("tiny_count tinyint", matches!(&row[2], CellValue::Int64(-128))),
        ("small_count smallint", matches!(&row[3], CellValue::Int64(-32_768))),
        ("medium_count mediumint", matches!(&row[4], CellValue::Int64(8_388_607))),
        ("count_value int", matches!(&row[5], CellValue::Int64(2_147_483_647))),
        (
            "big_count bigint",
            matches!(&row[6], CellValue::Int64(9_223_372_036_854_775_807)),
        ),
        ("tiny_unsigned", matches!(&row[7], CellValue::Int64(255))),
        ("int_unsigned", matches!(&row[8], CellValue::Int64(4_294_967_295))),
        (
            "big_unsigned above i64::MAX stays exact",
            matches!(&row[9], CellValue::Decimal(value) if value == "18446744073709551615"),
        ),
        (
            "ratio float",
            matches!(&row[10], CellValue::Float64(value) if (*value - 1.5).abs() < f64::EPSILON),
        ),
        (
            "precise_ratio double",
            matches!(&row[11], CellValue::Float64(value) if (*value - 0.1).abs() < f64::EPSILON),
        ),
        (
            "amount decimal(30,10) keeps every digit",
            matches!(&row[12], CellValue::Decimal(value) if value == "12345678901234567890.1234567890"),
        ),
        (
            "scaled_amount decimal(10,2) keeps declared scale",
            matches!(&row[13], CellValue::Decimal(value) if value == "10.50"),
        ),
        (
            "calendar_date",
            matches!(&row[14], CellValue::Date(value) if value == "2024-03-15"),
        ),
        (
            "wall_time",
            matches!(&row[15], CellValue::Time(value) if value == "10:20:30.123456"),
        ),
        (
            "long_time keeps the interval shape",
            matches!(&row[16], CellValue::Time(value) if value == "800:59:59.123456"),
        ),
        (
            "negative_time keeps its sign",
            matches!(&row[17], CellValue::Time(value) if value == "-100:30:15.500000"),
        ),
        (
            "local_stamp datetime has no invented offset",
            matches!(&row[18], CellValue::Timestamp(value) if value == "2024-03-15T10:20:30.123456"),
        ),
        (
            "instant timestamp is an instant",
            matches!(&row[19], CellValue::TimestampTz(value) if value == "2024-03-15T10:20:30.123456Z"),
        ),
        ("year_value year", matches!(&row[20], CellValue::Int64(2024))),
        ("bit_one bit(1)", matches!(&row[21], CellValue::Int64(1))),
        ("bit_eight bit(8)", matches!(&row[22], CellValue::Int64(170))),
        (
            "bit_sixtyfour bit(64) is not a boolean",
            matches!(&row[23], CellValue::Decimal(value) if value == "18446744073709551615"),
        ),
        (
            "token char(36)",
            matches!(&row[24], CellValue::Text(value) if value == "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
        ),
        (
            "label varchar round-trips its text",
            matches!(&row[25], CellValue::Text(value) if value == "Ünïcödé ✓ 日本語"),
        ),
        (
            "binary_collation stays byte-exact",
            matches!(&row[26], CellValue::Bytes(value) if value == b"bin-collated"),
        ),
        (
            "body text",
            matches!(&row[27], CellValue::Text(value) if value == "plain text body"),
        ),
        (
            "tiny_body text",
            matches!(&row[28], CellValue::Text(value) if value == "tiny"),
        ),
        (
            "medium_body text",
            matches!(&row[29], CellValue::Text(value) if value == "medium"),
        ),
        (
            "long_body longtext",
            matches!(&row[30], CellValue::Text(value) if value.starts_with("long text ") && value.len() == 400),
        ),
        (
            "raw_bytes binary(8)",
            matches!(&row[31], CellValue::Bytes(value) if value == &[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33]),
        ),
        (
            "raw_varbinary varbinary",
            matches!(&row[32], CellValue::Bytes(value) if value == &[0xff, 0x00, 0xfe, 0x01]),
        ),
        (
            "blob_data blob",
            matches!(&row[33], CellValue::Bytes(value) if value == &[0xff, 0x00, 0xfe, 0x01]),
        ),
        (
            "tiny_raw blob whose bytes are valid UTF-8 stays bytes",
            matches!(&row[34], CellValue::Bytes(value) if value == b"hello"),
        ),
        (
            "long_raw longblob",
            matches!(&row[35], CellValue::Bytes(value) if value.len() == 64 && value.iter().all(|byte| *byte == 0xab)),
        ),
        (
            "status enum",
            matches!(&row[36], CellValue::Text(value) if value == "shipped"),
        ),
        (
            "tags set",
            matches!(&row[37], CellValue::Text(value) if value == "alpha,gamma"),
        ),
        (
            "doc json",
            matches!(&row[38], CellValue::Json(value) if value["a"] == 1 && value["b"].is_array()),
        ),
        ("geo geometry stays byte-exact", matches!(&row[39], CellValue::Bytes(_))),
        ("missing null", matches!(&row[40], CellValue::Null)),
    ];

    let wrong: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    assert!(wrong.is_empty(), "decoder matrix mismatch: {wrong:?}");

    if let CellValue::Bytes(wkb) = &row[39] {
        assert_eq!(wkb.len(), 25, "SRID + WKB: {wkb:02x?}");
    }

    // The provider's own class names stay available to the policy layer.
    let declared: Vec<&str> = result.columns.iter().map(|c| c.data_type.as_str()).collect();
    for expected in [
        "BOOLEAN",
        "BIGINT UNSIGNED",
        "DECIMAL",
        "DATETIME",
        "TIMESTAMP",
        "TIME",
        "BIT",
        "JSON",
        "ENUM",
        "GEOMETRY",
        "BLOB",
        "TEXT",
    ] {
        assert!(
            declared.iter().any(|name| name.contains(expected)),
            "the declared type {expected} must reach the caller: {declared:?}"
        );
    }

    // The NULL row: the key keeps its value, every other family is a null cell rather
    // than an error or empty bytes.
    assert!(
        matches!(&result.rows[1].0[0], CellValue::Int64(2)),
        "the primary key keeps its value"
    );
    for (index, cell) in result.rows[1].0.iter().enumerate().skip(1) {
        assert!(
            matches!(cell, CellValue::Null),
            "column {index} of the NULL row must be Null, got {cell:?}"
        );
    }

    connector.disconnect(&handle).await.unwrap();
}

/// The query path an operator actually uses: aggregates, a routine, the composite key and
/// the unsigned class. This is where the first live run failed — `SELECT count(*)` came
/// back as a boolean, because every integer class is compatible with the driver's `i8`.
#[tokio::test]
async fn mysql_fixture_query_path_keeps_integers_and_routine_results_typed() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping MySQL fixture test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let count = connector
        .query(&handle, "SELECT count(*) AS total FROM decoder_matrix", &[])
        .await
        .expect("count query");
    assert!(
        matches!(&count.rows[0].0[0], CellValue::Int64(2)),
        "an aggregate integer must be an integer, got {:?}",
        count.rows[0].0[0]
    );

    let keys = connector
        .query(
            &handle,
            "SELECT oi.order_id, oi.line_no, oi.quantity, order_item_total(oi.order_id) AS total \
             FROM order_items oi ORDER BY oi.order_id, oi.line_no",
            &[],
        )
        .await
        .expect("composite key query");
    assert_eq!(keys.rows.len(), 3, "three seeded order lines");
    assert!(matches!(&keys.rows[0].0[0], CellValue::Int64(1001)));
    assert!(matches!(&keys.rows[0].0[1], CellValue::Int64(1)));
    assert!(matches!(&keys.rows[0].0[2], CellValue::Int64(2)));
    assert!(
        matches!(&keys.rows[0].0[3], CellValue::Int64(7)),
        "the routine returns an integer, got {:?}",
        keys.rows[0].0[3]
    );
    assert!(
        matches!(&keys.rows[2].0[2], CellValue::Null),
        "a NULL quantity stays NULL"
    );

    // The premise that makes the `TIMESTAMP` arm truthful: the driver pins the session
    // zone to UTC, so the value MySQL renders is the instant. If this ever changes, the
    // `TIMESTAMP` decoding in `mysql::query_mapper` has to be revisited with it.
    let zone = connector
        .query(&handle, "SELECT @@session.time_zone AS zone", &[])
        .await
        .expect("session time zone");
    assert!(
        matches!(&zone.rows[0].0[0], CellValue::Text(value) if value == "+00:00"),
        "the MySQL session time zone must stay UTC for TIMESTAMP decoding, got {:?}",
        zone.rows[0].0[0]
    );

    connector.disconnect(&handle).await.unwrap();
}

/// Schema metadata for the composite key, the foreign key and the indexes. Every MySQL
/// index used to arrive with an empty `table_name`, which the per-table filter in
/// `SchemaService` reads as "not this table" — so no MySQL index was ever shown.
#[tokio::test]
async fn mysql_fixture_introspection_reports_composite_key_foreign_key_and_indexes() {
    let Some((connector, handle)) = setup().await else {
        eprintln!("skipping MySQL fixture test: DATABASE_URL is not a mysql:// URL");
        return;
    };

    let intro = connector.introspect(&handle).await.expect("introspect");

    let primary = intro
        .primary_keys
        .iter()
        .find(|key| key.table_name == "order_items")
        .expect("order_items must have a primary key");
    assert_eq!(
        primary.columns,
        vec!["order_id".to_string(), "line_no".to_string()],
        "the composite key keeps its declared column order"
    );

    let index = intro
        .indexes
        .iter()
        .find(|index| index.name == "idx_order_items_sku")
        .expect("the user index must be introspected");
    assert_eq!(index.table_name, "order_items", "an index must carry its table");
    assert_eq!(index.columns, vec!["sku".to_string()]);
    assert!(!index.unique, "idx_order_items_sku is not unique");
    assert!(!index.primary);

    let primary_index = intro
        .indexes
        .iter()
        .find(|index| index.name == "PRIMARY" && index.table_name == "order_items")
        .expect("the primary key is also an index");
    assert!(primary_index.primary);

    let foreign_key = intro
        .foreign_keys
        .iter()
        .find(|key| key.name == "fk_order_items_category")
        .expect("the foreign key must be introspected");
    assert_eq!(foreign_key.from_table, "order_items");
    assert_eq!(foreign_key.from_columns, vec!["category_id".to_string()]);
    assert_eq!(foreign_key.to_table, "categories");
    assert_eq!(foreign_key.to_columns, vec!["id".to_string()]);

    assert!(
        intro.tables.iter().any(|table| table.name == "decoder_matrix"),
        "the tables list must contain the fixture table"
    );
    assert!(
        intro.views.iter().any(|view| view.name == "v_order_item_totals"),
        "the view must be introspected"
    );
    let trigger = intro
        .triggers
        .iter()
        .find(|trigger| trigger.name == "order_items_after_insert")
        .expect("the trigger must be introspected");
    assert_eq!(trigger.table_name, "order_items");
    assert_eq!(trigger.timing, "AFTER");
    assert_eq!(trigger.event, "INSERT");
    assert!(
        intro
            .functions
            .iter()
            .any(|function| function.name == "order_item_total"),
        "the routine must be introspected: {:?}",
        intro.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    let sku = intro
        .columns
        .iter()
        .find(|column| column.table_name == "order_items" && column.name == "sku")
        .expect("the sku column must be introspected");
    assert_eq!(sku.data_type, "varchar");
    assert!(sku.nullable);
    assert!(!sku.is_primary_key);

    connector.disconnect(&handle).await.unwrap();
}
