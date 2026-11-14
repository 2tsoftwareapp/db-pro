//! Tests for `../connection.rs` (moved out of the source file).
use super::*;

fn valid_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "test".into(),
        host: "localhost".into(),
        port: 5432,
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
    }
}

#[test]
fn connection_id_parse_valid_uuid() {
    let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
    let id = ConnectionId::parse(uuid_str).unwrap();
    assert_eq!(id.to_string(), uuid_str);
}

#[test]
fn connection_id_parse_invalid_uuid() {
    let result = ConnectionId::parse("not-a-uuid");
    assert!(result.is_err());
}

#[test]
fn connection_config_validate_valid() {
    let config = valid_config();
    assert!(config.validate().is_ok());
}

#[test]
fn connection_config_validate_empty_name() {
    let mut config = valid_config();
    config.name = "".into();
    let errors = config.validate().unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field, "name");
}

#[test]
fn connection_config_validate_multiple_errors() {
    let config = ConnectionConfig {
        name: "".into(),
        host: "".into(),
        port: 0,
        database: "".into(),
        username: "".into(),
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
};
    let errors = config.validate().unwrap_err();
    assert_eq!(errors.len(), 5);
}

#[test]
fn sqlite_does_not_require_host_port_username() {
    let config = ConnectionConfig {
        name: "local-db".into(),
        host: "".into(),
        port: 0,
        database: "/tmp/test.db".into(),
        username: "".into(),
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
};
    assert!(config.validate().is_ok());
}

#[test]
fn sqlite_rejects_ssh_tunnel_configuration() {
    let mut config = ConnectionConfig {
        name: "local-db".into(),
        host: "".into(),
        port: 0,
        database: "/tmp/test.db".into(),
        username: "".into(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: Some(SshTunnelConfig {
            host: "bastion.example".into(),
            port: 22,
            user: "deploy".into(),
            private_key_path: "/tmp/key".into(),
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
};

    let errors = config.validate().unwrap_err();
    assert!(errors.iter().any(|error| error.field == "ssh_tunnel"));
    config.ssh_tunnel = None;
    assert!(config.validate().is_ok());
}

#[test]
fn validate_zero_query_timeout() {
    let mut config = valid_config();
    config.query_timeout_ms = 0;
    let errors = config.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.field == "query_timeout_ms"));
}

#[test]
fn validate_max_rows_zero() {
    let mut config = valid_config();
    config.max_rows = 0;
    let errors = config.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.field == "max_rows"));
}

#[test]
fn validate_max_rows_exceedses_limit() {
    let mut config = valid_config();
    config.max_rows = MAX_MAX_ROWS + 1;
    let errors = config.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.field == "max_rows"));
}

#[test]
fn validate_name_too_long() {
    let mut config = valid_config();
    config.name = "x".repeat(MAX_CONNECTION_NAME_LEN + 1);
    let errors = config.validate().unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field, "name");
}

#[test]
fn validate_name_uses_character_count_for_unicode() {
    let mut config = valid_config();
    config.name = "é".repeat(MAX_CONNECTION_NAME_LEN);
    assert!(config.validate().is_ok());

    config.name.push('é');
    let errors = config.validate().unwrap_err();
    assert!(errors.iter().any(|error| error.field == "name"));
}

#[test]
fn validate_ssh_tunnel_empty_fields() {
    let mut config = valid_config();
    config.ssh_tunnel = Some(SshTunnelConfig {
        host: "".into(),
        port: 0,
        user: "".into(),
        private_key_path: "".into(),
        password: None,
    });
    let errors = config.validate().unwrap_err();
    let ssh_fields: Vec<&str> = errors
        .iter()
        .filter(|e| e.field.starts_with("ssh_tunnel."))
        .map(|e| e.field.as_str())
        .collect();
    assert_eq!(ssh_fields.len(), 4);
}

#[test]
fn validate_ssh_tunnel_rejects_option_like_host_and_user() {
    let mut config = valid_config();
    config.ssh_tunnel = Some(SshTunnelConfig {
        host: "-oProxyCommand=touch /tmp/pwned".into(),
        port: 22,
        user: "-bacon".into(),
        private_key_path: "/tmp/key".into(),
        password: None,
    });
    let errors = config.validate().unwrap_err();
    let messages: Vec<&str> = errors
        .iter()
        .filter(|e| e.field.starts_with("ssh_tunnel."))
        .map(|e| e.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "SSH tunnel host must not start with '-'",
            "SSH tunnel user must not start with '-'"
        ]
    );
}

#[test]
fn validate_ssh_tunnel_accepts_a_normal_host_and_user() {
    let mut config = valid_config();
    config.ssh_tunnel = Some(SshTunnelConfig {
        host: "bastion.example".into(),
        port: 22,
        user: "deploy".into(),
        private_key_path: "/tmp/key".into(),
        password: None,
    });
    assert!(config.validate().is_ok());
}

#[test]
fn ssh_password_is_not_serialized_or_debugged() {
    let config = SshTunnelConfig {
        host: "bastion.example".into(),
        port: 22,
        user: "deploy".into(),
        private_key_path: "/tmp/key".into(),
        password: Some("super-secret".into()),
    };

    let json = serde_json::to_string(&config).expect("SSH config should serialize");
    let debug = format!("{config:?}");
    assert!(!json.contains("super-secret"));
    assert!(!json.contains("password"));
    assert!(!debug.contains("super-secret"));
    assert!(debug.contains("has_password"));

    let decoded: SshTunnelConfig = serde_json::from_str(
        r#"{"host":"bastion.example","port":22,"user":"deploy","private_key_path":"/tmp/key","password":"legacy"}"#,
    )
    .expect("legacy SSH metadata should remain readable");
    assert_eq!(decoded.password.as_deref(), Some("legacy"));
}

#[test]
fn without_secrets_clears_ssh_password_for_profile_export() {
    let mut config = valid_config();
    config.ssh_tunnel = Some(SshTunnelConfig {
        host: "bastion".into(),
        port: 22,
        user: "ubuntu".into(),
        private_key_path: "~/.ssh/id_rsa".into(),
        password: Some("s3cret".into()),
    });
    let exported = ConnectionProfileExport::from_configs([config]);
    assert_eq!(exported.version, 1);
    assert!(exported.connections[0].ssh_tunnel.as_ref().unwrap().password.is_none());
}
