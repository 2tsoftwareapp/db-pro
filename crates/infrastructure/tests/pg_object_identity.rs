//! Real-provider regressions for #499 / #508. Use a disposable PostgreSQL database.
//! DATABASE_URL=postgres://user:password@localhost:5432/database \
//! cargo test -p db-pro-infrastructure --test pg_object_identity -- --ignored
//! Event-trigger coverage requires a superuser. No preloaded fixture tables are needed.

use db_pro_core::domain::connection::{ConnectionConfig, ConnectionHandle, DriverType, SslMode};
use db_pro_core::domain::cross_connection::ObjectDependency;
use db_pro_core::domain::error::DbError;
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::postgres::connector::PostgresConnector;
use db_pro_infrastructure::postgres::cross_connection::get_object_dependencies;
use db_pro_infrastructure::postgres::event_trigger::PostgresEventTriggerPort;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

async fn setup() -> (Arc<PostgresConnector>, ConnectionHandle) {
    // Match the existing provider fixture URL convention (plain local credentials, TLS disabled).
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must name a disposable PostgreSQL database");
    let rest = url
        .strip_prefix("postgres://")
        .expect("expected postgres:// fixture URL");
    let (rest, _) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, database) = rest.rsplit_once('/').expect("fixture database");
    let (auth, host_port) = authority.rsplit_once('@').expect("fixture username/password");
    let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "5432"));
    let config = ConnectionConfig {
        name: "pg-object-identity".into(),
        host: host.into(),
        port: port.parse().expect("fixture port"),
        database: database.into(),
        username: username.into(),
        driver: DriverType::Postgres,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 30_000,
        max_rows: 10_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
};
    let connector = Arc::new(PostgresConnector::new());
    let handle = connector.connect(&config, password).await.expect("PG connect");
    (connector, handle)
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL and PostgreSQL superuser"]
async fn pg_event_trigger_function_refs_create_fire_and_introspect() {
    let (connector, handle) = setup().await;
    let port = PostgresEventTriggerPort::new(connector.clone());
    let schema = unique_name("event_ref");
    let function = unique_name("on_ddl");
    let trigger = unique_name("audit_ddl");
    connector
        .execute(&handle, &format!("CREATE SCHEMA {schema}"), &[])
        .await
        .unwrap();
    connector
        .execute(
            &handle,
            &format!("CREATE TABLE {schema}.hits (object_identity text)"),
            &[],
        )
        .await
        .unwrap();
    // Both visible and non-visible functions have the same behavior; their distinct OIDs
    // make accidental schema/case resolution observable through the inventory.
    for function_schema in ["public", schema.as_str()] {
        connector
            .execute(
                &handle,
                &format!(
                    "CREATE FUNCTION {function_schema}.{function}() RETURNS event_trigger LANGUAGE plpgsql AS $$
                     BEGIN
                         INSERT INTO {schema}.hits
                         SELECT object_identity FROM pg_event_trigger_ddl_commands()
                         WHERE schema_name = '{schema}' AND command_tag = 'CREATE TABLE';
                     END $$"
                ),
                &[],
            )
            .await
            .unwrap();
    }
    let refs = [
        (function.clone(), function.clone()),
        (format!("{function}()"), function.clone()),
        (format!("PUBLIC.{}", function.to_ascii_uppercase()), function.clone()),
        (format!("public.{function}()"), function.clone()),
        (format!("{schema}.{function}"), format!("{schema}.{function}")),
        (
            format!("{}.{}()", schema.to_ascii_uppercase(), function.to_ascii_uppercase()),
            format!("{schema}.{function}"),
        ),
    ];
    for (index, (function_ref, signature)) in refs.iter().enumerate() {
        assert!(matches!(
            port.create_event_trigger(
                &handle,
                &trigger,
                "ddl_command_end",
                function_ref,
                "CREATE TABLE",
                false
            )
            .await,
            Err(DbError::Validation(_))
        ));
        port.create_event_trigger(&handle, &trigger, "ddl_command_end", function_ref, "CREATE TABLE", true)
            .await
            .unwrap();
        let inventory = port.inventory(&handle, "fixture").await.unwrap();
        let info = inventory.triggers.iter().find(|item| item.name == trigger).unwrap();
        assert_eq!(info.function_signature, format!("{signature}()"));
        assert_eq!(info.event, "ddl_command_end");
        assert_eq!(info.enabled, "O");
        assert_eq!(info.tags, ["CREATE TABLE"]);
        connector
            .execute(
                &handle,
                &format!("CREATE TABLE {schema}.probe_{index} (id integer)"),
                &[],
            )
            .await
            .unwrap();
        assert!(matches!(
            port.alter_event_trigger(&handle, &trigger, "disable", false).await,
            Err(DbError::Validation(_))
        ));
        assert!(matches!(
            port.drop_event_trigger(&handle, &trigger, false).await,
            Err(DbError::Validation(_))
        ));
        port.drop_event_trigger(&handle, &trigger, true).await.unwrap();
    }
    let pool = connector.get_pool(&handle).await.unwrap();
    let hits: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT object_identity FROM {schema}.hits ORDER BY object_identity"
    ))
    .fetch_all(&pool)
    .await
    .unwrap();
    let expected: Vec<String> = (0..refs.len()).map(|index| format!("{schema}.probe_{index}")).collect();
    assert_eq!(
        hits, expected,
        "each created event trigger must actually execute its function"
    );
    connector
        .execute(&handle, &format!("DROP FUNCTION public.{function}()"), &[])
        .await
        .unwrap();
    connector
        .execute(&handle, &format!("DROP SCHEMA {schema} CASCADE"), &[])
        .await
        .unwrap();
    connector.disconnect(&handle).await.unwrap();
}

