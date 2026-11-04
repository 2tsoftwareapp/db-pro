use async_trait::async_trait;
use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle, SslMode};
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::QueryParam;
use db_pro_core::domain::schema::IntrospectResult;
use db_pro_core::ports::DbConnector;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlSslMode};
use sqlx::{Column, Row as SqlxRow};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::RwLock;

use super::introspect::MySqlIntrospect;
use super::query_mapper::MySqlQueryMapper;

struct MySqlEntry {
    pool: MySqlPool,
    max_rows: u64,
    query_timeout: Duration,
    /// `CONNECTION_ID()` of queries running on this handle. Cancel issues `KILL QUERY`.
    active_connections: Mutex<Vec<u64>>,
    /// Connection held from a successful `BEGIN` until `COMMIT` or `ROLLBACK`.
    open_session: crate::transaction_session::SessionSlot<sqlx::pool::PoolConnection<sqlx::MySql>>,
}

pub struct MySqlConnector {
    pools: RwLock<HashMap<u64, MySqlEntry>>,
    next_id: AtomicU64,
}

impl MySqlConnector {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn get_pool(&self, handle: &ConnectionHandle) -> Option<MySqlPool> {
        self.pools.read().await.get(&handle.0).map(|entry| entry.pool.clone())
    }

    fn track_connection(&self, handle_id: u64, connection_id: u64) {
        let Ok(pools) = self.pools.try_read() else {
            return;
        };
        let Some(entry) = pools.get(&handle_id) else {
            return;
        };
        entry
            .active_connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(connection_id);
    }

