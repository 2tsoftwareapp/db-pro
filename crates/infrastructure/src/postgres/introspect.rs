// cc-scan:allow-file LINE_TOO_LONG — SQL literals stay single-line; wrapping changes statement text
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::schema::*;
use sqlx::postgres::PgRow;
use sqlx::Row as _;
use std::collections::HashSet;

/// Introspect a live catalog that other sessions can change under us.
///
/// The queries below call catalog functions (`pg_get_indexdef`, `pg_get_expr`,
/// `pg_get_constraintdef`, …) that are evaluated with a *fresh* catalog snapshot, so
/// an object created or dropped after this query's snapshot either decodes as a NULL
/// column (handled at the decode site) or makes the server raise a transient
/// catalog-cache error. A refresh is a read of whatever consistent-enough state the
/// catalog is in, so only that transient form is retried, a bounded number of times,
/// with a short backoff; every other error is returned unchanged. The failure this
/// replaces was `cache lookup failed for attribute 1 of relation …` and
/// `ColumnDecode { index: "definition", UnexpectedNullError }` — see
/// `docs/release/evidence/v01-runtime/providers/42-introspection-under-concurrent-ddl.md`.
pub async fn run_introspection(pool: &sqlx::PgPool) -> Result<IntrospectResult, DbError> {
    const ATTEMPTS: usize = 3;
    let mut attempt = 1;
    loop {
        match run_introspection_once(pool).await {
            Ok(result) => return Ok(result),
            Err(error) if attempt < ATTEMPTS && is_transient_catalog_error(&error) => {
                tokio::time::sleep(std::time::Duration::from_millis(10 * attempt as u64)).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// The catalog races PostgreSQL reports while a concurrent session is running DDL.
/// Matching is deliberately narrow: a message this list does not name is a real
/// failure and must not be retried behind the user's back.
fn is_transient_catalog_error(error: &DbError) -> bool {
    let DbError::QueryFailed(message) = error else {
        return false;
    };
    [
        "cache lookup failed",
        "could not open relation with OID",
        "tuple concurrently updated",
        "cached plan must not change result type",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
async fn run_introspection_once(pool: &sqlx::PgPool) -> Result<IntrospectResult, DbError> {
    // Run independent introspection queries in parallel
    // cc-scan:allow LINE_TOO_LONG — literal must not wrap
    let (schemas, tables, raw_cols, primary_keys, indexes, foreign_keys, check_constraints, views, triggers, functions) = tokio::join!(
        introspect_schemas(pool),
        introspect_tables(pool),
        introspect_columns_raw(pool),
        introspect_primary_keys(pool),
        introspect_indexes(pool),
        introspect_foreign_keys(pool),
        introspect_check_constraints(pool),
        introspect_views(pool),
        introspect_triggers(pool),
        introspect_functions(pool),
    );

    let schemas = schemas?;
    let tables = tables?;
    let raw_cols = raw_cols?;
    let primary_keys = primary_keys?;
    let indexes = indexes?;
    let foreign_keys = foreign_keys?;
    let check_constraints = check_constraints?;
    let views = views?;
    let triggers = triggers?;
    let functions = functions?;

    // Build PK column set from already-fetched primary_keys (no extra query)
    let (pk_column_set, unique_column_set) = column_flag_sets(&primary_keys, &indexes);
    let columns = assemble_columns(raw_cols, &pk_column_set, &unique_column_set);

    Ok(IntrospectResult {
        schemas,
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

/// (schema, table, column) sets marking primary-key and single-column-unique
/// membership, derived from the already-fetched catalogs.
type ColumnRef = (String, String, String);

fn column_flag_sets(primary_keys: &[PrimaryKey], indexes: &[Index]) -> (HashSet<ColumnRef>, HashSet<ColumnRef>) {
    let pk_column_set = primary_keys
        .iter()
        .flat_map(|pk| {
            pk.columns
                .iter()
                .map(move |col| (pk.schema.clone(), pk.table_name.clone(), col.clone()))
        })
        .collect();
    let unique_column_set = indexes
        .iter()
        .filter(|index| (index.unique || index.primary) && index.columns.len() == 1)
        .flat_map(|index| {
            index
                .columns
                .iter()
                .map(move |column| (index.schema.clone(), index.table_name.clone(), column.clone()))
        })
        .collect();
    (pk_column_set, unique_column_set)
}

fn assemble_columns(
    raw_cols: Vec<RawColumn>,
    pk_column_set: &HashSet<ColumnRef>,
    unique_column_set: &HashSet<ColumnRef>,
) -> Vec<Column> {
    raw_cols
        .into_iter()
        .map(
            |(
                schema,
                table,
                name,
                data_type,
                ordinal,
                nullable,
                default,
                is_identity,
                is_generated,
                collation,
                enum_labels,
            )| {
                let is_pk = pk_column_set.contains(&(schema.clone(), table.clone(), name.clone()));
                let is_unique = unique_column_set.contains(&(schema.clone(), table.clone(), name.clone()));
                Column {
                    name,
                    data_type,
                    ordinal,
                    nullable,
                    default,
                    is_primary_key: is_pk,
                    is_unique,
                    is_identity,
                    is_generated,
                    collation,
                    enum_labels,
                    table_name: table,
                    schema,
                }
            },
        )
        .collect()
}

async fn introspect_schemas(pool: &sqlx::PgPool) -> Result<Vec<Schema>, DbError> {
    // Hide catalog schemas and session-local temp namespaces (`pg_temp_*`,
    // `pg_toast_temp_*`) — they are not user-managed schemas in the explorer.
    let rows = sqlx::query(
        r#"
        SELECT schema_name
        FROM information_schema.schemata
        WHERE schema_name NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND schema_name !~ '^pg_(toast_)?temp'
        ORDER BY schema_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let name: String = row.get("schema_name");
            Schema { name }
        })
        .collect())
}

/// Relations the explorer shows as tables: ordinary tables and partitioned
/// parents (#595).
///
/// `information_schema.tables` reports every *partition* as its own
/// `BASE TABLE`, so a table partitioned by month added one entry per month next
/// to its parent — and the parent is `relkind = 'p'`, which the row-count query
/// only joined for `relkind = 'r'`, so the parent was the one relation with no
/// count at all. Reading `pg_class` keeps partitions out of the list (they stay
/// reachable through their parent) and gives the parent the sum of its
/// partitions' estimates.
// cc-scan:allow LONG_FUNCTION — one catalog query plus its row mapping
async fn introspect_tables(pool: &sqlx::PgPool) -> Result<Vec<Table>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            c.relname AS table_name,
            n.nspname AS schema_name,
            c.relkind::text AS rel_kind,
            c.reltuples AS row_count,
            CASE
                WHEN c.relkind = 'p' THEN (
                    SELECT SUM(child.reltuples)::bigint
                    FROM pg_inherits i
                    JOIN pg_class child ON child.oid = i.inhrelid
                    WHERE i.inhparent = c.oid AND child.reltuples >= 0
                )
                ELSE NULL
            END AS partition_row_estimate
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relkind IN ('r', 'p')
          AND NOT c.relispartition
          AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
        ORDER BY n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| {
            let name: String = row.get("table_name");
            let schema: String = row.get("schema_name");
            let rel_kind: String = row.get("rel_kind");
            let row_count = if rel_kind == "p" {
                // A partitioned parent stores no rows of its own, so `reltuples`
                // is -1 there; the honest number is what its partitions hold.
                row.try_get::<Option<i64>, _>("partition_row_estimate")
                    .ok()
                    .flatten()
                    .and_then(|total| u64::try_from(total).ok())
            } else {
                let raw: f32 = row.get("row_count");
                (raw >= 0.0).then(|| raw as u64)
            };
            Table {
                name,
                schema,
                row_count,
            }
        })
        .collect()
}

// cc-scan:allow COMPLEXITY,LINE_TOO_LONG,LONG_FUNCTION — one catalog query plus its row mapping
async fn introspect_columns_raw(pool: &sqlx::PgPool) -> Result<Vec<RawColumn>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.nspname AS table_schema,
            c.relname AS table_name,
            a.attname AS column_name,
            pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
            a.attnum::integer AS ordinal,
            NOT a.attnotnull AS is_nullable,
            pg_get_expr(d.adbin, d.adrelid) AS column_default,
            (a.attidentity <> '') AS is_identity,
            (a.attgenerated <> '') AS is_generated,
            CASE WHEN coll.collname = 'default' THEN NULL ELSE coll.collname END AS collation_name,
            (SELECT array_agg(e.enumlabel ORDER BY e.enumsortorder)
             FROM pg_catalog.pg_enum e WHERE e.enumtypid = a.atttypid) AS enum_labels
        FROM pg_attribute a
        JOIN pg_class c ON a.attrelid = c.oid
        JOIN pg_namespace n ON c.relnamespace = n.oid
        LEFT JOIN pg_attrdef d ON a.attrelid = d.adrelid AND a.attnum = d.adnum
        LEFT JOIN pg_collation coll ON a.attcollation = coll.oid
        WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
          AND a.attnum > 0
          AND NOT a.attisdropped
        ORDER BY n.nspname, c.relname, a.attnum
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let table_schema: String = row.get("table_schema");
            let table_name: String = row.get("table_name");
            let column_name: String = row.get("column_name");
            let data_type: String = row.get("data_type");
            let ordinal: i32 = row.get("ordinal");
            let nullable: bool = row.get("is_nullable");
            let default: Option<String> = row.get("column_default");
            let is_identity: bool = row.get("is_identity");
            let is_generated: bool = row.get("is_generated");
            let collation: Option<String> = row.get("collation_name");
            let enum_labels: Option<Vec<String>> = row.get("enum_labels");
            (
                table_schema,
                table_name,
                column_name,
                data_type,
                usize::try_from(ordinal.max(0)).unwrap_or(0),
                nullable,
                default,
                is_identity,
                is_generated,
                collation,
                enum_labels.unwrap_or_default(),
            )
        })
        .collect())
}

/// Primary keys, partition children included (#595).
///
/// The catalog query reads `pg_constraint`, so a partition inherits its parent's
/// primary key; the result is folded to one entry per table so a partitioned
/// parent lists its key columns once instead of once per partition.
async fn introspect_primary_keys(pool: &sqlx::PgPool) -> Result<Vec<PrimaryKey>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.nspname AS schema_name,
            c.relname AS table_name,
            a.attname AS column_name,
            key.ordinality AS position
        FROM pg_constraint con
        JOIN pg_class c ON c.oid = con.conrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        CROSS JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS key(attnum, ordinality)
        JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = key.attnum
        WHERE con.contype = 'p'
          AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
        ORDER BY n.nspname, c.relname, key.ordinality
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    let mut primary_keys: Vec<PrimaryKey> = Vec::new();
    for row in rows {
        let schema: String = row.get("schema_name");
        let table_name: String = row.get("table_name");
        let column_name: String = row.get("column_name");
        match primary_keys
            .iter_mut()
            .find(|pk| pk.schema == schema && pk.table_name == table_name)
        {
            Some(pk) => pk.columns.push(column_name),
            None => primary_keys.push(PrimaryKey {
                constraint_name: format!("{table_name}_pkey"),
                schema,
                table_name,
                columns: vec![column_name],
            }),
        }
    }
    Ok(primary_keys)
}

