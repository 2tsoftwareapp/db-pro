use super::*;
use crate::domain::connection::{ConnectionConfig, DriverType, SslMode};
use crate::ports::{MockConnectionRepository, MockDbConnector, MockIntrospectionCache, MockSecretStore};

fn test_config() -> ConnectionConfig {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
    ConnectionConfig {
        name: "test".into(),
        host: "localhost".into(),
        port: 5432,
        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
        database: "testdb".into(),
        username: "user".into(),
        driver: DriverType::Postgres,
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

fn sqlite_config() -> ConnectionConfig {
    let mut config = test_config();
    config.driver = DriverType::SQLite;
    config.database = "/tmp/db-pro-test.sqlite".into();
    config
}

fn ssh_tunnel() -> crate::domain::connection::SshTunnelConfig {
    crate::domain::connection::SshTunnelConfig {
        host: "bastion.example".into(),
        port: 22,
        user: "deploy".into(),
        private_key_path: "/tmp/key".into(),
        password: None,
    }
}

fn build_service(
    connector: MockDbConnector,
    repo: MockConnectionRepository,
    secrets: MockSecretStore,
) -> ConnectionService {
    ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(secrets),
        Arc::new(ConnectionRegistry::new()),
    )
}

#[path = "connection_service_security_tests.rs"]
mod security_tests;

#[path = "connection_service_provider_tests.rs"]
mod provider_tests;

#[path = "connection_service_lifecycle_tests.rs"]
mod lifecycle_tests;

#[tokio::test]
async fn create_valid_connection() {
    let config = test_config();
    let mut repo = MockConnectionRepository::new();
    repo.expect_save().returning(|_| Ok(()));

    let mut secrets = MockSecretStore::new();
    secrets.expect_store_secret().returning(|_, _| Ok(()));

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    let result = svc.create(config, "pass").await;
    assert!(result.is_ok());
    let conn = result.unwrap();
    assert!(conn.secret_ref.is_some());
}

#[tokio::test]
async fn create_invalid_config_rejected() {
    let mut config = test_config();
    config.name = String::new();

    let svc = build_service(
        MockDbConnector::new(),
        MockConnectionRepository::new(),
        MockSecretStore::new(),
    );
    let result = svc.create(config, "pass").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn connectivity_tests_reject_invalid_provider_config_before_connector_call() {
    let mut config = sqlite_config();
    config.ssh_tunnel = Some(ssh_tunnel());
    let svc = build_service(
        MockDbConnector::new(),
        MockConnectionRepository::new(),
        MockSecretStore::new(),
    );

    let error = svc
        .test_connectivity(&config, "")
        .await
        .expect_err("invalid SQLite SSH configuration must be rejected before testing");
    assert!(matches!(error, DbError::Validation(message) if message.contains("SSH tunnels")));

    let error = svc
        .test_connectivity_with_secret(&ConnectionId::new(), &config, "")
        .await
        .expect_err("secret-backed connectivity must share the same validation boundary");
    assert!(matches!(error, DbError::Validation(message) if message.contains("SSH tunnels")));
}

#[tokio::test]
async fn connect_success() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .returning(|_| Ok(Some("password".into())));

    let mut connector = MockDbConnector::new();
    connector.expect_connect().returning(|_, _| Ok(ConnectionHandle(1)));

    let registry = Arc::new(ConnectionRegistry::new());
    let svc = ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(secrets),
        Arc::clone(&registry),
    );

    let handle = svc.connect(&id).await.unwrap();
    assert_eq!(handle, ConnectionHandle(1));
    assert!(registry.is_active(&id));
}

#[tokio::test]
async fn connect_legacy_postgres_uses_default_secret_key() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let default_key = ConnectionService::secret_key(&id);
    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .withf({
            let default_key = default_key.clone();
            move |key| key == default_key
        })
        .returning(|_| Ok(Some("legacy-password".into())));

    let mut connector = MockDbConnector::new();
    connector.expect_connect().returning(|_, password| {
        assert_eq!(password, "legacy-password");
        Ok(ConnectionHandle(1))
    });

    let svc = build_service(connector, repo, secrets);
    assert_eq!(svc.connect(&id).await.unwrap(), ConnectionHandle(1));
}

