use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::QueryParam;

use crate::sqlite::actor::SqliteHandle;

/// A single schema migration.
pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub sql: &'static str,
}

/// Ordered registry of all schema migrations.
///
/// Each migration is applied in order. The `schema_version` table tracks
/// which versions have been applied. Adding a new migration:
///
/// 1. Append to the `MIGRATIONS` array below.
/// 2. Increment `LATEST_VERSION`.
/// 3. Write the SQL as `ALTER TABLE` / `CREATE TABLE IF NOT EXISTS` etc.
/// 4. Never modify an already-applied migration — only add new ones.
pub const LATEST_VERSION: u32 = 4;

pub static MIGRATIONS: &[Migration] = &[
    // v1 is the baseline schema created by `SCHEMA` constant.
    // No migration SQL needed — the initial `CREATE TABLE IF NOT EXISTS`
    // statements handle bootstrap.
    Migration {
        version: 1,
        description: "initial schema with versioning table",
        sql: "",
    },
    Migration {
        version: 2,
        description: "add database and schema columns to query_history",
        sql: "ALTER TABLE query_history ADD COLUMN database TEXT; ALTER TABLE query_history ADD COLUMN schema TEXT;",
    },
    Migration {
        version: 3,
        description: "add success and error_message columns to query_history (#600)",
        sql: "ALTER TABLE query_history ADD COLUMN success INTEGER NOT NULL DEFAULT 1",
    },
    Migration {
        version: 4,
        description: "add error_message column to query_history (#600)",
        sql: "ALTER TABLE query_history ADD COLUMN error_message TEXT",
    },
];

/// Get the current schema version from the database, or 0 if no versioning exists.
pub async fn current_version(handle: &SqliteHandle) -> Result<u32, DbError> {
    let rows = handle
        .raw_query("SELECT COALESCE(MAX(version), 0) FROM schema_version".into(), vec![])
        .await?;

    let version_text = rows
        .first()
        .and_then(|row| row.first())
        .ok_or_else(|| DbError::Internal("schema_version query returned no version".into()))?;
    let version = version_text
        .parse::<u32>()
        .map_err(|error| DbError::Internal(format!("invalid schema version {version_text:?}: {error}")))?;

    Ok(version)
}

/// `ALTER TABLE query_history ADD COLUMN` when `column` is absent (#600).
///
/// Migrations are re-runnable: a partially applied version must not fail on the
/// columns it already added.
async fn add_column_if_missing(
    handle: &SqliteHandle,
    table_info: &[Vec<String>],
    column: &str,
    sql_type: &str,
) -> Result<(), DbError> {
    let present = table_info
        .iter()
        .any(|col| col.get(1).map(String::as_str) == Some(column));
    if present {
        return Ok(());
    }
    handle
        .execute_statement(format!("ALTER TABLE query_history ADD COLUMN {column} {sql_type}"))
        .await?;
    Ok(())
}

/// Apply all pending migrations to bring the schema up to `LATEST_VERSION`.
///
/// This is idempotent: already-applied versions are skipped.
// cc-scan:allow COMPLEXITY,HARD_COMPLEXITY,LONG_FUNCTION — per-version ladder, one branch per column
pub async fn migrate(handle: &SqliteHandle) -> Result<u32, DbError> {
    let current = current_version(handle).await?;
    if current > LATEST_VERSION {
        return Err(DbError::Internal(format!(
            "database schema version {current} is newer than supported version {LATEST_VERSION}"
        )));
    }

    for migration in MIGRATIONS {
        if migration.version <= current {
            continue;
        }

        // Execute migration SQL if non-empty.
        if !migration.sql.is_empty() {
            if matches!(migration.version, 2..=4) {
                // To avoid duplicate column errors, check each column individually
                // and only add missing ones. This handles partially applied v2 migrations.
                let table_info = handle
                    .raw_query("PRAGMA table_info(query_history)".into(), vec![])
                    .await?;
                let has_database = table_info
                    .iter()
                    .any(|col| col.get(1).map(|name| name.as_str()) == Some("database"));
                let has_schema = table_info
                    .iter()
                    .any(|col| col.get(1).map(|name| name.as_str()) == Some("schema"));

                if !has_database {
                    handle
                        .execute_statement("ALTER TABLE query_history ADD COLUMN database TEXT".into())
                        .await?;
                }
                if !has_schema {
                    handle
                        .execute_statement("ALTER TABLE query_history ADD COLUMN schema TEXT".into())
                        .await?;
                }
                if migration.version >= 3 {
                    add_column_if_missing(handle, &table_info, "success", "INTEGER NOT NULL DEFAULT 1").await?;
                }
                if migration.version >= 4 {
                    add_column_if_missing(handle, &table_info, "error_message", "TEXT").await?;
                }
            } else {
                handle.execute_statement(migration.sql.into()).await?;
            }
        }

        // Record the migration using parameterized query to prevent SQL injection.
        let now = chrono::Utc::now().to_rfc3339();
        handle
            .execute_param(
                "INSERT INTO schema_version (version, applied_at, description) VALUES (?, ?, ?)".into(),
                vec![
                    QueryParam::Int64(migration.version as i64),
                    QueryParam::Text(now),
                    QueryParam::Text(migration.description.to_string()),
                ],
            )
            .await?;
    }

    current_version(handle).await
}

#[cfg(test)]
#[path = "tests/migration_tests.rs"]
mod tests;
