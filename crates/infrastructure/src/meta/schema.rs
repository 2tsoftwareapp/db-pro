pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS connections (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS query_history (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    sql TEXT NOT NULL,
    executed_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    row_count INTEGER NOT NULL,
    database TEXT,
    schema TEXT,
    -- #600: failed statements are recorded too, with their redacted error text.
    success INTEGER NOT NULL DEFAULT 1,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_query_history_conn
    ON query_history(connection_id, executed_at DESC);

CREATE TABLE IF NOT EXISTS saved_queries (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    name TEXT NOT NULL,
    sql TEXT NOT NULL,
    folder TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_saved_queries_conn
    ON saved_queries(connection_id);

CREATE TABLE IF NOT EXISTS saved_query_folders (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_saved_query_folders_conn
    ON saved_query_folders(connection_id);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    default_connection_id TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS introspection_cache (
    connection_id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_configs (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    name TEXT NOT NULL,
    sql TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL DEFAULT 30000,
    max_rows INTEGER NOT NULL DEFAULT 1000,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_run_configs_conn
    ON run_configs(connection_id);
"#;
