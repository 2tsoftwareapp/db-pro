// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use crate::{UiConnectionDraft, UiConnectionSummary, UiDriver, UiSslMode};
use db_pro_core::domain::cloud_presets::{
    apply_preset, find_preset, parse_connection_snippet, validate_cloud_endpoint, CloudProvider,
};
use db_pro_core::domain::connection::{ConnectionConfig, ConnectionEnvironment, DriverType, SshTunnelConfig, SslMode};

/// Convert string representation of driver into [`UiDriver`].
pub fn ui_driver_from_str(driver: &str) -> UiDriver {
    if driver.eq_ignore_ascii_case("sqlite") {
        UiDriver::Sqlite
    } else if driver.eq_ignore_ascii_case("mysql") {
        UiDriver::Mysql
    } else if driver.eq_ignore_ascii_case("sql server") || driver.eq_ignore_ascii_case("sqlserver") {
        UiDriver::SqlServer
    } else {
        UiDriver::Postgres
    }
}

/// Convert [`UiDriver`] to domain [`DriverType`].
pub fn driver_to_domain(driver: UiDriver) -> DriverType {
    match driver {
        UiDriver::Postgres => DriverType::Postgres,
        UiDriver::Mysql => DriverType::Mysql,
        UiDriver::Sqlite => DriverType::SQLite,
        UiDriver::SqlServer => DriverType::SqlServer,
    }
}

/// Convert domain [`DriverType`] to [`UiDriver`].
pub fn driver_from_domain(driver: DriverType) -> UiDriver {
    match driver {
        DriverType::Postgres => UiDriver::Postgres,
        DriverType::Mysql => UiDriver::Mysql,
        DriverType::SQLite => UiDriver::Sqlite,
        DriverType::SqlServer => UiDriver::SqlServer,
    }
}

/// Convert [`UiSslMode`] to domain [`SslMode`].
pub fn ssl_mode_to_domain(mode: UiSslMode) -> SslMode {
    match mode {
        UiSslMode::Disable => SslMode::Disable,
        UiSslMode::Require => SslMode::Require,
        UiSslMode::VerifyCa => SslMode::VerifyCa,
        UiSslMode::VerifyFull => SslMode::VerifyFull,
    }
}

/// Convert domain [`SslMode`] to [`UiSslMode`].
pub fn ssl_mode_from_domain(mode: SslMode) -> UiSslMode {
    match mode {
        SslMode::Disable => UiSslMode::Disable,
        SslMode::Require => UiSslMode::Require,
        SslMode::VerifyCa => UiSslMode::VerifyCa,
        SslMode::VerifyFull => UiSslMode::VerifyFull,
    }
}

/// Map a saved [`UiConnectionSummary`] into a working [`UiConnectionDraft`] for editing.
pub fn summary_to_edit_draft(connection: &UiConnectionSummary) -> UiConnectionDraft {
    UiConnectionDraft {
        name: connection.name.clone(),
        host: connection.host.clone(),
        port: connection.port.to_string(),
        database: connection.database.clone(),
        username: connection.username.clone(),
        password: String::new(),
        driver: ui_driver_from_str(&connection.driver),
        ssl_mode: connection.ssl_mode,
        readonly: connection.readonly,
        create_if_missing: connection.create_if_missing,
        group: connection.group.clone().unwrap_or_default(),
        tags: connection.tags.join(", "),
        favorite: connection.favorite,
        environment: connection.environment.as_label().to_owned(),
        ssh_tunnel_enabled: false,
        ssh_host: String::new(),
        ssh_port: "22".to_owned(),
        ssh_user: String::new(),
        ssh_private_key: String::new(),
        ssh_profile_id: String::new(),
        ssl_root_cert_path: String::new(),
        ssl_client_cert_path: String::new(),
        ssl_client_key_path: String::new(),
        cloud_preset: String::new(),
        auth_kind: if connection
            .tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("auth:ephemeral-token"))
        {
            "ephemeral_token".into()
        } else {
            "password".into()
        },
        cloud_snippet: String::new(),
        cloud_guidance: String::new(),
    }
}

/// Map a saved [`UiConnectionSummary`] into a duplicated [`UiConnectionDraft`].
pub fn summary_to_duplicate_draft(connection: &UiConnectionSummary) -> UiConnectionDraft {
    let mut draft = summary_to_edit_draft(connection);
    draft.name = format!("{} (Copy)", connection.name);
    draft
}

