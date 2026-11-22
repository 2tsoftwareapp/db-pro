use db_pro_core::domain::error::DbError;
use db_pro_core::domain::schema::*;
use sqlx::mysql::{MySqlPool, MySqlRow};
use sqlx::Row;

/// Reads one `information_schema` column without the driver's compatibility gate.
///
/// Two MySQL 8 facts make the gate unusable here, both measured live against the fixture:
/// the labels come back in upper case (`TABLE_NAME`), so every column is aliased to the
/// lower-case label read here, and the text columns are reported with the binary flag set
/// (their type name is `VARBINARY`), so the gate rejects `String` even though the payload is
/// the column's own UTF-8 text. Skipping the gate skips no decoding: `T`'s decoder still
/// reads the payload.
fn info<'r, T>(row: &'r MySqlRow, name: &str) -> T
where
    T: sqlx::Decode<'r, sqlx::MySql>,
{
    row.try_get_unchecked::<T, _>(name)
        .unwrap_or_else(|error| panic!("MySQL information_schema column {name} is unreadable: {error}"))
}

const FOREIGN_KEY_SQL: &str = "SELECT
                tc.TABLE_NAME AS table_name,
                kcu.COLUMN_NAME AS column_name,
                kcu.REFERENCED_TABLE_NAME AS referenced_table_name,
                kcu.REFERENCED_COLUMN_NAME AS referenced_column_name,
                kcu.REFERENCED_TABLE_SCHEMA AS referenced_table_schema,
                rc.CONSTRAINT_NAME AS constraint_name,
                rc.UPDATE_RULE AS update_rule,
                rc.DELETE_RULE AS delete_rule
             FROM information_schema.TABLE_CONSTRAINTS tc
             JOIN information_schema.KEY_COLUMN_USAGE kcu
               ON tc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA
              AND tc.TABLE_NAME = kcu.TABLE_NAME
              AND tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
             JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
               ON tc.CONSTRAINT_SCHEMA = rc.CONSTRAINT_SCHEMA
              AND tc.TABLE_NAME = rc.TABLE_NAME
              AND tc.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
             WHERE tc.TABLE_SCHEMA = ? AND tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
             ORDER BY tc.CONSTRAINT_NAME, kcu.ORDINAL_POSITION";

pub struct MySqlIntrospect;

impl MySqlIntrospect {
    // cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
    pub async fn introspect(pool: &MySqlPool) -> Result<IntrospectResult, DbError> {
        let database: String = sqlx::query_scalar("SELECT DATABASE()")
            .fetch_one(pool)
            .await
            .map_err(|e| DbError::QueryFailed(format!("MySQL introspect failed: {}", e)))?;

        let schema = Schema { name: database.clone() };
        let (tables, _table_names) = Self::fetch_tables(pool, &database).await?;
        let columns = Self::fetch_columns(pool, &database).await?;
        let primary_keys = Self::fetch_primary_keys(pool, &database).await?;
        let indexes = Self::fetch_indexes(pool, &database).await?;
        let foreign_keys = Self::fetch_foreign_keys(pool, &database).await?;
        let views = Self::fetch_views(pool, &database).await?;
        let triggers = Self::fetch_triggers(pool, &database).await?;
        let functions = Self::fetch_functions(pool, &database).await?;
        let check_constraints = Self::fetch_check_constraints(pool, &database).await?;

        Ok(IntrospectResult {
            schemas: vec![schema],
            tables,
            columns,
            primary_keys,
            indexes,
            foreign_keys,
            check_constraints,
            views,
            triggers,
            functions,
            table_definition_sql: Vec::new(),
        })
    }

    async fn fetch_tables(pool: &MySqlPool, database: &str) -> Result<(Vec<Table>, Vec<String>), DbError> {
        // Tables
        //
        // `information_schema` reports its own column labels in upper case on MySQL 8, and
        // `Row::get(&str)` matches a label exactly, so every selected column is aliased to
        // the lower-case label this module reads. Without the aliases the first `get` fails
        // with `ColumnNotFound("table_name")` and takes the whole introspection down.
        let table_rows =
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            sqlx::query("SELECT TABLE_NAME AS table_name, TABLE_TYPE AS table_type FROM information_schema.TABLES WHERE TABLE_SCHEMA = ?")
                .bind(database)
                .fetch_all(pool)
                .await
                .map_err(|e| DbError::QueryFailed(format!("MySQL introspect tables failed: {}", e)))?;

        let mut tables = Vec::new();
        let mut table_names = Vec::new();
        for row in &table_rows {
            let name: String = info(row, "table_name");
            table_names.push(name.clone());
            tables.push(Table {
                name,
                schema: database.to_owned(),
                row_count: None,
            });
        }

        Ok((tables, table_names))
    }

