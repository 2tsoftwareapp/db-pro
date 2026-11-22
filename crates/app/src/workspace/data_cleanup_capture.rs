//! Deterministic presentation-only fixtures for native data-feature captures.
use super::*;
use db_pro_core::domain::{monitoring::*, pg_settings::*, user::*};

impl DbProApp {
    /// Select a real feature view without executing provider operations.
    pub fn open_data_cleanup_for_capture(&mut self, surface: &str, light: bool) -> bool {
        if !matches!(
            surface,
            "schema-definition"
                | "monitoring"
                | "monitoring-workload"
                | "postgres-settings"
                | "transfers"
                | "security"
                | "diagram-design"
                | "table-ddl"
        ) {
            return false;
        }
        self.open_diagram_workspace_for_capture();
        self.workspace.agent_open = false;
        self.workspace.bottom_panel_open = false;
        self.workspace.sidebar_open = true;
        self.connection.lifecycle.mark_connections_requested();
        self.connection.catalog.replace(vec![crate::UiConnectionSummary {
            id: "capture-conn".into(),
            name: "Presentation fixture · PostgreSQL".into(),
            host: "localhost".into(),
            port: 5432,
            database: "sample_store".into(),
            username: "app_reader".into(),
            driver: "postgresql".into(),
            ssl_mode: crate::UiSslMode::Disable,
            readonly: false,
            create_if_missing: false,
            tags: Vec::new(),
            group: None,
            favorite: false,
            environment: db_pro_core::domain::connection::ConnectionEnvironment::Development,
        }]);
        self.management.monitoring.monitoring_poll = false;
        match surface {
            "schema-definition" => {
                self.workspace.activity = Activity::Explorer;
                self.workspace.active_tab = WorkspaceTab::SchemaObject;
                self.schema.explorer.selected_table = None;
                self.schema.explorer.schema.views = vec![crate::UiViewSummary {
                    schema: "public".into(), name: "customer_order_summary".into(),
                    definition: "CREATE VIEW public.customer_order_summary AS\nSELECT c.id, c.email,\n       COUNT(o.id) AS order_count,\n       COALESCE(SUM(o.total), 0) AS lifetime_value\nFROM public.customers AS c\nLEFT JOIN public.orders AS o ON o.customer_id = c.id\nGROUP BY c.id, c.email;".into(),
                }];
                self.schema.explorer.selected_schema_object =
                    Some(SchemaObjectSelection::View("customer_order_summary".into()));
                self.schema.explorer.schema_object_view = SchemaObjectView::Definition;
            }
            "table-ddl" => self.open_table_ddl_for_capture(light),
            "diagram-design" => {
                let template = self.schema.explorer.schema.table_details[0].clone();
                for index in 0..205 {
                    let mut table = template.clone();
                    table.name = format!("warehouse_archive_{index:03}");
                    table.foreign_keys.clear();
                    self.schema.explorer.schema.table_details.push(table);
                }
                self.schema.diagram.search = "customers".into();
                self.schema.diagram.show_all = false;
                self.schema.diagram.design.enabled = true;
                let names: Vec<_> = self
                    .schema
                    .explorer
                    .schema
                    .table_details
                    .iter()
                    .map(|table| format!("{}.{}", table.schema, table.name))
                    .collect();
                self.schema
                    .diagram
                    .design
                    .set_schema_fingerprint(crate::diagram::design_mode::schema_fingerprint_from_names(&names));
                self.schema.diagram.design.add_draft_table("public", "customer_notes");
                self.schema.diagram.new_schema = "public".into();
                self.schema.diagram.foreign_key_name = "customer_notes_customer_id_fkey".into();
                self.schema.diagram.foreign_key_from = "public.customer_notes.customer_id".into();
                self.schema.diagram.foreign_key_to = "public.customers.id".into();
            }
            "transfers" => {
                self.workspace.activity = Activity::Transfers;
                self.management.masking.masking_columns_csv = "email,phone".into();
                self.management.transfer.source_connection_id = "sample-store-dev".into();
                self.management.transfer.target_connection_id = "sample-store-test".into();
                self.management.transfer.copy_tables = "public.customers".into();
                self.management.transfer.copy_format = "csv".into();
                self.management.transfer.copy_conflict = "abort".into();
                self.management.transfer.capture_focus_once = true;
            }
            "security" => {
                self.workspace.activity = Activity::Security;
                self.management.security.security_users = vec![DatabaseUser {
                    name: "analytics_reader".into(),
                    is_super: false,
                    can_create_db: false,
                    can_create_role: false,
                    can_login: true,
                    is_predefined: false,
                }];
                self.management.security.security_selected_role = Some("analytics_reader".into());
                self.management.security.security_memberships = vec![RoleMembership {
                    role: "reporting".into(),
                    member: "analytics_reader".into(),
                    admin_option: false,
                }];
                self.management.security.security_privileges = vec![Privilege {
                    object_kind: PrivilegeObjectKind::Table,
                    schema: "public".into(),
                    object_name: "customer_order_summary".into(),
                    privilege_type: "SELECT".into(),
                }];
            }
            "monitoring" | "monitoring-workload" | "postgres-settings" => {
                self.workspace.activity = Activity::Monitor;
                let mut snapshot = monitoring_capture_snapshot();
                if surface != "monitoring" {
                    snapshot.sessions.clear();
                }
                if surface == "postgres-settings" {
                    snapshot.workload = None;
                    self.management.pg_settings.pg_settings = Some(PgSettingsSnapshot {
                        connection_id: "capture-conn".into(),
                        fetched_at_ms: 1_800_000_000_000,
                        message: "4 settings · presentation fixture".into(),
                        settings: vec![
                            capture_setting(
                                "work_mem",
                                "16384",
                                "user",
                                false,
                                false,
                                "Memory used by internal sort operations",
                            ),
                            capture_setting(
                                "statement_timeout",
                                "30000",
                                "user",
                                false,
                                false,
                                "Cancel statements exceeding this duration",
                            ),
                            capture_setting(
                                "shared_buffers",
                                "32768",
                                "postmaster",
                                true,
                                false,
                                "Shared memory buffers used by the server",
                            ),
                            capture_setting(
                                "ssl_passphrase_command",
                                "",
                                "sighup",
                                false,
                                true,
                                "Passphrase command is never displayed",
                            ),
                        ],
                    });
                    self.management.pg_settings.capture_focus_once = true;
                }
                self.management.monitoring.monitoring_filter_active_only = false;
                self.management.monitoring.capture_workload_focus_once = surface == "monitoring-workload";
                self.management.monitoring.monitoring_snapshot = Some(snapshot);
            }
            _ => unreachable!(),
        }
        self.preferences.dark_mode = !light;
        self.preferences.settings.appearance.dark_mode = !light;
        self.theme = if light { DbProTheme::light() } else { DbProTheme::dark() };
        true
    }
}