/// Map a [`UiConnectionDraft`] to a domain [`ConnectionConfig`].
pub fn draft_to_domain_config(draft: &UiConnectionDraft) -> ConnectionConfig {
    let driver = driver_to_domain(draft.driver);
    let ssl_mode = ssl_mode_to_domain(draft.ssl_mode);
    let default_port = match draft.driver {
        UiDriver::Mysql => 3306,
        UiDriver::SqlServer => 1433,
        _ => 5432,
    };
    let port = draft.port.parse().unwrap_or(default_port);

    let ssh_tunnel = if draft.ssh_tunnel_enabled {
        Some(SshTunnelConfig {
            host: draft.ssh_host.clone(),
            port: draft.ssh_port.parse().unwrap_or(22),
            user: draft.ssh_user.clone(),
            private_key_path: draft.ssh_private_key.clone(),
            password: None,
        })
    } else {
        None
    };

    ConnectionConfig {
        name: draft.name.clone(),
        host: draft.host.clone(),
        port,
        database: draft.database.clone(),
        username: draft.username.clone(),
        driver,
        ssl_mode,
        ssh_tunnel,
        ssh_profile_id: {
            let id = draft.ssh_profile_id.trim();
            if id.is_empty() {
                None
            } else {
                Some(id.to_owned())
            }
        },
        ssl_root_cert_path: {
            let p = draft.ssl_root_cert_path.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_owned())
            }
        },
        ssl_client_cert_path: {
            let p = draft.ssl_client_cert_path.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_owned())
            }
        },
        ssl_client_key_path: {
            let p = draft.ssl_client_key_path.trim();
            if p.is_empty() {
                None
            } else {
                Some(p.to_owned())
            }
        },
        query_timeout_ms: 30_000,
        max_rows: 500,
        color: None,
        tags: draft
            .tags
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        group: {
            let g = draft.group.trim();
            if g.is_empty() {
                None
            } else {
                Some(g.to_owned())
            }
        },
        favorite: draft.favorite,
        environment: match draft.environment.as_str() {
            "Staging" => ConnectionEnvironment::Staging,
            "Production" => ConnectionEnvironment::Production,
            "Custom" => ConnectionEnvironment::Custom,
            _ => ConnectionEnvironment::Development,
        },
        readonly: draft.readonly,
        create_if_missing: draft.create_if_missing,
    }
}

/// Parse and apply a connection URI snippet into a draft.
pub fn apply_connection_snippet(draft: &mut UiConnectionDraft, snippet: &str) -> Result<(), String> {
    let parsed = parse_connection_snippet(snippet)?;
    draft.host = parsed.host;
    draft.port = parsed.port.to_string();
    draft.database = parsed.database;
    draft.username = parsed.username;
    if let Some(pwd) = parsed.password {
        draft.password = pwd;
    }
    if let Some(mode) = parsed.ssl_mode {
        draft.ssl_mode = ssl_mode_from_domain(mode);
    }
    draft.driver = driver_from_domain(parsed.driver);
    // The URI was consumed into draft fields; dropping it keeps any embedded
    // plaintext password out of the persisted draft state.
    draft.cloud_snippet.clear();
    Ok(())
}

/// Apply a cloud preset configuration onto a connection draft, returning guidance text.
pub fn apply_cloud_preset_to_draft(draft: &mut UiConnectionDraft, key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Ok(String::new());
    }
    let (provider, driver) = match key {
        "aws_rds:postgres" => (CloudProvider::AwsRds, DriverType::Postgres),
        "aws_rds:mysql" => (CloudProvider::AwsRds, DriverType::Mysql),
        "aws_aurora:postgres" => (CloudProvider::AwsAurora, DriverType::Postgres),
        "gcp_cloudsql:postgres" => (CloudProvider::GcpCloudSql, DriverType::Postgres),
        "gcp_cloudsql:mysql" => (CloudProvider::GcpCloudSql, DriverType::Mysql),
        "azure:postgres" => (CloudProvider::AzureDatabase, DriverType::Postgres),
        "azure:mysql" => (CloudProvider::AzureDatabase, DriverType::Mysql),
        _ => return Err(format!("unknown cloud preset `{key}`")),
    };

    let preset = find_preset(provider, driver).ok_or_else(|| "preset not found for driver".to_owned())?;

    let mut cfg = ConnectionConfig {
        name: draft.name.clone(),
        host: draft.host.clone(),
        port: draft.port.parse().unwrap_or(preset.default_port),
        database: draft.database.clone(),
        username: draft.username.clone(),
        driver,
        ssl_mode: SslMode::Require,
        ..ConnectionConfig::default()
    };
    apply_preset(&mut cfg, &preset);

    draft.driver = driver_from_domain(cfg.driver);
    draft.port = cfg.port.to_string();
    draft.ssl_mode = ssl_mode_from_domain(cfg.ssl_mode);

    if draft.host.trim().is_empty()
        || draft.host == "localhost"
        || draft.host.contains("xxxxx")
        || draft.host.contains("x.x.x")
    {
        draft.host = cfg.host;
    }
    if draft.name.trim().is_empty() {
        draft.name = cfg.name;
    }
    if !preset.token_auth_supported && draft.auth_kind == "ephemeral_token" {
        draft.auth_kind = "password".into();
    }

    let warn = validate_cloud_endpoint(&draft.host, provider).err().unwrap_or_default();

    let guidance = format!(
        "{} · {}{}",
        preset.ca_guidance,
        preset.notes,
        if warn.is_empty() {
            String::new()
        } else {
            format!(" · note: {warn}")
        }
    );

    Ok(guidance)
}

#[cfg(test)]
#[path = "tests/mapper_tests.rs"]
mod tests;
