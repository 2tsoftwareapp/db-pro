//! Schema object workspace — views/triggers/routines (#192 routine workbench).

use super::schema_object_resolver::resolve_schema_object;
use super::*;
use db_pro_core::application::ObjectMutationService;
use db_pro_core::domain::object_mutation::{
    MutationOptions, ObjectAction, ObjectDefinition, ObjectMutationRequest, ObjectRef, RoutineDefinition,
};
use db_pro_core::ports::dialect::SqlDialect;

struct QuoteDialect;

impl SqlDialect for QuoteDialect {
    fn placeholder(&self, index: usize) -> String {
        format!("${index}")
    }

    fn quote_identifier(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
}

impl DbProApp {
    pub(super) fn draw_schema_object_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(selection) = self.schema.explorer.selected_schema_object.clone() else {
            self.activate_welcome_tab();
            return;
        };
        let is_view = matches!(selection, SchemaObjectSelection::View(_));
        let is_function = matches!(selection, SchemaObjectSelection::Function { .. });
        let Some(details) = resolve_schema_object(&self.schema.explorer.schema, &selection) else {
            return;
        };

        let surface_context = schema_object_surface_view::SchemaObjectSurfaceContext {
            theme: self.theme,
            icon: details.icon,
            kind: &details.kind,
            schema: &details.schema,
            name: &details.name,
            metadata: details.metadata.as_deref(),
            query: &details.query,
            is_view,
            active_view: self.schema.explorer.schema_object_view,
        };
        for action in surface_context.draw(ui) {
            self.apply_schema_object_surface_action(action);
        }
        ui.add_space(SPACE_MD);
        if is_function {
            self.draw_routine_workbench(ui, &selection);
        } else if is_view && self.schema.explorer.schema_object_view == SchemaObjectView::Data {
            if self.table.data_query.result.is_none()
                && self.table.data_query.request.is_none()
                && self.table.data_query.error.is_none()
            {
                self.request_table_data();
            }
            self.draw_table_data(ui, &details.name);
        } else {
            schema_object_surface_view::SchemaDefinitionContext {
                theme: self.theme,
                kind: &details.kind,
                definition: &details.definition,
                dialect: self.editor_dialect(),
            }
            .draw(ui);
        }
    }

    fn draw_routine_workbench(&mut self, ui: &mut egui::Ui, selection: &SchemaObjectSelection) {
        let SchemaObjectSelection::Function {
            name,
            identity_arguments,
        } = selection
        else {
            return;
        };
        let Some(function) = self
            .schema
            .explorer
            .schema
            .functions
            .iter()
            .find(|function| &function.name == name && &function.identity_arguments == identity_arguments)
            .cloned()
        else {
            return;
        };

        let input_count = function
            .parameters
            .iter()
            .filter(|parameter| {
                let mode = parameter.mode.to_ascii_uppercase();
                matches!(mode.as_str(), "IN" | "INOUT" | "VARIADIC" | "")
            })
            .count();
        if self.management.routine.routine_param_values.len() != input_count {
            self.management.routine.sync_from(&function);
        }
        let dialect = self.editor_dialect();
        let driver = self.active_driver().to_owned();
        // The debug-capability block goes FIRST: it is the status banner for this workbench,
        // and below the two editor cards it would never be seen at laptop window heights (#252).
        self.draw_routine_debug_status(ui);
        ui.add_space(SPACE_MD);
        let action = {
            let mut context = routine_workbench_surface_view::RoutineWorkbenchContext {
                theme: self.theme,
                function: &function,
                source_draft: &mut self.management.routine.routine_source_draft,
                ddl_preview: self.management.routine.routine_ddl_preview.as_deref(),
                drop_confirm: &mut self.management.routine.routine_drop_confirm,
                parameter_values: &mut self.management.routine.routine_param_values,
                parameter_nulls: &mut self.management.routine.routine_param_nulls,
                dialect,
                driver: &driver,
            };
            routine_workbench_surface_view::draw_workbench(&mut context, ui)
        };
        if let Some(action) = action {
            self.apply_routine_workbench_action(&function, action);
        }
    }

    /// Draws the PL/pgSQL debug-capability status block above the workbench cards (#252).
    ///
    /// The block is probe-driven: opening the workbench on a live connection fires one
    /// catalog read, and the block renders exactly what that read measured. DB Pro never
    /// installs server extensions, so the guidance states the manual DBA path instead of
    /// offering a one-click fix.
    fn draw_routine_debug_status(&mut self, ui: &mut egui::Ui) {
        let active_connection_id = self.active_database_connection_id();
        if let Some(connection_id) = active_connection_id.as_deref() {
            if self.management.routine.needs_debug_probe(connection_id) {
                self.request_routine_debug_probe(connection_id);
            }
        }
        let stale_probe = active_connection_id.is_some()
            && self.management.routine.routine_debug_probe_connection.as_deref() != active_connection_id.as_deref();
        let probe = if stale_probe { None } else { self.management.routine.routine_debug_probe.as_ref() };
        let probing = self.management.routine.routine_debug_probe_request.is_some();
        let environment_label = self
            .active_connection()
            .filter(|connection| connection.environment == db_pro_core::domain::connection::ConnectionEnvironment::Production)
            .map(|connection| connection.environment.as_label());
        let guidance = routine_debug_status_view::build_debug_guidance(probe, probing, environment_label);
        let mut recheck = false;
        {
            let mut context = routine_debug_status_view::RoutineDebugStatusContext {
                theme: self.theme,
                guidance,
                state: probe.map(|probe| probe.state),
                probing,
                can_recheck: active_connection_id.is_some(),
                recheck_slot: &mut recheck,
            };
            routine_debug_status_view::draw_debug_status(&mut context, ui);
        }
        if recheck {
            if let Some(connection_id) = active_connection_id.as_deref() {
                self.request_routine_debug_probe(connection_id);
            }
        }
    }

