use std::sync::Arc;

use crate::domain::connection::{ConnectionConfig, ConnectionId, DriverType};
use crate::domain::error::DbError;
use crate::domain::routine_debug::{RoutineDebugProbe, DEBUG_CAPABILITY_PROBE_SQL};
use crate::domain::safety::{validate_against_policy, ConnectionSafetyPolicy};
use crate::domain::schema::{IntrospectResult, RelationKind, TableInfo, View};
use crate::ports::{ConnectionRepository, DbConnector, IntrospectionCache};

use super::driver_dialect::DriverDialect;
use super::registry::ConnectionRegistry;
use super::sql_policy::{placeholder_remains, split_statements_for_driver};
use schema_ddl::{build_create_table_ddl, format_trigger_ddl};
use schema_table_info::build_table_info;

#[path = "schema_service/schema_ddl.rs"]
mod schema_ddl;
#[path = "schema_service/schema_table_info.rs"]
mod schema_table_info;

pub struct SchemaService {
    connector: Box<dyn DbConnector>,
    cache: Box<dyn IntrospectionCache>,
    registry: Arc<ConnectionRegistry>,
    connections: Box<dyn ConnectionRepository>,
}

impl SchemaService {
    pub fn new(
        connector: Box<dyn DbConnector>,
        cache: Box<dyn IntrospectionCache>,
        registry: Arc<ConnectionRegistry>,
        connections: Box<dyn ConnectionRepository>,
    ) -> Self {
        Self {
            connector,
            cache,
            registry,
            connections,
        }
    }

    async fn safety_policy_for(&self, connection_id: &ConnectionId) -> Result<ConnectionSafetyPolicy, DbError> {
        let config = self.connection_config(connection_id).await?;
        if config.readonly {
            Ok(ConnectionSafetyPolicy::read_only())
        } else {
            Ok(ConnectionSafetyPolicy::full_access())
        }
    }

    async fn connection_config(&self, connection_id: &ConnectionId) -> Result<ConnectionConfig, DbError> {
        self.connections
            .get_config(connection_id)
            .await?
            .ok_or_else(|| DbError::ConnectionFailed(format!("connection {connection_id} not found")))
    }