    fn track_guard(&self, handle_id: u64, connection_id: u64) -> ActiveMySql<'_> {
        self.track_connection(handle_id, connection_id);
        ActiveMySql {
            connector: self,
            handle_id,
            connection_id,
        }
    }

    async fn execute_transaction_joined(
        &self,
        handle: &ConnectionHandle,
        mut lease: crate::transaction_session::SessionLease<sqlx::pool::PoolConnection<sqlx::MySql>>,
        statements: &[String],
        read_statements: &[bool],
    ) -> Result<Vec<db_pro_core::ports::TransactionStatementResult>, db_pro_core::ports::TransactionFailure> {
        use db_pro_core::ports::{TransactionFailure, TransactionFailureOutcome, TransactionFailurePhase};

        let max_rows = self
            .pools
            .read()
            .await
            .get(&handle.0)
            .map(|entry| entry.max_rows)
            .unwrap_or(1000);
        let result = {
            let conn = std::ops::DerefMut::deref_mut(lease.conn());
            let connection_id = match sqlx::query_scalar::<_, u64>("SELECT CONNECTION_ID()")
                .fetch_one(&mut *conn)
                .await
            {
                Ok(connection_id) => connection_id,
                Err(error) => {
                    return Err(TransactionFailure {
                        phase: TransactionFailurePhase::Statement,
                        statement_index: 0,
                        outcome: TransactionFailureOutcome::LeftOpen,
                        results: Vec::new(),
                        error: DbError::QueryFailed(format!("MySQL connection id failed: {error}")),
                    });
                }
            };
            let _active = self.track_guard(handle.0, connection_id);
            let mut results = Vec::with_capacity(statements.len());
            let mut failure = None;
            for (index, (statement, is_read)) in statements.iter().zip(read_statements).enumerate() {
                let started = std::time::Instant::now();
                let statement_result = if *is_read {
                    use futures_util::StreamExt;
                    let mut stream = sqlx::query(statement).fetch(&mut *conn);
                    let mut rows = Vec::new();
                    let res = loop {
                        if rows.len() as u64 >= max_rows {
                            break MySqlQueryMapper::map_rows(rows);
                        }
                        match stream.next().await {
                            Some(Ok(row)) => rows.push(row),
                            Some(Err(error)) => {
                                break Err(DbError::QueryFailed(format!("MySQL statement {index} failed: {error}")));
                            }
                            None => break MySqlQueryMapper::map_rows(rows),
                        }
                    };
                    drop(stream);
                    match res {
                        Ok(mut query_result) => {
                            query_result.duration_ms = started.elapsed().as_millis() as u64;
                            Ok(db_pro_core::ports::TransactionStatementResult::Query(query_result))
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    match sqlx::query(statement).execute(&mut *conn).await {
                        Ok(result) => Ok(db_pro_core::ports::TransactionStatementResult::Affected {
                            row_count: result.rows_affected(),
                            duration_ms: started.elapsed().as_millis() as u64,
                        }),
                        Err(error) => Err(DbError::QueryFailed(format!("MySQL statement {index} failed: {error}"))),
                    }
                };
                match statement_result {
                    Ok(result) => results.push(result),
                    Err(error) => {
                        failure = Some(TransactionFailure {
                            phase: TransactionFailurePhase::Statement,
                            statement_index: index,
                            outcome: TransactionFailureOutcome::LeftOpen,
                            results: std::mem::take(&mut results),
                            error,
                        });
                        break;
                    }
                }
            }
            match failure {
                Some(failure) => Err(failure),
                None => Ok(results),
            }
        };
        lease.succeed();
        result
    }

    async fn execute_parameterized_transaction_joined(
        &self,
        handle: &ConnectionHandle,
        mut lease: crate::transaction_session::SessionLease<sqlx::pool::PoolConnection<sqlx::MySql>>,
        timeout: Duration,
        statements: &[db_pro_core::ports::ParameterizedTransactionStatement],
    ) -> Result<Vec<db_pro_core::ports::TransactionStatementResult>, db_pro_core::ports::TransactionFailure> {
        use db_pro_core::ports::{TransactionFailureOutcome, TransactionStatementResult};

        let result = {
            let conn = std::ops::DerefMut::deref_mut(lease.conn());
            let connection_id = match sqlx::query_scalar::<_, u64>("SELECT CONNECTION_ID()")
                .fetch_one(&mut *conn)
                .await
            {
                Ok(connection_id) => connection_id,
                Err(error) => {
                    return Err(boundary_left_open(DbError::QueryFailed(format!(
                        "MySQL connection id failed: {error}"
                    ))));
                }
            };
            let _active = self.track_guard(handle.0, connection_id);
            let savepoint = crate::transaction_session::savepoint_name();
            if let Err(error) = mysql_text_execute(&mut *conn, &format!("SAVEPOINT {savepoint}")).await {
                return Err(boundary_left_open(DbError::QueryFailed(format!(
                    "MySQL SAVEPOINT failed: {error}"
                ))));
            }
            let deadline = std::time::Instant::now() + timeout;
            let mut results = Vec::with_capacity(statements.len());
            let mut failure = None;
            for (index, statement) in statements.iter().enumerate() {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    failure = Some(left_open(
                        index,
                        std::mem::take(&mut results),
                        DbError::QueryTimeout {
                            timeout_ms: timeout.as_millis() as u64,
                        },
                    ));
                    break;
                }
                let execution = tokio::time::timeout(remaining, async {
                    let mut args = sqlx::mysql::MySqlArguments::default();
                    super::query_mapper::bind_params(&statement.params, &mut args)?;
                    let affected = sqlx::query_with(&statement.sql, args)
                        .execute(&mut *conn)
                        .await
                        .map_err(|e| DbError::QueryFailed(format!("MySQL statement {index} failed: {e}")))?
                        .rows_affected();
                    Ok::<u64, DbError>(affected)
                })
                .await;
                let affected = match execution {
                    Ok(Ok(affected)) => affected,
                    Ok(Err(error)) => {
                        failure = Some(left_open(index, std::mem::take(&mut results), error));
                        break;
                    }
                    Err(_) => {
                        failure = Some(left_open(
                            index,
                            std::mem::take(&mut results),
                            DbError::QueryTimeout {
                                timeout_ms: timeout.as_millis() as u64,
                            },
                        ));
                        break;
                    }
                };
                if statement.expect_affected_rows && affected == 0 {
                    let error = DbError::Conflict("table mutation affected no rows".into());
                    failure = Some(left_open(index, std::mem::take(&mut results), error));
                    break;
                }
                if statement.max_affected_rows.is_some_and(|maximum| affected > maximum) {
                    let error = DbError::Internal(format!(
                        "table mutation affected {affected} rows; expected at most {}",
                        statement.max_affected_rows.unwrap_or_default()
                    ));
                    failure = Some(left_open(index, std::mem::take(&mut results), error));
                    break;
                }
                results.push(TransactionStatementResult::Affected {
                    row_count: affected,
                    duration_ms: 0,
                });
            }
            if let Some(mut failure) = failure {
                let rollback = mysql_text_execute(&mut *conn, &format!("ROLLBACK TO SAVEPOINT {savepoint}")).await;
                if let Err(rollback_error) = rollback {
                    failure.outcome = TransactionFailureOutcome::Unknown;
                    failure.error = DbError::Internal(format!(
                        "{}; savepoint rollback failed: {rollback_error}",
                        failure.error
                    ));
                    lease.discard_if_abandoned();
                    return Err(failure);
                }
                if let Err(release_error) =
                    mysql_text_execute(&mut *conn, &format!("RELEASE SAVEPOINT {savepoint}")).await
                {
                    failure.error =
                        DbError::Internal(format!("{}; savepoint release failed: {release_error}", failure.error));
                }
                return Err(failure);
            }
            if let Err(error) = mysql_text_execute(&mut *conn, &format!("RELEASE SAVEPOINT {savepoint}")).await {
                return Err(left_open(
                    statements.len(),
                    results,
                    DbError::QueryFailed(format!("MySQL RELEASE SAVEPOINT failed: {error}")),
                ));
            }
            Ok(results)
        };
        lease.succeed();
        result
    }

    fn untrack_connection(&self, handle_id: u64, connection_id: u64) {
        let Ok(pools) = self.pools.try_read() else {
            return;
        };
        let Some(entry) = pools.get(&handle_id) else {
            return;
        };
        let mut active = entry
            .active_connections
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(index) = active.iter().position(|id| *id == connection_id) {
            active.swap_remove(index);
        }
    }
}

