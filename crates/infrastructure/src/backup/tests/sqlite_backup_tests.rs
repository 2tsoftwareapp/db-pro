//! Tests for `../sqlite_backup.rs` (moved out of the source file).
use super::*;
use db_pro_core::domain::backup::{BackupFormat, BackupOptions, RestoreOptions};
use db_pro_core::domain::connection::{DriverType, SslMode};

fn config(database: &Path) -> ConnectionConfig {
    ConnectionConfig {
        name: "test".into(),
        host: String::new(),
        port: 0,
        database: database.to_string_lossy().into_owned(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
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
    }
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn backup_and_restore_publish_only_valid_sqlite_files() {
    let root = std::env::temp_dir().join(format!("db-pro-backup-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("source.db");
    let backup = root.join("backup.db");
    let invalid = root.join("invalid.db");

    let connection = rusqlite::Connection::open(&source).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'snapshot');",
        )
        .unwrap();
    drop(connection);

    let engine = SqliteBackupEngine::new(config(&source));
    let result = engine
        .backup(
            &BackupOptions {
                connection_id: "test".into(),
                output_path: backup.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
                schemas: vec![],
                tables: vec![],
            },
            "",
        )
        .await
        .unwrap();
    assert!(result.size_bytes > 0);
    let backup_connection = rusqlite::Connection::open(&backup).unwrap();
    assert_eq!(
        backup_connection
            .query_row("SELECT value FROM items", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "snapshot"
    );
    drop(backup_connection);

    std::fs::write(&invalid, b"not a sqlite database").unwrap();
    let error = engine
        .restore(
            &RestoreOptions {
                connection_id: "test".into(),
                input_path: invalid.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
            },
            "",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("SQLite") || error.to_string().contains("database"));
    assert!(source.exists());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn backup_publish_does_not_overwrite_existing_destination() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("db-pro-backup-race-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&root).await?;
    let source = root.join("temporary.db");
    let destination = root.join("backup.db");
    tokio::fs::write(&source, b"new backup").await?;
    tokio::fs::write(&destination, b"old backup").await?;

    let error = match publish_backup_without_overwrite(&source, &destination).await {
        Ok(()) => return Err("existing backup destination was overwritten".into()),
        Err(error) => error,
    };

    assert!(matches!(error, DbError::Validation(message) if message.contains("already exists")));
    assert_eq!(tokio::fs::read(&destination).await?, b"old backup");
    assert_eq!(tokio::fs::read(&source).await?, b"new backup");
    tokio::fs::remove_dir_all(root).await?;
    Ok(())
}

fn temporary_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("db-pro-backup-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn values(database: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(database).unwrap();
    let mut statement = connection.prepare("SELECT value FROM items ORDER BY value").unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn create_items_database(database: &Path, values_sql: &str) {
    let connection = rusqlite::Connection::open(database).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); {values_sql}"
        ))
        .unwrap();
}

fn options(database: &Path) -> BackupOptions {
    BackupOptions {
        connection_id: "test".into(),
        output_path: database.to_string_lossy().into_owned(),
        format: BackupFormat::Plain,
        schemas: vec![],
        tables: vec![],
    }
}

/// A database in WAL mode keeps committed data in the `-wal` file until a
/// checkpoint. The backup must still contain it.
#[tokio::test]
async fn backup_captures_committed_writes_that_are_still_in_the_wal() {
    let root = temporary_root("wal-source");
    let source = root.join("source.db");
    let backup = root.join("backup.db");

    let writer = rusqlite::Connection::open(&source).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'committed');",
        )
        .unwrap();
    assert!(
        root.join("source.db-wal").exists(),
        "the write must still be in the WAL"
    );

    SqliteBackupEngine::new(config(&source))
        .backup(&options(&backup), "")
        .await
        .expect("backup should succeed");

    assert_eq!(values(&backup), vec!["committed"]);
    drop(writer);
    std::fs::remove_dir_all(root).unwrap();
}

/// A writer holding an open write transaction must not produce a torn backup:
/// the artifact contains the pre-transaction state, never the uncommitted row.
#[tokio::test]
async fn backup_during_an_open_write_transaction_is_a_consistent_snapshot() {
    let root = temporary_root("concurrent-writer");
    let source = root.join("source.db");
    let backup = root.join("backup.db");

    let writer = rusqlite::Connection::open(&source).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'before');",
        )
        .unwrap();
    writer.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    writer
        .execute_batch("BEGIN IMMEDIATE; INSERT INTO items VALUES (2, 'uncommitted');")
        .unwrap();

    SqliteBackupEngine::new(config(&source))
        .backup(&options(&backup), "")
        .await
        .expect("backup should succeed while a transaction is open");

    // Exactly the committed state: the open transaction is neither visible
    // nor able to produce a half-applied snapshot.
    assert_eq!(values(&backup), vec!["before"]);

    writer.execute_batch("COMMIT").unwrap();
    assert_eq!(values(&source), vec!["before", "uncommitted"]);
    drop(writer);
    std::fs::remove_dir_all(root).unwrap();
}

/// A `-wal` left at the output path (its database is gone) belongs to that
/// other database: publishing the artifact next to it would make the backup
/// read back as foreign content.
#[tokio::test]
async fn backup_does_not_publish_next_to_a_stale_wal() {
    let root = temporary_root("output-sidecar");
    let source = root.join("source.db");
    let backup = root.join("backup.db");

    create_items_database(&source, "INSERT INTO items VALUES (1, 'real-backup-content');");
    // A valid WAL written for a different database, left at the output path.
    let other = root.join("other.db");
    let writer = rusqlite::Connection::open(&other).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
            .execute_batch(
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'foreign-wal-content');",
            )
            .unwrap();
    let valid_wal = std::fs::read(root.join("other.db-wal")).unwrap();
    drop(writer);
    std::fs::remove_file(&other).unwrap();
    std::fs::write(root.join("backup.db-wal"), &valid_wal).unwrap();

    SqliteBackupEngine::new(config(&source))
        .backup(&options(&backup), "")
        .await
        .expect("backup should succeed");

    assert!(
        !root.join("backup.db-wal").exists(),
        "the stale sidecar must not survive"
    );
    assert_eq!(values(&backup), vec!["real-backup-content"]);
    std::fs::remove_dir_all(root).unwrap();
}

