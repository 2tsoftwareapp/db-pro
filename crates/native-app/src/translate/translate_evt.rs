//! RuntimeEvent → UiEvent mapping.
use super::super::*;

use super::translate_cmd::*;
use super::translate_map::*;

// cc-scan:allow HUGE_FUNCTION — flat RuntimeEvent→UiEvent router; each arm delegates to a per-event translator
pub(crate) fn translate_event(event: RuntimeEvent) -> Option<UiEvent> {
    match event {
        RuntimeEvent::ConnectionsLoaded {
            request_id,
            connections,
        } => translate_connections_loaded(request_id, connections),
        RuntimeEvent::SchemaLoaded { request_id, schema } => translate_schema_loaded(request_id, schema),
        RuntimeEvent::TableInfoLoaded { request_id, table_info } => translate_table_info_loaded(request_id, table_info),
        RuntimeEvent::TableDdlLoaded { request_id, sql } => translate_table_ddl_loaded(request_id, sql),
        RuntimeEvent::DdlCompleted {
            request_id,
            affected_rows,
        } => translate_ddl_completed(request_id, affected_rows),
        RuntimeEvent::QueryFoldersLoaded { request_id, folders } => translate_query_folders_loaded(request_id, folders),
        RuntimeEvent::SavedQueriesLoaded { request_id, queries } => translate_saved_queries_loaded(request_id, queries),
        RuntimeEvent::OperationProgress {
            request_id,
            operation,
            status,
        } => translate_operation_progress(request_id, operation, status),
        RuntimeEvent::BackupCompleted {
            request_id,
            output_path,
            size_bytes,
        } => translate_backup_completed(request_id, output_path, size_bytes),
        RuntimeEvent::MonitoringSnapshotLoaded {
            request_id,
            connection_id,
            snapshot,
        } => Some(UiEvent::MonitoringSnapshotLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            snapshot: *snapshot,
        }),
        RuntimeEvent::MonitoringWorkloadLoaded {
            request_id,
            connection_id,
            workload,
        } => Some(UiEvent::MonitoringWorkloadLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            workload,
        }),
        RuntimeEvent::AuditPageLoaded {
            request_id,
            connection_id,
            page,
        } => Some(UiEvent::AuditPageLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            page,
        }),
        RuntimeEvent::RoutineDebugProbeLoaded {
            request_id,
            connection_id,
            probe,
        } => Some(UiEvent::RoutineDebugProbeLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            probe,
        }),
        RuntimeEvent::PgSettingsLoaded {
            request_id,
            connection_id,
            snapshot,
        } => Some(UiEvent::PgSettingsLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            snapshot,
        }),
        RuntimeEvent::PgSettingActionCompleted {
            request_id,
            action,
            name,
        } => Some(UiEvent::PgSettingActionCompleted {
            request_id: ui_request_id(request_id),
            action: action.to_owned(),
            name,
        }),
        RuntimeEvent::FdwInventoryLoaded {
            request_id,
            connection_id,
            inventory,
        } => Some(UiEvent::FdwInventoryLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            inventory,
        }),
        RuntimeEvent::FdwActionCompleted {
            request_id,
            action,
            name,
        } => Some(UiEvent::FdwActionCompleted {
            request_id: ui_request_id(request_id),
            action: action.to_owned(),
            name,
        }),
        RuntimeEvent::ReplicationInventoryLoaded {
            request_id,
            connection_id,
            inventory,
        } => Some(UiEvent::ReplicationInventoryLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            inventory,
        }),
        RuntimeEvent::ReplicationActionCompleted {
            request_id,
            action,
            name,
        } => Some(UiEvent::ReplicationActionCompleted {
            request_id: ui_request_id(request_id),
            action: action.to_owned(),
            name,
        }),
        RuntimeEvent::EventTriggerInventoryLoaded {
            request_id,
            connection_id,
            inventory,
        } => Some(UiEvent::EventTriggerInventoryLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            inventory,
        }),
        RuntimeEvent::EventTriggerActionCompleted {
            request_id,
            action,
            name,
        } => Some(UiEvent::EventTriggerActionCompleted {
            request_id: ui_request_id(request_id),
            action: action.to_owned(),
            name,
        }),
        RuntimeEvent::MonitoringActionCompleted {
            request_id,
            action,
            backend_id,
            succeeded,
        } => Some(UiEvent::MonitoringActionCompleted {
            request_id: ui_request_id(request_id),
            action: action.to_owned(),
            backend_id,
            succeeded,
        }),
        RuntimeEvent::UsersLoaded {
            request_id,
            connection_id,
            users,
        } => Some(UiEvent::UsersLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            users,
        }),
        RuntimeEvent::PrivilegesLoaded {
            request_id,
            connection_id,
            role_name,
            privileges,
        } => Some(UiEvent::PrivilegesLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            role_name,
            privileges,
        }),
        RuntimeEvent::MembershipsLoaded {
            request_id,
            connection_id,
            member,
            memberships,
        } => Some(UiEvent::MembershipsLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            member,
            memberships,
        }),
        RuntimeEvent::TableRlsLoaded {
            request_id,
            connection_id,
            state,
        } => Some(UiEvent::TableRlsLoaded {
            request_id: ui_request_id(request_id),
            connection_id,
            state,
        }),
        RuntimeEvent::DataDiffLoaded {
            request_id,
            source_id,
            target_id,
            diff,
        } => Some(UiEvent::DataDiffLoaded {
            request_id: ui_request_id(request_id),
            source_id,
            target_id,
            diff,
        }),
        RuntimeEvent::OperationCompleted { request_id, operation } => {
            translate_operation_completed(request_id, operation)
        }
        RuntimeEvent::TableChangesFailed {
            request_id,
            code,
            message,
            statement_index,
            rolled_back,
        } => Some(UiEvent::TableChangesFailed {
            request_id: ui_request_id(request_id),
            code,
            message,
            statement_index,
            rolled_back,
        }),
        RuntimeEvent::Connected {
            request_id,
            connection_id,
        } => translate_connected(request_id, connection_id),
        RuntimeEvent::QueryCompleted { request_id, result } => translate_query_completed(request_id, result),
        RuntimeEvent::QueryMultiCompleted { request_id, output } => Some(UiEvent::QueryMultiCompleted {
            request_id: ui_request_id(request_id),
            output: map_multi_query_output(output),
        }),
        RuntimeEvent::QuerySaved { request_id, query } => Some(UiEvent::QuerySaved {
            request_id: ui_request_id(request_id),
            query: UiSavedQuerySummary {
                id: query.id,
                name: query.name,
                sql: query.sql,
                folder: query.folder,
            },
        }),
        RuntimeEvent::ExplainCompleted { request_id, plan } => translate_explain_completed(request_id, plan),
        RuntimeEvent::TableDataLoaded {
            request_id,
            result,
            total_rows,
        } => translate_table_data_loaded(request_id, result, total_rows),
        RuntimeEvent::QueryCancelled { request_id } => translate_query_cancelled(request_id),
        RuntimeEvent::QueryFailedDetailed { request_id, error } => Some(UiEvent::QueryFailedDetailed {
            request_id: ui_request_id(request_id),
            code: error.code,
            message: error.message,
            position: error.position,
        }),
        RuntimeEvent::AgentProviderReady { provider, detail } => Some(UiEvent::AgentProviderReady { provider, detail }),
        RuntimeEvent::AgentFailed { request_id, message } => translate_agent_failed(request_id, message),
        RuntimeEvent::AgentWorkflow { request_id, event } => Some(UiEvent::AgentWorkflow {
            request_id: ui_request_id(request_id),
            event,
        }),
        RuntimeEvent::AgentConfigured {
            request_id,
            provider,
            detail,
        } => Some(UiEvent::AgentConfigured {
            request_id: ui_request_id(request_id),
            provider,
            detail,
        }),
        RuntimeEvent::AgentForgotten { request_id } => Some(UiEvent::AgentForgotten {
            request_id: ui_request_id(request_id),
        }),
        RuntimeEvent::SqlPredictionReady {
            request_id,
            document_id,
            document_version,
            anchor,
            replacement_range,
            prediction,
        } => Some(UiEvent::SqlPredictionReady {
            request_id: ui_request_id(request_id),
            document_id,
            document_version,
            anchor,
            replacement_range,
            prediction,
        }),
        RuntimeEvent::SqlPredictionFailed {
            request_id,
            document_id,
            document_version,
            anchor,
            replacement_range,
            message,
        } => Some(UiEvent::SqlPredictionFailed {
            request_id: ui_request_id(request_id),
            document_id,
            document_version,
            anchor,
            replacement_range,
            message,
        }),
        RuntimeEvent::Failed { request_id, message } => translate_failed(request_id, message),
    }
}

