#[path = "../agent/agent_actions.rs"]
mod agent_actions;
#[path = "../agent/agent_confirmation.rs"]
mod agent_confirmation;
#[path = "../agent/agent_context.rs"]
mod agent_context;
#[path = "../agent/agent_context_actions_view.rs"]
mod agent_context_actions_view;
#[path = "../agent/agent_events.rs"]
mod agent_events;
#[path = "../agent/agent_header_view.rs"]
mod agent_header_view;
#[path = "../agent/agent_patch.rs"]
mod agent_patch;
#[path = "../agent/agent_result_projection.rs"]
mod agent_result_projection;
#[path = "../agent/agent_settings_view.rs"]
mod agent_settings_view;
#[path = "../agent/agent_state.rs"]
mod agent_state;
#[path = "../agent/agent_surface_view.rs"]
mod agent_surface_view;
#[path = "../agent/agent_thread_surface_view.rs"]
mod agent_thread_surface_view;
#[path = "../agent/agent_view.rs"]
mod agent_view;
#[path = "../agent/agent_workflow_reducer.rs"]
mod agent_workflow_reducer;
#[path = "../agent/agent_workflow_state.rs"]
mod agent_workflow_state;
#[path = "app_state.rs"]
mod app_state;
#[path = "app_types.rs"]
mod app_types;
#[path = "../tools/audit_state.rs"]
mod audit_state;
#[path = "../table/cell_inspector.rs"]
mod cell_inspector;
#[path = "../table/change_set.rs"]
mod change_set;
#[path = "command_dispatch.rs"]
mod command_dispatch;
#[path = "../component_gallery/component_gallery_agent.rs"]
mod component_gallery_agent;
#[path = "../component_gallery/component_gallery_feedback.rs"]
mod component_gallery_feedback;
#[path = "../component_gallery/component_gallery_inputs.rs"]
mod component_gallery_inputs;
#[path = "../component_gallery/component_gallery_overlays.rs"]
mod component_gallery_overlays;
#[path = "../component_gallery/component_gallery_rendering.rs"]
mod component_gallery_rendering;
#[path = "../component_gallery/component_gallery_surfaces.rs"]
mod component_gallery_surfaces;
#[path = "../component_gallery/component_gallery_view.rs"]
mod component_gallery_view;
#[path = "../connection/mod.rs"]
pub mod connection;
#[path = "../workspace/data_cleanup_capture.rs"]
mod data_cleanup_capture;
#[path = "../workspace/workstation_cleanup_capture.rs"]
mod workstation_cleanup_capture;