// cc-scan:allow HUGE_FUNCTION — one catalog query plus its row mapping; the SQL literal dominates the length
async fn introspect_indexes(pool: &sqlx::PgPool) -> Result<Vec<Index>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.nspname AS schema_name,
            tbl.relname AS table_name,
            idx.relname AS index_name,
            am.amname AS method,
            i.indisprimary AS is_primary,
            i.indisunique AS is_unique,
            pg_get_indexdef(i.indexrelid) AS definition,
            pg_get_expr(i.indpred, i.indrelid) AS predicate,
            ARRAY(
                SELECT att.attname
                FROM unnest(i.indkey) WITH ORDINALITY AS key(attnum, ord)
                JOIN pg_attribute att
                  ON att.attrelid = i.indrelid AND att.attnum = key.attnum
                WHERE key.ord <= i.indnkeyatts
                ORDER BY key.ord
            ) AS columns,
            ARRAY(
                SELECT att.attname
                FROM unnest(i.indkey) WITH ORDINALITY AS key(attnum, ord)
                JOIN pg_attribute att
                  ON att.attrelid = i.indrelid AND att.attnum = key.attnum
                WHERE key.ord > i.indnkeyatts
                ORDER BY key.ord
            ) AS include_columns
        FROM pg_index i
        JOIN pg_class idx ON idx.oid = i.indexrelid
        JOIN pg_class tbl ON tbl.oid = i.indrelid
        JOIN pg_namespace n ON n.oid = tbl.relnamespace
        JOIN pg_am am ON am.oid = idx.relam
        WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
        ORDER BY n.nspname, tbl.relname, idx.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| -> Result<Index, DbError> {
            let schema: String = row.get("schema_name");
            let table: String = row.get("table_name");
            let name: String = row.get("index_name");
            let method: String = row.get("method");
            let primary: bool = row.get("is_primary");
            let unique: bool = row.get("is_unique");
            // `pg_get_indexdef` is evaluated with a *fresh* catalog snapshot, so an
            // index created or dropped by another session after this query's
            // snapshot makes the function return NULL for a row the snapshot still
            // shows. A meanwhile-dropped index must not fail the whole refresh.
            let definition = optional_string(&row, "definition")?.unwrap_or_default();
            let predicate: Option<String> = row.get("predicate");
            let mut columns: Vec<String> = row.get("columns");
            if columns.is_empty() && !definition.is_empty() {
                columns = parse_index_columns(&definition);
            }
            let include_columns: Vec<String> = row.get("include_columns");

            Ok(Index {
                name,
                columns,
                unique,
                method,
                primary,
                include_columns,
                predicate,
                definition,
                origin: IndexOrigin::User,
                table_name: table,
                schema,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Extract column names from a PostgreSQL index definition string.
///
/// Handles expressions like `USING btree (col1, col2)` or `USING hash (col1)`.
/// Also handles functional indexes with parenthesized expressions by tracking
/// parenthesis depth.
fn parse_index_columns(indexdef: &str) -> Vec<String> {
    let bytes = indexdef.as_bytes();
    let Some(open) = find_unquoted_open_parenthesis(bytes) else {
        return Vec::new();
    };

    let Some(close) = find_matching_parenthesis(bytes, open) else {
        return Vec::new();
    };

    split_index_columns(&indexdef[open + 1..close])
}

fn find_unquoted_open_parenthesis(bytes: &[u8]) -> Option<usize> {
    let mut quote = None;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if quote_byte == b'\'' && byte == b'\\' {
                index = index.saturating_add(2);
                continue;
            }
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index = index.saturating_add(2);
                    continue;
                }
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => return Some(index),
                _ => {}
            }
        }
        index += 1;
    }

    None
}

