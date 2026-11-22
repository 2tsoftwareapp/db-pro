//! Staged connection networking diagnostics (#223).
//!
//! Reports DNS → TCP → SSH → TLS → Auth without embedding secrets.

use crate::domain::connection::{ConnectionConfig, DriverType, SslMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStage {
    Dns,
    Tcp,
    Ssh,
    Tls,
    Auth,
}

impl ConnectionStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dns => "DNS",
            Self::Tcp => "TCP",
            Self::Ssh => "SSH",
            Self::Tls => "TLS",
            Self::Auth => "DB AUTH",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: ConnectionStage,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionDiagnosticsReport {
    pub stages: Vec<StageResult>,
    pub failed_stage: Option<ConnectionStage>,
}

impl ConnectionDiagnosticsReport {
    pub fn summary(&self) -> String {
        if let Some(stage) = self.failed_stage {
            format!(
                "Connection failed at {} — {}",
                stage.label(),
                self.stages
                    .iter()
                    .find(|s| s.stage == stage)
                    .map(|s| s.message.as_str())
                    .unwrap_or("unknown")
            )
        } else if self.stages.is_empty() {
            "Transport preflight deferred to the full connection test".to_owned()
        } else {
            "All connection stages succeeded".to_owned()
        }
    }
}

/// Redact common secret patterns from diagnostic text.
pub fn redact_diag_message(raw: &str) -> String {
    // Redact a credential *value*, never the prose around it. The previous
    // needle `"PASSWORD "` matched `password authentication failed for user …`
    // — the single most common PostgreSQL error — and replaced the whole reason
    // with `[redacted]` (#609).
    let lower = raw.to_ascii_lowercase();
    let mut cut = None;
    for needle in ["password=", "pwd=", "private_key", "begin rsa", "begin openssh"] {
        if let Some(idx) = lower.find(needle) {
            cut = Some(idx);
            break;
        }
    }
    if cut.is_none() {
        // `password = value`, with spaces around the operator.
        if let Some(idx) = lower.find("password") {
            let after = lower[idx + "password".len()..].trim_start();
            if after.starts_with('=') {
                cut = Some(idx);
            }
        }
    }
    if cut.is_none() {
        // SQL literal form: `CREATE ROLE app PASSWORD 'x'`.
        cut = sql_password_literal_start(&lower);
    }
    match cut {
        Some(idx) => format!("{}[redacted]", &raw[..idx]),
        None => raw.to_owned(),
    }
}

/// Byte offset of a `PASSWORD` keyword followed by a `'…'` literal, if any.
fn sql_password_literal_start(lower: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = lower[from..].find("password") {
        let idx = from + rel;
        let after = lower[idx + "password".len()..].trim_start();
        if after.starts_with('\'') {
            return Some(idx);
        }
        from = idx + "password".len();
    }
    None
}

/// Run pre-auth network stages locally. Auth is reported from the caller after test_connection.
pub fn probe_network_stages(config: &ConnectionConfig) -> ConnectionDiagnosticsReport {
    // SQLite is local-file — skip network stages.
    if config.driver == DriverType::SQLite {
        return ConnectionDiagnosticsReport {
            stages: vec![StageResult {
                stage: ConnectionStage::Dns,
                ok: true,
                message: "SQLite uses a local file path — DNS/TCP/SSH/TLS not applicable".into(),
            }],
            failed_stage: None,
        };
    }
    // The database address may only resolve or be reachable from the bastion.
    // Direct probes bypass the requested route, and cannot establish SSH/TLS
    // success. Defer to the actual full-chain connection test instead.
    if config.ssh_tunnel.is_some() || config.ssh_profile_id.is_some() {
        return ConnectionDiagnosticsReport {
            stages: Vec::new(),
            failed_stage: None,
        };
    }

    let host = config.host.trim();
    if host.is_empty() {
        return ConnectionDiagnosticsReport {
            stages: vec![StageResult {
                stage: ConnectionStage::Dns,
                ok: false,
                message: "Host is empty".into(),
            }],
            failed_stage: Some(ConnectionStage::Dns),
        };
    }

    let mut stages = Vec::new();
    if let Err(report) = probe_dns(host, config.port, &mut stages) {
        return report;
    }
    if let Err(report) = probe_tcp(host, config.port, &mut stages) {
        return report;
    }
    stages.push(ssh_stage());
    stages.push(tls_stage(config));
    ConnectionDiagnosticsReport {
        stages,
        failed_stage: None,
    }
}

/// Stages that end the probe early return `Err(report)` carrying the finished
/// failure report; `Ok` leaves a successful stage pushed on `stages`.
fn probe_dns(host: &str, port: u16, stages: &mut Vec<StageResult>) -> Result<(), ConnectionDiagnosticsReport> {
    let addr = format!("{host}:{port}");
    match std::net::ToSocketAddrs::to_socket_addrs(&addr) {
        Ok(mut iter) => match iter.next() {
            Some(resolved) => {
                stages.push(StageResult {
                    stage: ConnectionStage::Dns,
                    ok: true,
                    message: format!("Resolved {host} → {resolved}"),
                });
                Ok(())
            }
            None => Err(fail_at(
                stages,
                ConnectionStage::Dns,
                format!("No addresses resolved for {host}"),
            )),
        },
        Err(err) => Err(fail_at(
            stages,
            ConnectionStage::Dns,
            redact_diag_message(&format!("DNS failed: {err}")),
        )),
    }
}

fn probe_tcp(host: &str, port: u16, stages: &mut Vec<StageResult>) -> Result<(), ConnectionDiagnosticsReport> {
    let tcp_target = format!("{host}:{port}");
    match std::net::TcpStream::connect_timeout(
        &std::net::ToSocketAddrs::to_socket_addrs(&tcp_target)
            .ok()
            .and_then(|mut i| i.next())
            .unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], port))),
        std::time::Duration::from_secs(3),
    ) {
        Ok(_) => {
            stages.push(StageResult {
                stage: ConnectionStage::Tcp,
                ok: true,
                message: format!("TCP connect to {tcp_target} succeeded"),
            });
            Ok(())
        }
        Err(err) => Err(fail_at(
            stages,
            ConnectionStage::Tcp,
            redact_diag_message(&format!("TCP failed: {err}")),
        )),
    }
}

