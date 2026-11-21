pub mod agent;
#[path = "agent/executor.rs"]
mod agent_executor;
#[path = "agent/orchestrator.rs"]
mod agent_orchestrator;
mod api;
mod worker;

pub use agent::{AgentContext, AgentDraft, CodexProvider, CodexProviderError, SqlPredictionContext};
pub use agent_executor::{AgentToolExecutor, AgentToolRunner};
pub use agent_orchestrator::{AgentRunOrchestrator, AgentWorkflowEvent};
pub use api::{
    AuditApi, BackupApi, ColumnSummary, ConnectionApi, ConnectionSummary, DataDiffApi, DbErrorDto, ExportApi,
    ForeignKeySummary, FunctionSummary, MonitoringApi, PostgresApi, QueryApi, QueryFolderSummary, RlsApi,
    RoutineParameterSummary, SavedQuerySummary, SchemaApi, SchemaSummary, TableDataApi, TableMutationFailure,
    TableSummary, TriggerSummary, UserApi, ViewSummary,
};
pub use db_pro_core::domain::agent_workflow::AgentExecutionContext;
pub use worker::{spawn_worker, RuntimeCommand, RuntimeEvent, RuntimeRequestId};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use db_pro_core::application::{
    AuditService, BackupService, ConnectionRegistry, ConnectionService, DataDiffService, ExportService,
    ExtensionRegistry, MonitoringService, QueryService, RlsService, SchemaService, TableDataService, UserService,
};
use db_pro_core::domain::extension::example_diagnostics_extension;
use db_pro_core::ports::SecretStore;
use db_pro_infrastructure::backup::pg_dump::PgDumpEngine;
use db_pro_infrastructure::backup::sqlite_backup::SqliteBackupEngine;
use db_pro_infrastructure::connector::CompositeConnector;
use db_pro_infrastructure::meta::store::SQLiteMetaStore;
use db_pro_infrastructure::postgres::monitoring::PostgresMonitoringPort;
use db_pro_infrastructure::postgres::rls_manager::PostgresRlsManager;
use db_pro_infrastructure::postgres::user_manager::PostgresUserManager;
use db_pro_infrastructure::secret::keyring_vault::KeyringVault;
use db_pro_infrastructure::sqlite::monitoring::SqliteMonitoringPort;
use thiserror::Error;

/// The application-owned runtime shared by native UI frontends.
///
/// This type deliberately contains no Tauri or egui types. Tauri commands and
/// the native egui task bridge can both depend on these services without one
/// frontend becoming the owner of application wiring.
pub struct DbProRuntime {
    data_dir: PathBuf,
    connections: Arc<ConnectionService>,
    queries: Arc<QueryService>,
    schema: Arc<SchemaService>,
    table_data: Arc<TableDataService>,
    export: Arc<ExportService>,
    backup: Arc<BackupService>,
    users: Arc<UserService>,
    rls: Arc<RlsService>,
    monitoring: Arc<MonitoringService>,
    audit: Arc<AuditService>,
    extensions: Arc<ExtensionRegistry>,
    data_diff: Arc<DataDiffService>,
    connector: Arc<CompositeConnector>,
    registry: Arc<ConnectionRegistry>,
    meta_store: SQLiteMetaStore,
    secrets: Arc<KeyringVault>,
}