#[tokio::test]
async fn connect_hydrates_ssh_password_from_secret_store() {
    let id = ConnectionId::new();
    let mut config = test_config();
    config.ssh_tunnel = Some(ssh_tunnel());
    let conn = Connection::new(config).with_secret_ref("db-key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().times(2).returning(|key| {
        if key == "db-key" {
            Ok(Some("db-password".into()))
        } else {
            assert!(key.ends_with("/ssh_password"));
            Ok(Some("ssh-password".into()))
        }
    });

    let mut connector = MockDbConnector::new();
    connector.expect_connect().returning(|config, password| {
        assert_eq!(password, "db-password");
        assert_eq!(
            config.ssh_tunnel.as_ref().and_then(|ssh| ssh.password.as_deref()),
            Some("ssh-password")
        );
        Ok(ConnectionHandle(1))
    });

    let svc = build_service(connector, repo, secrets);
    svc.connect(&id).await.expect("connect should hydrate SSH credentials");
}

#[tokio::test]
async fn connect_already_active_returns_existing_handle() {
    let id = ConnectionId::new();
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, ConnectionHandle(1));

    let svc = ConnectionService::new(
        Box::new(MockDbConnector::new()),
        Box::new(MockConnectionRepository::new()),
        Box::new(MockSecretStore::new()),
        Arc::clone(&registry),
    );

    let result = svc.connect(&id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ConnectionHandle(1));
}

#[tokio::test]
async fn connect_duplicate_disconnect_failure_propagates() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());
    let registry = Arc::new(ConnectionRegistry::new());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .returning(|_| Ok(Some("password".into())));

    let reg_for_mock = Arc::clone(&registry);
    let mut connector = MockDbConnector::new();
    connector.expect_connect().returning(move |_, _| {
        reg_for_mock.register(id, ConnectionHandle(99));
        Ok(ConnectionHandle(2))
    });
    connector
        .expect_disconnect()
        .returning(|_| Err(DbError::Internal("disconnect failed".into())));

    let svc = ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(secrets),
        Arc::clone(&registry),
    );

    let result = svc.connect(&id).await;
    assert!(result.is_err());
    assert_eq!(registry.get(&id), Some(ConnectionHandle(99)));
}

#[tokio::test]
// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
async fn duplicate_connect_cleanup_handle_can_be_retried() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());
    let registry = Arc::new(ConnectionRegistry::new());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .returning(|_| Ok(Some("password".into())));

    let reg_for_mock = Arc::clone(&registry);
    let mut connector = MockDbConnector::new();
    connector.expect_connect().returning(move |_, _| {
        reg_for_mock.register(id, ConnectionHandle(99));
        Ok(ConnectionHandle(2))
    });
    let disconnect_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    connector.expect_disconnect().times(2).returning({
        let disconnect_calls = Arc::clone(&disconnect_calls);
        move |handle| match disconnect_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => {
                assert_eq!(*handle, ConnectionHandle(2));
                Err(DbError::Internal("temporary disconnect failure".into()))
            }
            1 => {
                assert_eq!(*handle, ConnectionHandle(2));
                Ok(())
            }
            _ => unreachable!("disconnect called more than expected"),
        }
    });

    let svc = ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(secrets),
        Arc::clone(&registry),
    );

    let error = svc
        .connect(&id)
        .await
        .expect_err("duplicate cleanup should report failure");
    assert!(matches!(error, DbError::Internal(message) if message == "temporary disconnect failure"));

    // The active registry entry is intentionally absent in this isolated mock;
    // disconnect still retries the orphaned duplicate handle.
    registry.unregister(&id);
    svc.disconnect(&id).await.expect("orphaned handle should be retried");
}

