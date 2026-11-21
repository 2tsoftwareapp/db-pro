//! Tests for `../connection_diagnostics.rs` (moved out of the source file).
use super::*;
use crate::domain::connection::{ConnectionConfig, ConnectionEnvironment, DriverType, SslMode};

fn base_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "t".into(),
        host: "127.0.0.1".into(),
        port: 1,
        database: "db".into(),
        username: "u".into(),
        driver: DriverType::Postgres,
        ssl_mode: SslMode::Require,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 1000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: ConnectionEnvironment::Development,
        readonly: false,
        create_if_missing: false,
    }
}

#[test]
fn redact_strips_password_material() {
    let msg = redact_diag_message("auth failed password=supersecret rest");
    assert!(!msg.contains("supersecret"));
    assert!(msg.contains("[redacted]"));
}

#[test]
fn probe_reports_tcp_failure_without_auth() {
    let report = probe_network_stages(&base_config());
    assert!(report.stages.iter().any(|s| s.stage == ConnectionStage::Dns && s.ok));
    assert!(report.stages.iter().any(|s| s.stage == ConnectionStage::Tcp && !s.ok));
    assert_eq!(report.failed_stage, Some(ConnectionStage::Tcp));
    assert!(!report.summary().to_lowercase().contains("password"));
}

#[test]
fn auth_stage_appended() {
    let report = with_auth_result(probe_network_stages(&base_config()), false, "password=leak");
    let auth = report.stages.iter().find(|s| s.stage == ConnectionStage::Auth).unwrap();
    assert!(!auth.ok);
    assert!(!auth.message.contains("leak"));
}

#[test]
fn tunneled_diagnostics_do_not_probe_direct_database_or_claim_unobserved_ssh_success() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut config = base_config();
    config.port = listener.local_addr().unwrap().port();
    config.ssh_tunnel = Some(crate::domain::connection::SshTunnelConfig {
        host: "bastion.invalid".into(),
        port: 22,
        user: "deploy".into(),
        private_key_path: "/tmp/key".into(),
        password: None,
    });

    let report = probe_network_stages(&config);
    assert!(
        report.stages.is_empty(),
        "no transport stage has been exercised by preflight"
    );
    assert!(!report.summary().contains("succeeded"));
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    let report = with_auth_result(report, false, "SSH tunnel to bastion.invalid failed");
    assert!(report.failed_stage.is_some());
    assert!(report.stages.iter().all(|stage| !stage.ok));
    assert!(report.summary().contains("SSH tunnel to bastion.invalid failed"));
}

#[test]
fn ssh_profile_preflight_is_deferred_to_full_connection_test() {
    let mut config = base_config();
    config.ssh_profile_id = Some("profile".into());
    let report = probe_network_stages(&config);
    assert!(report.stages.is_empty());
    let report = with_auth_result(report, true, "Connection succeeded");
    assert!(report.failed_stage.is_none());
    assert_eq!(report.stages.len(), 1);
    assert!(report.stages[0].ok);
}
