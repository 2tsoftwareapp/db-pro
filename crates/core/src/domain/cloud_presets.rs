//! Cloud managed-database connection presets (#254).
//!
//! Guidance + draft defaults only — no cloud control plane, no provisioning.

use crate::domain::connection::{ConnectionConfig, DriverType, SslMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    AwsRds,
    AwsAurora,
    GcpCloudSql,
    AzureDatabase,
}

impl CloudProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::AwsRds => "AWS RDS",
            Self::AwsAurora => "AWS Aurora",
            Self::GcpCloudSql => "Google Cloud SQL",
            Self::AzureDatabase => "Azure Database",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionAuthKind {
    #[default]
    Password,
    /// Short-lived IAM / access token — must not be persisted as a long-lived password.
    EphemeralToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudConnectionPreset {
    pub provider: CloudProvider,
    pub driver: DriverType,
    pub default_port: u16,
    pub ssl_mode: SslMode,
    pub ca_guidance: &'static str,
    pub host_hint: &'static str,
    pub notes: &'static str,
    /// IAM / token auth is only offered when we can document the hook without inventing APIs.
    pub token_auth_supported: bool,
}

// cc-scan:allow HUGE_FUNCTION — static preset catalog; the vec! literal IS the data table
pub fn presets() -> Vec<CloudConnectionPreset> {
    vec![
        CloudConnectionPreset {
            provider: CloudProvider::AwsRds,
            driver: DriverType::Postgres,
            default_port: 5432,
            ssl_mode: SslMode::VerifyFull,
            ca_guidance: "Download the AWS RDS regional CA bundle (rds-ca-*.pem) and set SSL root CA path.",
            host_hint: "mydb.xxxxx.region.rds.amazonaws.com",
            notes: "Prefer Verify Full + RDS CA. Use SSH bastion via SSH profiles when the instance is private.",
            token_auth_supported: true,
        },
        CloudConnectionPreset {
            provider: CloudProvider::AwsRds,
            driver: DriverType::Mysql,
            default_port: 3306,
            ssl_mode: SslMode::VerifyFull,
            ca_guidance: "Download the AWS RDS regional CA bundle and set SSL root CA path.",
            host_hint: "mydb.xxxxx.region.rds.amazonaws.com",
            notes: "MySQL on RDS defaults to TLS. Token/IAM auth is session-only when selected.",
            token_auth_supported: true,
        },
        CloudConnectionPreset {
            provider: CloudProvider::AwsAurora,
            driver: DriverType::Postgres,
            default_port: 5432,
            ssl_mode: SslMode::VerifyFull,
            ca_guidance: "Use the same AWS RDS CA bundle as RDS; point SSL root CA to that PEM.",
            host_hint: "cluster.cluster-xxxxx.region.rds.amazonaws.com",
            notes: "Connect to the writer or reader endpoint. Do not store IAM auth tokens as passwords.",
            token_auth_supported: true,
        },
        CloudConnectionPreset {
            provider: CloudProvider::GcpCloudSql,
            driver: DriverType::Postgres,
            default_port: 5432,
            ssl_mode: SslMode::VerifyCa,
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            ca_guidance: "Use Cloud SQL server CA (and optional client cert/key) from the instance Connections → Security tab.",
            host_hint: "x.x.x.x or private IP / Cloud SQL Auth Proxy localhost",
            notes: "Public IP needs authorized networks. Prefer Auth Proxy or SSH tunnel for private IP.",
            token_auth_supported: false,
        },
        CloudConnectionPreset {
            provider: CloudProvider::GcpCloudSql,
            driver: DriverType::Mysql,
            default_port: 3306,
            ssl_mode: SslMode::VerifyCa,
            ca_guidance: "Download Cloud SQL server CA; optional client certificate for require_ssl.",
            host_hint: "x.x.x.x or Cloud SQL Auth Proxy localhost",
            notes: "Cloud SQL Auth Proxy typically uses localhost + Require; public IP uses VerifyCa + CA.",
            token_auth_supported: false,
        },
        CloudConnectionPreset {
            provider: CloudProvider::AzureDatabase,
            driver: DriverType::Postgres,
            default_port: 5432,
            ssl_mode: SslMode::VerifyFull,
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            ca_guidance: "Azure Database requires TLS; DigiCert Global Root G2 / Microsoft RSA is commonly trusted by OS stores — set CA path if your OS trust is incomplete.",
            host_hint: "myserver.postgres.database.azure.com",
            notes: "Username is often user@server. Enforce SSL; use SSH profile for private access.",
            token_auth_supported: false,
        },
        CloudConnectionPreset {
            provider: CloudProvider::AzureDatabase,
            driver: DriverType::Mysql,
            default_port: 3306,
            ssl_mode: SslMode::VerifyFull,
            ca_guidance: "Azure Database for MySQL requires TLS; provide DigiCert/Microsoft root CA when needed.",
            host_hint: "myserver.mysql.database.azure.com",
            notes: "SSL is required on Azure Flexible Server. Prefer bastion/SSH profile for VNet-only hosts.",
            token_auth_supported: false,
        },
    ]
}

pub fn find_preset(provider: CloudProvider, driver: DriverType) -> Option<CloudConnectionPreset> {
    presets()
        .into_iter()
        .find(|p| p.provider == provider && p.driver == driver)
}

/// Apply preset defaults onto a connection config (non-destructive for filled host/db/user).
pub fn apply_preset(config: &mut ConnectionConfig, preset: &CloudConnectionPreset) {
    config.driver = preset.driver;
    config.port = preset.default_port;
    config.ssl_mode = preset.ssl_mode;
    if config.host.trim().is_empty() {
        config.host = preset.host_hint.to_owned();
    }
    if config.name.trim().is_empty() {
        config.name = format!("{} {}", preset.provider.label(), driver_label(preset.driver));
    }
    let tag = preset.provider.label().to_owned();
    if !config.tags.iter().any(|t| t == &tag) {
        config.tags.push(tag);
    }
}

fn driver_label(driver: DriverType) -> &'static str {
    match driver {
        DriverType::Postgres => "PostgreSQL",
        DriverType::Mysql => "MySQL",
        DriverType::SQLite => "SQLite",
        DriverType::SqlServer => "SQL Server",
    }
}