fn dependency_rows(rows: Vec<ObjectDependency>) -> Vec<(String, String, String, String)> {
    rows.into_iter()
        .map(|row| {
            (
                row.object_type,
                row.object_name,
                row.depends_on_type,
                row.depends_on_name,
            )
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires disposable DATABASE_URL with CREATE SCHEMA permission"]
async fn pg_relation_dependencies_use_exact_catalog_identity() {
    let (connector, handle) = setup().await;
    // A single-connection pool makes each search_path change deterministic.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let schema = unique_name("dep_exact");
    let decoy_schema = unique_name("dep_decoy");
    let hostile_schema = format!("{}%_\".x", unique_name("dep"));
    let schemas = [&schema, &decoy_schema, &hostile_schema];
    let names = [
        "same_name",
        "percent%name",
        "under_score",
        "quote\"name",
        "dot.name",
        "x';--",
    ];
    for schema_name in schemas {
        let quoted_schema = quote_ident(schema_name);
        sqlx::query(&format!("CREATE SCHEMA {quoted_schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE TYPE {quoted_schema}.marker AS ENUM ('value')"))
            .execute(&pool)
            .await
            .unwrap();
        for name in names {
            let table = format!("{quoted_schema}.{}", quote_ident(name));
            sqlx::query(&format!(
                "CREATE TABLE {table} (id integer, value {quoted_schema}.marker)"
            ))
            .execute(&pool)
            .await
            .unwrap();
        }
        // These match the old LIKE wildcards / prefix but must not join the selected relation.
        for name in ["percentXname.extra", "underXscore.extra", "same_name.extra"] {
            sqlx::query(&format!(
                "CREATE TABLE {quoted_schema}.{} (id integer)",
                quote_ident(name)
            ))
            .execute(&pool)
            .await
            .unwrap();
        }
        // Reverse dependents belong to other catalog rows, not to the selected source table.
        sqlx::query(&format!("CREATE INDEX reverse_index ON {quoted_schema}.same_name (id)"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "CREATE VIEW {quoted_schema}.reverse_view AS SELECT * FROM {quoted_schema}.same_name"
        ))
        .execute(&pool)
        .await
        .unwrap();
    }
    for active_schema in [&schema, &decoy_schema] {
        sqlx::query("SELECT set_config('search_path', $1, false)")
            .bind(format!("{}, public", quote_ident(active_schema)))
            .execute(&pool)
            .await
            .unwrap();
        for schema_name in schemas {
            for name in names {
                let expected_source: String = sqlx::query_scalar("SELECT format('%I', $1::text)")
                    .bind(name)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                let qualified_schema: String = sqlx::query_scalar("SELECT format('%I', $1::text)")
                    .bind(schema_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                let qualified_source = format!("{qualified_schema}.{expected_source}");
                let source = if schema_name == active_schema {
                    expected_source.clone()
                } else {
                    format!("{qualified_schema}.{expected_source}")
                };
                let target_type = format!("{qualified_schema}.marker");
                let expected = vec![
                    (
                        "pg_class".into(),
                        source.clone(),
                        "pg_namespace".into(),
                        qualified_schema.clone(),
                    ),
                    ("pg_class".into(), source, "pg_type".into(), target_type),
                ];
                let rows = get_object_dependencies(&pool, schema_name, name).await.unwrap();
                assert_eq!(
                    dependency_rows(rows),
                    expected,
                    "{schema_name}.{name}, active {active_schema}"
                );
                // Exercise the outward connector boundary too; its default search_path is public.
                let rows = connector
                    .get_object_dependencies(&handle, schema_name, name)
                    .await
                    .unwrap();
                assert_eq!(
                    dependency_rows(rows),
                    vec![
                        (
                            "pg_class".into(),
                            qualified_source.clone(),
                            "pg_namespace".into(),
                            qualified_schema.clone()
                        ),
                        (
                            "pg_class".into(),
                            qualified_source,
                            "pg_type".into(),
                            format!("{qualified_schema}.marker")
                        ),
                    ],
                    "connector {schema_name}.{name}"
                );
            }
        }
        assert!(get_object_dependencies(&pool, &schema, "missing")
            .await
            .unwrap()
            .is_empty());
        assert!(get_object_dependencies(&pool, "missing_schema", "same_name")
            .await
            .unwrap()
            .is_empty());
    }
    // AUTO dependencies are retained (owned sequence -> exact table, not the reverse).
    sqlx::query(&format!(
        "CREATE SEQUENCE {schema}.owned_seq OWNED BY {schema}.same_name.id"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let rows = connector
        .get_object_dependencies(&handle, &schema, "owned_seq")
        .await
        .unwrap();
    assert!(rows
        .iter()
        .any(|row| row.depends_on_type == "pg_class" && row.depends_on_name == format!("{schema}.same_name")));
    assert!(connector
        .get_object_dependencies(&handle, &schema, "missing")
        .await
        .unwrap()
        .is_empty());
    sqlx::query("RESET search_path").execute(&pool).await.unwrap();
    for schema_name in schemas {
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", quote_ident(schema_name)))
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
    connector.disconnect(&handle).await.unwrap();
}