pub use component_gallery_view::ComponentGalleryState;
#[path = "../tools/audit_activity_view.rs"]
mod audit_activity_view;
#[path = "capability_lookup.rs"]
mod capability_lookup;
#[path = "../schema/database_management_state.rs"]
mod database_management_state;
#[path = "../table/ddl_events.rs"]
mod ddl_events;
#[path = "../diagram_surface/diagram_canvas_view.rs"]
mod diagram_canvas_view;
#[path = "../diagram_surface/diagram_design_actions.rs"]
mod diagram_design_actions;
#[path = "../diagram_surface/diagram_design_panel_view.rs"]
mod diagram_design_panel_view;
#[path = "../diagram_surface/diagram_state.rs"]
mod diagram_state;
#[path = "../diagram_surface/diagram_view.rs"]
mod diagram_view;
#[path = "../runtime/event_router.rs"]
mod event_router;
#[path = "../tools/event_trigger_activity_view.rs"]
mod event_trigger_activity_view;
#[path = "../tools/event_trigger_state.rs"]
mod event_trigger_state;
#[path = "../runtime/events.rs"]
mod events;
#[path = "../query_workbench/events_query_dispatch.rs"]
mod events_query_dispatch;
#[path = "../explorer/explorer_connection_node_view.rs"]
mod explorer_connection_node_view;
#[path = "../explorer/explorer_connection_row_view.rs"]
mod explorer_connection_row_view;
#[path = "../explorer/explorer_connections.rs"]
mod explorer_connections;
#[path = "../explorer/explorer_database_node_view.rs"]
mod explorer_database_node_view;
#[path = "../explorer/explorer_details.rs"]
mod explorer_details;
#[path = "../explorer/explorer_folders.rs"]
mod explorer_folders;
#[path = "../explorer/explorer_navigation.rs"]
mod explorer_navigation;
#[path = "../explorer/explorer_schema_feedback_view.rs"]
mod explorer_schema_feedback_view;
#[path = "../explorer/explorer_schema_node_view.rs"]
mod explorer_schema_node_view;
#[path = "../explorer/explorer_schema_object_folders_view.rs"]
mod explorer_schema_object_folders_view;
#[path = "../explorer/explorer_schema_object_row_view.rs"]
mod explorer_schema_object_row_view;
#[path = "../explorer/explorer_schema_objects_view.rs"]
mod explorer_schema_objects_view;
#[path = "../explorer/explorer_schema_tree_view.rs"]
mod explorer_schema_tree_view;
#[path = "../explorer/explorer_surface_view.rs"]
mod explorer_surface_view;
#[path = "../explorer/explorer_table_details_view.rs"]
mod explorer_table_details_view;
#[path = "../explorer/explorer_table_folder_view.rs"]
mod explorer_table_folder_view;
#[path = "../explorer/explorer_table_row_view.rs"]
mod explorer_table_row_view;
#[path = "../explorer/explorer_toolbar_view.rs"]
mod explorer_toolbar_view;
#[path = "../explorer/explorer_tree.rs"]
mod explorer_tree;
#[path = "../explorer/explorer_view.rs"]
mod explorer_view;
#[path = "../tools/fdw_activity_view.rs"]
mod fdw_activity_view;
#[path = "../tools/fdw_state.rs"]
mod fdw_state;
#[path = "../shell/feedback_state.rs"]
mod feedback_state;
#[path = "../files/file_picker_events.rs"]
mod file_picker_events;
#[path = "../files/files_activity_tabs.rs"]
mod files_activity_tabs;
#[path = "../files/files_activity_view.rs"]
mod files_activity_view;
#[path = "../files/files_agent_context_view.rs"]
mod files_agent_context_view;
#[path = "../files/files_git_view.rs"]
mod files_git_view;
#[path = "../files/files_search_view.rs"]
mod files_search_view;
#[path = "../files/files_tasks_view.rs"]
mod files_tasks_view;
#[path = "../files/files_tree_view.rs"]
mod files_tree_view;
#[path = "../files/git_workspace.rs"]
mod git_workspace;
#[path = "../files/ide_workspace.rs"]
mod ide_workspace;
#[path = "../files/ide_workspace_scan.rs"]
mod ide_workspace_scan;
#[path = "../files/ide_workspace_types.rs"]
mod ide_workspace_types;
#[path = "../tools/maintenance_activity_view.rs"]
mod maintenance_activity_view;
#[path = "../tools/management_events.rs"]
mod management_events;
#[path = "../tools/masking.rs"]
mod masking;
#[path = "../tools/masking_state.rs"]
mod masking_state;
#[path = "../monitoring/monitoring_activity_view.rs"]
mod monitoring_activity_view;
#[path = "../monitoring/monitoring_confirmation_view.rs"]
mod monitoring_confirmation_view;
#[path = "../monitoring/monitoring_header_view.rs"]
mod monitoring_header_view;
#[path = "../monitoring/monitoring_sessions_view.rs"]
mod monitoring_sessions_view;
#[path = "../monitoring/monitoring_snapshot_view.rs"]
mod monitoring_snapshot_view;
#[path = "../monitoring/monitoring_state.rs"]
mod monitoring_state;
#[path = "../monitoring/monitoring_surface_view.rs"]
mod monitoring_surface_view;
#[path = "../monitoring/monitoring_workload_view.rs"]
mod monitoring_workload_view;
#[path = "../shell/navigation_view.rs"]
mod navigation_view;
#[path = "../runtime/operation_events.rs"]
mod operation_events;
#[path = "../shell/overlay_state.rs"]
mod overlay_state;
#[path = "../palette/palette_actions.rs"]
mod palette_actions;
#[path = "../palette/palette_catalog.rs"]
mod palette_catalog;
#[path = "../palette/palette_state.rs"]
mod palette_state;
#[path = "../tools/pg_settings_activity_view.rs"]
mod pg_settings_activity_view;
#[path = "../tools/pg_settings_state.rs"]
mod pg_settings_state;
#[path = "../settings/preferences_state.rs"]
mod preferences_state;
#[path = "../query_workbench/query_execution_actions.rs"]
mod query_execution_actions;
#[path = "../query_workbench/query_execution_events.rs"]
mod query_execution_events;
#[path = "../query_workbench/query_execution_state.rs"]
mod query_execution_state;
#[path = "../query_workbench/query_explain_actions.rs"]
mod query_explain_actions;
#[path = "../query_workbench/query_failure_events.rs"]
mod query_failure_events;
#[path = "../query_workbench/query_history_events.rs"]
mod query_history_events;
#[path = "../query_workbench/query_library_events.rs"]
mod query_library_events;
#[path = "../query_workbench/query_library_state.rs"]
mod query_library_state;
#[path = "../query_workbench/query_multi_result_events.rs"]
mod query_multi_result_events;
#[path = "../query_workbench/query_prediction_events.rs"]
mod query_prediction_events;
#[path = "../query_workbench/query_queue_events.rs"]
mod query_queue_events;
#[path = "../query_workbench/query_result_events.rs"]
mod query_result_events;
#[path = "../query_workbench/query_save_actions.rs"]
mod query_save_actions;
#[path = "../query_workbench/query_save_commands.rs"]
mod query_save_commands;
#[path = "../query_workbench/query_save_events.rs"]
mod query_save_events;
#[path = "../tools/replication_activity_view.rs"]
mod replication_activity_view;
#[path = "../tools/replication_state.rs"]
mod replication_state;
#[path = "../schema/routine_debug_status_view.rs"]
mod routine_debug_status_view;
#[path = "../schema/routine_state.rs"]
mod routine_state;
#[path = "../schema/routine_workbench_surface_view.rs"]
mod routine_workbench_surface_view;
#[path = "../tools/saved_task_sql.rs"]
mod saved_task_sql;
#[path = "../tools/saved_task_state.rs"]
mod saved_task_state;
#[path = "../security/security_state.rs"]
mod security_state;
#[path = "../settings/settings_appearance_view.rs"]
mod settings_appearance_view;
#[path = "../settings/settings_backup_view.rs"]
mod settings_backup_view;
#[path = "../settings/settings_diagnostics_view.rs"]
mod settings_diagnostics_view;
#[path = "../settings/settings_editor_view.rs"]
mod settings_editor_view;
#[path = "../settings/settings_general_view.rs"]
mod settings_general_view;
#[path = "../settings/settings_keybindings_view.rs"]
mod settings_keybindings_view;
#[path = "../settings/settings_model.rs"]
mod settings_model;
#[path = "../settings/settings_navigation_view.rs"]
mod settings_navigation_view;
#[path = "../settings/settings_surface_view.rs"]
mod settings_surface_view;
#[path = "../settings/settings_system_view.rs"]
mod settings_system_view;
#[path = "../settings/settings_view.rs"]
mod settings_view;
#[path = "../tools/synthetic_data_state.rs"]
mod synthetic_data_state;
#[path = "../tools/transfer_activity_surface_view.rs"]
mod transfer_activity_surface_view;
#[path = "../tools/transfer_harness_view.rs"]
mod transfer_harness_view;
#[path = "../tools/transfer_state.rs"]
mod transfer_state;
use audit_state::AuditState;
pub(crate) use capability_lookup::CapabilityLookup;
use database_management_state::DatabaseManagementState;
pub(crate) use diagram_state::{DiagramState, ErMatchCountCache};
use event_trigger_state::EventTriggerState;
use fdw_state::FdwState;
pub(crate) use feedback_state::FeedbackState;
use masking_state::MaskingState;
use monitoring_state::MonitoringState;
pub(crate) use overlay_state::OverlayState;
pub(crate) use palette_state::PaletteState;
use pg_settings_state::PgSettingsState;
pub(crate) use preferences_state::PreferencesState;
pub(crate) use query_execution_state::{PendingDestructiveRunOrigin, QueryExecutionPolicyState};
pub(crate) use query_library_state::QueryLibraryState;
use replication_state::ReplicationState;
use routine_state::RoutineState;
pub(crate) use saved_task_state::SavedTaskState;
use security_state::SecurityState;
pub(crate) use settings_model::{AppSettings, SettingsSection, SqlLintSettings, SETTINGS_STORAGE_KEY};
use synthetic_data_state::SyntheticDataState;
use transfer_state::TransferState;
pub(crate) use workspace_shell::WorkspaceShellState;
#[path = "../shell/activity_bar_view.rs"]
mod activity_bar_view;
#[path = "../connection/connection_events.rs"]
mod connection_events;
#[path = "../connection/connection_status.rs"]
mod connection_status;
#[path = "../palette/palette_search_view.rs"]
mod palette_search_view;
#[path = "../palette/palette_surface_view.rs"]
mod palette_surface_view;
#[path = "../palette/palette_view.rs"]
mod palette_view;
#[path = "../tools/search_service.rs"]
mod search_service;
pub(crate) use search_service::{SearchFingerprintParts, SearchIndex, SearchService};
#[path = "../shell/sidebar/problems_view.rs"]
mod problems_view;
#[path = "../query_workbench/query_actions_view.rs"]
mod query_actions_view;
#[path = "../query_workbench/query_completion_popup_view.rs"]
mod query_completion_popup_view;
#[path = "../query_workbench/query_context_picker_view.rs"]
mod query_context_picker_view;
#[path = "../query_workbench/query_context_view.rs"]
mod query_context_view;
#[path = "../query_workbench/query_diagnostics_view.rs"]
mod query_diagnostics_view;
#[path = "../query_workbench/query_dialog_surface_view.rs"]
mod query_dialog_surface_view;
#[path = "../query_workbench/query_dialogs_view.rs"]
mod query_dialogs_view;
#[path = "../query_workbench/query_documents.rs"]
mod query_documents;
#[path = "../query_workbench/query_editor_panel.rs"]
mod query_editor_panel;
#[path = "../query_workbench/query_editor_state.rs"]
mod query_editor_state;
#[path = "../query_workbench/query_editor_support.rs"]
mod query_editor_support;
#[path = "../query_workbench/query_editor_surface_view.rs"]
mod query_editor_surface_view;
#[path = "../query_workbench/query_feature_state.rs"]
mod query_feature_state;
#[path = "../query_workbench/query_folder_delete_dialog.rs"]
mod query_folder_delete_dialog;
#[path = "../query_workbench/query_output_actions_view.rs"]
mod query_output_actions_view;
#[path = "../query_workbench/query_output_dock_surface_view.rs"]
mod query_output_dock_surface_view;
#[path = "../query_workbench/query_output_panes_view.rs"]
mod query_output_panes_view;
#[path = "../query_workbench/query_output_state.rs"]
mod query_output_state;
#[path = "../query_workbench/query_output_tabs_view.rs"]
mod query_output_tabs_view;
#[path = "../query_workbench/query_output_view.rs"]
mod query_output_view;
#[path = "../query_workbench/query_parameters_view.rs"]
mod query_parameters_view;
#[path = "../query_workbench/query_results_surface_view.rs"]
mod query_results_surface_view;
#[path = "../query_workbench/query_row_limit.rs"]
mod query_row_limit;
#[path = "../query_workbench/query_run_control_view.rs"]
mod query_run_control_view;
#[path = "../query_workbench/query_save_dialog_surface_view.rs"]
mod query_save_dialog_surface_view;
#[path = "../query_workbench/query_search_view.rs"]
mod query_search_view;
#[path = "../query_workbench/query_session.rs"]
mod query_session;
#[path = "../query_workbench/query_shell_surface_view.rs"]
mod query_shell_surface_view;
#[path = "../query_workbench/query_snippets.rs"]
mod query_snippets;
#[path = "../query_workbench/query_state.rs"]
mod query_state;
#[path = "../query_workbench/query_status_bar_surface_view.rs"]
mod query_status_bar_surface_view;
#[path = "../query_workbench/query_transaction_surface_view.rs"]
mod query_transaction_surface_view;
#[path = "../query_workbench/query_view.rs"]
mod query_view;
#[path = "../result_grid/result_grid_cell.rs"]
mod result_grid_cell;
#[path = "../result_grid/result_grid_cell_menu_view.rs"]
mod result_grid_cell_menu_view;
#[path = "../result_grid/result_grid_cell_surface_view.rs"]
mod result_grid_cell_surface_view;
#[path = "../result_grid/result_grid_clipboard.rs"]
mod result_grid_clipboard;
#[path = "../result_grid/result_grid_edit.rs"]
mod result_grid_edit;
#[path = "../result_grid/result_grid_export.rs"]
pub(crate) mod result_grid_export;
#[path = "../result_grid/result_grid_header.rs"]
mod result_grid_header;
#[path = "../result_grid/result_grid_header_content_view.rs"]
mod result_grid_header_content_view;
#[path = "../result_grid/result_grid_header_menu_view.rs"]
mod result_grid_header_menu_view;
#[path = "../result_grid/result_grid_header_surface_view.rs"]
mod result_grid_header_surface_view;
#[path = "../result_grid/result_grid_interaction_surface_view.rs"]
mod result_grid_interaction_surface_view;
#[path = "../result_grid/result_grid_keyboard_view.rs"]
mod result_grid_keyboard_view;
#[path = "../result_grid/result_grid_projection.rs"]
mod result_grid_projection;
#[path = "../result_grid/result_grid_row_gutter_view.rs"]
mod result_grid_row_gutter_view;
#[path = "../result_grid/result_grid_row_view.rs"]
mod result_grid_row_view;
#[path = "../result_grid/result_grid_selection.rs"]
mod result_grid_selection;
#[path = "../result_grid/result_grid_toolbar_view.rs"]
mod result_grid_toolbar_view;
#[path = "../result_grid/result_grid_view.rs"]
pub(crate) mod result_grid_view;
#[path = "../runtime/runtime_event_handlers.rs"]
mod runtime_event_handlers;
#[path = "../shell/sidebar/sidebar_activities_view.rs"]
mod sidebar_activities_view;
#[path = "../shell/sidebar/sidebar_chrome_view.rs"]
mod sidebar_chrome_view;
#[path = "../shell/sidebar/constants.rs"]
mod sidebar_constants;
#[path = "../shell/sidebar/sidebar_data_view.rs"]
mod sidebar_data_view;
#[path = "../shell/sidebar/sidebar_history_view.rs"]
mod sidebar_history_view;
#[path = "../shell/sidebar/sidebar_problems_view.rs"]
mod sidebar_problems_view;
#[path = "../shell/sidebar/sidebar_queries_surface_view.rs"]
mod sidebar_queries_surface_view;
#[path = "../shell/sidebar/sidebar_queries_view.rs"]
mod sidebar_queries_view;
#[path = "../shell/sidebar/sidebar_query_library_view.rs"]
mod sidebar_query_library_view;
#[path = "../shell/sidebar/sidebar_query_shortcuts_view.rs"]
mod sidebar_query_shortcuts_view;
#[path = "../shell/sidebar/sidebar_view.rs"]
mod sidebar_view;
#[path = "../tools/synthetic_data.rs"]
mod synthetic_data;
#[path = "../table/table_conflict_dialog_surface.rs"]
mod table_conflict_dialog_surface;
#[path = "../table/table_data_filter_view.rs"]
mod table_data_filter_view;
#[path = "../table/table_data_mutation_toolbar_view.rs"]
mod table_data_mutation_toolbar_view;
#[path = "../table/table_data_pagination_view.rs"]
mod table_data_pagination_view;
#[path = "../table/table_data_placeholder_view.rs"]
mod table_data_placeholder_view;
#[path = "../table/table_data_query_state.rs"]
mod table_data_query_state;
#[path = "../table/table_data_state.rs"]
mod table_data_state;
#[path = "../table/table_data_surface_view.rs"]
mod table_data_surface_view;
#[path = "../table/table_data_toolbar_surface_view.rs"]
mod table_data_toolbar_surface_view;
#[path = "../table/table_data_view.rs"]
mod table_data_view;
#[path = "../table/table_editing_state.rs"]
mod table_editing_state;
#[path = "../table/table_editor_state.rs"]
mod table_editor_state;
#[path = "../table/table_events.rs"]
mod table_events;
#[path = "../table/table_fk_lookup.rs"]
mod table_fk_lookup;
#[path = "../table/table_mutation_actions.rs"]
mod table_mutation_actions;
#[path = "../table/table_mutation_dialog_surface.rs"]
mod table_mutation_dialog_surface;
#[path = "../table/table_mutation_dialogs_view.rs"]
mod table_mutation_dialogs_view;
#[path = "../table/table_mutation_state.rs"]
mod table_mutation_state;
#[path = "../table/table_state.rs"]
mod table_state;
#[path = "../tools/tasks_view.rs"]
mod tasks_view;
#[path = "../tools/tools_hub_surface_view.rs"]
mod tools_hub_surface_view;
#[path = "../visual_query_builder/visual_query_builder_state.rs"]
mod visual_query_builder_state;
#[path = "../visual_query_builder/visual_query_builder_view.rs"]
mod visual_query_builder_view;
#[path = "../shell/welcome/welcome_surface_view.rs"]
mod welcome_surface_view;
#[path = "../workspace/workspace_actions.rs"]
mod workspace_actions;
#[path = "../workspace/workspace_files_state.rs"]
mod workspace_files_state;
#[path = "../workspace/workspace_session.rs"]
mod workspace_session;
#[path = "../workspace/workspace_session_state.rs"]
mod workspace_session_state;
#[path = "../workspace/workspace_shell.rs"]
mod workspace_shell;
pub(crate) use agent_state::AgentState;
pub(crate) use query_editor_state::QueryEditorState;
pub(crate) use query_feature_state::QueryFeatureState;
pub(crate) use query_output_state::QueryOutputState;
pub(crate) use query_state::QuerySessionState;
pub(crate) use result_grid_projection::GridSelectionCache;
use schema_compare_state::SchemaCompareState;
pub(crate) use schema_explorer_state::{ExplorerMatchMode, ExplorerObjectFilter, SchemaExplorerState};
use schema_workspace_state::SchemaWorkspaceState;
pub(crate) use table_data_query_state::TableDataQueryState;
pub(crate) use table_data_state::TableDataState;
pub(crate) use table_editing_state::TableEditingState;
pub(crate) use table_editor_state::TableEditorState;
pub(crate) use table_mutation_actions::StagedApplyFailure;
pub(crate) use table_mutation_state::TableMutationState;
pub(crate) use table_state::TableState;
pub(crate) use workspace_feature_state::WorkspaceFeatureState;
pub(crate) use workspace_files_state::WorkspaceFilesState;
pub(crate) use workspace_session_state::WorkspaceSessionState;
#[path = "../tools/saved_tasks_surface_view.rs"]
mod saved_tasks_surface_view;
#[path = "../schema/schema_actions.rs"]
mod schema_actions;
#[path = "../schema/schema_compare.rs"]
pub(crate) mod schema_compare;
#[path = "../schema/schema_compare_state.rs"]
mod schema_compare_state;
#[path = "../schema/schema_compare_view.rs"]
mod schema_compare_view;
#[path = "../schema/schema_events.rs"]
mod schema_events;
#[path = "../explorer/schema_explorer_state.rs"]
mod schema_explorer_state;
#[path = "../schema/schema_object_resolver.rs"]
mod schema_object_resolver;
#[path = "../schema/schema_object_surface_view.rs"]
mod schema_object_surface_view;
#[path = "../schema/schema_object_view.rs"]
mod schema_object_view;
#[path = "../schema/schema_workbench.rs"]
mod schema_workbench;
#[path = "../schema/schema_workbench_actions.rs"]
mod schema_workbench_actions;
#[path = "../schema/schema_workbench_form.rs"]
mod schema_workbench_form;
#[path = "../schema/schema_workbench_mutation.rs"]
mod schema_workbench_mutation;
#[path = "../schema/schema_workbench_secondary_view.rs"]
mod schema_workbench_secondary_view;
#[path = "../schema/schema_workbench_surface_view.rs"]
mod schema_workbench_surface_view;
#[path = "../schema/schema_workspace_state.rs"]
mod schema_workspace_state;
#[path = "../security/security_activity_view.rs"]
mod security_activity_view;
#[path = "../security/security_confirmation_view.rs"]
mod security_confirmation_view;
#[path = "../security/security_rls.rs"]
mod security_rls;
#[path = "../security/security_rls_view.rs"]
mod security_rls_view;
#[path = "../security/security_role_details_view.rs"]
mod security_role_details_view;
#[path = "../security/security_roles_view.rs"]
mod security_roles_view;
#[path = "../shell/chrome/shell_chrome_view.rs"]
mod shell_chrome_view;
#[path = "../shell/chrome/shell_frame_view.rs"]
mod shell_frame_view;
#[path = "../shell/chrome/shell_statusbar_view.rs"]
mod shell_statusbar_view;
#[path = "../shell/chrome/shell_topbar_view.rs"]
mod shell_topbar_view;
#[cfg(test)]
#[path = "../settings/tests/shortcut_catalog_tests.rs"]
mod shortcut_catalog_tests;
#[cfg(test)]
#[path = "../table/tests/table_data_export_tests.rs"]
mod table_data_export_tests;
#[cfg(test)]
#[path = "../table/tests/table_layout_tests.rs"]
mod table_layout_tests;
#[path = "../table/table_ddl_surface_view.rs"]
mod table_ddl_surface_view;
#[path = "../table/table_ddl_view.rs"]
mod table_ddl_view;
#[path = "../table/table_editor_context.rs"]
mod table_editor_context;
#[path = "../table/table_editor_values.rs"]
mod table_editor_values;
#[path = "../table/table_editor_view.rs"]
mod table_editor_view;
#[cfg(test)]
#[path = "../table/tests/table_fk_lookup_tests.rs"]
mod table_fk_lookup_tests;
#[path = "../table/table_insert_row_surface_view.rs"]
mod table_insert_row_surface_view;
#[path = "../table/table_insert_row_view.rs"]
mod table_insert_row_view;
#[path = "../table/table_metadata_surface_view.rs"]
mod table_metadata_surface_view;
#[path = "../table/table_metadata_view.rs"]
mod table_metadata_view;
#[path = "../table/table_relations_surface_view.rs"]
mod table_relations_surface_view;
#[path = "../table/table_relations_view.rs"]
mod table_relations_view;
#[cfg(test)]
#[path = "../table/tests/table_row_edit_tests.rs"]
mod table_row_edit_tests;
#[cfg(test)]
#[path = "../table/tests/table_row_identity_tests.rs"]
mod table_row_identity_tests;
#[path = "../table/table_scroll_surface_view.rs"]
mod table_scroll_surface_view;
#[path = "../table/table_structure_view.rs"]
mod table_structure_view;
#[path = "../table/table_view.rs"]
mod table_view;
#[path = "../table/table_workspace_surface_view.rs"]
mod table_workspace_surface_view;
#[cfg(test)]
#[path = "tests/app_tests.rs"]
mod tests;
#[path = "../tools/transfer_activity_view.rs"]
mod transfer_activity_view;
#[path = "../workspace/workspace_feature_state.rs"]
mod workspace_feature_state;
#[path = "../workspace/workspace_tabs_surface_view.rs"]
mod workspace_tabs_surface_view;
#[path = "../workspace/workspace_view.rs"]
mod workspace_view;

pub use crate::query::{QueryDocument, QueryExecutionState};
pub(crate) use app_types::*;
#[path = "app_lifecycle.rs"]
mod app_lifecycle;
#[path = "app_storage.rs"]
mod app_storage;
