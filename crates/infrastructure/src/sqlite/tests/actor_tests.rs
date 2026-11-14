//! Tests for `../actor.rs` (moved out of the source file).
use super::*;

fn memory_actor() -> SqliteActor {
    SqliteActor {
        conn: rusqlite::Connection::open_in_memory().expect("in-memory sqlite should open"),
    }
}

#[test]
fn batch_refuses_transaction_control_statements() {
    let actor = memory_actor();
    // A COMMIT inside the batch would escape the wrapping transaction and make
    // rollback a no-op — it must be refused before any statement runs (#359).
    let err = actor
        .handle_execute_batch(&[
            "CREATE TABLE a(x)".to_owned(),
            "COMMIT".to_owned(),
            "DROP TABLE a".to_owned(),
        ])
        .expect_err("batch containing COMMIT must be refused");
    assert!(err.to_string().contains("COMMIT"));
    // Nothing was executed: the table does not exist.
    let after = actor.handle_execute_batch(&["SELECT 1 FROM a".to_owned()]);
    assert!(after.is_err());
}

#[test]
fn batch_refuses_begin_and_savepoint() {
    let actor = memory_actor();
    for control in ["BEGIN", "SAVEPOINT s1", "ROLLBACK"] {
        let err = actor
            .handle_execute_batch(&["CREATE TABLE t(x)".to_owned(), control.to_owned()])
            .expect_err("transaction-control statements must be refused");
        assert!(err.to_string().contains("batch refused"));
    }
}

#[test]
fn batch_without_transaction_control_commits_atomically() {
    let actor = memory_actor();
    let total = actor
        .handle_execute_batch(&[
            "CREATE TABLE a(x INTEGER)".to_owned(),
            "INSERT INTO a VALUES (1), (2), (3)".to_owned(),
        ])
        .expect("ordinary batch should succeed");
    assert_eq!(total, 3);
}

#[test]
fn dispatch_guarded_converts_panic_into_internal_error() {
    let actor = memory_actor();
    let result: Result<(), DbError> = dispatch_guarded(&actor, |_| panic!("simulated handler crash"));
    let err = result.expect_err("a panicking handler must surface an error");
    let message = err.to_string();
    assert!(
        message.contains("panic"),
        "error should mention the panic, got: {message}"
    );
    assert!(
        message.contains("simulated handler crash"),
        "panic payload should be preserved, got: {message}"
    );
}

#[tokio::test]
async fn spawn_sets_busy_timeout_on_actor_connection() {
    let handle = SqliteActor::spawn(":memory:").expect("in-memory actor should spawn");
    let rows = handle
        .raw_query("PRAGMA busy_timeout".into(), Vec::new())
        .await
        .expect("pragma read should succeed");
    let value: i64 = rows[0][0].parse().expect("busy_timeout pragma should be numeric");
    assert!(value >= 10_000, "busy_timeout should be at least 10s, got {value}ms");
    handle.shutdown().await;
}