fn fail_at(stages: &mut Vec<StageResult>, stage: ConnectionStage, message: String) -> ConnectionDiagnosticsReport {
    stages.push(StageResult {
        stage,
        ok: false,
        message,
    });
    ConnectionDiagnosticsReport {
        stages: std::mem::take(stages),
        failed_stage: Some(stage),
    }
}

fn ssh_stage() -> StageResult {
    StageResult {
        stage: ConnectionStage::Ssh,
        ok: true,
        message: "SSH not configured — skipped".into(),
    }
}

fn tls_stage(config: &ConnectionConfig) -> StageResult {
    let root_ca = || {
        config
            .ssl_root_cert_path
            .as_deref()
            .filter(|p| !p.is_empty())
            .unwrap_or("(system CAs)")
    };
    let message = match config.ssl_mode {
        SslMode::Disable => "TLS disabled".to_owned(),
        SslMode::Require => "TLS required (encrypted, cert not verified)".to_owned(),
        SslMode::VerifyCa => format!("TLS VerifyCa using root cert path `{}`", root_ca()),
        SslMode::VerifyFull => format!("TLS VerifyFull (hostname check) using root cert path `{}`", root_ca()),
    };
    StageResult {
        stage: ConnectionStage::Tls,
        ok: true,
        message,
    }
}

pub fn with_auth_result(
    mut report: ConnectionDiagnosticsReport,
    auth_ok: bool,
    auth_message: impl Into<String>,
) -> ConnectionDiagnosticsReport {
    report.stages.push(StageResult {
        stage: ConnectionStage::Auth,
        ok: auth_ok,
        message: redact_diag_message(&auth_message.into()),
    });
    if !auth_ok && report.failed_stage.is_none() {
        report.failed_stage = Some(ConnectionStage::Auth);
    }
    report
}

#[cfg(test)]
#[path = "tests/connection_diagnostics_tests.rs"]
mod tests;
