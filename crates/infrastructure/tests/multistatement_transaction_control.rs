//! Live SQLite verification of the `#147` rule: a multi-statement batch may not carry its own
//! transaction control.
//!
//! These tests run the **product path** against a real database file — `QueryService::execute_multi`
//! with the real `SQLiteConnector` and the real meta store, i.e. everything the query editor uses
//! except the UI/IPC layer — plus one test that calls `DbConnector::execute_transaction` directly.
//!
//! The PostgreSQL half of the same rule lives in `pg_integration.rs` (`#[ignore]`d, it needs
//! `DATABASE_URL`); this file needs no fixture.
//!
//! Run with: `cargo test --package db-pro-infrastructure --test multistatement_transaction_control`

use db_pro_core::application::query_service::QueryService;
use db_pro_core::application::registry::ConnectionRegistry;
use db_pro_core::domain::connection::{
    Connection, ConnectionConfig, ConnectionHandle, ConnectionId, DriverType, SslMode,
};
use db_pro_core::domain::query::CellValue;
use db_pro_core::ports::{ConnectionRepository, DbConnector, TransactionFailureOutcome};
use db_pro_infrastructure::meta::store::SQLiteMetaStore;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;
use std::sync::Arc;

fn config(database: &str) -> ConnectionConfig {
    ConnectionConfig {
        name: "multistatement-transaction-control".into(),
        host: String::new(),
        port: 0,
        database: database.into(),
        username: String::new(),
        driver: DriverType::SQLite,
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
        // The harness owns a fresh scratch directory, so the file is expected to
        // be created on first connect (#597 made that an explicit opt-in).
        create_if_missing: true,
    }
}

