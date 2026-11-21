use serde::{Deserialize, Serialize};

mod string_i64 {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<i64>().map_err(serde::de::Error::custom)
    }
}

/// Bound statement parameter. `PartialEq` (not `Eq`): `Float64` carries `f64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum QueryParam {
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "bool")]
    Bool(bool),
    #[serde(rename = "int64")]
    Int64(#[serde(with = "string_i64")] i64),
    #[serde(rename = "float64")]
    Float64(f64),
    #[serde(rename = "decimal")]
    Decimal(String),
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "bytes")]
    Bytes(Vec<u8>),
    #[serde(rename = "uuid")]
    Uuid(String),
    #[serde(rename = "datetime")]
    DateTime(String),
    #[serde(rename = "time")]
    Time(String),
    #[serde(rename = "interval")]
    Interval(String),
    #[serde(rename = "inet")]
    Inet(String),
    #[serde(rename = "json")]
    Json(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum CellValue {
    #[serde(rename = "null")]
    Null,
    #[serde(rename = "bool")]
    Bool(bool),
    #[serde(rename = "int64")]
    Int64(#[serde(with = "string_i64")] i64),
    #[serde(rename = "float64")]
    Float64(f64),
    #[serde(rename = "decimal")]
    Decimal(String),
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "bytes")]
    Bytes(Vec<u8>),
    #[serde(rename = "uuid")]
    Uuid(String),
    #[serde(rename = "datetime")]
    DateTime(String),
    /// `TIMESTAMP`: a local date-time that carries no timezone at all, so the
    /// canonical string never gains a `Z` or an offset — `YYYY-MM-DDTHH:MM:SS.ffffff`.
    #[serde(rename = "timestamp")]
    Timestamp(String),
    /// `TIMESTAMPTZ`: an absolute instant, normalized to UTC — `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
    #[serde(rename = "timestamptz")]
    TimestampTz(String),
    /// `TIMETZ`: a wall-clock time with its own explicit offset, which is not the
    /// session's — `HH:MM:SS.ffffff±HH:MM`.
    #[serde(rename = "timetz")]
    TimeTz(String),
    #[serde(rename = "date")]
    Date(String),
    #[serde(rename = "time")]
    Time(String),
    #[serde(rename = "interval")]
    Interval(String),
    #[serde(rename = "inet")]
    Inet(String),
    #[serde(rename = "json")]
    Json(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row(pub Vec<CellValue>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Row>,
    pub row_count: u64,
    pub duration_ms: u64,
}

impl QueryResult {
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            duration_ms: 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let col_count = self.columns.len();
        if self.columns.is_empty() && !self.rows.is_empty() {
            return Err("result has returned rows but no columns".into());
        }
        for (i, row) in self.rows.iter().enumerate() {
            if row.0.len() != col_count {
                return Err(format!("row {i} has {} cells but expected {col_count}", row.0.len()));
            }
        }
        if !self.columns.is_empty() && self.row_count != self.rows.len() as u64 {
            return Err(format!(
                "row_count {} does not match {} returned rows",
                self.row_count,
                self.rows.len()
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum QueryError {
    #[error("connection not found: {connection_id}")]
    ConnectionNotFound { connection_id: String },

    #[error("syntax error at line {line}: {message}")]
    SyntaxError { line: u32, message: String },

    #[error("permission denied on table: {table}")]
    PermissionDenied { table: String },

    #[error("query timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("connection lost")]
    ConnectionLost,

    #[error("multi-statement execution is disabled")]
    MultiStatementDisabled,

    #[error("unsupported parameter type: {0}")]
    UnsupportedParameterType(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl QueryError {
    pub fn code(&self) -> &'static str {
        match self {
            QueryError::ConnectionNotFound { .. } => "CONNECTION_NOT_FOUND",
            QueryError::SyntaxError { .. } => "SYNTAX_ERROR",
            QueryError::PermissionDenied { .. } => "PERMISSION_DENIED",
            QueryError::Timeout { .. } => "QUERY_TIMEOUT",
            QueryError::ConnectionLost => "CONNECTION_LOST",
            QueryError::MultiStatementDisabled => "MULTI_STATEMENT_DISABLED",
            QueryError::UnsupportedParameterType(_) => "UNSUPPORTED_PARAMETER_TYPE",
            QueryError::Validation(_) => "VALIDATION_ERROR",
            QueryError::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn message_id(&self) -> &'static str {
        match self {
            QueryError::ConnectionNotFound { .. } => "error.connection.not_found",
            QueryError::SyntaxError { .. } => "error.query.syntax",
            QueryError::PermissionDenied { .. } => "error.query.permission",
            QueryError::Timeout { .. } => "error.query.timeout",
            QueryError::ConnectionLost => "error.connection.lost",
            QueryError::MultiStatementDisabled => "error.query.multi_statement",
            QueryError::UnsupportedParameterType(_) => "error.query.unsupported_param",
            QueryError::Validation(_) => "error.validation",
            QueryError::Internal(_) => "error.internal",
        }
    }
}

#[cfg(test)]
#[path = "tests/query_tests.rs"]
mod tests;
