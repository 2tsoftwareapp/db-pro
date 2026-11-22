//! S4 — Schema Triggers Runtime Verification
//!
//! Tests the full trigger lifecycle on SQLite:
//! CREATE trigger → DML → observe effect → introspect → DROP → verify gone.

use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;

async fn setup() -> (SQLiteConnector, db_pro_core::domain::connection::ConnectionHandle) {
    let connector = SQLiteConnector::new();
    let config = ConnectionConfig {
        name: "s4-trigger-test".into(),
        host: String::new(),
        port: 0,
        database: ":memory:".into(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 10_000,
        max_rows: 1_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};
    let handle = connector.connect(&config, "").await.unwrap();
    (connector, handle)
}

#[tokio::test]
async fn trigger_lifecycle_create_introspect_drop() {
    let (connector, handle) = setup().await;

    // ── Setup: create a table ──────────────────────────────────────────
    connector
        .execute(
            &handle,
            "CREATE TABLE audit_log (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                action TEXT NOT NULL, \
                created_at TEXT NOT NULL DEFAULT (datetime('now'))\
             )",
            &[],
        )
        .await
        .unwrap();

    connector
        .execute(
            &handle,
            "CREATE TABLE items (\
                id INTEGER PRIMARY KEY, \
                name TEXT NOT NULL, \
                deleted INTEGER NOT NULL DEFAULT 0\
             )",
            &[],
        )
        .await
        .unwrap();

    // ── Phase 1: no triggers yet ───────────────────────────────────────
    let result = connector.introspect(&handle).await.unwrap();
    assert!(result.triggers.is_empty(), "should have no triggers before CREATE");

    // ── Phase 2: CREATE trigger ────────────────────────────────────────
    connector
        .execute(
            &handle,
            "CREATE TRIGGER log_item_delete \
             AFTER UPDATE ON items \
             FOR EACH ROW \
             WHEN OLD.deleted = 0 AND NEW.deleted = 1 \
             BEGIN \
                 INSERT INTO audit_log(action) VALUES('item_deleted'); \
             END",
            &[],
        )
        .await
        .unwrap();

    // ── Phase 3: introspect and verify all fields ──────────────────────
    let result = connector.introspect(&handle).await.unwrap();
    assert_eq!(result.triggers.len(), 1, "should have exactly one trigger");

    let trigger = &result.triggers[0];
    assert_eq!(trigger.name, "log_item_delete");
    assert_eq!(trigger.table_name, "items");
    assert_eq!(trigger.timing, "AFTER");
    assert_eq!(trigger.event, "UPDATE");
    assert!(trigger.enabled);
    assert!(
        trigger.definition.contains("log_item_delete"),
        "definition should contain trigger name"
    );
    assert!(
        trigger.definition.contains("audit_log"),
        "definition should contain the body SQL"
    );

    // ── Phase 4: DML → observe trigger effect ─────────────────────────
    connector
        .execute(&handle, "INSERT INTO items(id, name) VALUES (1, 'widget')", &[])
        .await
        .unwrap();

    // Soft-delete: should fire the trigger.
    connector
        .execute(&handle, "UPDATE items SET deleted = 1 WHERE id = 1", &[])
        .await
        .unwrap();

    // Verify the audit log entry was created by the trigger.
    let audit = connector
        .query(&handle, "SELECT action FROM audit_log ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(audit.rows.len(), 1, "trigger should have inserted one audit row");

    // ── Phase 5: DROP trigger ──────────────────────────────────────────
    connector
        .execute(&handle, "DROP TRIGGER log_item_delete", &[])
        .await
        .unwrap();

    let result = connector.introspect(&handle).await.unwrap();
    assert!(result.triggers.is_empty(), "trigger should be gone after DROP");

    // ── Phase 6: DML after DROP → no more trigger effect ───────────────
    connector
        .execute(&handle, "INSERT INTO items(id, name) VALUES (2, 'gadget')", &[])
        .await
        .unwrap();
    connector
        .execute(&handle, "UPDATE items SET deleted = 1 WHERE id = 2", &[])
        .await
        .unwrap();

    let audit = connector
        .query(&handle, "SELECT COUNT(*) AS cnt FROM audit_log", &[])
        .await
        .unwrap();
    // COUNT(*) always returns one row; check the actual count value.
    let count_cell = &audit.rows[0].0[0];
    assert!(
        matches!(count_cell, db_pro_core::domain::query::CellValue::Int64(1)),
        "audit_log should still have exactly 1 row (trigger should not fire after DROP)"
    );
}

#[tokio::test]
async fn trigger_before_insert_introspection() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "CREATE TABLE logs (id INTEGER PRIMARY KEY, msg TEXT)", &[])
        .await
        .unwrap();

    // BEFORE INSERT trigger.
    connector
        .execute(
            &handle,
            "CREATE TRIGGER validate_msg \
             BEFORE INSERT ON logs \
             FOR EACH ROW \
             WHEN NEW.msg IS NULL \
             BEGIN \
                 SELECT RAISE(ABORT, 'msg cannot be null'); \
             END",
            &[],
        )
        .await
        .unwrap();

    let result = connector.introspect(&handle).await.unwrap();
    let trigger = result
        .triggers
        .iter()
        .find(|t| t.name == "validate_msg")
        .expect("BEFORE INSERT trigger should exist");

    assert_eq!(trigger.table_name, "logs");
    assert_eq!(trigger.timing, "BEFORE");
    assert_eq!(trigger.event, "INSERT");
    assert!(trigger.definition.contains("RAISE"));
}