// cc-scan:allow DEEP_NESTING — nesting mirrors structure depth
fn find_matching_parenthesis(bytes: &[u8], open: usize) -> Option<usize> {
    let mut quote = None;
    let mut depth = 0usize;
    let mut index = open;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote_byte) = quote {
            if quote_byte == b'\'' && byte == b'\\' {
                index = index.saturating_add(2);
                continue;
            }
            if byte == quote_byte {
                if bytes.get(index + 1) == Some(&quote_byte) {
                    index = index.saturating_add(2);
                    continue;
                }
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }

        index += 1;
    }

    None
}

// cc-scan:allow LONG_FUNCTION — linear pipeline — one cohesive pass
fn split_index_columns(col_str: &str) -> Vec<String> {
    let mut columns = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;

    for ch in col_str.chars() {
        if let Some(quote_char) = quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if quote_char == '\'' && ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    columns.push(trimmed);
                }
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        columns.push(trimmed);
    }

    columns
}

const FOREIGN_KEY_SQL: &str = r#"
        SELECT
            con.conname AS constraint_name,
            nsp.nspname AS from_schema,
            cls.relname AS from_table,
            src_att.attname AS from_column,
            fnsp.nspname AS to_schema,
            fcls.relname AS to_table,
            dst_att.attname AS to_column,
            CASE con.confupdtype
                WHEN 'a' THEN 'NO ACTION'
                WHEN 'r' THEN 'RESTRICT'
                WHEN 'c' THEN 'CASCADE'
                WHEN 'n' THEN 'SET NULL'
                WHEN 'd' THEN 'SET DEFAULT'
                ELSE 'UNKNOWN'
            END AS on_update,
            CASE con.confdeltype
                WHEN 'a' THEN 'NO ACTION'
                WHEN 'r' THEN 'RESTRICT'
                WHEN 'c' THEN 'CASCADE'
                WHEN 'n' THEN 'SET NULL'
                WHEN 'd' THEN 'SET DEFAULT'
                ELSE 'UNKNOWN'
            END AS on_delete,
            CASE con.confmatchtype
                WHEN 'f' THEN 'FULL'
                WHEN 'p' THEN 'PARTIAL'
                ELSE 'SIMPLE'
            END AS match_option,
            con.condeferrable AS deferrable,
            con.condeferred AS initially_deferred
        FROM pg_constraint con
        JOIN pg_namespace nsp ON nsp.oid = con.connamespace
        JOIN pg_class cls ON cls.oid = con.conrelid
        JOIN pg_class fcls ON fcls.oid = con.confrelid
        JOIN pg_namespace fnsp ON fnsp.oid = fcls.relnamespace
        JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS src(attnum, ord) ON true
        JOIN LATERAL unnest(con.confkey) WITH ORDINALITY AS dst(attnum, ord)
            ON dst.ord = src.ord
        JOIN pg_attribute src_att
            ON src_att.attrelid = cls.oid AND src_att.attnum = src.attnum
        JOIN pg_attribute dst_att
            ON dst_att.attrelid = fcls.oid AND dst_att.attnum = dst.attnum
        WHERE con.contype = 'f'
          AND nsp.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND nsp.nspname !~ '^pg_(toast_)?temp'
        ORDER BY nsp.nspname, cls.relname, con.conname, src.ord
        "#;

