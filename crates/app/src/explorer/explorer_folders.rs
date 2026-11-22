//! Category folders of the Codex navigator: Views, Functions and Triggers.

use super::explorer_schema_object_folders_view::SchemaObjectFolderAction;
use super::schema_workbench::SchemaWorkbenchMode;
use super::*;
use db_pro_core::domain::object_mutation::ObjectAction;

impl DbProApp {
    pub(super) fn apply_schema_object_folder_action(&mut self, action: SchemaObjectFolderAction, ui: &mut egui::Ui) {
        match action {
            SchemaObjectFolderAction::Open(request) => {
                self.open_schema_object(request.selection, &request.schema, &request.name, &request.kind);
            }
            SchemaObjectFolderAction::OpenQuery(query) => {
                self.open_query_document_with(query, None);
            }
            SchemaObjectFolderAction::OpenSelectTop(from) => {
                // Dialect-correct row limit: SQL Server rejects `LIMIT` (#260).
                let query = result_grid_export::select_top_snippet(self.active_driver(), &from, 100);
                self.open_query_document_with(query, None);
            }
            SchemaObjectFolderAction::ModifyView(view) => {
                self.schema.workbench.mode = SchemaWorkbenchMode::View;
                self.schema.workbench.schema = view.schema.clone();
                self.schema.workbench.name = view.name.clone();
                self.schema.workbench.select_sql = view.definition.clone();
                self.schema.workbench.materialized = false;
                self.workspace.active_tab = WorkspaceTab::SchemaWorkbench;
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                self.feedback.set_runtime_message(format!("Loaded view `{}.{}` into workbench", view.schema, view.name));
            }
            SchemaObjectFolderAction::DropObject { schema, name, kind } => {
                let mode = match kind.to_ascii_uppercase().as_str() {
                    "VIEW" => SchemaWorkbenchMode::View,
                    "TRIGGER" => SchemaWorkbenchMode::Trigger,
                    _ => SchemaWorkbenchMode::Table,
                };
                self.schema.workbench.mode = mode;
                self.schema.workbench.schema = schema.clone();
                self.schema.workbench.name = name.clone();
                self.schema.workbench.parent_table = name.clone();
                self.plan_workbench_action(ObjectAction::Drop);
                self.schema.workbench.apply_confirmation = true;
                self.workspace.active_tab = WorkspaceTab::SchemaWorkbench;
                self.feedback.set_runtime_message(format!("Planned drop for {kind} `{schema}.{name}`"));
            }
            SchemaObjectFolderAction::ToggleTrigger {
                schema,
                table,
                name,
                enable,
            } => self.plan_trigger_toggle(&schema, &table, &name, enable),
            SchemaObjectFolderAction::CopyName(name) => {
                ui.output_mut(|output| output.commands.push(egui::OutputCommand::CopyText(name.clone())));
                self.feedback.runtime_message = t!("explorer.copy_to_clipboard", name = name).to_string();
            }
            SchemaObjectFolderAction::RefreshSchema => {
                if let Some(connection_id) = self.connection.lifecycle.active_connection_id().map(str::to_owned) {
                    self.request_schema_introspection(connection_id, true);
                }
            }
        }
    }

    fn plan_trigger_toggle(&mut self, schema: &str, table: &str, name: &str, enable: bool) {
        use db_pro_core::domain::capabilities::CapabilityFeature;
        use db_pro_core::domain::object_mutation::{
            MutationOptions, ObjectDefinition, ObjectKind, ObjectMutationRequest, ObjectRef, TriggerDefinition,
        };

        if let Some(reason) = self
            .query_capabilities()
            .feature_limitation(CapabilityFeature::TriggerEnableDisable)
        {
            self.feedback.runtime_message = format!("Trigger enable/disable is unavailable: {reason}");
            return;
        }
        let action = if enable { ObjectAction::Enable } else { ObjectAction::Disable };
        let request = ObjectMutationRequest {
            driver: self.active_query_driver().to_owned(),
            action,
            target: Some(ObjectRef {
                kind: ObjectKind::Trigger,
                schema: Some(schema.to_owned()),
                name: name.to_owned(),
                parent: Some(table.to_owned()),
            }),
            definition: ObjectDefinition::Trigger(TriggerDefinition {
                schema: schema.to_owned(),
                table: table.to_owned(),
                name: name.to_owned(),
                timing: String::new(),
                event: String::new(),
                body: String::new(),
            }),
            options: MutationOptions::default(),
        };
        self.schema.workbench.mode = SchemaWorkbenchMode::Trigger;
        self.schema.workbench.schema = schema.to_owned();
        self.schema.workbench.name = name.to_owned();
        self.schema.workbench.parent_table = table.to_owned();
        self.schema.workbench.apply_confirmation = false;
        self.run_plan(request);
        self.workspace.activity = Activity::Schema;
        self.workspace.active_tab = WorkspaceTab::SchemaWorkbench;
        self.workspace.sidebar_open = true;
        self.feedback.runtime_message = format!(
            "Planned {} trigger `{schema}.{name}`",
            if enable { "enable" } else { "disable" }
        );
    }

    /// Activates a schema object (view / function / trigger) in the workspace.
    /// `schema` is empty for objects that are not schema-qualified (triggers).
    pub(super) fn open_schema_object(
        &mut self,
        selection: SchemaObjectSelection,
        schema: &str,
        name: &str,
        kind: &str,
    ) {
        explorer_navigation::SchemaObjectActivationContext::new(
            &mut self.schema.explorer,
            &mut self.table,
            &mut self.workspace,
            &mut self.management.routine,
            &mut self.feedback,
        )
        .open(explorer_navigation::SchemaObjectActivation {
            selection,
            schema: schema.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
        });
    }
}