    async fn fetch_columns(pool: &MySqlPool, database: &str) -> Result<Vec<Column>, DbError> {
        // Columns
        let column_rows = sqlx::query(
            "SELECT TABLE_NAME AS table_name, COLUMN_NAME AS column_name, DATA_TYPE AS data_type, \
             IS_NULLABLE AS is_nullable, COLUMN_DEFAULT AS column_default, EXTRA AS extra, \
             COLUMN_KEY AS column_key, ORDINAL_POSITION AS ordinal_position
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = ?
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect columns failed: {}", e)))?;

        let mut columns = Vec::new();
        for row in &column_rows {
            let col = Column {
                name: info(row, "column_name"),
                data_type: info(row, "data_type"),
                ordinal: info::<u32>(row, "ordinal_position") as usize,
                nullable: info::<String>(row, "is_nullable") == "YES",
                default: info::<Option<String>>(row, "column_default"),
                is_primary_key: info::<String>(row, "column_key") == "PRI",
                is_unique: info::<String>(row, "column_key") == "UNI",
                is_identity: info::<String>(row, "extra").contains("auto_increment"),
                is_generated: info::<String>(row, "extra").contains("GENERATED"),
                collation: None,
                enum_labels: Vec::new(),
                table_name: info(row, "table_name"),
                schema: database.to_owned(),
            };
            columns.push(col);
        }

        Ok(columns)
    }

    async fn fetch_primary_keys(pool: &MySqlPool, database: &str) -> Result<Vec<PrimaryKey>, DbError> {
        // Primary keys
        let pk_rows = sqlx::query(
            "SELECT TABLE_NAME AS table_name, COLUMN_NAME AS column_name, CONSTRAINT_NAME AS constraint_name
             FROM information_schema.KEY_COLUMN_USAGE
             WHERE TABLE_SCHEMA = ? AND CONSTRAINT_NAME = 'PRIMARY'
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect PK failed: {}", e)))?;

        let mut pk_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for row in &pk_rows {
            let table: String = info(row, "table_name");
            let col: String = info(row, "column_name");
            pk_map.entry(table).or_default().push(col);
        }
        let primary_keys = pk_map
            .into_iter()
            .map(|(table_name, columns)| PrimaryKey {
                constraint_name: format!("{table_name}_pk"),
                columns,
                table_name,
                schema: database.to_owned(),
            })
            .collect();

        Ok(primary_keys)
    }

    // cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
    async fn fetch_indexes(pool: &MySqlPool, database: &str) -> Result<Vec<Index>, DbError> {
        // Indexes
        let index_rows = sqlx::query(
            "SELECT TABLE_NAME AS table_name, INDEX_NAME AS index_name, COLUMN_NAME AS column_name, \
             NON_UNIQUE AS non_unique, INDEX_TYPE AS index_type
             FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA = ?
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect indexes failed: {}", e)))?;

        // Grouped by table *and* name: an index name is only unique within its table, and a
        // grouped index without its table cannot be placed — `SchemaService` filters indexes
        // by table name, so an empty one silently dropped every MySQL index.
        let mut index_groups: std::collections::HashMap<(String, String), (Vec<String>, bool, String)> =
            std::collections::HashMap::new();
        for row in &index_rows {
            let table: String = info(row, "table_name");
            let idx_name: String = info(row, "index_name");
            let col: String = info(row, "column_name");
            let non_unique: i32 = info(row, "non_unique");
            let idx_type: String = info(row, "index_type");
            let entry = index_groups
                .entry((table, idx_name))
                .or_insert_with(|| (Vec::new(), non_unique == 0, idx_type));
            entry.0.push(col);
        }
        let indexes = index_groups
            .into_iter()
            .map(|((table_name, name), (columns, unique, method))| {
                let is_primary = name == "PRIMARY";
                Index {
                    name,
                    columns,
                    unique: unique && !is_primary,
                    method,
                    primary: is_primary,
                    include_columns: Vec::new(),
                    predicate: None,
                    definition: String::new(),
                    origin: IndexOrigin::User,
                    table_name,
                    schema: database.to_owned(),
                }
            })
            .collect();

        Ok(indexes)
    }

    // cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
    async fn fetch_foreign_keys(pool: &MySqlPool, database: &str) -> Result<Vec<ForeignKey>, DbError> {
        // Foreign keys
        let fk_rows = sqlx::query(FOREIGN_KEY_SQL)
            .bind(database)
            .fetch_all(pool)
            .await
            .map_err(|e| DbError::QueryFailed(format!("MySQL introspect FK failed: {}", e)))?;

        // allow: temporary map grouping information_schema rows by constraint_name before
        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
        // building FK models; the internal tuple is local to this function, creating a struct would just appease the lint.
        #[allow(clippy::type_complexity)]
        let mut fk_groups: std::collections::HashMap<
            String,
            (Vec<String>, Vec<String>, String, String, String, String, String),
        > = std::collections::HashMap::new();
        for row in &fk_rows {
            let constraint: String = info(row, "constraint_name");
            let entry = fk_groups.entry(constraint).or_insert_with(|| {
                (
                    Vec::new(),
                    Vec::new(),
                    info(row, "table_name"),
                    info(row, "referenced_table_name"),
                    info(row, "referenced_table_schema"),
                    info(row, "update_rule"),
                    info(row, "delete_rule"),
                )
            });
            entry.0.push(info(row, "column_name"));
            entry.1.push(info(row, "referenced_column_name"));
        }
        let foreign_keys = fk_groups
            .into_iter()
            .map(
                |(name, (from_cols, to_cols, from_table, to_table, to_schema, on_update, on_delete))| ForeignKey {
                    name,
                    from_table,
                    from_columns: from_cols,
                    to_table,
                    to_columns: to_cols,
                    schema: database.to_owned(),
                    to_schema,
                    on_update,
                    on_delete,
                    match_option: String::new(),
                    deferrable: false,
                    initially_deferred: false,
                },
            )
            .collect();

        Ok(foreign_keys)
    }

    async fn fetch_views(pool: &MySqlPool, database: &str) -> Result<Vec<View>, DbError> {
        // Views
        let view_rows = sqlx::query(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "SELECT TABLE_NAME AS table_name, VIEW_DEFINITION AS view_definition FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ?",
        )
                .bind(database)
                .fetch_all(pool)
                .await
                .map_err(|e| DbError::QueryFailed(format!("MySQL introspect views failed: {}", e)))?;

        let views = view_rows
            .iter()
            .map(|row| View {
                name: info(row, "table_name"),
                schema: database.to_owned(),
                definition: info::<Option<String>>(row, "view_definition").unwrap_or_default(),
                kind: RelationKind::View,
            })
            .collect();

        Ok(views)
    }

    async fn fetch_triggers(pool: &MySqlPool, database: &str) -> Result<Vec<Trigger>, DbError> {
        // Triggers
        let trigger_rows = sqlx::query(
            "SELECT TRIGGER_NAME AS trigger_name, EVENT_MANIPULATION AS event_manipulation, \
             EVENT_OBJECT_TABLE AS event_object_table, ACTION_STATEMENT AS action_statement, \
             ACTION_TIMING AS action_timing
             FROM information_schema.TRIGGERS
             WHERE TRIGGER_SCHEMA = ?",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect triggers failed: {}", e)))?;

        let triggers = trigger_rows
            .iter()
            .map(|row| Trigger {
                name: info(row, "trigger_name"),
                table_name: info(row, "event_object_table"),
                schema: database.to_owned(),
                timing: info(row, "action_timing"),
                event: info(row, "event_manipulation"),
                definition: info::<Option<String>>(row, "action_statement").unwrap_or_default(),
                function_def: String::new(),
                enabled: true,
            })
            .collect();

        Ok(triggers)
    }

    async fn fetch_functions(pool: &MySqlPool, database: &str) -> Result<Vec<Function>, DbError> {
        // Functions
        let routine_rows = sqlx::query(
            "SELECT ROUTINE_NAME AS routine_name, ROUTINE_TYPE AS routine_type, DATA_TYPE AS data_type, \
             ROUTINE_DEFINITION AS routine_definition
             FROM information_schema.ROUTINES
             WHERE ROUTINE_SCHEMA = ?",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect routines failed: {}", e)))?;