type ForeignKeyGroupKey = (String, String, String, String, String);
type ForeignKeyGroupValue = (Vec<String>, Vec<String>, String, String, String, bool, bool);

async fn introspect_foreign_keys(pool: &sqlx::PgPool) -> Result<Vec<ForeignKey>, DbError> {
    let rows = sqlx::query(FOREIGN_KEY_SQL)
        .fetch_all(pool)
        .await
        .map_err(crate::error::from_sqlx)?;
    Ok(group_foreign_key_rows(rows))
}

// cc-scan:allow HUGE_FUNCTION — FK row grouping: one accumulate-then-emit pass over catalog rows
fn group_foreign_key_rows(rows: Vec<sqlx::postgres::PgRow>) -> Vec<ForeignKey> {
    let mut map: std::collections::HashMap<ForeignKeyGroupKey, ForeignKeyGroupValue> = std::collections::HashMap::new();
    let mut order: Vec<ForeignKeyGroupKey> = Vec::new();

    for row in rows {
        let name: String = row.get("constraint_name");
        let from_table: String = row.get("from_table");
        let from_column: String = row.get("from_column");
        let to_table: String = row.get("to_table");
        let to_column: String = row.get("to_column");
        let schema: String = row.get("from_schema");
        let to_schema: String = row.get("to_schema");
        let on_update: String = row.get("on_update");
        let on_delete: String = row.get("on_delete");
        let match_option: String = row.get("match_option");
        let deferrable: bool = row.get("deferrable");
        let initially_deferred: bool = row.get("initially_deferred");

        let key = (
            name.clone(),
            from_table.clone(),
            to_table.clone(),
            schema.clone(),
            to_schema.clone(),
        );

        let (from_cols, to_cols, _, _, _, _, _) = map.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            (
                Vec::new(),
                Vec::new(),
                on_update.clone(),
                on_delete.clone(),
                match_option.clone(),
                deferrable,
                initially_deferred,
            )
        });
        from_cols.push(from_column);
        to_cols.push(to_column);
    }

    order
        .into_iter()
        .map(|(name, from_table, to_table, schema, to_schema)| {
            let (from_columns, to_columns, on_update, on_delete, match_option, deferrable, initially_deferred) = map
                .remove(&(
                    name.clone(),
                    from_table.clone(),
                    to_table.clone(),
                    schema.clone(),
                    to_schema.clone(),
                ))
                .unwrap_or_default();
            ForeignKey {
                name,
                from_table,
                from_columns,
                to_table,
                to_columns,
                schema,
                to_schema,
                on_update,
                on_delete,
                match_option,
                deferrable,
                initially_deferred,
            }
        })
        .collect()
}

