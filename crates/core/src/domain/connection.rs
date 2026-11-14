use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverType {
    Postgres,
    SQLite,
    Mysql,
    #[serde(rename = "sqlserver")]
    SqlServer,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    #[default]
    Disable,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub private_key_path: String,
    /// SSH password is accepted for an in-memory connection draft only. It is
    /// stored in the SecretStore by ConnectionService and never in metadata.
    #[serde(skip_serializing, default)]
    pub password: Option<String>,
}

impl fmt::Debug for SshTunnelConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshTunnelConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("private_key_path", &self.private_key_path)
            .field("has_password", &self.password.is_some())
            .finish()
    }
}

/// Reusable SSH tunnel profile referenced by connections (#223).
/// Secrets (password) live in SecretStore under `ssh-profile:{id}:password`.
#[derive(Clone, Serialize, Deserialize)]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub private_key_path: String,
    #[serde(skip_serializing, default)]
    pub password: Option<String>,
}

impl fmt::Debug for SshProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshProfile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("private_key_path", &self.private_key_path)
            .field("has_password", &self.password.is_some())
            .finish()
    }
}

impl SshProfile {
    pub fn to_tunnel_config(&self) -> SshTunnelConfig {
        SshTunnelConfig {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            private_key_path: self.private_key_path.clone(),
            password: self.password.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub driver: DriverType,
    #[serde(default)]
    pub ssl_mode: SslMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_tunnel: Option<SshTunnelConfig>,
    /// Reference to a reusable [`SshProfile`] id (preferred over inline tunnel copy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_profile_id: Option<String>,
    /// Path to CA / root certificate (VerifyCa / VerifyFull).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_root_cert_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_client_cert_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_client_key_path: Option<String>,
    #[serde(default = "default_query_timeout")]
    pub query_timeout_ms: u64,
    #[serde(default = "default_max_rows")]
    pub max_rows: u64,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// Pinned/favorite connection for quick access.
    #[serde(default)]
    pub favorite: bool,
    /// Workspace environment label for safety UX.
    #[serde(default)]
    pub environment: ConnectionEnvironment,
    /// If true, only read-only queries (SELECT, SHOW, EXPLAIN) are allowed,
    /// enforced both by the statement classifier and by the server
    /// (`default_transaction_read_only` for PostgreSQL, `PRAGMA query_only` for
    /// SQLite) — a `SELECT` can call a function that writes (#588).
    #[serde(default)]
    pub readonly: bool,
    /// SQLite only: allow `connect` to create the database file when it does not
    /// exist. Off by default, so a mistyped path fails instead of silently
    /// producing an empty database (#597).
    #[serde(default)]
    pub create_if_missing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionEnvironment {
    #[default]
    Development,
    Staging,
    Production,
    Custom,
}

impl ConnectionEnvironment {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Development => "Development",
            Self::Staging => "Staging",
            Self::Production => "Production",
            Self::Custom => "Custom",
        }
    }
}

/// Non-secret connection profile for import/export (#204).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfileExport {
    pub version: u32,
    pub connections: Vec<ConnectionConfig>,
}

impl ConnectionConfig {
    /// Clone suitable for profile export: SSH passwords cleared, no secret material.
    pub fn without_secrets(&self) -> Self {
        let mut cfg = self.clone();
        if let Some(tunnel) = cfg.ssh_tunnel.as_mut() {
            tunnel.password = None;
        }
        cfg
    }

    pub fn is_production(&self) -> bool {
        matches!(self.environment, ConnectionEnvironment::Production)
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: 5432,
            database: String::new(),
            username: String::new(),
            driver: DriverType::Postgres,
            ssl_mode: SslMode::Disable,
            ssh_tunnel: None,
            ssh_profile_id: None,
            ssl_root_cert_path: None,
            ssl_client_cert_path: None,
            ssl_client_key_path: None,
            query_timeout_ms: default_query_timeout(),
            max_rows: default_max_rows(),
            color: None,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: ConnectionEnvironment::Development,
            readonly: false,
            create_if_missing: false,
        }
    }
}

impl ConnectionProfileExport {
    pub fn from_configs(configs: impl IntoIterator<Item = ConnectionConfig>) -> Self {
        Self {
            version: 1,
            connections: configs.into_iter().map(|c| c.without_secrets()).collect(),
        }
    }
}

fn default_query_timeout() -> u64 {
    30_000
}

fn default_max_rows() -> u64 {
    500
}

const MAX_CONNECTION_NAME_LEN: usize = 128;
const MAX_MAX_ROWS: u64 = 100_000;

