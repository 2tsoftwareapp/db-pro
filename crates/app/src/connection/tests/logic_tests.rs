//! Tests for `../logic.rs` (moved out of the source file).
use super::*;
use crate::{RequestId, UiCommand, UiConnectionDraft, UiDriver};

fn valid_network_draft() -> UiConnectionDraft {
    UiConnectionDraft {
        name: "conn".to_owned(),
        host: "db.example.com".to_owned(),
        database: "appdb".to_owned(),
        username: "alice".to_owned(),
        port: "5432".to_owned(),
        driver: UiDriver::Postgres,
        ..Default::default()
    }
}

#[test]
fn network_draft_requires_host() {
    let mut draft = valid_network_draft();
    draft.host = "   ".to_owned();
    assert_eq!(
        validate_connection_draft(&draft),
        Err(t!("connection.validation_host_required").to_string())
    );
}

#[test]
fn network_draft_requires_username() {
    let mut draft = valid_network_draft();
    draft.username = String::new();
    assert_eq!(
        validate_connection_draft(&draft),
        Err(t!("connection.validation_username_required").to_string())
    );
}

#[test]
fn draft_validation_messages_are_localized() {
    let mut draft = valid_network_draft();
    draft.name = String::new();
    let message = validate_connection_draft(&draft).expect_err("empty name must fail");
    assert_eq!(message, t!("connection.validation_name_required"));
}

#[test]
fn network_draft_with_host_and_username_passes() {
    assert_eq!(validate_connection_draft(&valid_network_draft()), Ok(()));
}

#[test]
fn sqlite_draft_is_exempt_from_host_and_username() {
    let draft = UiConnectionDraft {
        name: "local file".to_owned(),
        database: "app.db".to_owned(),
        driver: UiDriver::Sqlite,
        host: String::new(),
        username: String::new(),
        port: String::new(),
        ..Default::default()
};
    assert_eq!(validate_connection_draft(&draft), Ok(()));
}

#[test]
fn empty_port_reports_required_not_range_error() {
    // UAT-10: a blank port used to surface the range message, which reads as
    // "you typed a bad number" when nothing was typed at all.
    let mut draft = valid_network_draft();
    draft.port = String::new();
    let message = validate_connection_draft(&draft).expect_err("empty port must fail");
    assert_eq!(message, t!("connection.validation_port_required"));

    draft.port = "70000".to_owned();
    let message = validate_connection_draft(&draft).expect_err("out-of-range port must fail");
    assert_eq!(message, t!("connection.validation_port_invalid"));
}

#[test]
fn connect_command_keeps_connection_identity_explicit() {
    assert!(matches!(
        build_connect_command(RequestId(7), "conn-1".to_owned()),
        UiCommand::Connect {
            request_id: RequestId(7),
            connection_id,
        } if connection_id == "conn-1"
    ));
}
