use super::*;

/// Query seeded into the editor and the first query tab at startup.
const DEFAULT_QUERY: &str = "select\n  id, first_name, last_name, hire_date\nfrom employees\nlimit 100;";

impl DbProApp {
    pub fn set_table_copy_connector(&mut self, connector: std::sync::Arc<dyn db_pro_core::application::TableCopyConnector>) {
        self.table_copy = Some(connector);
    }

    pub fn with_task_bridge(task_bridge: TaskBridge) -> Self {
        Self::with_task_bridge_and_storage(task_bridge, None)
    }

    pub fn with_task_bridge_and_storage(task_bridge: TaskBridge, storage: Option<&dyn eframe::Storage>) -> Self {
        let mut app = Self {
            task_bridge,
            ..Self::default()
        };
        if let Some(storage) = storage {
            app.load_saved_tasks_from_storage(storage);
            app.load_named_sessions_from_storage(storage);
            {
                let mut storage_context =
                    app_storage::NativeStorageContext::new(app_storage::NativeStorageDependencies {
                        preferences: &mut app.preferences,
                        agent: &mut app.agent,
                        theme: &mut app.theme,
                        workspace: &mut app.workspace,
                        query: &mut app.query,
                        table: &mut app.table,
                        connection: &mut app.connection,
                        schema_explorer: &mut app.schema.explorer,
                        diagram: &mut app.schema.diagram,
                    });
                storage_context.restore_preferences(storage);
                storage_context.restore_connection_profiles(storage);
                storage_context.restore_shell_layout(storage);
                storage_context.restore_table_layout(storage);
                storage_context.restore_query_state(storage);
                storage_context.restore_diagram_layouts(storage);
            }
            // Shell/layout restore after documents + pins so tab refs resolve (#222).
            app.restore_last_workspace_session_from_storage(storage);
            app_storage::NativeStorageContext::new(app_storage::NativeStorageDependencies {
                preferences: &mut app.preferences,
                agent: &mut app.agent,
                theme: &mut app.theme,
                workspace: &mut app.workspace,
                query: &mut app.query,
                table: &mut app.table,
                connection: &mut app.connection,
                schema_explorer: &mut app.schema.explorer,
                diagram: &mut app.schema.diagram,
            })
            .restore_workspace_files(storage);
        }
        app
    }
}

impl Default for DbProApp {
    fn default() -> Self {
        Self {
            theme: DbProTheme::default(),
            preferences: PreferencesState::default(),
            workspace: WorkspaceFeatureState::default(),
            query: QueryFeatureState {
                session: QuerySessionState {
                    documents: vec![QueryDocument::new("query-1", "Query 1", DEFAULT_QUERY)],
                    ..Default::default()
                },
                ..Default::default()
            },
            palette: PaletteState::default(),
            agent: AgentState::default(),
            task_bridge: TaskBridge::default(),
            feedback: FeedbackState::with_message("Ready"),
            table: TableEditorState {
                data: TableDataState::default(),
                data_query: TableDataQueryState::default(),
                editing: TableEditingState::default(),
                mutation: TableMutationState::default(),
                state: TableState {
                    table_dependency_filter: "all".to_owned(),
                    table_constraint_filter: "all".to_owned(),
                    ..Default::default()
                },
            },
            overlay: OverlayState::default(),
            connection: ConnectionFeatureState::default(),
            schema: SchemaWorkspaceState::default(),
            management: DatabaseManagementState::default(),
            saved_tasks: SavedTaskState::default(),
            table_copy: None,
            initial_frames_count: 0,
            gallery_state: ComponentGalleryState::default(),
            #[cfg(debug_assertions)]
            dev_tools: crate::dev_tools::DevToolsState::default(),
            capture_evidence_seeded: false,
        }
    }
}

#[cfg(test)]
#[path = "tests/app_state_tests.rs"]
mod tests;

