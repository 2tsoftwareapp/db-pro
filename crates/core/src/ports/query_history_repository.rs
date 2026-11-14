use async_trait::async_trait;

use crate::domain::connection::ConnectionId;
use crate::domain::error::DbError;
use crate::domain::history::QueryHistory;
use crate::domain::query::QueryResult;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait QueryHistoryRepository: Send + Sync {
    /// Persist one history entry.
    ///
    /// `result` is `None` when the statement failed before producing a result set
    /// (#600) — failed statements are recorded with `success = false` and the error
    /// text, because that is when the SQL is most worth keeping.
    async fn save(
        &self,
        connection_id: &ConnectionId,
        sql: &str,
        result: Option<&QueryResult>,
        database: Option<String>,
        schema: Option<String>,
        error_message: Option<String>,
    ) -> Result<(), DbError>;

    async fn list(&self, connection_id: &ConnectionId, limit: u32) -> Result<Vec<QueryHistory>, DbError>;
}