async fn introspect_check_constraints(pool: &sqlx::PgPool) -> Result<Vec<CheckConstraint>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            con.conname AS constraint_name,
            nsp.nspname AS schema_name,
            cls.relname AS table_name,
            pg_get_constraintdef(con.oid) AS definition
        FROM pg_constraint con
        JOIN pg_namespace nsp ON nsp.oid = con.connamespace
        JOIN pg_class cls ON cls.oid = con.conrelid
        WHERE con.contype = 'c'
          AND nsp.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND nsp.nspname !~ '^pg_(toast_)?temp'
        ORDER BY nsp.nspname, cls.relname, con.conname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| -> Result<CheckConstraint, DbError> {
            let name: String = row.get("constraint_name");
            let schema: String = row.get("schema_name");
            let table_name: String = row.get("table_name");
            // Same fresh-snapshot rule as `pg_get_indexdef`: a constraint dropped
            // after this query's snapshot makes `pg_get_constraintdef` return NULL.
            let definition = optional_string(&row, "definition")?.unwrap_or_default();
            Ok(CheckConstraint {
                name,
                table_name,
                schema,
                definition,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn required_string(row: &PgRow, column: &str) -> Result<String, DbError> {
    row.try_get(column).map_err(crate::error::from_sqlx)
}

fn optional_string(row: &PgRow, column: &str) -> Result<Option<String>, DbError> {
    row.try_get(column).map_err(crate::error::from_sqlx)
}

/// Plain views, materialized views and foreign tables (#595).
///
/// `information_schema.views` covers exactly one of the three: a materialized
/// view (`relkind = 'm'`) is not listed there at all, and a foreign table is
/// `table_type = 'FOREIGN'`. The app creates materialized views in its own
/// workbench (#184), so a materialized view the user had just created stayed
/// invisible in the explorer even though its columns came back from
/// `introspect_columns_raw` — an inconsistent tree. `pg_class` + `pg_get_viewdef`
/// covers all three kinds with one definition source.
async fn introspect_views(pool: &sqlx::PgPool) -> Result<Vec<View>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.nspname AS table_schema,
            c.relname AS table_name,
            c.relkind::text AS rel_kind,
            CASE
                WHEN c.relkind = 'f' THEN COALESCE(
                    (SELECT string_agg(option_name || '=' || option_value, ', ' ORDER BY option_name)
                     FROM pg_options_to_table(c.reloptions)),
                    ''
                )
                ELSE COALESCE(pg_get_viewdef(c.oid, true), '')
            END AS definition
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relkind IN ('v', 'm', 'f')
          AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
        ORDER BY n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| -> Result<View, DbError> {
            let schema = required_string(&row, "table_schema")?;
            let name = required_string(&row, "table_name")?;
            let rel_kind = optional_string(&row, "rel_kind")?.unwrap_or_default();
            let definition = optional_string(&row, "definition")?.unwrap_or_default();
            Ok(View {
                kind: match rel_kind.as_str() {
                    "m" => RelationKind::MaterializedView,
                    "f" => RelationKind::ForeignTable,
                    _ => RelationKind::View,
                },
                name,
                schema,
                definition,
            })
        })
        .collect()
}

/// User triggers, straight from `pg_trigger` (#594).
///
/// `information_schema.triggers` is not a usable source here: it emits one row
/// *per event* (so `INSERT OR UPDATE` becomes two triggers with the same name),
/// and nothing in it joins to `pg_trigger`. The previous
/// `pg_t.tgname = t.trigger_name` join therefore multiplied every trigger by
/// every table that used the same name (`set_updated_at` is the common case) and
/// reported another table's `tgenabled`/function alongside it.
///
/// `pg_get_triggerdef` is authoritative, so `definition` is a complete
/// `CREATE TRIGGER` statement for PostgreSQL too: `timing`, `event` and
/// `enabled` are derived from `tgtype`/`tgenabled` for display only.
async fn introspect_triggers(pool: &sqlx::PgPool) -> Result<Vec<Trigger>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            pg_t.tgname AS trigger_name,
            c.relname AS table_name,
            n.nspname AS table_schema,
            pg_get_triggerdef(pg_t.oid, true) AS trigger_def,
            pg_t.tgenabled::text AS enabled_flag,
            pg_t.tgtype::int AS trigger_type,
            COALESCE(pg_get_functiondef(pg_proc.oid), '') AS function_def
        FROM pg_trigger pg_t
        JOIN pg_class c ON c.oid = pg_t.tgrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_proc ON pg_proc.oid = pg_t.tgfoid
        WHERE NOT pg_t.tgisinternal
            AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
            AND n.nspname !~ '^pg_(toast_)?temp'
        ORDER BY n.nspname, c.relname, pg_t.tgname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| -> Result<Trigger, DbError> {
            let name = required_string(&row, "trigger_name")?;
            let table_name = optional_string(&row, "table_name")?.unwrap_or_default();
            let schema = optional_string(&row, "table_schema")?.unwrap_or_default();
            let definition = optional_string(&row, "trigger_def")?.unwrap_or_default();
            let trigger_type: i32 = row.try_get("trigger_type").unwrap_or(0);
            let enabled_flag = optional_string(&row, "enabled_flag")?.unwrap_or_else(|| "O".into());
            let function_def = optional_string(&row, "function_def")?.unwrap_or_default();
            Ok(Trigger {
                name,
                table_name,
                schema,
                timing: trigger_timing_from_tgtype(trigger_type),
                event: trigger_events_from_tgtype(trigger_type),
                definition,
                function_def,
                enabled: trigger_enabled_from_flag(&enabled_flag),
            })
        })
        .collect()
}