#[tokio::test]
async fn trigger_with_special_identifiers() {
    let (connector, handle) = setup().await;

    // Table with spaces and unicode in name.
    connector
        .execute(
            &handle,
            "CREATE TABLE \"Ünïcödé table\" (id INTEGER PRIMARY KEY, val TEXT)",
            &[],
        )
        .await
        .unwrap();

    // BEFORE trigger on table with special name.
    connector
        .execute(
            &handle,
            "CREATE TRIGGER \"special_trig\" \
             BEFORE UPDATE ON \"Ünïcödé table\" \
             FOR EACH ROW \
             BEGIN \
                 SELECT 1; \
             END",
            &[],
        )
        .await
        .unwrap();

    let result = connector.introspect(&handle).await.unwrap();
    let trigger = result
        .triggers
        .iter()
        .find(|t| t.name == "special_trig")
        .expect("trigger with special identifiers should exist");

    assert_eq!(trigger.table_name, "Ünïcödé table");
    assert_eq!(trigger.timing, "BEFORE");
    assert_eq!(trigger.event, "UPDATE");
}

#[tokio::test]
async fn trigger_instead_of_on_view() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "CREATE TABLE base_tbl (id INTEGER PRIMARY KEY, val TEXT)", &[])
        .await
        .unwrap();

    connector
        .execute(&handle, "CREATE VIEW my_view AS SELECT id, val FROM base_tbl", &[])
        .await
        .unwrap();

    // SQLite supports INSTEAD OF triggers only on views.
    connector
        .execute(
            &handle,
            "CREATE TRIGGER view_insert_trig \
             INSTEAD OF INSERT ON my_view \
             FOR EACH ROW \
             BEGIN \
                 INSERT INTO base_tbl(id, val) VALUES (NEW.id, NEW.val); \
             END",
            &[],
        )
        .await
        .unwrap();

    let result = connector.introspect(&handle).await.unwrap();
    let trigger = result
        .triggers
        .iter()
        .find(|t| t.name == "view_insert_trig")
        .expect("INSTEAD OF trigger on view should exist");

    assert_eq!(trigger.table_name, "my_view");
    assert_eq!(trigger.timing, "INSTEAD OF");
    assert_eq!(trigger.event, "INSERT");
}

#[tokio::test]
async fn multiple_triggers_on_same_table() {
    let (connector, handle) = setup().await;

    connector
        .execute(&handle, "CREATE TABLE events (id INTEGER PRIMARY KEY, data TEXT)", &[])
        .await
        .unwrap();

    connector
        .execute(
            &handle,
            "CREATE TRIGGER trg_after_insert \
             AFTER INSERT ON events \
             FOR EACH ROW BEGIN SELECT 1; END",
            &[],
        )
        .await
        .unwrap();

    connector
        .execute(
            &handle,
            "CREATE TRIGGER trg_before_delete \
             BEFORE DELETE ON events \
             FOR EACH ROW BEGIN SELECT 1; END",
            &[],
        )
        .await
        .unwrap();

    let result = connector.introspect(&handle).await.unwrap();
    assert_eq!(result.triggers.len(), 2, "should have two triggers");

    let names: Vec<&str> = result.triggers.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"trg_after_insert"));
    assert!(names.contains(&"trg_before_delete"));

    // Both should reference the same table.
    for t in &result.triggers {
        assert_eq!(t.table_name, "events");
    }

    // Verify distinct timing/event.
    let insert_trig = result.triggers.iter().find(|t| t.name == "trg_after_insert").unwrap();
    let delete_trig = result.triggers.iter().find(|t| t.name == "trg_before_delete").unwrap();
    assert_eq!(insert_trig.timing, "AFTER");
    assert_eq!(insert_trig.event, "INSERT");
    assert_eq!(delete_trig.timing, "BEFORE");
    assert_eq!(delete_trig.event, "DELETE");
}