#[derive(Debug, Error)]
pub enum RuntimeInitError {
    #[error("failed to create application data directory: {0}")]
    CreateDataDir(#[source] std::io::Error),
    #[error("failed to initialize metadata store: {0}")]
    Metadata(#[from] db_pro_core::domain::error::DbError),
}

/// Keyring service name used for every DB Pro secret entry.
pub const KEYRING_SERVICE: &str = "com.dbpro.app";

/// Stable secret key for the Agent provider API key.
///
/// Keep the legacy key name so existing installations do not lose their saved
/// key when ownership moves from the native adapter to the runtime vault.
pub const AGENT_API_KEY_SECRET: &str = "agent/groq_api_key";

/// Opt-in that enables the development encrypted-file secret fallback in a release build.
///
/// The file's encryption key is derived from [`KEYRING_SERVICE`], which is not a secret, so the
/// fallback is never enabled in a release build by default (#142).
pub const ALLOW_FILE_SECRET_FALLBACK_ENV: &str = "DB_PRO_ALLOW_FILE_SECRET_FALLBACK";

/// Skip the OS keyring entirely (session + encrypted-file stores only).
///
/// Local `cargo run` / debug iteration should not trigger macOS Keychain / Windows Credential
/// Manager / Secret Service prompts. Set to `1`/`true`, or rely on debug builds / the native
/// app's cargo-target auto-disable. Force the OS keyring back on with [`USE_KEYRING_ENV`].
pub const DISABLE_KEYRING_ENV: &str = "DB_PRO_DISABLE_KEYRING";

/// Force the OS keyring on even in debug / when [`DISABLE_KEYRING_ENV`] would otherwise apply.
pub const USE_KEYRING_ENV: &str = "DB_PRO_USE_KEYRING";

/// Whether the development encrypted-file secret fallback is enabled for this build.
///
/// Debug builds keep it (development and CI have no OS keyring guarantee); a release build only
/// enables it when [`ALLOW_FILE_SECRET_FALLBACK_ENV`] is set to `1` or `true`. When the OS
/// keyring is disabled, the file fallback is also enabled so secrets still survive a restart
/// during local development.
pub fn file_secret_fallback_enabled() -> bool {
    if !os_keyring_enabled() {
        return true;
    }
    file_secret_fallback_enabled_for(
        cfg!(debug_assertions),
        std::env::var(ALLOW_FILE_SECRET_FALLBACK_ENV).ok().as_deref(),
    )
}

/// Whether the OS keyring layer should be contacted for secrets.
///
/// Debug builds default to off (no Keychain prompts while iterating). Release builds default
/// to on. Either side can be overridden with [`DISABLE_KEYRING_ENV`] / [`USE_KEYRING_ENV`].
pub fn os_keyring_enabled() -> bool {
    os_keyring_enabled_for(
        cfg!(debug_assertions),
        std::env::var(USE_KEYRING_ENV).ok().as_deref(),
        std::env::var(DISABLE_KEYRING_ENV).ok().as_deref(),
    )
}

fn env_flag_enabled(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some(v) if v == "1" || v.eq_ignore_ascii_case("true")
    )
}

/// The decision behind [`os_keyring_enabled`], free of build/process-global state.
fn os_keyring_enabled_for(debug_build: bool, use_keyring: Option<&str>, disable_keyring: Option<&str>) -> bool {
    if env_flag_enabled(use_keyring) {
        return true;
    }
    if env_flag_enabled(disable_keyring) {
        return false;
    }
    !debug_build
}

/// The decision behind [`file_secret_fallback_enabled`], free of build/process-global state.
fn file_secret_fallback_enabled_for(debug_build: bool, opt_in: Option<&str>) -> bool {
    if debug_build {
        return true;
    }
    env_flag_enabled(opt_in)
}

/// Build the secret store used by the application.
///
/// The OS keyring is the primary store in shipping builds. Local development skips it (see
/// [`os_keyring_enabled`]) and uses the encrypted-file + in-memory session fallbacks instead so
/// macOS Keychain / platform credential prompts never interrupt `cargo run`.
fn build_secret_store(secrets_dir: PathBuf) -> KeyringVault {
    let use_os_keyring = os_keyring_enabled();
    let service_name = if use_os_keyring {
        KEYRING_SERVICE.to_owned()
    } else {
        // Empty service name disables the keyring layer deterministically (see KeyringVault).
        String::new()
    };
    let mut vault = KeyringVault::new(service_name, secrets_dir).with_session_fallback();
    if !use_os_keyring {
        tracing::info!(
            "{DISABLE_KEYRING_ENV}: OS keyring disabled — secrets use the encrypted-file + session stores \
             (set {USE_KEYRING_ENV}=1 to force the OS keyring)"
        );
    }
    if file_secret_fallback_enabled() {
        if use_os_keyring {
            tracing::warn!(
                "encrypted-file secret fallback enabled (debug build or {ALLOW_FILE_SECRET_FALLBACK_ENV} set): \
                 its key is derived from the service name, so use the OS keyring outside development and CI"
            );
        }
        vault = vault.with_fallback();
    }
    vault
}

impl DbProRuntime {
    /// Build the shared service graph once at application startup.
    pub async fn new(data_dir: impl AsRef<Path>) -> Result<Arc<Self>, RuntimeInitError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).map_err(RuntimeInitError::CreateDataDir)?;

