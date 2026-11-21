//! Tests for `../backup_service.rs` (moved out of the source file).
use super::*;
use crate::domain::backup::BackupFormat;
use crate::domain::connection::{DriverType, SshTunnelConfig, SslMode};
use crate::ports::{MockBackupEngine, MockConnectionRepository, MockIntrospectionCache, MockSecretStore};
use std::sync::{Arc, Mutex};

fn permissive_cache() -> MockIntrospectionCache {
    let mut cache = MockIntrospectionCache::new();
    cache.expect_invalidate().returning(|_| Ok(()));
    cache
}

fn postgres_config_with_ssh() -> ConnectionConfig {
    ConnectionConfig {
        name: "postgres over ssh".into(),
        host: "db.internal".into(),
        port: 5432,
        database: "app".into(),
        username: "app_user".into(),
        driver: DriverType::Postgres,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: Some(SshTunnelConfig {
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
            host: "bastion.example".into(),
            port: 22,
            user: "tunnel_user".into(),
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
            private_key_path: "/tmp/test-key".into(),
            password: None,
        }),
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

fn postgres_config_without_ssh() -> ConnectionConfig {
    let mut config = postgres_config_with_ssh();
    config.ssh_tunnel = None;
    config
}

fn sqlite_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "sqlite backup".into(),
        host: String::new(),
        port: 0,
        database: "/tmp/test.db".into(),
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
async fn backup_factory_receives_ssh_configuration() {
    let connection_id = ConnectionId::new();
    let mut connections = MockConnectionRepository::new();
    let config = postgres_config_with_ssh();
    let connection = Connection::new(config.clone()).with_secret_ref("custom/key".into());
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().times(2).returning(|key| {
        if key == "custom/key" {
            Ok(Some("password".into()))
        } else {
            assert!(key.ends_with("/ssh_password"));
            Ok(Some("ssh-password".into()))
        }
    });

    let saw_ssh_config = Arc::new(Mutex::new(false));
    let saw_ssh_config_for_factory = Arc::clone(&saw_ssh_config);
    let pg_factory = Box::new(move |config: &ConnectionConfig| {
        let ssh_tunnel = config.ssh_tunnel.as_ref().expect("SSH config should be present");
        assert_eq!(ssh_tunnel.password.as_deref(), Some("ssh-password"));
        *saw_ssh_config_for_factory.lock().expect("test mutex poisoned") = true;
        let mut engine = MockBackupEngine::new();
        engine.expect_backup().returning(|_, _| {
            Ok(BackupResult {
                output_path: "/tmp/backup.dump".into(),
                size_bytes: 1,
            })
        });
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let sqlite_factory =
        Box::new(|_: &str| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) as Box<dyn BackupEngine> });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(secrets),
        Arc::new(ConnectionRegistry::new()),
        Box::new(permissive_cache()),
        pg_factory,
        sqlite_factory,
    );

    let result = service
        .backup(&BackupOptions {
            connection_id: connection_id.to_string(),
            output_path: "/tmp/backup.dump".into(),
            format: BackupFormat::Custom,
            schemas: vec![],
            tables: vec![],
        })
        .await;

    assert!(result.is_ok());
    assert!(*saw_ssh_config.lock().expect("test mutex poisoned"));
}

#[tokio::test]
async fn backup_uses_persisted_custom_secret_reference() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(postgres_config_without_ssh()).with_secret_ref("migrated/password".into());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .withf(|key| key == "migrated/password")
        .returning(|_| Ok(Some("custom".into())));

    let pg_factory = Box::new(|_: &ConnectionConfig| {
        let mut engine = MockBackupEngine::new();
        engine
            .expect_backup()
            .withf(|_, password| password == "custom")
            .returning(|_, _| {
                Ok(BackupResult {
                    output_path: "/tmp/backup.dump".into(),
                    size_bytes: 1,
                })
            });
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let sqlite_factory = Box::new(|_: &str| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(secrets),
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
        Arc::new(ConnectionRegistry::new()),
        Box::new(permissive_cache()),
        pg_factory,
        sqlite_factory,
    );

    service
        .backup(&BackupOptions {
            connection_id: connection_id.to_string(),
            output_path: "/tmp/backup.dump".into(),
            format: BackupFormat::Custom,
            schemas: vec![],
            tables: vec![],
        })
        .await
        .expect("backup should use the persisted custom secret");
}

#[tokio::test]
async fn restore_uses_persisted_custom_secret_reference() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(postgres_config_without_ssh()).with_secret_ref("migrated/password".into());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .withf(|key| key == "migrated/password")
        .returning(|_| Ok(Some("custom".into())));

    let pg_factory = Box::new(|_: &ConnectionConfig| {
        let mut engine = MockBackupEngine::new();
        engine
            .expect_restore()
            .withf(|_, password| password == "custom")
            .returning(|_, _| Ok(()));
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let sqlite_factory = Box::new(|_: &str| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(secrets),
        Arc::new(ConnectionRegistry::new()),
        Box::new(permissive_cache()),
        pg_factory,
        sqlite_factory,
    );

    service
        .restore(&RestoreOptions {
            connection_id: connection_id.to_string(),
            input_path: "/tmp/backup.dump".into(),
            format: BackupFormat::Custom,
        })
        .await
        .expect("restore should use the persisted custom secret");
}

#[tokio::test]
async fn sqlite_backup_does_not_require_a_database_secret() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(sqlite_config());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let pg_factory = Box::new(|_: &ConnectionConfig| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) });
    let sqlite_factory = Box::new(|_: &str| {
        let mut engine = MockBackupEngine::new();
        engine
            .expect_backup()
            .withf(|_, password| password.is_empty())
            .returning(|_, _| {
                Ok(BackupResult {
                    output_path: "/tmp/sqlite-backup.db".into(),
                    size_bytes: 1,
                })
            });
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(MockSecretStore::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(permissive_cache()),
        pg_factory,
        sqlite_factory,
    );

    service
        .backup(&BackupOptions {
            connection_id: connection_id.to_string(),
            output_path: "/tmp/sqlite-backup.db".into(),
            format: BackupFormat::Custom,
            schemas: vec![],
            tables: vec![],
        })
        .await
        .expect("SQLite backup should not require a database secret");
}

#[tokio::test]
async fn sqlite_restore_does_not_require_a_database_secret() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(sqlite_config());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let pg_factory = Box::new(|_: &ConnectionConfig| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) });
    let sqlite_factory = Box::new(|_: &str| {
        let mut engine = MockBackupEngine::new();
        engine
            .expect_restore()
            .withf(|_, password| password.is_empty())
            .returning(|_, _| Ok(()));
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(MockSecretStore::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(permissive_cache()),
        pg_factory,
        sqlite_factory,
    );

    service
        .restore(&RestoreOptions {
            connection_id: connection_id.to_string(),
            input_path: "/tmp/sqlite-backup.db".into(),
            format: BackupFormat::Custom,
        })
        .await
        .expect("SQLite restore should not require a database secret");
}

#[tokio::test]
async fn sqlite_restore_is_refused_while_the_connection_is_active() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(sqlite_config());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(connection_id, crate::domain::connection::ConnectionHandle::new(1));

    let restore_reached_the_engine = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = Arc::clone(&restore_reached_the_engine);
    let sqlite_factory = Box::new(move |_: &str| {
        let observed = Arc::clone(&observed);
        let mut engine = MockBackupEngine::new();
        engine.expect_restore().returning(move |_, _| {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(MockSecretStore::new()),
        registry,
        Box::new(permissive_cache()),
        Box::new(|_: &ConnectionConfig| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) }),
        sqlite_factory,
    );

    let error = service
        .restore(&RestoreOptions {
            connection_id: connection_id.to_string(),
            input_path: "/tmp/sqlite-backup.db".into(),
            format: BackupFormat::Custom,
        })
        .await
        .expect_err("restore must be refused while the connection is active");

    assert!(
        error.to_string().contains("disconnect the SQLite connection"),
        "{error}"
    );
    assert!(
        !restore_reached_the_engine.load(std::sync::atomic::Ordering::SeqCst),
        "the engine must not run a restore that replaces a live database file"
    );
}

#[tokio::test]
async fn successful_restore_drops_the_cached_schema_for_the_connection() {
    let connection_id = ConnectionId::new();
    let connection = Connection::new(sqlite_config());
    let mut connections = MockConnectionRepository::new();
    connections
        .expect_get()
        .returning(move |_| Ok(Some(connection.clone())));

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_invalidate()
        .withf(move |id| *id == connection_id)
        .times(1)
        .returning(|_| Ok(()));

    let sqlite_factory = Box::new(|_: &str| {
        let mut engine = MockBackupEngine::new();
        engine.expect_restore().returning(|_, _| Ok(()));
        Box::new(engine) as Box<dyn BackupEngine>
    });
    let service = BackupService::new(
        Box::new(connections),
        Box::new(MockSecretStore::new()),
        Arc::new(ConnectionRegistry::new()),
        Box::new(cache),
        Box::new(|_: &ConnectionConfig| -> Box<dyn BackupEngine> { Box::new(MockBackupEngine::new()) }),
        sqlite_factory,
    );

    service
        .restore(&RestoreOptions {
            connection_id: connection_id.to_string(),
            input_path: "/tmp/sqlite-backup.db".into(),
            format: BackupFormat::Custom,
        })
        .await
        .expect("restore should succeed");
}