#[tokio::test]
async fn busy_connection_waits_for_competing_writer() {
    // A second connection to the same file would see SQLITE_BUSY instantly
    // without busy_timeout (#493); it must wait for the lock instead.
    let path = std::env::temp_dir().join(format!("dbpro-busy-timeout-{}.db", std::process::id()));
    let path_str = path.to_string_lossy().to_string();

    let holder = rusqlite::Connection::open(&path).expect("holder connection should open");
    holder.execute_batch("CREATE TABLE t(x)").expect("schema setup failed");

    let handle = SqliteActor::spawn(&path_str).expect("actor should spawn");

    // Take an exclusive write lock on the file, then release it after a delay
    // so the actor's insert runs while the lock is still held.
    holder.execute_batch("BEGIN EXCLUSIVE").expect("lock should be taken");
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(holder); // closing the connection rolls back the open lock
    });

    let affected = handle
        .execute_param("INSERT INTO t VALUES (1)".into(), Vec::new())
        .await
        .expect("write must wait for the lock instead of failing busy");
    assert_eq!(affected, 1);

    releaser.join().expect("releaser thread should finish");
    handle.shutdown().await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn actor_survives_panicking_command_and_keeps_serving() {
    let handle = SqliteActor::spawn(":memory:").expect("in-memory actor should spawn");

    let (tx, rx) = oneshot::channel();
    handle
        .sender
        .send(SqliteCommand::Poison { responder: tx })
        .expect("poison command should be accepted");
    let err = rx
        .await
        .expect("responder should receive a value, not be dropped")
        .expect_err("poisoned command must surface an error");
    assert!(
        err.to_string().contains("panic"),
        "error should mention the panic, got: {err}"
    );

    // The actor thread must still be alive and serving queued commands.
    let rows = handle
        .raw_query("SELECT 1".into(), Vec::new())
        .await
        .expect("actor must keep serving commands after a handler panic");
    assert_eq!(rows[0][0], "1");
    handle.shutdown().await;
}

#[test]
fn trigger_body_is_one_unit_for_the_batch_splitter() {
    // #586: `execute_batch` prepares one statement at a time, so the inner `;`
    // of a trigger body used to be read as a statement boundary.
    let trigger = "CREATE TRIGGER trg AFTER INSERT ON t BEGIN UPDATE t SET b = 'x'; INSERT INTO log VALUES (1); END";
    let units = expand_batch_units(trigger);
    assert_eq!(units.len(), 3, "{units:?}");
    assert_eq!(units[2], "END");
}

#[test]
fn existing_mode_refuses_a_missing_file_without_creating_it() {
    // #597: a mistyped path must not become an empty database.
    let path = std::env::temp_dir().join(format!("dbpro-missing-{}.db", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);

    let error = open_connection(&path_str, SqliteOpenMode::Existing)
        .err()
        .expect("a missing file must not be created");
    assert!(error.to_string().contains("does not exist"), "{error}");
    assert!(!path.exists(), "no file may be created");
}

#[test]
fn create_if_missing_mode_still_creates_the_file() {
    let path = std::env::temp_dir().join(format!("dbpro-create-{}.db", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);

    open_connection(&path_str, SqliteOpenMode::CreateIfMissing).expect("explicit creation is allowed");
    assert!(path.exists(), "the file is created on request");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn read_only_mode_rejects_writes_at_the_engine() {
    // #588: the statement classifier is a UX layer; SQLite itself must refuse.
    let path = std::env::temp_dir().join(format!("dbpro-readonly-{}.db", std::process::id()));
    let path_str = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);
    rusqlite::Connection::open(&path)
        .expect("seed connection")
        .execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);")
        .expect("seed schema");

    let conn = open_connection(&path_str, SqliteOpenMode::ReadOnly).expect("read-only open");
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0)).expect("reads work");
    assert_eq!(count, 1);
    let write = conn.execute_batch("INSERT INTO t VALUES (2)");
    assert!(write.is_err(), "read-only connection must refuse the write");
    drop(conn);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn batch_creates_a_trigger_whose_body_contains_semicolons() {
    // End-to-end form of #586: the whole batch runs inside one transaction.
    let actor = memory_actor();
    actor
        .handle_execute_batch(&["CREATE TABLE t(a INTEGER, b TEXT)".to_owned()])
        .expect("table setup");
    let total = actor
        .handle_execute_batch(&[
            "CREATE TRIGGER trg AFTER INSERT ON t BEGIN UPDATE t SET b = 'x'; INSERT INTO t(a) VALUES (new.a + 100); END"
                .to_owned(),
        ])
        .expect("trigger creation must succeed");
    // Creating a table and a trigger changes no rows.
    assert_eq!(total, 0);
    let trigger_exists: i64 = actor
        .conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = 'trg'", [], |row| row.get(0))
        .expect("inspect sqlite_master");
    assert_eq!(trigger_exists, 1);
}