fn translate_table_info_loaded(
    request_id: RuntimeRequestId,
    table_info: db_pro_core::domain::schema::TableInfo,
) -> Option<UiEvent> {
    Some(UiEvent::TableInfoLoaded {
        request_id: ui_request_id(request_id),
        table_info: map_table_info(table_info),
    })
}

fn translate_table_ddl_loaded(request_id: RuntimeRequestId, sql: String) -> Option<UiEvent> {
    Some(UiEvent::TableDdlLoaded {
        request_id: ui_request_id(request_id),
        sql,
    })
}

fn translate_ddl_completed(request_id: RuntimeRequestId, affected_rows: u64) -> Option<UiEvent> {
    Some(UiEvent::DdlCompleted {
        request_id: ui_request_id(request_id),
        affected_rows,
    })
}

fn translate_operation_progress(
    request_id: RuntimeRequestId,
    operation: &'static str,
    status: &'static str,
) -> Option<UiEvent> {
    Some(UiEvent::OperationProgress {
        request_id: ui_request_id(request_id),
        operation: operation.to_owned(),
        status: status.to_owned(),
    })
}

fn translate_backup_completed(request_id: RuntimeRequestId, output_path: String, size_bytes: u64) -> Option<UiEvent> {
    Some(UiEvent::BackupCompleted {
        request_id: ui_request_id(request_id),
        output_path,
        size_bytes,
    })
}

