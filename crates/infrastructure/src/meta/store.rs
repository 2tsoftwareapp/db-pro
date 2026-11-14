use db_pro_core::domain::error::DbError;

use super::migration;
use super::schema::SCHEMA;
use crate::sqlite::actor::{SqliteActor, SqliteHandle};

#[derive(Clone)]
pub struct SQLiteMetaStore {
    pub(crate) actor: SqliteHandle,
}

impl SQLiteMetaStore {
    /// Wrap an already-migrated handle (tests, and callers that own the actor).
    #[cfg(test)]
    pub(crate) fn from_handle(actor: SqliteHandle) -> Self {
        Self { actor }
    }

    pub async fn new(db_path: &str) -> Result<Self, DbError> {
        let actor = SqliteActor::spawn(db_path)?;
        // Create baseline schema (all tables use IF NOT EXISTS).
        actor.execute_statement(SCHEMA.into()).await?;
        // Apply any pending migrations to bring schema up to date.
        migration::migrate(&actor).await?;
        Ok(Self { actor })
    }

    pub fn handle(&self) -> &SqliteHandle {
        &self.actor
    }
}