#[tokio::test]
async fn connect_not_found() {
    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(|_| Ok(None));

    let svc = build_service(MockDbConnector::new(), repo, MockSecretStore::new());
    let result = svc.connect(&ConnectionId::new()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_connectivity_with_secret_uses_saved_password_when_empty() {
    let id = ConnectionId::new();
    let key = ConnectionService::secret_key(&id);

    let mut connector = MockDbConnector::new();
    connector.expect_test_connection().returning(|_, password| {
        assert_eq!(password, "saved-pass");
        Ok(())
    });

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning({
        let key = key.clone();
        move |k| {
            assert_eq!(k, &key);
            Ok(Some("saved-pass".into()))
        }
    });

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(|_| Ok(None));

    let svc = build_service(connector, repo, secrets);
    let result = svc.test_connectivity_with_secret(&id, &test_config(), "").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_connectivity_with_secret_uses_persisted_custom_secret_ref() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("custom/key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));

    let mut connector = MockDbConnector::new();
    connector.expect_test_connection().returning(|_, password| {
        assert_eq!(password, "saved-pass");
        Ok(())
    });

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning(|key| {
        assert_eq!(key, "custom/key");
        Ok(Some("saved-pass".into()))
    });

    let svc = build_service(connector, repo, secrets);
    svc.test_connectivity_with_secret(&id, &test_config(), "")
        .await
        .expect("custom persisted secret should be used");
}

#[tokio::test]
async fn test_connectivity_with_secret_uses_provided_password_when_present() {
    let id = ConnectionId::new();

    let mut connector = MockDbConnector::new();
    connector.expect_test_connection().returning(|_, password| {
        assert_eq!(password, "typed-pass");
        Ok(())
    });

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .returning(|_| Ok(Some("saved-pass".into())));

    let svc = build_service(connector, MockConnectionRepository::new(), secrets);
    let result = svc
        .test_connectivity_with_secret(&id, &test_config(), "typed-pass")
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_connectivity_with_secret_hydrates_ssh_password() {
    let id = ConnectionId::new();
    let mut config = test_config();
    config.ssh_tunnel = Some(ssh_tunnel());

    let mut connector = MockDbConnector::new();
    connector.expect_test_connection().returning(|config, password| {
        assert_eq!(password, "typed-pass");
        assert_eq!(
            config.ssh_tunnel.as_ref().and_then(|ssh| ssh.password.as_deref()),
            Some("ssh-password")
        );
        Ok(())
    });

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning(|key| {
        assert!(key.ends_with("/ssh_password"));
        Ok(Some("ssh-password".into()))
    });

    let svc = build_service(connector, MockConnectionRepository::new(), secrets);
    svc.test_connectivity_with_secret(&id, &config, "typed-pass")
        .await
        .expect("test connectivity should hydrate SSH credentials");
}

#[tokio::test]
async fn delete_active_disconnects_first() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());

    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, ConnectionHandle(1));

    let mut connector = MockDbConnector::new();
    connector.expect_disconnect().returning(|_| Ok(()));

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning({
        let conn = conn.clone();
        move |_| Ok(Some(conn.clone()))
    });
    repo.expect_delete().returning(|_| Ok(()));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning(|_| Ok(Some("pass".into())));
    secrets.expect_delete_secret().returning(|_| Ok(()));

    let svc = ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(secrets),
        Arc::clone(&registry),
    );

    svc.delete(&id).await.unwrap();
    assert!(!registry.is_active(&id));
}

#[tokio::test]
async fn delete_continues_when_secret_retrieve_fails() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));
    repo.expect_delete().returning(|_| Ok(()));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning(|_| {
        Err(DbError::Internal(
            "keyring entry creation failed: Attribute service name is empty".into(),
        ))
    });
    secrets.expect_delete_secret().returning(|_| Ok(()));

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    svc.delete(&id)
        .await
        .expect("delete must succeed even when the secret store cannot be read");
}