/// Removes a `CONNECTION_ID()` when the statement future ends, including on error.
struct ActiveMySql<'a> {
    connector: &'a MySqlConnector,
    handle_id: u64,
    connection_id: u64,
}

impl Drop for ActiveMySql<'_> {
    fn drop(&mut self) {
        self.connector.untrack_connection(self.handle_id, self.connection_id);
    }
}

impl Default for MySqlConnector {
    fn default() -> Self {
        Self::new()
    }
}

async fn open_pool(config: &ConnectionConfig, password: &str) -> Result<MySqlPool, DbError> {
    let ssl_mode = match config.ssl_mode {
        SslMode::Disable => MySqlSslMode::Disabled,
        SslMode::Require => MySqlSslMode::Required,
        SslMode::VerifyCa => MySqlSslMode::VerifyCa,
        SslMode::VerifyFull => MySqlSslMode::VerifyIdentity,
    };
    let options = MySqlConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.username)
        .password(password)
        .database(&config.database)
        .ssl_mode(ssl_mode);
    MySqlPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
        .map_err(|error| DbError::ConnectionFailed(format!("MySQL connect failed: {error}")))
}

#[async_trait]
impl DbConnector for MySqlConnector {
    async fn connect(&self, config: &ConnectionConfig, password: &str) -> Result<ConnectionHandle, DbError> {
        let pool = open_pool(config, password).await?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.pools.write().await.insert(
            id,
            MySqlEntry {
                pool,
                max_rows: config.max_rows,
                query_timeout: Duration::from_millis(config.query_timeout_ms.max(100)),
                active_connections: Mutex::new(Vec::new()),
                open_session: crate::transaction_session::SessionSlot::default(),
            },
        );
        Ok(ConnectionHandle::new(id))
    }

    async fn disconnect(&self, handle: &ConnectionHandle) -> Result<(), DbError> {
        if let Some(entry) = self.pools.write().await.remove(&handle.0) {
            entry.pool.close().await;
        }
        Ok(())
    }

    async fn test_connection(&self, config: &ConnectionConfig, password: &str) -> Result<(), DbError> {
        open_pool(config, password).await?.close().await;
        Ok(())
    }