/// Replacing the database file is not enough: a `-wal`/`-shm` pair left next
/// to the target by an earlier session belongs to the *old* database and
/// would be replayed into the restored file, so the restore would silently
/// not take effect on what a reader sees.
#[tokio::test]
async fn restore_replaces_the_database_and_invalidates_stale_sidecars() {
    let root = temporary_root("stale-sidecars");
    let target = root.join("target.db");
    let artifact = root.join("artifact.db");

    create_items_database(&artifact, "INSERT INTO items VALUES (1, 'from-backup');");

    // A target whose sidecars were left behind by a crashed WAL-mode session.
    let writer = rusqlite::Connection::open(&target).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'from-old-db');",
        )
        .unwrap();
    let stale_wal = std::fs::read(root.join("target.db-wal")).unwrap();
    let stale_shm = std::fs::read(root.join("target.db-shm")).unwrap();
    drop(writer);
    std::fs::write(root.join("target.db-wal"), &stale_wal).unwrap();
    std::fs::write(root.join("target.db-shm"), &stale_shm).unwrap();

    SqliteBackupEngine::new(config(&target))
        .restore(
            &RestoreOptions {
                connection_id: "test".into(),
                input_path: artifact.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
            },
            "",
        )
        .await
        .expect("restore should succeed");

    assert!(
        !root.join("target.db-wal").exists(),
        "the old database's WAL must not survive"
    );
    assert!(
        !root.join("target.db-shm").exists(),
        "the old database's SHM must not survive"
    );
    assert_eq!(values(&target), vec!["from-backup"]);
    std::fs::remove_dir_all(root).unwrap();
}

/// A failed publish must keep the previous target and remove the staged file.
#[tokio::test]
async fn failed_restore_keeps_the_previous_target_and_removes_the_staged_file() {
    let root = temporary_root("failed-publish");
    let input = root.join("input.db");
    let target = root.join("target.db");

    create_items_database(&input, "INSERT INTO items VALUES (1, 'good');");
    // A target that rename(2) cannot replace: a non-empty directory at the path.
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), b"kept").unwrap();

    let error = SqliteBackupEngine::new(config(&target))
        .restore(
            &RestoreOptions {
                connection_id: "test".into(),
                input_path: input.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
            },
            "",
        )
        .await
        .expect_err("publishing over a directory must fail");

    assert!(
        error.to_string().contains("failed to publish"),
        "unexpected error: {error}"
    );
    assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"kept");
    let leftovers: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".db-pro-"))
        .collect();
    assert!(leftovers.is_empty(), "staged files left behind: {leftovers:?}");
    std::fs::remove_dir_all(root).unwrap();
}

/// A backup that cannot be produced must leave no accepted or partial artifact.
#[tokio::test]
async fn failed_backup_leaves_no_artifact_and_no_staged_file() {
    let root = temporary_root("failed-backup");
    let source = root.join("source.db");
    let backup = root.join("backup.db");

    // Not a database: VACUUM INTO cannot read it.
    std::fs::write(&source, b"not a sqlite database").unwrap();

    SqliteBackupEngine::new(config(&source))
        .backup(&options(&backup), "")
        .await
        .expect_err("backing up a non-database must fail");

    assert!(!backup.exists(), "a failed backup must not publish an artifact");
    let leftovers: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".db-pro-"))
        .collect();
    assert!(leftovers.is_empty(), "staged files left behind: {leftovers:?}");
    std::fs::remove_dir_all(root).unwrap();
}

/// Restore must include committed source rows that are still stored in its WAL.
#[tokio::test]
async fn restore_captures_committed_writes_that_are_still_in_the_wal() {
    let root = temporary_root("restore-wal-source");
    let source = root.join("source.db");
    let target = root.join("target.db");

    let writer = rusqlite::Connection::open(&source).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'committed-in-wal');",
        )
        .unwrap();
    assert!(root.join("source.db-wal").exists(), "committed row must remain in WAL");
    create_items_database(&target, "INSERT INTO items VALUES (1, 'old-target');");

    SqliteBackupEngine::new(config(&target))
        .restore(
            &RestoreOptions {
                connection_id: "test".into(),
                input_path: source.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
            },
            "",
        )
        .await
        .expect("restore should snapshot the source WAL");

    assert_eq!(values(&target), vec!["committed-in-wal"]);
    drop(writer);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn restore_accepts_read_only_sqlite_artifact() {
    use std::os::unix::fs::PermissionsExt;

    let root = temporary_root("restore-read-only");
    let source = root.join("read-only-source.db");
    let target = root.join("target.db");
    create_items_database(&source, "INSERT INTO items VALUES (1, 'read-only-artifact');");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o444)).unwrap();
    create_items_database(&target, "INSERT INTO items VALUES (1, 'old-target');");

    SqliteBackupEngine::new(config(&target))
        .restore(
            &RestoreOptions {
                connection_id: "test".into(),
                input_path: source.to_string_lossy().into_owned(),
                format: BackupFormat::Plain,
            },
            "",
        )
        .await
        .expect("read-only SQLite artifact should be restorable");

    assert_eq!(values(&target), vec!["read-only-artifact"]);
    std::fs::remove_dir_all(root).unwrap();
}