fn translate_operation_completed(request_id: RuntimeRequestId, operation: &'static str) -> Option<UiEvent> {
    Some(UiEvent::OperationCompleted {
        request_id: ui_request_id(request_id),
        operation: operation.to_owned(),
    })
}

fn translate_connected(request_id: RuntimeRequestId, connection_id: String) -> Option<UiEvent> {
    Some(UiEvent::Connected {
        request_id: ui_request_id(request_id),
        connection_id,
    })
}

fn translate_query_completed(
    request_id: RuntimeRequestId,
    result: db_pro_core::domain::query::QueryResult,
) -> Option<UiEvent> {
    Some(UiEvent::QueryCompleted {
        request_id: ui_request_id(request_id),
        result: map_query_result(result),
    })
}

// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
fn map_multi_query_output(output: db_pro_core::application::MultiQueryResult) -> UiQueryExecutionOutput {
    let db_pro_core::application::MultiQueryResult {
        results,
        result_kinds,
        total_duration_ms,
        error,
    } = output;
    let mut statements = results
        .into_iter()
        .enumerate()
        .map(|(statement_index, result)| {
            let kind = result_kinds.get(statement_index).copied().unwrap_or({
                // Compatibility for callers that construct the pre-kind
                // shape directly. Runtime-produced results always carry
                // the explicit core metadata.
                if result.columns.is_empty() {
                    db_pro_core::application::StatementResultKind::Command
                } else {
                    db_pro_core::application::StatementResultKind::ResultSet
                }
            });
            let affected_rows =
                matches!(kind, db_pro_core::application::StatementResultKind::Command).then_some(result.row_count);
            let message = affected_rows.map(|rows| format!("{rows} rows affected"));
            let duration_ms = result.duration_ms;
            UiStatementOutput {
                statement_index,
                result_set: matches!(kind, db_pro_core::application::StatementResultKind::ResultSet)
                    .then(|| map_query_result(result)),
                affected_rows,
                duration_ms,
                message,
                error: None,
            }
        })
        .collect::<Vec<_>>();
    if let Some((statement_index, error)) = error {
        statements.push(UiStatementOutput {
            statement_index,
            result_set: None,
            affected_rows: None,
            duration_ms: 0,
            message: None,
            error: Some(UiQueryError {
                code: error.code,
                message: error.message,
                position: error.position,
                detail: error.detail,
                hint: error.hint,
            }),
        });
        statements.sort_by_key(|statement| statement.statement_index);
    }
    UiQueryExecutionOutput {
        statements,
        total_duration_ms,
    }
}

fn translate_explain_completed(request_id: RuntimeRequestId, plan: String) -> Option<UiEvent> {
    Some(UiEvent::ExplainCompleted {
        request_id: ui_request_id(request_id),
        plan,
    })
}

fn translate_table_data_loaded(
    request_id: RuntimeRequestId,
    result: db_pro_core::domain::query::QueryResult,
    total_rows: u64,
) -> Option<UiEvent> {
    Some(UiEvent::TableDataLoaded {
        request_id: ui_request_id(request_id),
        result: map_query_result(result),
        total_rows,
    })
}

fn translate_query_cancelled(request_id: RuntimeRequestId) -> Option<UiEvent> {
    Some(UiEvent::QueryCancelled {
        request_id: ui_request_id(request_id),
    })
}

fn translate_agent_failed(request_id: RuntimeRequestId, message: String) -> Option<UiEvent> {
    Some(UiEvent::AgentFailed {
        request_id: ui_request_id(request_id),
        message,
    })
}

fn translate_failed(request_id: RuntimeRequestId, message: String) -> Option<UiEvent> {
    Some(UiEvent::QueryFailed {
        request_id: ui_request_id(request_id),
        message,
    })
}