    async fn query(
        &self,
        handle: &ConnectionHandle,
        sql: &str,
        params: &[QueryParam],
    ) -> Result<db_pro_core::domain::query::QueryResult, DbError> {
        let pools = self.pools.read().await;
        let entry = pools
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("no MySQL pool for handle".into()))?;
        let timeout = entry.query_timeout;
        let max_rows = entry.max_rows;
        let pool = entry.pool.clone();
        let session = entry.open_session.clone();
        drop(pools);

        let mut args = sqlx::mysql::MySqlArguments::default();
        super::query_mapper::bind_params(params, &mut args)?;

        let control = db_pro_core::domain::transaction_control::transaction_control(sql);
        let mut lease = crate::transaction_session::checkout(
            session,
            || async {
                pool.acquire()
                    .await
                    .map_err(|error| DbError::QueryFailed(format!("MySQL acquire failed: {error}")))
            },
            control,
        )
        .await?;
        let query_future = async {
            let conn = std::ops::DerefMut::deref_mut(lease.conn());
            let connection_id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
                .fetch_one(&mut *conn)
                .await
                .map_err(|error| DbError::QueryFailed(format!("MySQL connection id failed: {error}")))?;
            let _active = self.track_guard(handle.0, connection_id);

            use futures_util::StreamExt;
            let mut stream = sqlx::query_with(sql, args).fetch(&mut *conn);
            let mut rows = Vec::new();
            let result = loop {
                if rows.len() as u64 >= max_rows {
                    break MySqlQueryMapper::map_rows(rows);
                }
                match stream.next().await {
                    Some(Ok(row)) => rows.push(row),
                    Some(Err(error)) => {
                        break Err(DbError::QueryFailed(format!("MySQL query failed: {error}")));
                    }
                    None => break MySqlQueryMapper::map_rows(rows),
                }
            };
            drop(stream);
            result
        };
        let result = tokio::time::timeout(timeout, query_future)
            .await
            .map_err(|_| DbError::QueryTimeout {
                timeout_ms: timeout.as_millis() as u64,
            })?;
        if result.is_ok() {
            lease.succeed();
        }
        result
    }

    async fn execute(&self, handle: &ConnectionHandle, sql: &str, params: &[QueryParam]) -> Result<u64, DbError> {
        let pools = self.pools.read().await;
        let entry = pools
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("no MySQL pool for handle".into()))?;
        let timeout = entry.query_timeout;
        let pool = entry.pool.clone();
        let session = entry.open_session.clone();
        drop(pools);
        let mut args = sqlx::mysql::MySqlArguments::default();
        super::query_mapper::bind_params(params, &mut args)?;

        let control = db_pro_core::domain::transaction_control::transaction_control(sql);
        let mut lease = crate::transaction_session::checkout(
            session,
            || async {
                pool.acquire()
                    .await
                    .map_err(|error| DbError::QueryFailed(format!("MySQL acquire failed: {error}")))
            },
            control,
        )
        .await?;
        let exec_future = async {
            let conn = std::ops::DerefMut::deref_mut(lease.conn());
            let connection_id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
                .fetch_one(&mut *conn)
                .await
                .map_err(|error| DbError::QueryFailed(format!("MySQL connection id failed: {error}")))?;
            let _active = self.track_guard(handle.0, connection_id);
            if control.is_some() && params.is_empty() {
                mysql_text_execute(&mut *conn, sql)
                    .await
                    .map(|result| result.rows_affected())
                    .map_err(|e| DbError::QueryFailed(format!("MySQL execute failed: {}", e)))
            } else {
                sqlx::query_with(sql, args)
                    .execute(&mut *conn)
                    .await
                    .map(|result| result.rows_affected())
                    .map_err(|e| DbError::QueryFailed(format!("MySQL execute failed: {}", e)))
            }
        };
        let result = tokio::time::timeout(timeout, exec_future)
            .await
            .map_err(|_| DbError::QueryTimeout {
                timeout_ms: timeout.as_millis() as u64,
            })?;
        if result.is_ok() {
            lease.succeed();
        }
        result
    }

    async fn cancel(&self, handle: &ConnectionHandle) -> Result<(), DbError> {
        let pools = self.pools.read().await;
        let Some(entry) = pools.get(&handle.0) else {
            return Err(DbError::ConnectionFailed("no MySQL pool for handle".into()));
        };
        let connection_ids = entry
            .active_connections
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let pool = entry.pool.clone();
        drop(pools);
        if connection_ids.is_empty() {
            return Ok(());
        }
        for connection_id in connection_ids {
            // KILL QUERY does not accept a placeholder.
            let statement = format!("KILL QUERY {connection_id}");
            sqlx::query(&statement)
                .execute(&pool)
                .await
                .map_err(|error| DbError::QueryFailed(format!("MySQL cancel failed: {error}")))?;
        }
        Ok(())
    }

    async fn execute_batch(&self, handle: &ConnectionHandle, statements: &[String]) -> Result<u64, DbError> {
        let read_statements = vec![false; statements.len()];
        let results = self
            .execute_transaction(handle, statements, &read_statements)
            .await
            .map_err(|failure| failure.error)?;

        let mut total_affected: u64 = 0;
        for result in results {
            if let db_pro_core::ports::TransactionStatementResult::Affected { row_count, .. } = result {
                total_affected = total_affected.saturating_add(row_count);
            }
        }
        Ok(total_affected)
    }

    async fn execute_transaction(
        &self,
        handle: &ConnectionHandle,
        statements: &[String],
        read_statements: &[bool],
    ) -> Result<Vec<db_pro_core::ports::TransactionStatementResult>, db_pro_core::ports::TransactionFailure> {
        if statements.len() != read_statements.len() {
            return Err(db_pro_core::ports::TransactionFailure {
                phase: db_pro_core::ports::TransactionFailurePhase::Validation,
                statement_index: 0,
                outcome: db_pro_core::ports::TransactionFailureOutcome::NotStarted,
                results: Vec::new(),
                error: DbError::Validation("MySQL transaction statement metadata length mismatch".into()),
            });
        }

        let pools = self.pools.read().await;
        let entry = pools
            .get(&handle.0)
            .ok_or_else(|| db_pro_core::ports::TransactionFailure {
                phase: db_pro_core::ports::TransactionFailurePhase::Validation,
                statement_index: 0,
                outcome: db_pro_core::ports::TransactionFailureOutcome::NotStarted,
                results: Vec::new(),
                error: DbError::ConnectionFailed("no MySQL pool for handle".into()),
            })?;
        let pool = entry.pool.clone();
        let session = entry.open_session.clone();
        drop(pools);

        let joined = match crate::transaction_session::checkout_if_held(session, || async {
            pool.acquire()
                .await
                .map_err(|error| DbError::ConnectionFailed(format!("MySQL acquire failed: {error}")))
        })
        .await
        {
            Ok(joined) => joined,
            Err(error) => {
                return Err(db_pro_core::ports::TransactionFailure {
                    phase: db_pro_core::ports::TransactionFailurePhase::Begin,
                    statement_index: 0,
                    outcome: db_pro_core::ports::TransactionFailureOutcome::NotStarted,
                    results: Vec::new(),
                    error,
                });
            }
        };
        if let Some(lease) = joined {
            return self
                .execute_transaction_joined(handle, lease, statements, read_statements)
                .await;
        }

        let mut tx = pool.begin().await.map_err(|e| db_pro_core::ports::TransactionFailure {
            phase: db_pro_core::ports::TransactionFailurePhase::Begin,
            statement_index: 0,
            outcome: db_pro_core::ports::TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::QueryFailed(format!("MySQL transaction begin failed: {}", e)),
        })?;
        let _active = match sqlx::query_scalar::<_, u64>("SELECT CONNECTION_ID()")
            .fetch_one(&mut *tx)
            .await
        {
            Ok(connection_id) => Some(self.track_guard(handle.0, connection_id)),
            Err(_) => None,
        };

        let mut results = Vec::with_capacity(statements.len());
        for (idx, (stmt, is_read)) in statements.iter().zip(read_statements).enumerate() {
            let started = std::time::Instant::now();
            if *is_read {
                match sqlx::query(stmt).fetch_all(&mut *tx).await {
                    Ok(rows) => match MySqlQueryMapper::map_rows(rows) {
                        Ok(mut query_result) => {
                            query_result.duration_ms = started.elapsed().as_millis() as u64;
                            results.push(db_pro_core::ports::TransactionStatementResult::Query(query_result));
                        }
                        Err(e) => {
                            if let Err(rollback_error) = tx.rollback().await {
                                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                                tracing::warn!(statement_index = idx, error = %rollback_error, "MySQL rollback after row mapping error also failed");
                            }
                            return Err(db_pro_core::ports::TransactionFailure {
                                phase: db_pro_core::ports::TransactionFailurePhase::Statement,
                                statement_index: idx,
                                outcome: db_pro_core::ports::TransactionFailureOutcome::RolledBack,
                                results,
                                error: e,
                            });
                        }
                    },
                    Err(e) => {
                        if let Err(rollback_error) = tx.rollback().await {
                            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                            tracing::warn!(statement_index = idx, error = %rollback_error, "MySQL rollback after failed query also failed — transaction may still be open on the connection");
                        }
                        return Err(db_pro_core::ports::TransactionFailure {
                            phase: db_pro_core::ports::TransactionFailurePhase::Statement,
                            statement_index: idx,
                            outcome: db_pro_core::ports::TransactionFailureOutcome::RolledBack,
                            results,
                            error: DbError::QueryFailed(format!("MySQL statement {} failed: {}", idx, e)),
                        });
                    }
                }
            } else {
                match sqlx::query(stmt).execute(&mut *tx).await {
                    Ok(result) => {
                        results.push(db_pro_core::ports::TransactionStatementResult::Affected {
                            row_count: result.rows_affected(),
                            duration_ms: started.elapsed().as_millis() as u64,
                        });
                    }
                    Err(e) => {
                        if let Err(rollback_error) = tx.rollback().await {
                            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                            tracing::warn!(statement_index = idx, error = %rollback_error, "MySQL rollback after failed statement also failed — transaction may still be open on the connection");
                        }
                        return Err(db_pro_core::ports::TransactionFailure {
                            phase: db_pro_core::ports::TransactionFailurePhase::Statement,
                            statement_index: idx,
                            outcome: db_pro_core::ports::TransactionFailureOutcome::RolledBack,
                            results,
                            error: DbError::QueryFailed(format!("MySQL statement {} failed: {}", idx, e)),
                        });
                    }
                }
            }
        }

        tx.commit().await.map_err(|e| db_pro_core::ports::TransactionFailure {
            phase: db_pro_core::ports::TransactionFailurePhase::Commit,
            statement_index: statements.len(),
            outcome: db_pro_core::ports::TransactionFailureOutcome::Unknown,
            results: Vec::new(),
            error: DbError::QueryFailed(format!("MySQL commit failed: {}", e)),
        })?;

        Ok(results)
    }

    async fn execute_parameterized_transaction(
        &self,
        handle: &ConnectionHandle,
        statements: &[db_pro_core::ports::ParameterizedTransactionStatement],
    ) -> Result<Vec<db_pro_core::ports::TransactionStatementResult>, db_pro_core::ports::TransactionFailure> {
        use db_pro_core::ports::{
            TransactionFailure, TransactionFailureOutcome, TransactionFailurePhase, TransactionStatementResult,
        };

        let pools = self.pools.read().await;
        let entry = pools.get(&handle.0).ok_or_else(|| TransactionFailure {
            phase: TransactionFailurePhase::Validation,
            statement_index: 0,
            outcome: TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::ConnectionFailed("no MySQL pool for handle".into()),
        })?;
        let timeout = entry.query_timeout;
        let pool = entry.pool.clone();
        let session = entry.open_session.clone();
        drop(pools);

        let joined = match crate::transaction_session::checkout_if_held(session, || async {
            pool.acquire()
                .await
                .map_err(|error| DbError::ConnectionFailed(format!("MySQL acquire failed: {error}")))
        })
        .await
        {
            Ok(joined) => joined,
            Err(error) => {
                return Err(TransactionFailure {
                    phase: TransactionFailurePhase::Begin,
                    statement_index: 0,
                    outcome: TransactionFailureOutcome::NotStarted,
                    results: Vec::new(),
                    error,
                });
            }
        };
        if let Some(lease) = joined {
            return self
                .execute_parameterized_transaction_joined(handle, lease, timeout, statements)
                .await;
        }

        let mut tx = pool.begin().await.map_err(|e| TransactionFailure {
            phase: TransactionFailurePhase::Begin,
            statement_index: 0,
            outcome: TransactionFailureOutcome::NotStarted,
            results: Vec::new(),
            error: DbError::QueryFailed(format!("MySQL transaction begin failed: {e}")),
        })?;
        let _active = match sqlx::query_scalar::<_, u64>("SELECT CONNECTION_ID()")
            .fetch_one(&mut *tx)
            .await
        {
            Ok(connection_id) => Some(self.track_guard(handle.0, connection_id)),
            Err(_) => None,
        };

        let mut results = Vec::with_capacity(statements.len());
        for (index, statement) in statements.iter().enumerate() {
            let mut args = sqlx::mysql::MySqlArguments::default();
            if let Err(error) = super::query_mapper::bind_params(&statement.params, &mut args) {
                let (error, outcome) = match tx.rollback().await {
                    Ok(()) => (error, TransactionFailureOutcome::RolledBack),
                    Err(rollback_error) => (
                        DbError::Internal(format!("bind failed: {error}; rollback failed: {rollback_error}")),
                        TransactionFailureOutcome::Unknown,
                    ),
                };
                return Err(TransactionFailure {
                    phase: TransactionFailurePhase::Statement,
                    statement_index: index,
                    outcome,
                    results,
                    error,
                });
            }

            let affected = match sqlx::query_with(&statement.sql, args).execute(&mut *tx).await {
                Ok(result) => result.rows_affected(),
                Err(e) => {
                    let error = DbError::QueryFailed(format!("MySQL statement {index} failed: {e}"));
                    let (error, outcome) = match tx.rollback().await {
                        Ok(()) => (error, TransactionFailureOutcome::RolledBack),
                        Err(rollback_error) => (
                            DbError::Internal(format!("statement failed: {error}; rollback failed: {rollback_error}")),
                            TransactionFailureOutcome::Unknown,
                        ),
                    };
                    return Err(TransactionFailure {
                        phase: TransactionFailurePhase::Statement,
                        statement_index: index,
                        outcome,
                        results,
                        error,
                    });
                }
            };

            if statement.expect_affected_rows && affected == 0 {
                let error = DbError::Conflict("table mutation affected no rows".into());
                let (error, outcome) = match tx.rollback().await {
                    Ok(()) => (error, TransactionFailureOutcome::RolledBack),
                    Err(rollback_error) => (
                        DbError::Internal(format!("mutation affected no rows; rollback failed: {rollback_error}")),
                        TransactionFailureOutcome::Unknown,
                    ),
                };
                return Err(TransactionFailure {
                    phase: TransactionFailurePhase::Statement,
                    statement_index: index,
                    outcome,
                    results,
                    error,
                });
            }
            if statement.max_affected_rows.is_some_and(|maximum| affected > maximum) {
                let error = DbError::Internal(format!(
                    "table mutation affected {affected} rows; expected at most {}",
                    statement.max_affected_rows.unwrap_or_default()
                ));
                let (error, outcome) = match tx.rollback().await {
                    Ok(()) => (error, TransactionFailureOutcome::RolledBack),
                    Err(rollback_error) => (
                        DbError::Internal(format!(
                            "mutation invariant failed: {error}; rollback failed: {rollback_error}"
                        )),
                        TransactionFailureOutcome::Unknown,
                    ),
                };
                return Err(TransactionFailure {
                    phase: TransactionFailurePhase::Statement,
                    statement_index: index,
                    outcome,
                    results,
                    error,
                });
            }

            results.push(TransactionStatementResult::Affected {
                row_count: affected,
                duration_ms: 0,
            });
        }

        if let Err(e) = tx.commit().await {
            return Err(TransactionFailure {
                phase: TransactionFailurePhase::Commit,
                statement_index: statements.len(),
                outcome: TransactionFailureOutcome::Unknown,
                results,
                error: DbError::QueryFailed(format!("MySQL commit failed: {e}")),
            });
        }

        Ok(results)
    }

    async fn introspect(&self, handle: &ConnectionHandle) -> Result<IntrospectResult, DbError> {
        let pool = self
            .get_pool(handle)
            .await
            .ok_or_else(|| DbError::ConnectionFailed("no MySQL pool for handle".into()))?;
        MySqlIntrospect::introspect(&pool).await
    }

    async fn explain(
        &self,
        handle: &ConnectionHandle,
        sql: &str,
        params: &[QueryParam],
        analyze: bool,
    ) -> Result<serde_json::Value, DbError> {
        let pools = self.pools.read().await;
        let entry = pools
            .get(&handle.0)
            .ok_or_else(|| DbError::ConnectionFailed("no MySQL pool for handle".into()))?;
        let pool = entry.pool.clone();
        let session = entry.open_session.clone();
        drop(pools);

        let explain_sql = if analyze {
            format!("EXPLAIN ANALYZE {}", sql)
        } else {
            format!("EXPLAIN {}", sql)
        };
        let mut lease = crate::transaction_session::checkout(
            session,
            || async {
                pool.acquire()
                    .await
                    .map_err(|error| DbError::QueryFailed(format!("MySQL acquire failed: {error}")))
            },
            None,
        )
        .await?;
        let mut args = sqlx::mysql::MySqlArguments::default();
        super::query_mapper::bind_params(params, &mut args)?;
        let rows = sqlx::query_with(&explain_sql, args)
            .fetch_all(std::ops::DerefMut::deref_mut(lease.conn()))
            .await
            .map_err(|e| DbError::QueryFailed(format!("MySQL EXPLAIN failed: {}", e)))?;
        lease.succeed();

        let json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (i, col) in row.columns().iter().enumerate() {
                    let val: Option<String> = row.try_get(i).ok().flatten();
                    map.insert(
                        col.name().to_string(),
                        val.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                    );
                }
                serde_json::Value::Object(map)
            })
            .collect();

        Ok(serde_json::Value::Array(json_rows))
    }

    fn dialect(&self, _handle: &ConnectionHandle) -> Result<Box<dyn db_pro_core::ports::SqlDialect>, DbError> {
        Ok(Box::new(MySqlDialect))
    }
}