#[tokio::test]
async fn delete_repo_failure_restores_secret() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config()).with_secret_ref("key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));
    repo.expect_delete()
        .returning(|_| Err(DbError::Internal("repo unavailable".into())));

    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .returning(|_| Ok(Some("saved-pass".into())));
    secrets.expect_delete_secret().returning(|_| Ok(()));
    secrets.expect_store_secret().returning(|key, password| {
        assert_eq!(key, "key");
        assert_eq!(password, "saved-pass");
        Ok(())
    });

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    let error = svc.delete(&id).await.expect_err("repo failure should propagate");
    assert!(matches!(error, DbError::Internal(message) if message == "repo unavailable"));
}

#[tokio::test]
async fn delete_removes_ssh_secret_with_connection() {
    let id = ConnectionId::new();
    let mut config = test_config();
    config.ssh_tunnel = Some(ssh_tunnel());
    let conn = Connection::new(config).with_secret_ref("db-key".into());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning({
        let conn = conn.clone();
        move |_| Ok(Some(conn.clone()))
    });
    repo.expect_delete().returning(|_| Ok(()));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().times(2).returning(|key| {
        if key == "db-key" {
            Ok(Some("db-password".into()))
        } else {
            assert!(key.ends_with("/ssh_password"));
            Ok(Some("ssh-password".into()))
        }
    });
    secrets.expect_delete_secret().times(2).returning(|key| {
        assert!(key == "db-key" || key.ends_with("/ssh_password"));
        Ok(())
    });

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    svc.delete(&id).await.expect("delete should remove both secrets");
}

#[tokio::test]
async fn delete_legacy_postgres_removes_default_secret_key() {
    let id = ConnectionId::new();
    let conn = Connection::new(test_config());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(conn.clone())));
    repo.expect_delete().returning(|_| Ok(()));

    let default_key = ConnectionService::secret_key(&id);
    let mut secrets = MockSecretStore::new();
    secrets
        .expect_retrieve_secret()
        .withf({
            let default_key = default_key.clone();
            move |key| key == default_key
        })
        .returning(|_| Ok(Some("legacy-password".into())));
    secrets
        .expect_delete_secret()
        .withf({
            let default_key = default_key.clone();
            move |key| key == default_key
        })
        .returning(|_| Ok(()));

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    svc.delete(&id).await.expect("legacy database secret should be removed");
}

#[tokio::test]
async fn delete_invalidates_schema_cache_after_removing_connection() {
    let id = ConnectionId::new();
    let connection = Connection::new(sqlite_config());
    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(connection.clone())));
    repo.expect_delete().returning(|_| Ok(()));

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_invalidate()
        .withf(move |received| received == &id)
        .returning(|_| Ok(()));

    let svc =
        build_service(MockDbConnector::new(), repo, MockSecretStore::new()).with_introspection_cache(Box::new(cache));
    svc.delete(&id)
        .await
        .expect("connection deletion should invalidate stale schema");
}

#[tokio::test]
async fn create_cleans_up_secret_on_repo_failure() {
    let config = test_config();

    let mut repo = MockConnectionRepository::new();
    repo.expect_save()
        .returning(|_| Err(DbError::Internal("db error".into())));

    let mut secrets = MockSecretStore::new();
    secrets.expect_store_secret().returning(|_, _| Ok(()));
    secrets.expect_delete_secret().returning(|_| Ok(()));

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    let result = svc.create(config, "pass").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_password_assigns_secret_ref_for_legacy_connection() {
    let id = ConnectionId::new();
    let legacy_connection = Connection::new(test_config());
    let saved = Arc::new(std::sync::Mutex::new(None));

    let mut repo = MockConnectionRepository::new();
    repo.expect_get()
        .returning(move |_| Ok(Some(legacy_connection.clone())));
    repo.expect_save().returning({
        let saved = Arc::clone(&saved);
        move |connection| {
            *saved.lock().expect("saved connection lock") = Some(connection.clone());
            Ok(())
        }
    });

    let key = ConnectionService::secret_key(&id);
    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning({
        let key = key.clone();
        move |received| {
            assert_eq!(received, &key);
            Ok(None)
        }
    });
    secrets.expect_store_secret().returning(move |received, value| {
        assert_eq!(received, &ConnectionService::secret_key(&id));
        assert_eq!(value, "new-password");
        Ok(())
    });

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    svc.update(&id, test_config(), Some("new-password")).await.unwrap();

    let saved = saved.lock().expect("saved connection lock");
    assert_eq!(
        saved.as_ref().and_then(|connection| connection.secret_ref.as_deref()),
        Some(key.as_str())
    );
}

#[tokio::test]
async fn update_invalidates_schema_cache_after_persisting_new_connection_config() {
    let id = ConnectionId::new();
    let previous = Connection::new(test_config());
    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(previous.clone())));
    repo.expect_save().returning(|_| Ok(()));

    let mut cache = MockIntrospectionCache::new();
    cache
        .expect_invalidate()
        .withf(move |received| received == &id)
        .returning(|_| Ok(()));

    let svc =
        build_service(MockDbConnector::new(), repo, MockSecretStore::new()).with_introspection_cache(Box::new(cache));
    let mut config = test_config();
    config.database = "new-database".into();
    svc.update(&id, config, None)
        .await
        .expect("connection update should invalidate stale schema");
}