    pub async fn introspect(
        &self,
        connection_id: &ConnectionId,
        force_refresh: bool,
    ) -> Result<IntrospectResult, DbError> {
        if !force_refresh {
            match self.cache.get(connection_id).await {
                Ok(Some(cached)) => {
                    if self.cached_schema_still_usable(connection_id, &cached).await {
                        return Ok(cached);
                    }
                    tracing::warn!(
                        connection_id = %connection_id,
                        "discarding stale introspection cache that no longer matches the database"
                    );
                    if let Err(invalidate_error) = self.cache.invalidate(connection_id).await {
                        tracing::warn!(
                            connection_id = %connection_id,
                            error = %invalidate_error,
                            "failed to discard stale introspection cache"
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        connection_id = %connection_id,
                        error = %error,
                        "discarding invalid introspection cache"
                    );
                    if let Err(invalidate_error) = self.cache.invalidate(connection_id).await {
                        tracing::warn!(
                            connection_id = %connection_id,
                            error = %invalidate_error,
                            "failed to discard invalid introspection cache"
                        );
                    }
                }
            }
        }

        let handle = self
            .registry
            .get(connection_id)
            .ok_or_else(|| DbError::ConnectionFailed(format!("connection {connection_id} is not active")))?;

        let result = self.connector.introspect(&handle).await?;

        if let Err(e) = self.cache.save(connection_id, &result).await {
            tracing::warn!("failed to cache introspection: {e}");
        }

        Ok(result)
    }

    /// Returns false when a cached SQLite schema would mislead the Explorer.
    ///
    /// Truncated or deleted `.sqlite` files previously kept serving a warm cache
    /// (`force_refresh=false` on connect), so the tree showed tables the query
    /// engine could no longer resolve.
    async fn cached_schema_still_usable(&self, connection_id: &ConnectionId, cached: &IntrospectResult) -> bool {
        let Ok(config) = self.connection_config(connection_id).await else {
            // Config lookup failed — keep the cache rather than blocking the UI.
            return true;
        };
        if config.driver != DriverType::SQLite {
            return true;
        }
        sqlite_file_matches_cached_schema(&config.database, cached)
    }

    /// Reads whether server-side PL/pgSQL debugging is possible on this connection (#252).
    ///
    /// Catalog reads only — the probe never installs an extension, never edits
    /// `shared_preload_libraries`, and never reloads server config, so it is safe against
    /// read-only and production connections. Drivers other than PostgreSQL have no
    /// PL/pgSQL debug provider to look for and are answered without a round trip.
    ///
    /// A failed probe is data, not an error: the caller always receives a
    /// [`RoutineDebugProbe`] whose state the workbench can explain.
    pub async fn routine_debug_probe(&self, connection_id: &ConnectionId) -> Result<RoutineDebugProbe, DbError> {
        let config = self.connection_config(connection_id).await?;
        if config.driver != DriverType::Postgres {
            return Ok(RoutineDebugProbe::unsupported_driver());
        }
        let Some(handle) = self.registry.get(connection_id) else {
            return Ok(RoutineDebugProbe::probe_failed(format!(
                "connection {connection_id} is not active"
            )));
        };
        let result = self.connector.query(&handle, DEBUG_CAPABILITY_PROBE_SQL, &[]).await;
        Ok(match result {
            Ok(result) => RoutineDebugProbe::from_probe_result(&result),
            Err(error) => RoutineDebugProbe::probe_failed(error.to_string()),
        })
    }

    pub async fn get_table_info(
        &self,
        connection_id: &ConnectionId,
        schema: &str,
        table: &str,
    ) -> Result<TableInfo, DbError> {
        let introspect = self.introspect(connection_id, false).await?;
        build_table_info(introspect, schema, table)
    }

    pub async fn get_table_ddl(
        &self,
        connection_id: &ConnectionId,
        schema: &str,
        table: &str,
    ) -> Result<String, DbError> {
        let introspect = self.introspect(connection_id, false).await?;

        // Views, materialized views and foreign tables carry their own
        // definition; only an ordinary table is rebuilt from metadata (#595).
        if let Some(view) = introspect.views.iter().find(|v| v.schema == schema && v.name == table) {
            return Ok(format_relation_ddl(view));
        }

        let tbl = introspect
            .tables
            .iter()
            .find(|t| t.schema == schema && t.name == table)
            .ok_or_else(|| DbError::NotFound(format!("table or view {schema}.{table}")))?
            .clone();

        // SQLite keeps the original CREATE TABLE text, which is the only source
        // that survives `AUTOINCREMENT`, `COLLATE`, `WITHOUT ROWID`, `STRICT`,
        // `ON CONFLICT` and generated columns (#596). Rebuilding from metadata
        // is the fallback for providers that store nothing.
        if let Some(stored) = introspect.stored_table_definition(schema, table) {
            let mut ddl = format!("{};\n", stored.trim().trim_end_matches(';'));
            for trigger in introspect
                .triggers
                .iter()
                .filter(|trigger| trigger.table_name == table && trigger.schema == schema)
            {
                ddl.push_str(&format_trigger_ddl(trigger));
                ddl.push('\n');
            }
            return Ok(ddl);
        }

        let check_constraints: Vec<_> = introspect
            .check_constraints
            .iter()
            .filter(|constraint| constraint.schema == schema && constraint.table_name == table)
            .cloned()
            .collect();

        let columns: Vec<_> = introspect
            .columns
            .into_iter()
            .filter(|c| c.schema == schema && c.table_name == table)
            .collect();

        let primary_key = introspect
            .primary_keys
            .into_iter()
            .find(|pk| pk.schema == schema && pk.table_name == table);

        let indexes: Vec<_> = introspect
            .indexes
            .into_iter()
            .filter(|i| i.schema == schema && i.table_name == table)
            .collect();

        let foreign_keys: Vec<_> = introspect
            .foreign_keys
            .into_iter()
            .filter(|fk| fk.from_table == table && fk.schema == schema)
            .collect();

        let triggers: Vec<_> = introspect
            .triggers
            .into_iter()
            .filter(|tr| tr.table_name == table && tr.schema == schema)
            .collect();

        let info = TableInfo {
            table: tbl,
            columns,
            primary_key,
            indexes,
            foreign_keys,
            check_constraints: check_constraints.clone(),
            dependencies: Vec::new(),
        };

        let driver = self.connection_config(connection_id).await?.driver;
        let mut ddl = build_create_table_ddl(&info, driver, &check_constraints);
        for trigger in &triggers {
            ddl.push_str(&format_trigger_ddl(trigger));
            ddl.push('\n');
        }

        Ok(ddl)
    }

    pub async fn execute_ddl(&self, connection_id: &ConnectionId, sql: &str) -> Result<u64, DbError> {
        reject_unexpanded_placeholder(sql)?;
        let driver = self.connection_config(connection_id).await?.driver;
        let statements = split_statements_for_driver(sql, driver);
        if statements.is_empty() {
            return Err(DbError::QueryFailed("empty SQL statement".into()));
        }
        self.execute_ddl_batch(connection_id, &statements).await
    }

    pub async fn execute_ddl_batch(&self, connection_id: &ConnectionId, statements: &[String]) -> Result<u64, DbError> {
        let driver = self.connection_config(connection_id).await?.driver;
        let policy = self.safety_policy_for(connection_id).await?;
        let mut flat = Vec::new();
        for sql in statements {
            reject_unexpanded_placeholder(sql)?;
            let parts = split_statements_for_driver(sql, driver);
            if parts.is_empty() {
                return Err(DbError::QueryFailed("empty SQL statement".into()));
            }
            for part in &parts {
                validate_against_policy(part, &policy).map_err(DbError::QueryFailed)?;
            }
            flat.extend(parts);
        }
        let statements = flat;

        let handle = self
            .registry
            .get(connection_id)
            .ok_or_else(|| DbError::ConnectionFailed(format!("connection {connection_id} is not active")))?;

        let affected = self.connector.execute_batch(&handle, &statements).await?;

        if let Err(e) = self.cache.invalidate(connection_id).await {
            tracing::warn!("failed to invalidate cache after batch DDL: {e}");
        }

        Ok(affected)
    }

    pub async fn invalidate_cache(&self, connection_id: &ConnectionId) -> Result<(), DbError> {
        self.cache.invalidate(connection_id).await
    }
}

/// Whether a SQLite path is concrete enough to fingerprint against the cache.
///
/// Unit tests often use a bare name like `testdb`; those stay cache-eligible
/// without touching the filesystem.
fn is_concrete_sqlite_path(database: &str) -> bool {
    let path = std::path::Path::new(database);
    path.is_absolute()
        || path.extension().is_some_and(|ext| {
            matches!(
                ext.to_str().unwrap_or_default().to_ascii_lowercase().as_str(),
                "sqlite" | "sqlite3" | "db" | "db3"
            )
        })
}

fn reject_unexpanded_placeholder(sql: &str) -> Result<(), DbError> {
    if placeholder_remains(sql) {
        return Err(DbError::QueryFailed(
            "migration SQL still contains a placeholder; expand it from introspection before apply".into(),
        ));
    }
    Ok(())
}

/// Every relation kind a schema tree can be built from, so a database that only
/// holds materialized views or foreign tables is not mistaken for an empty one.
fn cached_schema_is_empty(cached: &IntrospectResult) -> bool {
    cached.tables.is_empty()
        && cached.views.is_empty()
        && cached.triggers.is_empty()
        && cached.functions.is_empty()
        && cached.indexes.is_empty()
}

/// The `CREATE` statement for a view-like relation, per its provider kind.
///
/// A materialized view needs `CREATE MATERIALIZED VIEW … AS <query>`; emitting
/// the plain `CREATE VIEW` spelling — or the bare query — would not reproduce the
/// object the user sees in the explorer (#595).
fn format_relation_ddl(view: &View) -> String {
    let query = view.definition.trim().trim_end_matches(';');
    match view.kind {
        RelationKind::View => format!("{query};\n"),
        RelationKind::MaterializedView => {
            let dialect = DriverDialect::new(DriverType::Postgres);
            format!(
                "CREATE MATERIALIZED VIEW {} AS\n{query};\n",
                crate::application::sql_builder::qualify(&dialect, &view.schema, &view.name)
            )
        }
        RelationKind::ForeignTable => {
            // Introspection knows the relation and its FDW options, not the
            // column list or the server name, so the definition is reported as
            // what it is instead of a `CREATE VIEW` that would be wrong.
            format!(
                "-- foreign table {}.{}\n-- options: {}\n",
                view.schema, view.name, view.definition
            )
        }
    }
}

fn sqlite_file_matches_cached_schema(database: &str, cached: &IntrospectResult) -> bool {
    if !is_concrete_sqlite_path(database) {
        return true;
    }
    match std::fs::metadata(database) {
        Ok(meta) => {
            // Empty / truncated file cannot honestly describe a non-empty tree.
            if meta.len() == 0 {
                return cached_schema_is_empty(cached);
            }
            true
        }
        Err(_) => cached_schema_is_empty(cached),
    }
}

#[cfg(test)]
#[path = "tests/schema_service_tests.rs"]
mod tests;
