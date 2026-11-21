use super::mapper::draft_to_domain_config;
use crate::{RequestId, UiCommand, UiConnectionDraft, UiDriver};
use db_pro_core::domain::connection::SshProfile;
use db_pro_core::domain::connection_diagnostics::{
    probe_network_stages, redact_diag_message, with_auth_result, ConnectionDiagnosticsReport, ConnectionStage,
    StageResult,
};

/// Validate that the draft has all required fields and valid ports.
pub(super) fn is_valid_port(port: &str) -> bool {
    matches!(port.parse::<u16>(), Ok(port) if port > 0)
}

pub fn validate_connection_draft(draft: &UiConnectionDraft) -> Result<(), String> {
    // Messages are localized here, not hardcoded: the same string is rendered
    // beside the field and replayed as the dialog error toast.
    if draft.name.trim().is_empty() {
        return Err(t!("connection.validation_name_required").to_string());
    }
    if draft.database.trim().is_empty() {
        return Err(t!("connection.validation_database_required").to_string());
    }
    if matches!(draft.driver, UiDriver::Postgres | UiDriver::Mysql | UiDriver::SqlServer) {
        if draft.host.trim().is_empty() {
            return Err(t!("connection.validation_host_required").to_string());
        }
        if draft.username.trim().is_empty() {
            return Err(t!("connection.validation_username_required").to_string());
        }
        if draft.port.trim().is_empty() {
            return Err(t!("connection.validation_port_required").to_string());
        }
        if !is_valid_port(&draft.port) {
            return Err(t!("connection.validation_port_invalid").to_string());
        }
    }
    if draft.ssh_tunnel_enabled {
        if draft.ssh_host.trim().is_empty() {
            return Err(t!("connection.validation_ssh_host_required").to_string());
        }
        if draft.ssh_user.trim().is_empty() {
            return Err(t!("connection.validation_ssh_user_required").to_string());
        }
        if draft.ssh_private_key.trim().is_empty() {
            return Err(t!("connection.validation_ssh_key_required").to_string());
        }
        if draft.ssh_port.trim().is_empty() {
            return Err(t!("connection.validation_ssh_port_required").to_string());
        }
        if !is_valid_port(&draft.ssh_port) {
            return Err(t!("connection.validation_ssh_port_invalid").to_string());
        }
    }
    Ok(())
}

/// Apply a driver selection onto a draft. The form injects no defaults: a
/// switch only changes the driver and leaves every typed value — port, host,
/// credentials, TLS mode — exactly as the user set it.
pub fn select_driver(draft: &mut UiConnectionDraft, driver: UiDriver) {
    draft.driver = driver;
}

/// Save the current SSH fields in a draft as a new reusable [`SshProfile`].
pub fn save_ssh_profile(profiles: &mut Vec<SshProfile>, draft: &UiConnectionDraft) -> Result<String, String> {
    if draft.ssh_host.trim().is_empty() || draft.ssh_user.trim().is_empty() {
        return Err("SSH host and user are required to save a profile".to_owned());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let profile = SshProfile {
        id: id.clone(),
        name: format!("{}@{}", draft.ssh_user.trim(), draft.ssh_host.trim()),
        host: draft.ssh_host.clone(),
        port: draft.ssh_port.parse().unwrap_or(22),
        user: draft.ssh_user.clone(),
        private_key_path: draft.ssh_private_key.clone(),
        password: None,
    };
    profiles.push(profile);
    Ok(id)
}

/// Apply a saved [`SshProfile`] onto a draft.
pub fn apply_ssh_profile_to_draft(draft: &mut UiConnectionDraft, profile: &SshProfile) {
    draft.ssh_tunnel_enabled = true;
    draft.ssh_host = profile.host.clone();
    draft.ssh_port = profile.port.to_string();
    draft.ssh_user = profile.user.clone();
    draft.ssh_private_key = profile.private_key_path.clone();
    draft.ssh_profile_id = profile.id.clone();
}

/// Construct the appropriate [`UiCommand`] for testing or saving a connection.
pub fn build_connection_command(
    draft: UiConnectionDraft,
    editing_id: Option<String>,
    request_id: RequestId,
    save: bool,
) -> UiCommand {
    if save {
        if let Some(connection_id) = editing_id {
            UiCommand::UpdateConnection {
                request_id,
                connection_id,
                draft,
            }
        } else {
            UiCommand::CreateConnection { request_id, draft }
        }
    } else {
        UiCommand::TestConnection { request_id, draft }
    }
}

/// Build the runtime command for switching to a saved connection.
pub fn build_connect_command(request_id: RequestId, connection_id: String) -> UiCommand {
    UiCommand::Connect {
        request_id,
        connection_id,
    }
}

/// Build the initial saved-connection catalog request.
pub fn build_list_connections_command(request_id: RequestId) -> UiCommand {
    UiCommand::ListConnections { request_id }
}

/// Run network stage probes and produce diagnostics report.
pub fn probe_draft_diagnostics(
    draft: &UiConnectionDraft,
    auth_ok: bool,
    auth_message: &str,
) -> ConnectionDiagnosticsReport {
    let config = draft_to_domain_config(draft);
    let report = probe_network_stages(&config);
    with_auth_result(report, auth_ok, auth_message)
}

/// Mark the auth stage of a diagnostics report with the real failure reason.
///
/// The report is captured when the test is dispatched with a placeholder
/// ("Authentication pending…"); when the runtime reports the real failure we
/// rewrite that stage instead of leaving the placeholder on screen forever.
/// Network stages probed at dispatch time are preserved (#609).
pub(crate) fn mark_diagnostics_auth_failed(report: &mut ConnectionDiagnosticsReport, message: &str) {
    let message = redact_diag_message(message);
    // A probe-time transport failure (DNS/TCP) is the stage that explains the
    // failure — keep it as the headline. Otherwise the auth stage owns the
    // verdict, with the runtime's real reason.
    let transport_failed = report
        .failed_stage
        .is_some_and(|stage| stage != ConnectionStage::Auth);
    match report.stages.iter_mut().find(|stage| stage.stage == ConnectionStage::Auth) {
        Some(stage) => {
            stage.ok = false;
            stage.message = message;
        }
        None => report.stages.push(StageResult {
            stage: ConnectionStage::Auth,
            ok: false,
            message,
        }),
    }
    if !transport_failed {
        report.failed_stage = Some(ConnectionStage::Auth);
    }
}

#[cfg(test)]
#[path = "tests/logic_tests.rs"]
mod tests;