struct MySqlDialect;
impl db_pro_core::ports::SqlDialect for MySqlDialect {
    fn placeholder(&self, _index: usize) -> String {
        "?".to_string()
    }
    fn quote_identifier(&self, name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }
    fn text_like(&self, column: &str, placeholder: &str) -> String {
        format!("CAST({column} AS CHAR) LIKE {placeholder}")
    }
}

/// MySQL rejects SAVEPOINT/BEGIN/COMMIT/ROLLBACK on the prepared protocol (ERROR 1295).
/// COM_QUERY via `raw_sql`. Concrete `Executor` on `&mut MySqlConnection` (no HRTB / async block).
fn mysql_text_execute<'c>(
    conn: &'c mut sqlx::MySqlConnection,
    sql: &'c str,
) -> futures_util::future::BoxFuture<'c, Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>> {
    use sqlx::Executor;
    conn.execute(sqlx::raw_sql(sql))
}

fn left_open(
    statement_index: usize,
    results: Vec<db_pro_core::ports::TransactionStatementResult>,
    error: DbError,
) -> db_pro_core::ports::TransactionFailure {
    db_pro_core::ports::TransactionFailure {
        phase: db_pro_core::ports::TransactionFailurePhase::Statement,
        statement_index,
        outcome: db_pro_core::ports::TransactionFailureOutcome::LeftOpen,
        results,
        error,
    }
}

fn boundary_left_open(error: DbError) -> db_pro_core::ports::TransactionFailure {
    db_pro_core::ports::TransactionFailure {
        phase: db_pro_core::ports::TransactionFailurePhase::Begin,
        statement_index: 0,
        outcome: db_pro_core::ports::TransactionFailureOutcome::LeftOpen,
        results: Vec::new(),
        error,
    }
}

#[cfg(test)]
#[path = "tests/connector_tests.rs"]
mod tests;