    /// Sends one catalog-only debug-capability probe for `connection_id` and records the
    /// in-flight request so the reply can be authenticated (#252).
    fn request_routine_debug_probe(&mut self, connection_id: &str) {
        let request_id = self.next_request_id();
        let command = UiCommand::ProbeRoutineDebug {
            request_id,
            connection_id: connection_id.to_owned(),
        };
        if self.dispatch_command(command) {
            self.management.routine.begin_debug_probe(request_id.0, connection_id);
        }
    }

    fn apply_routine_workbench_action(
        &mut self,
        function: &UiFunctionSummary,
        action: routine_workbench_surface_view::RoutineWorkbenchAction,
    ) {
        use routine_workbench_surface_view::RoutineWorkbenchAction as Action;
        match action {
            Action::PreviewDdl => self.preview_routine_mutation(function, ObjectAction::Alter),
            Action::ApplyDdl => {
                self.preview_routine_mutation(function, ObjectAction::Alter);
                if let Some(sql) = self.management.routine.routine_ddl_preview.clone() {
                    self.execute_routine_query(sql, Vec::new());
                }
            }
            Action::RequestDrop => self.management.routine.routine_drop_confirm = true,
            Action::ConfirmDrop => {
                self.preview_routine_mutation(function, ObjectAction::Drop);
                if let Some(sql) = self.management.routine.routine_ddl_preview.clone() {
                    self.execute_routine_query(sql, Vec::new());
                }
                self.management.routine.routine_drop_confirm = false;
            }
            Action::CancelDrop => self.management.routine.routine_drop_confirm = false,
            Action::Execute { sql, arguments } => match arguments {
                // Typed arguments ride the existing `RunQuery` command chain;
                // the SQL text carries only placeholders (#252).
                Ok(params) => self.execute_routine_query(sql, params),
                Err(reason) => {
                    self.feedback.runtime_message = format!("Routine arguments invalid — nothing was sent: {reason}")
                }
            },
            Action::OpenInQuery(sql) => self.open_routine_query(sql),
        }
    }

    fn open_routine_query(&mut self, sql: String) {
        self.open_query_document_with(sql, None);
    }

    /// Dispatch the workbench's generated invocation. With bound arguments the
    /// run skips editor parameter discovery entirely — the typed `QueryParam`
    /// vector is the binding (#252). Without them (DDL previews, zero-arg
    /// routines) the plain editor path is preserved.
    fn execute_routine_query(&mut self, sql: String, params: Vec<db_pro_core::domain::query::QueryParam>) {
        self.open_routine_query(sql);
        if params.is_empty() {
            self.dispatch_query();
        } else {
            self.dispatch_bound_query(params);
        }
    }

    fn preview_routine_mutation(&mut self, function: &UiFunctionSummary, action: ObjectAction) {
        let definition = RoutineDefinition {
            schema: function.schema.clone(),
            name: function.name.clone(),
            routine_type: function.routine_type.clone(),
            identity_arguments: function.identity_arguments.clone(),
            definition_sql: self.management.routine.routine_source_draft.clone(),
            replace: true,
        };
        let request = ObjectMutationRequest {
            driver: self.active_driver().to_owned(),
            action,
            target: Some(ObjectRef {
                kind: db_pro_core::domain::object_mutation::ObjectKind::Routine,
                schema: Some(function.schema.clone()),
                name: function.name.clone(),
                parent: None,
            }),
            definition: ObjectDefinition::Routine(definition),
            options: MutationOptions {
                cascade: false,
                if_exists: true,
                if_not_exists: false,
                dry_run: action == ObjectAction::GenerateDdl,
            },
        };
        match ObjectMutationService::plan(&request, &QuoteDialect) {
            Ok(plan) => {
                self.management.routine.routine_ddl_preview = Some(plan.statements.join(";\n"));
                self.feedback.runtime_message = format!(
                    "Routine DDL preview · {} statement(s) · {}",
                    plan.statements.len(),
                    plan.safety
                );
            }
            Err(error) => {
                self.management.routine.routine_ddl_preview = None;
                self.feedback.runtime_message = format!("Routine plan failed: {error}");
            }
        }
    }

    fn apply_schema_object_surface_action(&mut self, action: schema_object_surface_view::SchemaObjectSurfaceAction) {
        match action {
            schema_object_surface_view::SchemaObjectSurfaceAction::OpenQuery(query) => {
                self.open_query_document_with(query, None);
            }
            schema_object_surface_view::SchemaObjectSurfaceAction::SelectView(view) => {
                self.schema.explorer.schema_object_view = view;
                if view == SchemaObjectView::Data {
                    self.table.data_query.clear_result();
                    self.table.data_query.total_rows = None;
                    self.table.data_query.error = None;
                    self.table.data_query.offset = 0;
                    self.request_table_data();
                }
            }
        }
    }
}
