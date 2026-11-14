use async_trait::async_trait;
use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle};
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{QueryParam, QueryResult};
use db_pro_core::domain::schema::IntrospectResult;
use db_pro_core::ports::{
    DbConnector, ParameterizedTransactionStatement, SqlDialect, TransactionFailure, TransactionFailureOutcome,
    TransactionFailurePhase, TransactionStatementResult,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use super::actor::SqliteHandle;

struct SqliteDialect;
impl SqlDialect for SqliteDialect {
    fn placeholder(&self, _index: usize) -> String {
        "?".to_string()
    }
    fn quote_identifier(&self, name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
}

pub struct ActorEntry {
    pub handle: SqliteHandle,
    pub max_rows: u64,
    pub query_timeout_ms: u64,
}

pub struct SQLiteConnector {
    actors: RwLock<HashMap<u64, ActorEntry>>,
    next_id: AtomicU64,
}

impl Default for SQLiteConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl SQLiteConnector {
    pub fn new() -> Self {
        Self {
            actors: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }
}

#[async_trait]
impl DbConnector for SQLiteConnector {
    async fn connect(&self, config: &ConnectionConfig, _password: &str) -> Result<ConnectionHandle, DbError> {
        let (db_path, mode) = sqlite_target(config)?;
        let handle = super::actor::SqliteActor::spawn_with_mode(db_path, mode)?;
        if config.readonly {
            // A read-only gate that only inspects the statement text still lets
            // a trigger, a generated column or a `RETURNING` write through, and
            // it cannot see writes done by `SELECT`-shaped pragmas (#588).
            // SQLite enforces it itself: `SQLITE_OPEN_READ_ONLY` for the file
            // plus `query_only` for anything that slipped past the flags.
            handle
                .execute_statement("PRAGMA query_only = 1".to_owned())
                .await
                .map_err(|error| DbError::ConnectionFailed(format!("cannot enable SQLite query_only: {error}")))?;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let entry = ActorEntry {
            handle,
            max_rows: config.max_rows,
            query_timeout_ms: config.query_timeout_ms,
        };
        self.actors.write().await.insert(id, entry);
        Ok(ConnectionHandle::new(id))
    }

    async fn disconnect(&self, handle: &ConnectionHandle) -> Result<(), DbError> {
        if let Some(entry) = self.actors.write().await.remove(&handle.0) {
            entry.handle.shutdown().await;
        }
        Ok(())
    }

    async fn test_connection(&self, config: &ConnectionConfig, _password: &str) -> Result<(), DbError> {
        // Testing must never create the file it is testing (#597): a typo would
        // otherwise "succeed" against a brand-new empty database.
        let (db_path, _) = sqlite_target(config)?;
        let actor = super::actor::SqliteActor::spawn_with_mode(db_path, super::actor::SqliteOpenMode::Existing)?;
        actor
            .execute("SELECT 1".into(), vec![], config.max_rows, config.query_timeout_ms)
            .await
            .map(|_| ())
    }
    async fn query(&self, handle: &ConnectionHandle, sql: &str, params: &[QueryParam]) -> Result<QueryResult, DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        entry
            .handle
            .execute(sql.into(), params.to_vec(), entry.max_rows, entry.query_timeout_ms)
            .await
    }

    async fn execute(&self, handle: &ConnectionHandle, sql: &str, params: &[QueryParam]) -> Result<u64, DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        let affected = entry
            .handle
            .execute_param_with_timeout(sql.into(), params.to_vec(), entry.query_timeout_ms)
            .await?;
        Ok(affected as u64)
    }

    async fn cancel(&self, handle: &ConnectionHandle) -> Result<(), DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        entry.handle.cancel(entry.query_timeout_ms).await
    }

    async fn execute_batch(&self, handle: &ConnectionHandle, statements: &[String]) -> Result<u64, DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        entry
            .handle
            .execute_batch(statements.to_vec(), entry.query_timeout_ms)
            .await
    }

    async fn execute_transaction(
        &self,
        handle: &ConnectionHandle,
        statements: &[String],
        read_statements: &[bool],
    ) -> Result<Vec<TransactionStatementResult>, TransactionFailure> {
        let actors = self.actors.read().await;
        let entry = actors.get(&handle.0).ok_or_else(|| TransactionFailure {
            phase: TransactionFailurePhase::Validation,
            statement_index: 0,
            outcome: TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::ConnectionFailed("handle not found".into()),
        })?;
        entry
            .handle
            .execute_transaction(
                statements.to_vec(),
                read_statements.to_vec(),
                entry.max_rows,
                entry.query_timeout_ms,
            )
            .await
    }

    async fn execute_parameterized_transaction(
        &self,
        handle: &ConnectionHandle,
        statements: &[ParameterizedTransactionStatement],
    ) -> Result<Vec<TransactionStatementResult>, TransactionFailure> {
        let actors = self.actors.read().await;
        let entry = actors.get(&handle.0).ok_or_else(|| TransactionFailure {
            phase: TransactionFailurePhase::Validation,
            statement_index: 0,
            outcome: TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::ConnectionFailed("handle not found".into()),
        })?;
        entry
            .handle
            .execute_parameterized_transaction(statements.to_vec(), entry.query_timeout_ms)
            .await
    }

    async fn introspect(&self, handle: &ConnectionHandle) -> Result<IntrospectResult, DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        entry.handle.introspect(entry.query_timeout_ms).await
    }

    async fn explain(
        &self,
        handle: &ConnectionHandle,
        sql: &str,
        params: &[QueryParam],
        _analyze: bool,
    ) -> Result<serde_json::Value, DbError> {
        let actors = self.actors.read().await;
        let entry = actors
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("handle not found".into()))?;
        // SQLite EXPLAIN QUERY PLAN has no ANALYZE equivalent that executes the query.
        entry
            .handle
            .explain(sql.into(), params.to_vec(), entry.query_timeout_ms)
            .await
    }

    fn dialect(&self, _handle: &ConnectionHandle) -> Result<Box<dyn SqlDialect>, DbError> {
        Ok(Box::new(SqliteDialect))
    }
}

#[cfg(test)]
#[path = "tests/connector_tests.rs"]
mod tests;

/// Where a SQLite connection points, and whether opening may create it (#597).
///
/// An empty `database` used to fall back to `:memory:` silently, so a user who
/// meant to reconnect to a file got a throwaway database and lost everything
/// they created in it on disconnect. In-memory is now an explicit choice
/// (`:memory:` or `memory:`), and a file path is only created on connect when
/// the connection was saved with `create_if_missing`.
fn sqlite_target(config: &ConnectionConfig) -> Result<(&str, super::actor::SqliteOpenMode), DbError> {
    let database = config.database.trim();
    if database.is_empty() {
        return Err(DbError::ConnectionFailed(
            "SQLite database path is empty; choose a file or select in-memory explicitly".into(),
        ));
    }
    if database.eq_ignore_ascii_case(":memory:") || database.eq_ignore_ascii_case("memory:") {
        return Ok((":memory:", super::actor::SqliteOpenMode::Memory));
    }
    let mode = if config.readonly {
        // The engine enforces read-only, so no write can slip through the
        // statement classifier (#588).
        super::actor::SqliteOpenMode::ReadOnly
    } else if config.create_if_missing {
        super::actor::SqliteOpenMode::CreateIfMissing
    } else {
        super::actor::SqliteOpenMode::Existing
    };
    Ok((database, mode))
}