        let secrets_dir = data_dir.join("secrets");
        std::fs::create_dir_all(&secrets_dir).map_err(RuntimeInitError::CreateDataDir)?;

        let meta_path = data_dir.join("meta.db");
        let meta_store = SQLiteMetaStore::new(&meta_path.to_string_lossy()).await?;
        let secret_store = Arc::new(build_secret_store(secrets_dir));
        let connector = Arc::new(CompositeConnector::new());
        let registry = Arc::new(ConnectionRegistry::new());

        let connections = Arc::new(
            ConnectionService::new(
                Box::new(Arc::clone(&connector)),
                Box::new(meta_store.clone()),
                Box::new(Arc::clone(&secret_store)),
                Arc::clone(&registry),
            )
            .with_introspection_cache(Box::new(meta_store.clone())),
        );

        let queries = Arc::new(
            QueryService::new(
                Box::new(Arc::clone(&connector)),
                Box::new(meta_store.clone()),
                Box::new(meta_store.clone()),
                Box::new(meta_store.clone()),
                Arc::clone(&registry),
                Box::new(meta_store.clone()),
            )
            .with_introspection_cache(Box::new(meta_store.clone())),
        );

        let schema = Arc::new(SchemaService::new(
            Box::new(Arc::clone(&connector)),
            Box::new(meta_store.clone()),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));

        let table_data = Arc::new(TableDataService::new(
            Box::new(Arc::clone(&connector)),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));
        let export = Arc::new(ExportService::new(
            Box::new(Arc::clone(&connector)),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));
        let backup = Arc::new(BackupService::new(
            Box::new(meta_store.clone()),
            Box::new(Arc::clone(&secret_store)),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
            Box::new(|config| Box::new(PgDumpEngine::new(config.clone()))),
            Box::new(|database| {
                Box::new(SqliteBackupEngine::new(db_config(
                    db_pro_core::domain::connection::DriverType::SQLite,
                    "",
                    0,
                    database,
                    "",
                )))
            }),
        ));
        let users = Arc::new(UserService::new(
            Box::new(PostgresUserManager::new(
                Arc::clone(&connector) as Arc<dyn db_pro_core::ports::DbConnector>
            )),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));
        let rls = Arc::new(RlsService::new(
            Box::new(PostgresRlsManager::new(
                Arc::clone(&connector) as Arc<dyn db_pro_core::ports::DbConnector>
            )),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));
        let connector_for_monitor: Arc<dyn db_pro_core::ports::DbConnector> = connector.clone();
        let monitoring = Arc::new(MonitoringService::new(
            Box::new(PostgresMonitoringPort::new(Arc::clone(&connector_for_monitor))),
            Box::new(SqliteMonitoringPort::new(connector_for_monitor)),
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
        ));
        let audit = Arc::new(AuditService::new(
            Arc::clone(&registry),
            Box::new(meta_store.clone()),
            Arc::clone(&connector) as Arc<dyn db_pro_core::ports::DbConnector>,
        ));
        let mut extensions = ExtensionRegistry::new();
        // Built-in example contribution; failures are isolated and must not abort boot.
        let _ = extensions.load(example_diagnostics_extension());
        let extensions = Arc::new(extensions);
        let data_diff = Arc::new(DataDiffService::new(
            Box::new(Arc::clone(&connector)),
            Arc::clone(&registry),
        ));

        Ok(Arc::new(Self {
            data_dir,
            connections,
            queries,
            schema,
            table_data,
            export,
            backup,
            users,
            rls,
            monitoring,
            audit,
            extensions,
            data_diff,
            connector,
            registry,
            meta_store,
            secrets: secret_store,
        }))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn connections(&self) -> Arc<ConnectionService> {
        Arc::clone(&self.connections)
    }

