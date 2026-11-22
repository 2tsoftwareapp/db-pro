//! Live PostgreSQL backup/restore verification through an SSH local forward.
//!
//! Required environment:
//! `DB_PRO_SSH_HOST`, `DB_PRO_SSH_PORT`, `DB_PRO_SSH_USER`, `DB_PRO_SSH_KEY`,
//! `DB_PRO_SSH_TARGET_HOST`, `DB_PRO_SSH_TARGET_PORT`, `DB_PRO_SSH_DATABASE`,
//! `DB_PRO_SSH_USERNAME`, and `DB_PRO_SSH_PASSWORD`.
//!
//! The SSH host key must already be trusted by the OpenSSH known-hosts policy;
//! this test deliberately does not disable verification or modify that policy.

use db_pro_core::domain::backup::{BackupFormat, BackupOptions, RestoreOptions};
use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SshTunnelConfig, SslMode};
use db_pro_core::domain::query::CellValue;
use db_pro_core::ports::{BackupEngine, DbConnector};
use db_pro_infrastructure::backup::pg_dump::PgDumpEngine;
use db_pro_infrastructure::postgres::connector::PostgresConnector;

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for SSH backup verification"))
}

fn has_live_fixture_configuration() -> bool {
    [
        "DB_PRO_SSH_HOST",
        "DB_PRO_SSH_PORT",
        "DB_PRO_SSH_USER",
        "DB_PRO_SSH_KEY",
        "DB_PRO_SSH_TARGET_HOST",
        "DB_PRO_SSH_TARGET_PORT",
        "DB_PRO_SSH_DATABASE",
        "DB_PRO_SSH_USERNAME",
        "DB_PRO_SSH_PASSWORD",
    ]
    .into_iter()
    .all(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

fn parse_port(name: &str) -> u16 {
    required_env(name)
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be a valid port"))
}

fn connection_config(database: String) -> ConnectionConfig {
    ConnectionConfig {
        name: "ssh-backup-runtime-verification".into(),
        host: required_env("DB_PRO_SSH_TARGET_HOST"),
        port: parse_port("DB_PRO_SSH_TARGET_PORT"),
        database,
        username: required_env("DB_PRO_SSH_USERNAME"),
        driver: DriverType::Postgres,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: Some(SshTunnelConfig {
            host: required_env("DB_PRO_SSH_HOST"),
            port: parse_port("DB_PRO_SSH_PORT"),
            user: required_env("DB_PRO_SSH_USER"),
            private_key_path: required_env("DB_PRO_SSH_KEY"),
            password: std::env::var("DB_PRO_SSH_TUNNEL_PASSWORD").ok(),
        }),
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
    }
}

async fn connect(config: &ConnectionConfig) -> (PostgresConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let connector = PostgresConnector::new();
    let password = required_env("DB_PRO_SSH_PASSWORD");
    let handle = connector
        .connect(config, &password)
        .await
        .expect("PostgreSQL connection through SSH tunnel should succeed");
    (connector, handle)
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL target and a local SSH server"]
async fn pg_backup_and_restore_work_through_live_ssh_tunnel() {
    if !has_live_fixture_configuration() {
        eprintln!("skipping live SSH backup verification: DB_PRO_SSH_* fixture is not configured");
        return;
    }

    let source_database = required_env("DB_PRO_SSH_DATABASE");
    let admin_config = connection_config("postgres".into());
    let restore_database = format!("dbpro_ssh_restore_{}", uuid::Uuid::new_v4().simple());
    let backup_path = std::env::temp_dir().join(format!("db-pro-ssh-backup-{}.sql", uuid::Uuid::new_v4()));
    let password = required_env("DB_PRO_SSH_PASSWORD");

    let (admin_connector, admin_handle) = connect(&admin_config).await;
    let source_engine = PgDumpEngine::new(connection_config(source_database));
    let backup_options = BackupOptions {
        connection_id: "ssh-live-verification".into(),
        output_path: backup_path.to_string_lossy().into_owned(),
        format: BackupFormat::Plain,
        schemas: vec![],
        tables: vec![],
    };

    let result = async {
        source_engine
            .backup(&backup_options, &password)
            .await
            .expect("pg_dump should complete through the SSH tunnel");
        assert!(
            tokio::fs::metadata(&backup_path)
                .await
                .expect("backup should exist")
                .len()
                > 0
        );

        admin_connector
            .execute(&admin_handle, &format!("CREATE DATABASE \"{restore_database}\""), &[])
            .await
            .expect("restore database should be created");

        PgDumpEngine::new(connection_config(restore_database.clone()))
            .restore(
                &RestoreOptions {
                    connection_id: "ssh-live-verification".into(),
                    input_path: backup_path.to_string_lossy().into_owned(),
                    format: BackupFormat::Plain,
                },
                &password,
            )
            .await
            .expect("psql restore should complete through the SSH tunnel");

        let (restore_connector, restore_handle) = connect(&connection_config(restore_database.clone())).await;
        let restored = restore_connector
            .query(&restore_handle, "SELECT count(*) FROM categories", &[])
            .await
            .expect("restored database should be queryable");
        assert!(matches!(restored.rows[0].0[0], CellValue::Int64(count) if count > 0));
        restore_connector
            .disconnect(&restore_handle)
            .await
            .expect("restore connection should disconnect");
        Ok::<(), String>(())
    }
    .await;

    let cleanup_error = admin_connector
        .execute(
            &admin_handle,
            &format!("DROP DATABASE IF EXISTS \"{restore_database}\""),
            &[],
        )
        .await
        .err();
    admin_connector
        .disconnect(&admin_handle)
        .await
        .expect("admin connection should disconnect");
    let _ = tokio::fs::remove_file(&backup_path).await;

    if let Some(error) = cleanup_error {
        panic!("cleanup failed after SSH backup verification: {error}");
    }
    result.expect("live SSH backup/restore verification failed");
}