/// Validate common managed-DB endpoint shapes (soft checks — warnings, not hard blocks).
pub fn validate_cloud_endpoint(host: &str, provider: CloudProvider) -> Result<(), String> {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.starts_with("127.") {
        return Ok(()); // local proxy / tunnel OK
    }
    let ok = match provider {
        CloudProvider::AwsRds | CloudProvider::AwsAurora => {
            host.contains(".rds.amazonaws.com") || host.contains(".rds.")
        }
        CloudProvider::GcpCloudSql => {
            // Public IP or proxy — accept any non-empty; warn only if clearly AWS/Azure.
            !host.contains(".rds.amazonaws.com") && !host.contains(".database.azure.com")
        }
        CloudProvider::AzureDatabase => host.contains(".database.azure.com") || host.contains(".azure.com"),
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "host `{}` does not look like a typical {} endpoint (you can still connect)",
            host,
            provider.label()
        ))
    }
}

/// Parse a provider console URI / JDBC-ish snippet into draft fields.
/// Passwords in the URI are returned separately and must be treated as ephemeral when `auth_kind` is token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedConnectionSnippet {
    pub driver: DriverType,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl_mode: Option<SslMode>,
}

// cc-scan:allow COMPLEXITY,LONG_FUNCTION — classifier/dispatch ladder — one case per branch
pub fn parse_connection_snippet(raw: &str) -> Result<ParsedConnectionSnippet, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty connection snippet".into());
    }
    // Strip jdbc: prefix if present.
    let s = trimmed.strip_prefix("jdbc:").unwrap_or(trimmed);
    let (scheme, rest) = s
        .split_once("://")
        .ok_or_else(|| "expected scheme://host form (postgres://, mysql://, sqlserver://, …)".to_owned())?;
    let driver = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => DriverType::Postgres,
        "mysql" | "mysql2" => DriverType::Mysql,
        "sqlserver" | "mssql" => DriverType::SqlServer,
        other => return Err(format!("unsupported scheme `{other}`")),
    };
    let (auth_host, path_query) = rest.split_once('/').unwrap_or((rest, ""));
    let (userinfo, hostport) = if let Some((u, h)) = auth_host.rsplit_once('@') {
        (Some(u), h)
    } else {
        (None, auth_host)
    };
    // #598: decode every credential component, not just the password. Azure
    // PostgreSQL users arrive as `user%40server` and databases as `my%20db`.
    let (username, password) = match userinfo {
        Some(u) => {
            if let Some((user, pass)) = u.split_once(':') {
                (percent_decode(user), Some(percent_decode(pass)))
            } else {
                (percent_decode(u), None)
            }
        }
        None => (String::new(), None),
    };
    let default_port = match driver {
        DriverType::Mysql => 3306,
        DriverType::SqlServer => 1433,
        _ => 5432,
    };
    let (host, port) = split_host_port(hostport, default_port)?;
    let (path, query) = path_query.split_once('?').unwrap_or((path_query, ""));
    let database = percent_decode(path.trim_matches('/'));
    let mut ssl_mode = None;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("").to_ascii_lowercase();
        let val = kv.next().unwrap_or("").to_ascii_lowercase();
        if key == "sslmode" || key == "ssl-mode" {
            ssl_mode = Some(match val.as_str() {
                "disable" => SslMode::Disable,
                "require" | "required" => SslMode::Require,
                "verify-ca" => SslMode::VerifyCa,
                "verify-full" => SslMode::VerifyFull,
                _ => SslMode::Require,
            });
        }
    }
    Ok(ParsedConnectionSnippet {
        driver,
        host,
        port,
        database,
        username,
        password,
        ssl_mode,
    })
}

