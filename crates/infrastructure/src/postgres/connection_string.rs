use db_pro_core::domain::connection::{ConnectionConfig, SslMode};
use db_pro_core::domain::error::DbError;
use sqlx::postgres::PgConnectOptions;

// cc-scan:allow LONG_FUNCTION — one assignment per connection-string field
pub fn build_options(config: &ConnectionConfig, password: &str) -> Result<PgConnectOptions, DbError> {
    let ssl_mode = match config.ssl_mode {
        SslMode::Disable => sqlx::postgres::PgSslMode::Disable,
        SslMode::Require => sqlx::postgres::PgSslMode::Require,
        SslMode::VerifyCa => sqlx::postgres::PgSslMode::VerifyCa,
        SslMode::VerifyFull => sqlx::postgres::PgSslMode::VerifyFull,
    };

    let mut options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.username)
        .password(password)
        .database(&config.database)
        .ssl_mode(ssl_mode);

    if let Some(path) = config
        .ssl_root_cert_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        options = options.ssl_root_cert(path);
    }
    if let Some(path) = config
        .ssl_client_cert_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        options = options.ssl_client_cert(path);
    }
    if config.readonly {
        // The classifier only sees the statement text, so `SELECT
        // pg_terminate_backend(…)`, `SELECT setval(…)` or `SELECT my_writer()`
        // passed as a read while still mutating the server (#588). The server
        // enforces it instead: every transaction this session opens is read-only,
        // and `SET default_transaction_read_only = off` cannot lift it because
        // the session default is re-applied per transaction start.
        options = options.options(std::iter::once(("default_transaction_read_only", "on")));
    }

    if let Some(path) = config
        .ssl_client_key_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        options = options.ssl_client_key(path);
    }

    Ok(options)
}