/// True when a value would be parsed as a CLI option (`-oProxyCommand=…`) if it
/// reached `ssh` as the positional `user@host` argument.
fn is_option_like(value: &str) -> bool {
    value.trim_start().starts_with('-')
}

impl ConnectionConfig {
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        self.validate_name(&mut errors);
        self.validate_network_fields(&mut errors);
        self.validate_tunnel_support(&mut errors);
        self.validate_field_bounds(&mut errors);
        self.validate_ssh_tunnel(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_name(&self, errors: &mut Vec<ValidationError>) {
        if self.name.trim().is_empty() {
            errors.push(ValidationError {
                field: "name".into(),
                message: "Connection name is required".into(),
            });
        } else if self.name.chars().count() > MAX_CONNECTION_NAME_LEN {
            errors.push(ValidationError {
                field: "name".into(),
                message: format!("Connection name must be at most {MAX_CONNECTION_NAME_LEN} characters"),
            });
        }
    }

    /// Host/port/credentials apply to network drivers; SQLite opens a file.
    fn validate_network_fields(&self, errors: &mut Vec<ValidationError>) {
        if matches!(self.driver, DriverType::SQLite) {
            return;
        }
        if self.host.trim().is_empty() {
            errors.push(ValidationError {
                field: "host".into(),
                message: "Host is required".into(),
            });
        }
        if self.port == 0 {
            errors.push(ValidationError {
                field: "port".into(),
                message: "Port must be between 1 and 65535".into(),
            });
        }
        if self.username.trim().is_empty() {
            errors.push(ValidationError {
                field: "username".into(),
                message: "Username is required".into(),
            });
        }
    }

    /// SSH tunnels are a PostgreSQL-only capability today.
    fn validate_tunnel_support(&self, errors: &mut Vec<ValidationError>) {
        if self.ssh_tunnel.is_none() {
            return;
        }
        let message = match self.driver {
            DriverType::SQLite => Some("SSH tunnels are supported only for PostgreSQL"),
            DriverType::SqlServer => Some("SQL Server SSH tunneling is not implemented by the provider yet"),
            DriverType::Postgres | DriverType::Mysql => None,
        };
        if let Some(message) = message {
            errors.push(ValidationError {
                field: "ssh_tunnel".into(),
                message: message.into(),
            });
        }
    }

    fn validate_field_bounds(&self, errors: &mut Vec<ValidationError>) {
        if self.database.trim().is_empty() {
            errors.push(ValidationError {
                field: "database".into(),
                message: "Database is required".into(),
            });
        }
        if self.query_timeout_ms == 0 {
            errors.push(ValidationError {
                field: "query_timeout_ms".into(),
                message: "Query timeout must be greater than 0".into(),
            });
        }
        if self.max_rows == 0 || self.max_rows > MAX_MAX_ROWS {
            errors.push(ValidationError {
                field: "max_rows".into(),
                message: format!("max_rows must be between 1 and {MAX_MAX_ROWS}"),
            });
        }
    }

    fn validate_ssh_tunnel(&self, errors: &mut Vec<ValidationError>) {
        let Some(ssh) = self.ssh_tunnel.as_ref() else {
            return;
        };
        if ssh.host.trim().is_empty() {
            errors.push(ValidationError {
                field: "ssh_tunnel.host".into(),
                message: "SSH tunnel host is required".into(),
            });
        } else if is_option_like(&ssh.host) {
            errors.push(ValidationError {
                field: "ssh_tunnel.host".into(),
                message: "SSH tunnel host must not start with '-'".into(),
            });
        }
        if ssh.port == 0 {
            errors.push(ValidationError {
                field: "ssh_tunnel.port".into(),
                message: "SSH tunnel port must be between 1 and 65535".into(),
            });
        }
        if ssh.user.trim().is_empty() {
            errors.push(ValidationError {
                field: "ssh_tunnel.user".into(),
                message: "SSH tunnel user is required".into(),
            });
        } else if is_option_like(&ssh.user) {
            errors.push(ValidationError {
                field: "ssh_tunnel.user".into(),
                message: "SSH tunnel user must not start with '-'".into(),
            });
        }
        if ssh.private_key_path.trim().is_empty() {
            errors.push(ValidationError {
                field: "ssh_tunnel.private_key_path".into(),
                message: "SSH tunnel private key path is required".into(),
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: ConnectionId,
    pub config: ConnectionConfig,
    pub secret_ref: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Connection {
    pub fn new(config: ConnectionConfig) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: ConnectionId::new(),
            config,
            secret_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_secret_ref(mut self, secret_ref: String) -> Self {
        self.secret_ref = Some(secret_ref);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionHandle(pub u64);

impl ConnectionHandle {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

#[cfg(test)]
#[path = "tests/connection_tests.rs"]
mod tests;
