//! Tests for `../cloud_presets.rs` (moved out of the source file).
use super::*;

#[test]
fn parse_postgres_uri() {
    let p = parse_connection_snippet(
        "postgresql://ada:s3cret@mydb.abc.eu-west-1.rds.amazonaws.com:5432/app?sslmode=verify-full",
    )
    .unwrap();
    assert_eq!(p.driver, DriverType::Postgres);
    assert_eq!(p.host, "mydb.abc.eu-west-1.rds.amazonaws.com");
    assert_eq!(p.port, 5432);
    assert_eq!(p.database, "app");
    assert_eq!(p.username, "ada");
    assert_eq!(p.password.as_deref(), Some("s3cret"));
    assert_eq!(p.ssl_mode, Some(SslMode::VerifyFull));
}

#[test]
fn parse_sql_server_uri() {
    let p = parse_connection_snippet("sqlserver://sa:p%40ss@localhost/inventory").unwrap();
    assert_eq!(p.driver, DriverType::SqlServer);
    assert_eq!(p.port, 1433);
    assert_eq!(p.password.as_deref(), Some("p@ss"));
}

#[test]
fn apply_rds_preset_sets_tls() {
    let mut cfg = ConnectionConfig::default();
    let preset = CloudConnectionPreset {
        provider: CloudProvider::AwsRds,
        driver: DriverType::Postgres,
        default_port: 5432,
        ssl_mode: SslMode::VerifyFull,
        ca_guidance: "ca",
        host_hint: "hint.rds.amazonaws.com",
        notes: "n",
        token_auth_supported: true,
    };
    apply_preset(&mut cfg, &preset);
    assert_eq!(cfg.ssl_mode, SslMode::VerifyFull);
    assert_eq!(cfg.port, 5432);
    assert!(cfg.tags.iter().any(|t| t == "AWS RDS"));
}

#[test]
fn validate_rds_host() {
    assert!(validate_cloud_endpoint("x.y.us-east-1.rds.amazonaws.com", CloudProvider::AwsRds).is_ok());
    assert!(validate_cloud_endpoint("example.com", CloudProvider::AwsRds).is_err());
    assert!(validate_cloud_endpoint("localhost", CloudProvider::AwsRds).is_ok());
}

#[test]
fn redacted_summary_has_no_secret() {
    let cfg = ConnectionConfig {
        name: "prod".into(),
        host: "db.example".into(),
        username: "u".into(),
        ..ConnectionConfig::default()
    };
    let s = redacted_export_summary(&cfg, ConnectionAuthKind::EphemeralToken);
    assert!(!s.contains("password"));
    assert!(s.contains("EphemeralToken") || s.contains("ephemeral_token") || s.contains("auth="));
}

/// #598: percent-decode must work on bytes, and every credential component
/// (user, password, database) must be decoded — not only the password.
#[test]
fn percent_decoding_is_utf8_aware_and_covers_all_parts_598() {
    let parsed = parse_connection_snippet(
        "postgresql://user%40srv:m%E1%BA%ADt%20kh%E1%BA%A9u@host:5432/my%20db",
    )
    .expect("uri parses");
    assert_eq!(parsed.username, "user@srv");
    assert_eq!(parsed.password.as_deref(), Some("mật khẩu"));
    assert_eq!(parsed.database, "my db");

    // Raw multi-byte UTF-8 in the URI is preserved (no Latin-1 re-encoding).
    let parsed = parse_connection_snippet("postgresql://admin:mậtkhẩu@host/db").expect("uri parses");
    assert_eq!(parsed.password.as_deref(), Some("mậtkhẩu"));
}

/// #598: a bracketed IPv6 host without a port keeps the default port instead of
/// failing on `1]`.
#[test]
fn ipv6_hosts_parse_with_and_without_port_598() {
    let parsed = parse_connection_snippet("postgresql://u:p@[::1]/db").expect("ipv6 without port");
    assert_eq!(parsed.host, "::1");
    assert_eq!(parsed.port, 5432);

    let parsed = parse_connection_snippet("postgresql://u:p@[2001:db8::1]:6432/db").expect("ipv6 with port");
    assert_eq!(parsed.host, "2001:db8::1");
    assert_eq!(parsed.port, 6432);

    // Bare (unbracketed) IPv6 also keeps the default port.
    let parsed = parse_connection_snippet("postgresql://u:p@2001:db8::1/db").expect("bare ipv6");
    assert_eq!(parsed.host, "2001:db8::1");
    assert_eq!(parsed.port, 5432);
}
