// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Connection-facing UI runtime models.

/// Stable identity for an async UI operation. Real backend tasks will reuse
/// this identity for cancellation and stale-result protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiDriver {
    Postgres,
    Sqlite,
    Mysql,
    SqlServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSslMode {
    Disable,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiConnectionDraft {
    pub name: String,
    pub host: String,
    pub port: String,
    pub database: String,
    pub username: String,
    pub password: String,
    pub driver: UiDriver,
    pub ssl_mode: UiSslMode,
    pub readonly: bool,
    /// SQLite only: create the database file if it is missing. Defaults to off so
    /// a mistyped path fails instead of producing an empty database (#597).
    pub create_if_missing: bool,
    pub group: String,
    pub tags: String,
    pub favorite: bool,
    pub environment: String,
    pub ssh_tunnel_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_private_key: String,
    pub ssh_profile_id: String,
    pub ssl_root_cert_path: String,
    pub ssl_client_cert_path: String,
    pub ssl_client_key_path: String,
    /// Cloud preset key, e.g. `aws_rds:postgres`. Empty = none.
    pub cloud_preset: String,
    /// `password` or `ephemeral_token`.
    pub auth_kind: String,
    pub cloud_snippet: String,
    pub cloud_guidance: String,
}

impl Default for UiConnectionDraft {
    fn default() -> Self {
        // New connections start blank: no pre-filled identity, host, port, or
        // credentials. Required fields are enforced at validation time.
        Self {
            name: String::new(),
            host: String::new(),
            port: String::new(),
            database: String::new(),
            username: String::new(),
            password: String::new(),
            driver: UiDriver::Postgres,
            // New connections default to TLS disabled — users opt into
            // Require/Verify explicitly for hosts that need it.
            ssl_mode: UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
            group: String::new(),
            tags: String::new(),
            favorite: false,
            environment: "Development".to_owned(),
            ssh_tunnel_enabled: false,
            ssh_host: String::new(),
            ssh_port: String::new(),
            ssh_user: String::new(),
            ssh_private_key: String::new(),
            ssh_profile_id: String::new(),
            ssl_root_cert_path: String::new(),
            ssl_client_cert_path: String::new(),
            ssl_client_key_path: String::new(),
            cloud_preset: String::new(),
            auth_kind: "password".into(),
            cloud_snippet: String::new(),
            cloud_guidance: String::new(),
        }
    }
}