    pub fn connection_api(&self) -> ConnectionApi {
        ConnectionApi::new(self.connections())
    }

    pub fn queries(&self) -> Arc<QueryService> {
        Arc::clone(&self.queries)
    }

    pub fn query_api(&self) -> QueryApi {
        QueryApi::new(self.queries())
    }

    pub fn schema(&self) -> Arc<SchemaService> {
        Arc::clone(&self.schema)
    }

    pub fn schema_api(&self) -> SchemaApi {
        SchemaApi::new(self.schema())
    }

    pub fn table_data(&self) -> Arc<TableDataService> {
        Arc::clone(&self.table_data)
    }

    pub fn table_data_api(&self) -> TableDataApi {
        TableDataApi::new(self.table_data())
    }

    pub fn export(&self) -> Arc<ExportService> {
        Arc::clone(&self.export)
    }

    pub fn export_api(&self) -> ExportApi {
        ExportApi::new(self.export())
    }

    pub fn backup(&self) -> Arc<BackupService> {
        Arc::clone(&self.backup)
    }

    pub fn backup_api(&self) -> BackupApi {
        BackupApi::new(self.backup())
    }

    pub fn users(&self) -> Arc<UserService> {
        Arc::clone(&self.users)
    }

    pub fn user_api(&self) -> UserApi {
        UserApi::new(self.users())
    }

    pub fn rls(&self) -> Arc<RlsService> {
        Arc::clone(&self.rls)
    }

    pub fn rls_api(&self) -> RlsApi {
        RlsApi::new(self.rls())
    }

    pub fn monitoring(&self) -> Arc<MonitoringService> {
        Arc::clone(&self.monitoring)
    }

    pub fn monitoring_api(&self) -> MonitoringApi {
        MonitoringApi::new(self.monitoring())
    }

    pub fn audit(&self) -> Arc<AuditService> {
        Arc::clone(&self.audit)
    }

    pub fn audit_api(&self) -> AuditApi {
        AuditApi::new(self.audit())
    }

    pub fn extensions(&self) -> Arc<ExtensionRegistry> {
        Arc::clone(&self.extensions)
    }

    pub fn data_diff(&self) -> Arc<DataDiffService> {
        Arc::clone(&self.data_diff)
    }

    pub fn data_diff_api(&self) -> DataDiffApi {
        DataDiffApi::new(self.data_diff())
    }

    pub fn postgres_api(&self) -> PostgresApi {
        PostgresApi::new(self.connector(), self.registry(), self.meta_store.clone())
    }

    pub fn connector(&self) -> Arc<CompositeConnector> {
        Arc::clone(&self.connector)
    }

    pub fn registry(&self) -> Arc<ConnectionRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn meta_store(&self) -> Arc<SQLiteMetaStore> {
        Arc::new(self.meta_store.clone())
    }

    pub async fn load_agent_api_key(&self) -> Result<Option<String>, db_pro_core::domain::error::DbError> {
        self.secrets.retrieve_secret(AGENT_API_KEY_SECRET).await
    }

    pub async fn store_agent_api_key(&self, api_key: &str) -> Result<(), db_pro_core::domain::error::DbError> {
        self.secrets.store_secret(AGENT_API_KEY_SECRET, api_key).await
    }

    pub async fn delete_agent_api_key(&self) -> Result<(), db_pro_core::domain::error::DbError> {
        self.secrets.delete_secret(AGENT_API_KEY_SECRET).await
    }
}

fn db_config(
    driver: db_pro_core::domain::connection::DriverType,
    host: &str,
    port: u16,
    database: &str,
    username: &str,
) -> db_pro_core::domain::connection::ConnectionConfig {
    db_pro_core::domain::connection::ConnectionConfig {
        name: String::new(),
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        username: username.to_owned(),
        driver,
        ssl_mode: db_pro_core::domain::connection::SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: db_pro_core::domain::connection::PRODUCT_MAX_ROWS,
        color: None,
        tags: Vec::new(),
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
    }
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