        let functions = routine_rows
            .iter()
            .map(|row| Function {
                name: info(row, "routine_name"),
                schema: database.to_owned(),
                routine_type: info(row, "routine_type"),
                data_type: info::<Option<String>>(row, "data_type").unwrap_or_default(),
                definition: info::<Option<String>>(row, "routine_definition").unwrap_or_default(),
                identity_arguments: String::new(),
                language: String::new(),
                volatility: String::new(),
                security_definer: false,
                parameters: Vec::new(),
            })
            .collect();

        Ok(functions)
    }

    async fn fetch_check_constraints(pool: &MySqlPool, database: &str) -> Result<Vec<CheckConstraint>, DbError> {
        // CHECK constraints (MySQL 8.0.16+)
        let check_rows = sqlx::query(
            "SELECT
                tc.CONSTRAINT_NAME AS constraint_name,
                tc.TABLE_NAME AS table_name,
                cc.CHECK_CLAUSE AS check_clause
             FROM information_schema.TABLE_CONSTRAINTS tc
             JOIN information_schema.CHECK_CONSTRAINTS cc
               ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA
              AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME
             WHERE tc.TABLE_SCHEMA = ?
               AND tc.CONSTRAINT_TYPE = 'CHECK'
             ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME",
        )
        .bind(database)
        .fetch_all(pool)
        .await
        .map_err(|e| DbError::QueryFailed(format!("MySQL introspect CHECK failed: {}", e)))?;

        let check_constraints = check_rows
            .iter()
            .map(|row| CheckConstraint {
                name: info(row, "constraint_name"),
                table_name: info(row, "table_name"),
                schema: database.to_owned(),
                definition: info::<Option<String>>(row, "check_clause").unwrap_or_default(),
            })
            .collect();

        Ok(check_constraints)
    }
}