/// `AFTER` is the absence of `BEFORE` and `INSTEAD OF` in `tgtype`.
fn trigger_timing_from_tgtype(trigger_type: i32) -> String {
    const TRIGGER_TYPE_BEFORE: i32 = 1 << 1;
    const TRIGGER_TYPE_INSTEAD: i32 = 1 << 6;
    if trigger_type & TRIGGER_TYPE_BEFORE != 0 {
        "BEFORE".into()
    } else if trigger_type & TRIGGER_TYPE_INSTEAD != 0 {
        "INSTEAD OF".into()
    } else {
        "AFTER".into()
    }
}

/// The statement events a `tgtype` mask carries, in `pg_trigger` bit order.
/// The row/stmt bit is not an event; `FOR EACH ROW` lives in the authoritative
/// `pg_get_triggerdef` text.
fn trigger_events_from_tgtype(trigger_type: i32) -> String {
    const TRIGGER_TYPE_INSERT: i32 = 1 << 2;
    const TRIGGER_TYPE_DELETE: i32 = 1 << 3;
    const TRIGGER_TYPE_UPDATE: i32 = 1 << 4;
    const TRIGGER_TYPE_TRUNCATE: i32 = 1 << 5;
    let mut events = Vec::new();
    if trigger_type & TRIGGER_TYPE_INSERT != 0 {
        events.push("INSERT");
    }
    if trigger_type & TRIGGER_TYPE_DELETE != 0 {
        events.push("DELETE");
    }
    if trigger_type & TRIGGER_TYPE_UPDATE != 0 {
        events.push("UPDATE");
    }
    if trigger_type & TRIGGER_TYPE_TRUNCATE != 0 {
        events.push("TRUNCATE");
    }
    events.join(" OR ")
}

