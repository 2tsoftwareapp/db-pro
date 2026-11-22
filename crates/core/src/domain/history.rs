use serde::{Deserialize, Serialize};

use super::connection::ConnectionId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryHistory {
    pub id: uuid::Uuid,
    pub connection_id: ConnectionId,
    pub sql: String,
    pub executed_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: u64,
    pub row_count: u64,
    pub database: Option<String>,
    pub schema: Option<String>,
    /// Whether the statement completed without error (#600).
    ///
    /// Failed statements are stored too: history is most valuable exactly when the
    /// SQL has to be fixed and re-run.
    pub success: bool,
    /// Server error text for a failed statement, already credential-redacted (#600).
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: uuid::Uuid,
    pub connection_id: ConnectionId,
    pub name: String,
    pub sql: String,
    pub folder: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQueryFolder {
    pub id: uuid::Uuid,
    pub connection_id: ConnectionId,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: uuid::Uuid,
    pub name: String,
    pub default_connection_id: Option<ConnectionId>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub language: String,
    pub default_connection_id: Option<String>,
    pub page_size: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "en".into(),
            default_connection_id: None,
            page_size: 500,
        }
    }
}