fn capture_setting(
    name: &str,
    value: &str,
    context: &str,
    pending_restart: bool,
    sensitive: bool,
    description: &str,
) -> PgSetting {
    PgSetting {
        name: name.into(),
        setting: value.into(),
        context: context.into(),
        category: "Resource Usage / Memory".into(),
        short_desc: Some(description.into()),
        source: "configuration file".into(),
        vartype: "string".into(),
        unit: None,
        min_val: None,
        max_val: None,
        enumvals: None,
        boot_val: None,
        reset_val: None,
        pending_restart,
        sensitive,
    }
}

fn monitoring_capture_snapshot() -> MonitoringSnapshot {
    MonitoringSnapshot {
        connection_id: "capture-conn".into(),
        driver: "postgresql".into(),
        sessions: (0..3)
            .map(|index| MonitorSession {
                backend_id: 4200 + index,
                database: Some("sample_store".into()),
                username: Some("app_reader".into()),
                application_name: Some("reporting-worker".into()),
                client_addr: Some("127.0.0.1".into()),
                state: Some("active".into()),
                wait_event_type: None,
                wait_event: None,
                query_text: Some("SELECT id, email FROM public.customers WHERE active = true;".into()),
                query_duration_ms: Some(125),
                backend_start: Some("2026-10-19 09:00:00+00".into()),
                xact_start: None,
                query_start: None,
                xact_age_ms: Some(125),
                backend_age_ms: Some(45_000),
                idle_in_transaction: false,
                is_current: index == 0,
            })
            .collect(),
        locks: Vec::new(),
        lock_error: None,
        relation_sizes: Vec::new(),
        server: None,
        local: None,
        workload: Some(StatStatementsSnapshot {
            extension_present: true,
            extension_version: Some("1.11".into()),
            message: "Normalized statements · presentation fixture".into(),
            statements: (0..3)
                .map(|index| StatStatement {
                    queryid: Some(100 + index),
                    userid: Some(10),
                    dbid: Some(20),
                    database: Some("sample_store".into()),
                    username: Some("app_reader".into()),
                    query: [
                        "SELECT id, email FROM public.customers WHERE active = $1",
                        "SELECT customer_id, SUM(total) FROM public.orders GROUP BY customer_id",
                        "SELECT * FROM public.products WHERE category_id = $1",
                    ][index as usize]
                        .into(),
                    calls: 1200,
                    total_time_ms: 680.0,
                    mean_time_ms: 0.57,
                    min_time_ms: 0.2,
                    max_time_ms: 2.1,
                    rows: 3600,
                    shared_blks_hit: 8400,
                    shared_blks_read: 120,
                    shared_blks_dirtied: 0,
                    shared_blks_written: 0,
                    local_blks_hit: 0,
                    local_blks_read: 0,
                    temp_blks_read: 0,
                    temp_blks_written: 0,
                })
                .collect(),
            sort: StatStatementSort::TotalTime,
            fetched_at_ms: 1_800_000_000_000,
        }),
        fetched_at_ms: 1_800_000_000_000,
        message: "Presentation fixture · no provider request".into(),
    }
}