fn trigger_enabled_from_flag(flag: &str) -> bool {
    matches!(flag, "O" | "R" | "A")
}

// cc-scan:allow COMPLEXITY,LONG_FUNCTION — classifier/dispatch ladder — one case per branch
async fn introspect_functions(pool: &sqlx::PgPool) -> Result<Vec<Function>, DbError> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.nspname AS routine_schema,
            p.proname AS routine_name,
            CASE p.prokind
                WHEN 'p' THEN 'PROCEDURE'
                ELSE 'FUNCTION'
            END AS routine_type,
            pg_get_function_result(p.oid) AS data_type,
            COALESCE(pg_get_functiondef(p.oid), '') AS definition,
            COALESCE(pg_get_function_identity_arguments(p.oid), '') AS identity_arguments,
            COALESCE(pg_get_function_arguments(p.oid), '') AS argument_list,
            COALESCE(l.lanname, '') AS language,
            CASE p.provolatile
                WHEN 'i' THEN 'IMMUTABLE'
                WHEN 's' THEN 'STABLE'
                ELSE 'VOLATILE'
            END AS volatility,
            p.prosecdef AS security_definer
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        LEFT JOIN pg_language l ON l.oid = p.prolang
        WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast') AND n.nspname !~ '^pg_(toast_)?temp'
          AND p.prokind IN ('f', 'p')
        ORDER BY n.nspname, p.proname, pg_get_function_identity_arguments(p.oid)
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::from_sqlx)?;

    rows.into_iter()
        .map(|row| -> Result<Function, DbError> {
            let schema = required_string(&row, "routine_schema")?;
            let name = required_string(&row, "routine_name")?;
            let routine_type = required_string(&row, "routine_type")?;
            let data_type = optional_string(&row, "data_type")?.unwrap_or_default();
            let definition = required_string(&row, "definition")?;
            let identity_arguments = optional_string(&row, "identity_arguments")?.unwrap_or_default();
            let argument_list = optional_string(&row, "argument_list")?.unwrap_or_default();
            let language = optional_string(&row, "language")?.unwrap_or_default();
            let volatility = optional_string(&row, "volatility")?.unwrap_or_default();
            let security_definer = row.try_get::<bool, _>("security_definer").unwrap_or(false);
            Ok(Function {
                name,
                schema,
                routine_type,
                data_type,
                definition,
                identity_arguments,
                language,
                volatility,
                security_definer,
                parameters: parse_routine_parameters(&argument_list),
            })
        })
        .collect()
}