/// Split `host[:port]`, including bracketed IPv6 without a port (#598).
fn split_host_port(hostport: &str, default_port: u16) -> Result<(String, u16), String> {
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| format!("unterminated IPv6 host `{hostport}`"))?;
        let port = match tail.strip_prefix(':') {
            Some(port) => port.parse().map_err(|_| format!("invalid port `{port}`"))?,
            None => default_port,
        };
        return Ok((host.to_owned(), port));
    }
    // An unbracketed host with more than one colon is a bare IPv6 address, not
    // `host:port` — `rsplit_once(':')` used to slice the address itself.
    let colons = hostport.matches(':').count();
    if colons > 1 {
        return Ok((hostport.to_owned(), default_port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) => Ok((
            host.to_owned(),
            port.parse().map_err(|_| format!("invalid port `{port}`"))?,
        )),
        None => Ok((hostport.to_owned(), default_port)),
    }
}

/// Percent-decode to bytes, then UTF-8 (#598).
///
/// Pushing each byte through `as char` re-encoded every multi-byte UTF-8
/// sequence as Latin-1 mojibake, so a Vietnamese password never matched.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Redacted export helper for cloud drafts (never include password/token).
pub fn redacted_export_summary(config: &ConnectionConfig, auth: ConnectionAuthKind) -> String {
    format!(
        "{} · {}@{}:{}/{} · ssl={:?} · auth={:?} · tags={}",
        config.name,
        config.username,
        config.host,
        config.port,
        config.database,
        config.ssl_mode,
        auth,
        config.tags.join(",")
    )
}

#[cfg(test)]
#[path = "tests/cloud_presets_tests.rs"]
mod tests;
