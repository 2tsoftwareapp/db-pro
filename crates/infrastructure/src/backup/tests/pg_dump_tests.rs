//! Tests for `../pg_dump.rs` (moved out of the source file).
use super::*;

#[test]
fn ensure_tool_available_rejects_missing_binary() {
    let err = PgDumpEngine::ensure_tool_available("db-pro-definitely-missing-tool-xyz").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not found on PATH"), "{msg}");
    assert!(msg.contains("Install PostgreSQL client tools"), "{msg}");
}

#[test]
fn ensure_tool_available_finds_common_unix_binary() {
    // `sh` is present on unix CI images; proves the PATH search path works.
    assert!(PgDumpEngine::ensure_tool_available("sh").is_ok());
}

/// #244 (E-2) asks for the argv shape to be asserted without a live server. This pins the
/// **chosen** behaviour (option (b) of the issue): the restore is not wrapped in a transaction,
/// so the app must say so on failure rather than silently implying an atomic restore.
#[test]
// cc-scan:allow HUGE_FUNCTION — argv-assertion test builds the full config inline; splitting fragments the scenario
fn restore_argv_has_no_transaction_boundary_and_is_documented_as_such() {
    let config = ConnectionConfig {
        name: "fixture".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 55432,
        database: "dbpro_fixture".to_owned(),
        username: "dbpro".to_owned(),
        driver: db_pro_core::domain::connection::DriverType::Postgres,
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: None,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};
    let plain = RestoreOptions {
        connection_id: "c1".to_owned(),
        input_path: "/tmp/dump.sql".to_owned(),
        format: BackupFormat::Plain,
    };
    let custom = RestoreOptions {
        connection_id: "c1".to_owned(),
        input_path: "/tmp/dump.dump".to_owned(),
        format: BackupFormat::Custom,
    };

    let pgpass_path = Path::new("/tmp/db-pro-pgpass-test");
    let plain_args = PgDumpEngine::restore_command(&plain, &config, pgpass_path)
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let custom_args = PgDumpEngine::restore_command(&custom, &config, pgpass_path)
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(plain_args.first().map(String::as_str), Some("-f"));
    assert_eq!(plain_args.get(1).map(String::as_str), Some("/tmp/dump.sql"));
    assert_eq!(custom_args.first().map(String::as_str), Some("/tmp/dump.dump"));
    for args in [&plain_args, &custom_args] {
        assert!(
            !args.iter().any(|arg| arg.contains("single-transaction")),
            "option (b) is chosen: the restore must not silently become transactional: {args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "--exit-on-error"),
            "option (b) is chosen: no exit-on-error flag either: {args:?}"
        );
        assert!(args.windows(2).any(|pair| pair == ["-d", "dbpro_fixture"]), "{args:?}");
        assert!(
            !args.iter().any(|arg| arg.contains("pw")),
            "password must not appear in argv (it rides in PGPASSFILE instead): {args:?}"
        );
    }

    // #430: credentials go through a pgpass file, never the child's environment.
    let envs: Vec<(String, Option<String>)> = PgDumpEngine::restore_command(&plain, &config, pgpass_path)
        .as_std()
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert_eq!(
        envs.iter()
            .find(|(key, _)| key == "PGPASSFILE")
            .and_then(|(_, v)| v.clone()),
        Some("/tmp/db-pro-pgpass-test".to_owned()),
        "PGPASSFILE must point at the pgpass file: {envs:?}"
    );
    assert!(
        !envs.iter().any(|(key, _)| key == "PGPASSWORD"),
        "PGPASSWORD must not reach the child environment: {envs:?}"
    );
}

#[test]
fn backup_staging_path_is_unpredictable_sibling() {
    let output = Path::new("/tmp/dumps/nightly.dump");
    let first = PgDumpEngine::backup_staging_path(output);
    let second = PgDumpEngine::backup_staging_path(output);

    assert_ne!(first, second, "each run must pick a fresh name");
    assert_eq!(
        first.parent(),
        output.parent(),
        "staging must share the destination directory"
    );
    assert_ne!(first, output, "pg_dump must never see the caller-visible path");
    assert!(
        first.file_name().unwrap().to_string_lossy().contains("pgdump-tmp"),
        "staging name must be recognizable for cleanup: {}",
        first.display()
    );
}

#[tokio::test]
async fn pgpass_file_is_owner_only_and_removed_on_drop() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    let config = ConnectionConfig {
        name: "fixture".to_owned(),
        host: "db.internal".to_owned(),
        port: 5432,
        database: "dbpro:fixture".to_owned(),
        username: "dbpro".to_owned(),
        driver: db_pro_core::domain::connection::DriverType::Postgres,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: None,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};

    let path = {
        let guard = PgPassFile::create(&config, "p\\a:ss").await?;
        let contents = tokio::fs::read_to_string(&guard.path).await?;
        assert_eq!(
            contents, "db.internal:5432:dbpro\\:fixture:dbpro:p\\\\a\\:ss\n",
            "pgpass fields must backslash-escape `:` and `\\`: {contents}"
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&guard.path)?.permissions().mode() & 0o777,
            0o600,
            "libpq rejects pgpass files others can read"
        );
        guard.path.clone()
    };
    assert!(!path.exists(), "drop must remove the credential file");
    Ok(())
}

#[tokio::test]
async fn external_command_timeout_returns_query_timeout() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 1"]);

    let error = PgDumpEngine::run_command(&mut command, 50, "test command")
        .await
        .expect_err("a sleeping command must exceed the test timeout");

    assert!(matches!(error, DbError::QueryTimeout { timeout_ms: 50 }));
}

#[tokio::test]
async fn backup_output_reservation_rejects_existing_file() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("db-pro-pg-backup-{}.dump", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, b"existing backup").await?;

    let error = match PgDumpEngine::reserve_backup_output(&path).await {
        Ok(()) => return Err("existing backup output was reserved".into()),
        Err(error) => error,
    };

    assert!(matches!(error, DbError::Validation(message) if message.contains("already exists")));
    tokio::fs::remove_file(path).await?;
    Ok(())
}

#[test]
fn failed_pgpass_write_removes_the_partial_file() {
    // #483: when write_all fails, `create` returns before the `PgPassFile` guard exists, so
    // the credential file the OpenOptions call already made must be unlinked here.
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::StorageFull, "disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let path = std::env::temp_dir().join(format!("db-pro-pgpass-test-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"").expect("fixture file");
    assert!(path.exists());

    let error = write_pgpass(Failing, &path, b"h:5432:d:u:p\n").expect_err("failing sink must error");
    assert!(matches!(error, DbError::Internal(_)), "{error:?}");
    assert!(!path.exists(), "partial credential file must be removed");
}

#[test]
fn successful_pgpass_write_leaves_file_for_guard() {
    let path = std::env::temp_dir().join(format!("db-pro-pgpass-test-{}", uuid::Uuid::new_v4()));
    let file = std::fs::File::create(&path).expect("fixture file");

    write_pgpass(file, &path, b"h:5432:d:u:p\n").expect("write succeeds");
    assert_eq!(std::fs::read(&path).expect("read back"), b"h:5432:d:u:p\n");
    std::fs::remove_file(path).expect("cleanup fixture");
}