#[tokio::test]
async fn update_repo_failure_restores_missing_secret() {
    let id = ConnectionId::new();
    let connection = Connection::new(test_config());

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(connection.clone())));
    repo.expect_save()
        .returning(|_| Err(DbError::Internal("save failed".into())));

    let mut secrets = MockSecretStore::new();
    secrets.expect_retrieve_secret().returning(|_| Ok(None));
    secrets.expect_store_secret().returning(|_, _| Ok(()));
    secrets.expect_delete_secret().returning(move |key| {
        assert_eq!(key, &ConnectionService::secret_key(&id));
        Ok(())
    });

    let svc = build_service(MockDbConnector::new(), repo, secrets);
    assert!(svc.update(&id, test_config(), Some("new-password")).await.is_err());
}

#[tokio::test]
async fn update_repo_failure_keeps_active_connection() {
    let id = ConnectionId::new();
    let connection = Connection::new(test_config());
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, ConnectionHandle(1));

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(connection.clone())));
    repo.expect_save()
        .returning(|_| Err(DbError::Internal("save failed".into())));

    let svc = ConnectionService::new(
        Box::new(MockDbConnector::new()),
        Box::new(repo),
        Box::new(MockSecretStore::new()),
        Arc::clone(&registry),
    );

    assert!(svc.update(&id, test_config(), None).await.is_err());
    assert_eq!(registry.get(&id), Some(ConnectionHandle(1)));
}

#[tokio::test]
async fn update_disconnect_failure_rolls_back_persisted_connection() {
    let id = ConnectionId::new();
    let connection = Connection::new(test_config());
    let registry = Arc::new(ConnectionRegistry::new());
    registry.register(id, ConnectionHandle(1));
    let save_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut repo = MockConnectionRepository::new();
    repo.expect_get().returning(move |_| Ok(Some(connection.clone())));
    repo.expect_save().times(2).returning({
        let save_count = Arc::clone(&save_count);
        move |saved| {
            let call = save_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if call == 0 {
                assert_eq!(saved.config.name, "updated");
            } else {
                assert_eq!(saved.config.name, "test");
            }
            Ok(())
        }
    });

    let mut connector = MockDbConnector::new();
    connector
        .expect_disconnect()
        .returning(|_| Err(DbError::ConnectionLost("close failed".into())));

    let svc = ConnectionService::new(
        Box::new(connector),
        Box::new(repo),
        Box::new(MockSecretStore::new()),
        Arc::clone(&registry),
    );

    let mut updated_config = test_config();
    updated_config.name = "updated".into();
    let error = svc
        .update(&id, updated_config, None)
        .await
        .expect_err("disconnect must fail");
    assert!(matches!(error, DbError::ConnectionLost(_)));
    assert_eq!(save_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(registry.get(&id), Some(ConnectionHandle(1)));
}
