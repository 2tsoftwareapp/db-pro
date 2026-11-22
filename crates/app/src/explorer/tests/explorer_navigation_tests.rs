//! Tests for `../explorer_navigation.rs` (moved out of the source file).
use super::*;
use crate::UiSslMode;

fn connection() -> UiConnectionSummary {
    UiConnectionSummary {
        id: "conn-1".to_owned(),
        name: "Primary DB".to_owned(),
        host: "localhost".to_owned(),
        port: 5432,
        database: "app".to_owned(),
        username: "postgres".to_owned(),
        driver: "PostgreSQL".to_owned(),
        ssl_mode: UiSslMode::Require,
        readonly: false,
        create_if_missing: false,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
    }
}

#[test]
fn connect_preserves_state_when_uncommitted() {
    let mut lifecycle = ConnectionLifecycleState::default();
    let mut schema = SchemaExplorerState::default();
    let mut table = TableEditorState::default();
    let mut workspace = WorkspaceShellState::default();
    let mut agent = AgentState::default();
    let mut execution = QueryExecutionPolicyState::default();
    let mut feedback = FeedbackState::default();

    lifecycle.set_active_connection_id(Some("conn-0".to_owned()));
    lifecycle.set_connected(true);

    let target = connection();
    let req = ExplorerConnectionContext::new(
        &mut lifecycle,
        &mut schema,
        &mut table,
        &mut workspace,
        &mut agent,
        &mut execution,
        &mut feedback,
    )
    .connect(&target);

    assert!(req.is_some());
    assert_eq!(lifecycle.active_connection_id(), Some("conn-0"));
    assert!(lifecycle.is_connected());
}