fn translate_connections_loaded(
    request_id: RuntimeRequestId,
    connections: Vec<db_pro_runtime::ConnectionSummary>,
) -> Option<UiEvent> {
    Some(UiEvent::ConnectionsLoaded {
        request_id: ui_request_id(request_id),
        connections: connections
            .into_iter()
            .map(|connection| UiConnectionSummary {
                id: connection.id,
                name: connection.name,
                host: connection.host,
                port: connection.port,
                database: connection.database,
                username: connection.username,
                driver: connection.driver,
                ssl_mode: ui_ssl_mode(connection.ssl_mode),
                readonly: connection.readonly,
                create_if_missing: false,
                tags: connection.tags,
                group: connection.group,
                favorite: connection.favorite,
                environment: connection.environment,
            })
            .collect(),
    })
}

fn translate_schema_loaded(request_id: RuntimeRequestId, schema: db_pro_runtime::SchemaSummary) -> Option<UiEvent> {
    Some(UiEvent::SchemaLoaded {
        request_id: ui_request_id(request_id),
        schema: map_schema_summary(schema),
    })
}

fn map_schema_summary(schema: db_pro_runtime::SchemaSummary) -> UiSchemaSummary {
    UiSchemaSummary {
        schemas: schema.schemas,
        tables: schema.tables,
        columns: schema.columns,
        table_details: schema.table_details.into_iter().map(map_table_summary).collect(),
        views: schema.views.into_iter().map(map_view_summary).collect(),
        triggers: schema.triggers.into_iter().map(map_trigger_summary).collect(),
        functions: schema.functions.into_iter().map(map_function_summary).collect(),
    }
}

fn map_table_summary(table: db_pro_runtime::TableSummary) -> UiTableSummary {
    UiTableSummary {
        schema: table.schema,
        name: table.name,
        row_count: table.row_count,
        columns: table.columns.into_iter().map(map_schema_column).collect(),
        foreign_keys: table.foreign_keys.into_iter().map(map_foreign_key).collect(),
    }
}

fn map_schema_column(column: db_pro_runtime::ColumnSummary) -> UiSchemaColumn {
    UiSchemaColumn {
        name: column.name,
        data_type: column.data_type,
        nullable: column.nullable,
        is_primary_key: column.is_primary_key,
    }
}

fn map_foreign_key(foreign_key: db_pro_runtime::ForeignKeySummary) -> UiSchemaForeignKey {
    UiSchemaForeignKey {
        name: foreign_key.name,
        from_columns: foreign_key.from_columns,
        to_schema: foreign_key.to_schema,
        to_table: foreign_key.to_table,
        to_columns: foreign_key.to_columns,
    }
}

fn map_view_summary(view: db_pro_runtime::ViewSummary) -> UiViewSummary {
    UiViewSummary {
        schema: view.schema,
        name: view.name,
        definition: view.definition,
    }
}

fn map_trigger_summary(trigger: db_pro_runtime::TriggerSummary) -> UiTriggerSummary {
    UiTriggerSummary {
        schema: trigger.schema,
        name: trigger.name,
        table_name: trigger.table_name,
        timing: trigger.timing,
        event: trigger.event,
        definition: trigger.definition,
        enabled: trigger.enabled,
    }
}

fn map_function_summary(function: db_pro_runtime::FunctionSummary) -> UiFunctionSummary {
    UiFunctionSummary {
        schema: function.schema,
        name: function.name,
        routine_type: function.routine_type,
        data_type: function.data_type,
        definition: function.definition,
        identity_arguments: function.identity_arguments,
        language: function.language,
        volatility: function.volatility,
        security_definer: function.security_definer,
        parameters: function
            .parameters
            .into_iter()
            .map(|p| db_pro_ui::UiRoutineParameter {
                name: p.name,
                data_type: p.data_type,
                mode: p.mode,
                has_default: p.has_default,
                default_expr: p.default_expr,
            })
            .collect(),
    }
}

fn translate_query_folders_loaded(
    request_id: RuntimeRequestId,
    folders: Vec<db_pro_runtime::QueryFolderSummary>,
) -> Option<UiEvent> {
    Some(UiEvent::QueryFoldersLoaded {
        request_id: ui_request_id(request_id),
        folders: folders
            .into_iter()
            .map(|folder| UiQueryFolderSummary {
                id: folder.id,
                name: folder.name,
            })
            .collect(),
    })
}

fn translate_saved_queries_loaded(
    request_id: RuntimeRequestId,
    queries: Vec<db_pro_runtime::SavedQuerySummary>,
) -> Option<UiEvent> {
    Some(UiEvent::SavedQueriesLoaded {
        request_id: ui_request_id(request_id),
        queries: queries
            .into_iter()
            .map(|query| UiSavedQuerySummary {
                id: query.id,
                name: query.name,
                sql: query.sql,
                folder: query.folder,
            })
            .collect(),
    })
}

#[cfg(test)]
#[path = "tests/translate_evt_tests.rs"]
mod tests;