/// Parse `pg_get_function_arguments` text into structured parameters.
///
/// Examples:
/// - `order_id integer, OUT total numeric`
/// - `VARIADIC tags text[] DEFAULT '{}'::text[]`
fn parse_routine_parameters(argument_list: &str) -> Vec<RoutineParameter> {
    let trimmed = argument_list.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut params = Vec::new();
    for raw in split_routine_args(trimmed) {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let mut mode = "IN".to_owned();
        let mut rest = part;
        for candidate in ["VARIADIC ", "INOUT ", "OUT ", "IN "] {
            if rest.len() >= candidate.len() && rest[..candidate.len()].eq_ignore_ascii_case(candidate) {
                mode = candidate.trim().to_ascii_uppercase();
                rest = rest[candidate.len()..].trim_start();
                break;
            }
        }
        let (type_and_name, default_expr, has_default) = if let Some(idx) = find_default_kw(rest) {
            (
                rest[..idx].trim(),
                rest[idx + "DEFAULT".len()..].trim().to_owned(),
                true,
            )
        } else {
            (rest, String::new(), false)
        };
        let (name, data_type) = split_name_and_type(type_and_name);
        params.push(RoutineParameter {
            name,
            data_type,
            mode,
            has_default,
            default_expr,
        });
    }
    params
}

fn split_routine_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in input.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

fn find_default_kw(input: &str) -> Option<usize> {
    let upper = input.to_ascii_uppercase();
    upper.find(" DEFAULT ").map(|idx| idx + 1)
}

fn split_name_and_type(input: &str) -> (String, String) {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return (String::new(), String::new());
    }
    if parts.len() == 1 {
        return (String::new(), parts[0].to_owned());
    }
    // name type...  OR  only type tokens when anonymous
    let first = parts[0];
    let looks_like_name = first
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && !first.contains('(')
        && !first.eq_ignore_ascii_case("double")
        && !first.eq_ignore_ascii_case("character")
        && !first.eq_ignore_ascii_case("timestamp")
        && !first.eq_ignore_ascii_case("time");
    if looks_like_name {
        (first.to_owned(), parts[1..].join(" "))
    } else {
        (String::new(), parts.join(" "))
    }
}

#[cfg(test)]
#[path = "tests/introspect_tests.rs"]
mod tests;