/// A scratch directory this test owns: one database file and one meta-store file.
fn scratch_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("db-pro-ms-tx-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// The service under test plus the pieces the assertions need: a second connection to the same
/// database file, so a test can read the table back and can call `execute_transaction` directly.
///
/// Two connectors rather than one because `SQLiteConnector` is not cloneable and the service owns
/// its own: the handle is only valid for the connector that opened it.
struct Harness {
    service: QueryService,
    reader: SQLiteConnector,
    reader_handle: ConnectionHandle,
    connection_id: ConnectionId,
    _dir: std::path::PathBuf,
}

async fn harness() -> Harness {
    let dir = scratch_dir();
    let database = dir.join("data.db").to_string_lossy().into_owned();
    let meta_path = dir.join("meta.db").to_string_lossy().into_owned();

    let service_connector = SQLiteConnector::new();
    let connection = Connection::new(config(&database));
    let service_handle = service_connector
        .connect(&config(&database), "")
        .await
        .expect("connect service connector");

    let reader = SQLiteConnector::new();
    let reader_handle = reader
        .connect(&config(&database), "")
        .await
        .expect("connect reader connector");
    reader
        .execute(&reader_handle, "CREATE TABLE probe (id INTEGER)", &[])
        .await
        .expect("create probe table");

    let meta = SQLiteMetaStore::new(&meta_path).await.expect("meta store");
    // The safety policy the service applies comes from the stored connection config.
    ConnectionRepository::save(&meta, &connection)
        .await
        .expect("save connection");

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(connection.id, service_handle);

    let service = QueryService::new(
        Box::new(service_connector),
        Box::new(meta.clone()),
        Box::new(meta.clone()),
        Box::new(meta.clone()),
        Arc::clone(&registry),
        Box::new(meta),
    );

    Harness {
        service,
        reader,
        reader_handle,
        connection_id: connection.id,
        _dir: dir,
    }
}

async fn probe_row_count(harness: &Harness) -> i64 {
    let result = harness
        .reader
        .query(&harness.reader_handle, "SELECT count(*) FROM probe", &[])
        .await
        .expect("count probe rows");
    match &result.rows[0].0[0] {
        CellValue::Int64(count) => *count,
        other => panic!("expected an Int64 count, got {other:?}"),
    }
}
/// The batch the issue measured: an insert that commits itself, then a failing statement. It must be
/// refused before dispatch, so **no statement runs at all** and the table stays empty — where the
/// defect left the first insert committed while the envelope reported a rollback.
#[tokio::test]
async fn sqlite_batch_that_commits_itself_is_refused_and_writes_nothing() {
    let harness = harness().await;

    let result = harness
        .service
        .execute_multi(
            &harness.connection_id,
            "INSERT INTO probe (id) VALUES (1); COMMIT; SELECT no_such_column FROM probe",
            None,
            None,
        )
        .await
        .expect("a refused batch is a result carrying an error");

    let (index, error) = result.error.expect("the batch is refused");
    assert_eq!(index, 1);
    assert!(error.message.contains("`COMMIT`"), "message: {}", error.message);
    assert!(result.results.is_empty(), "a refused batch runs no statement");
    assert_eq!(probe_row_count(&harness).await, 0, "nothing may be written");
}

/// The control case the issue's own evidence used: without transaction control in the script the
/// batch still runs as one transaction, so the earlier insert is rolled back with the failure.
#[tokio::test]
async fn sqlite_batch_without_transaction_control_still_rolls_back() {
    let harness = harness().await;

    let result = harness
        .service
        .execute_multi(
            &harness.connection_id,
            "INSERT INTO probe (id) VALUES (2); SELECT no_such_column FROM probe",
            None,
            None,
        )
        .await
        .expect("a failed batch is a result carrying an error");

    let (index, error) = result.error.expect("the failing statement is reported");
    assert_eq!(index, 1);
    assert!(
        error.message.contains("rolled back"),
        "the envelope must report the confirmed rollback, got: {}",
        error.message
    );
    assert_eq!(
        probe_row_count(&harness).await,
        0,
        "the earlier insert must be rolled back"
    );
}

/// Characterisation test: what `execute_transaction` does on its own when a script carries
/// transaction control. This is the defect the dispatch rule in `QueryService::execute_multi` exists
/// for, pinned on purpose: the connector is **not** changed by the #147 fix (the refusal is one
/// layer up), so this is what shows the assumption behind it is real on SQLite — and what fails
/// loudly if someone removes the refusal without giving the connector the guarantee itself.
///
/// Measured on this host (probe output in the evidence file), and **the shape differs per provider**:
/// a leading `BEGIN` is refused by SQLite itself ("cannot start a transaction within a transaction"),
/// so the harmful shape here is a `COMMIT` with no `BEGIN` in front of it — the user's `COMMIT` ends
/// the wrapper's transaction, the insert survives, and the envelope reports `Unknown` because the
/// wrapper's rollback has nothing left to roll back.
#[tokio::test]
async fn sqlite_connector_alone_still_lets_a_script_commit_itself() {
    let harness = harness().await;

    let failure = harness
        .reader
        .execute_transaction(
            &harness.reader_handle,
            &[
                "INSERT INTO probe (id) VALUES (3)".to_owned(),
                "COMMIT".to_owned(),
                "SELECT no_such_column FROM probe".to_owned(),
            ],
            &[false, false, true],
        )
        .await
        .expect_err("the statement after the COMMIT fails");

    assert_eq!(failure.statement_index, 2);
    assert_eq!(
        failure.outcome,
        TransactionFailureOutcome::Unknown,
        "the wrapper cannot roll back a transaction the script already committed"
    );
    assert!(
        failure.error.to_string().contains("no transaction is active"),
        "the rollback failure is what the envelope surfaces, got: {}",
        failure.error
    );
    assert_eq!(
        probe_row_count(&harness).await,
        1,
        "the user's COMMIT ended the wrapper transaction, so the insert survived"
    );
}

/// The other half of the same characterisation: on SQLite a leading `BEGIN` never reaches the
/// harmful state, because the database refuses it outright — a second transaction cannot start
/// inside the wrapper's. Worth pinning, because the PostgreSQL shape of this same batch behaves
/// differently (PG half in `pg_integration.rs`), and the dispatch rule exists so that neither shape
/// depends on which provider the user happens to be connected to.
#[tokio::test]
async fn sqlite_refuses_a_leading_begin_inside_the_wrapper_transaction() {
    let harness = harness().await;

    let failure = harness
        .reader
        .execute_transaction(
            &harness.reader_handle,
            &[
                "BEGIN".to_owned(),
                "INSERT INTO probe (id) VALUES (4)".to_owned(),
                "COMMIT".to_owned(),
                "SELECT no_such_column FROM probe".to_owned(),
            ],
            &[false, false, false, true],
        )
        .await
        .expect_err("BEGIN cannot start a transaction inside the wrapper's");

    assert_eq!(failure.statement_index, 0);
    assert_eq!(failure.outcome, TransactionFailureOutcome::RolledBack);
    assert_eq!(probe_row_count(&harness).await, 0);
}
