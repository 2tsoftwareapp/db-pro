use db_pro_core::domain::error::DbError;
use db_pro_core::domain::schema::*;
use rusqlite::OptionalExtension;
use std::collections::hash_map::Entry;

/// Escape a SQLite identifier by doubling any embedded double-quote characters
/// and wrapping in double quotes. This is safe for interpolation into PRAGMA
/// statements, which do not support `?` parameter binding for table names.
fn escape_identifier(name: &str) -> String {
    let escaped = name.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

// cc-scan:allow COMPLEXITY — one call per catalog facet
pub fn run_introspection(conn: &rusqlite::Connection) -> Result<IntrospectResult, DbError> {
    let table_names = fetch_table_names(conn)?;
    let views = introspect_views(conn)?;

    let mut object_names = table_names.clone();
    object_names.extend(views.iter().map(|v| v.name.clone()));

    let tables = introspect_tables(conn, &table_names)?;
    let mut columns = introspect_columns(conn, &object_names)?;
    let indexes = introspect_indexes(conn, &table_names)?;
    mark_unique_columns(&mut columns, &indexes);
    let foreign_keys = introspect_foreign_keys(conn, &table_names)?;
    let check_constraints = introspect_check_constraints(conn, &table_names)?;
    let triggers = introspect_triggers(conn)?;

    // Derive primary keys from the declared key order, not the column order (#596)
    let primary_keys = introspect_primary_keys(conn, &table_names)?;

    let schemas = vec![Schema { name: "main".into() }];

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
        functions: Vec::new(),
        table_definition_sql: introspect_table_definitions(conn, &table_names)?,
    })
}

/// Single-column unique/primary indexes make their column `is_unique`.
fn mark_unique_columns(columns: &mut [Column], indexes: &[Index]) {
    for index in indexes
        .iter()
        .filter(|index| (index.unique || index.primary) && index.columns.len() == 1)
    {
        for column_name in &index.columns {
            if let Some(column) = columns
                .iter_mut()
                .find(|column| column.table_name == index.table_name && column.name == *column_name)
            {
                column.is_unique = true;
            }
        }
    }
}

/// Primary keys in their declared order (#596).
///
/// `table_xinfo` reports each column's 1-based position *within* the key in its
/// `pk` field. Ordering by the column index instead turns `PRIMARY KEY (b, a)`
/// into `(a, b)` — a different index, and the order a composite foreign key
/// depends on.
fn introspect_primary_keys(conn: &rusqlite::Connection, table_names: &[String]) -> Result<Vec<PrimaryKey>, DbError> {
    let mut primary_keys = Vec::new();
    for table_name in table_names {
        let safe_name = escape_identifier(table_name);
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_xinfo({safe_name})"))
            .map_err(crate::error::from_rusqlite)?;
        let mut keyed: Vec<(i32, String)> = stmt
            .query_map([], |row| {
                let position: i32 = row.get(5)?;
                let name: String = row.get(1)?;
                Ok((position, name))
            })
            .map_err(crate::error::from_rusqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::error::from_rusqlite)?;
        keyed.retain(|(position, _)| *position > 0);
        if keyed.is_empty() {
            continue;
        }
        keyed.sort_by_key(|(position, _)| *position);
        primary_keys.push(PrimaryKey {
            constraint_name: format!("{table_name}_pk"),
            columns: keyed.into_iter().map(|(_, name)| name).collect(),
            table_name: table_name.clone(),
            schema: "main".into(),
        });
    }
    Ok(primary_keys)
}

fn fetch_table_names(conn: &rusqlite::Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(crate::error::from_rusqlite)?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;
    Ok(names)
}

fn introspect_tables(_conn: &rusqlite::Connection, table_names: &[String]) -> Result<Vec<Table>, DbError> {
    Ok(table_names
        .iter()
        .map(|name| Table {
            name: name.clone(),
            schema: "main".into(),
            row_count: None,
        })
        .collect())
}

/// Columns, including generated ones (#596).
///
/// `PRAGMA table_info` hides generated/virtual columns; `table_xinfo` reports
/// them with `hidden = 2` (virtual) or `3` (stored). A generated column the
/// explorer cannot see is also a column the DDL cannot reproduce — the table
/// would be cloned without it.
// cc-scan:allow COMPLEXITY — one arm per `table_xinfo` column state
fn introspect_columns(conn: &rusqlite::Connection, table_names: &[String]) -> Result<Vec<Column>, DbError> {
    let mut columns = Vec::new();
    for table_name in table_names {
        // PRAGMA does not support ? parameters for table names;
        // use safe identifier escaping instead.
        let safe_name = escape_identifier(table_name);
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_xinfo({safe_name})"))
            .map_err(crate::error::from_rusqlite)?;
        let mut cols: Vec<Column> = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let data_type: String = row.get(2)?;
                let notnull: bool = row.get(3)?;
                let default: Option<String> = row.get(4)?;
                let pk: bool = row.get::<_, i32>(5)? > 0;
                // `hidden` reports 0 for ordinary columns, 1 for hidden
                // virtual-table columns, and 2 or 3 for generated ones.
                let hidden: i32 = row.get::<_, i32>(6).unwrap_or(0);
                Ok(Column {
                    name,
                    data_type,
                    ordinal: usize::try_from(row.get::<_, i32>(0)?).unwrap_or(0),
                    nullable: !notnull,
                    default,
                    is_primary_key: pk,
                    is_unique: false,
                    is_identity: false,
                    is_generated: hidden >= 2,
                    collation: None,
                    enum_labels: Vec::new(),
                    table_name: table_name.clone(),
                    schema: "main".into(),
                })
            })
            .map_err(crate::error::from_rusqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::error::from_rusqlite)?;
        columns.append(&mut cols);
    }
    Ok(columns)
}

// cc-scan:allow COMPLEXITY,LONG_FUNCTION — classifier/dispatch ladder — one case per branch
fn introspect_indexes(conn: &rusqlite::Connection, table_names: &[String]) -> Result<Vec<Index>, DbError> {
    let mut indexes = Vec::new();
    for table_name in table_names {
        // PRAGMA does not support ? parameters for table names;
        // use safe identifier escaping instead.
        let safe_name = escape_identifier(table_name);
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list({safe_name})"))
            .map_err(crate::error::from_rusqlite)?;
        let index_list: Vec<(String, bool, String)> = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let unique: bool = row.get(2)?;
                let origin: String = row.get(3)?;
                Ok((name, unique, origin))
            })
            .map_err(crate::error::from_rusqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::error::from_rusqlite)?;

        for (index_name, unique, origin) in index_list {
            if origin == "pk" {
                // The table PRIMARY KEY definition already reproduces this
                // autoindex, and SQLite rejects recreating its internal name.
                continue;
            }
            let origin = if origin == "u" {
                IndexOrigin::UniqueConstraint
            } else {
                IndexOrigin::User
            };
            // PRAGMA does not support ? parameters for index names;
            // use safe identifier escaping instead.
            let safe_idx = escape_identifier(&index_name);
            let mut info_stmt = conn
                .prepare(&format!("PRAGMA index_info({safe_idx})"))
                .map_err(crate::error::from_rusqlite)?;
            let cols: Vec<String> = info_stmt
                .query_map([], |row| row.get(2))
                .map_err(crate::error::from_rusqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(crate::error::from_rusqlite)?;
            let definition: Option<String> = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    [&index_name],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map_err(crate::error::from_rusqlite)?
                .flatten();
            indexes.push(Index {
                name: index_name,
                columns: cols,
                unique,
                method: "btree".to_owned(),
                primary: false,
                include_columns: Vec::new(),
                predicate: None,
                definition: definition.unwrap_or_default(),
                origin,
                table_name: table_name.clone(),
                schema: "main".into(),
            });
        }
    }
    Ok(indexes)
}

fn sqlite_primary_key_columns(conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<String>, DbError> {
    let safe_name = escape_identifier(table_name);
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({safe_name})"))
        .map_err(crate::error::from_rusqlite)?;
    let mut columns: Vec<(i32, String)> = stmt
        .query_map([], |row| {
            let name: String = row.get(1)?;
            let pk_ordinal: i32 = row.get(5)?;
            Ok((pk_ordinal, name))
        })
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;

    columns.retain(|(pk_ordinal, _)| *pk_ordinal > 0);
    columns.sort_by_key(|(pk_ordinal, _)| *pk_ordinal);
    Ok(columns.into_iter().map(|(_, name)| name).collect())
}

type SqliteForeignKeyRow = (i32, i32, String, String, Option<String>, String, String, String);
type SqliteForeignKeyGroup = (String, Vec<String>, Vec<String>, String, String, String);

fn introspect_foreign_keys(conn: &rusqlite::Connection, table_names: &[String]) -> Result<Vec<ForeignKey>, DbError> {
    let mut foreign_keys = Vec::new();
    let mut referenced_pk_cache: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    for table_name in table_names {
        foreign_keys.extend(table_foreign_keys(conn, table_name, &mut referenced_pk_cache)?);
    }
    Ok(foreign_keys)
}

/// Foreign keys of one table — PRAGMA rows grouped by constraint id so
/// composite keys keep their column order.
fn table_foreign_keys(
    conn: &rusqlite::Connection,
    table_name: &str,
    referenced_pk_cache: &mut std::collections::HashMap<String, Vec<String>>,
) -> Result<Vec<ForeignKey>, DbError> {
    // PRAGMA does not support ? parameters for table names;
    // use safe identifier escaping instead.
    let safe_name = escape_identifier(table_name);
    let mut stmt = conn
        .prepare(&format!("PRAGMA foreign_key_list({safe_name})"))
        .map_err(crate::error::from_rusqlite)?;

    // SQLite returns NULL for the referenced column when the schema uses
    // shorthand syntax such as `REFERENCES parent`. Keep the FK sequence so
    // we can resolve that omitted target against the parent primary key.
    let rows: Vec<SqliteForeignKeyRow> = stmt
        .query_map([], |row| {
            let id: i32 = row.get(0)?;
            let seq: i32 = row.get(1)?;
            let to_table: String = row.get(2)?;
            let from_column: String = row.get(3)?;
            let to_column: Option<String> = row.get(4)?;
            let on_update: String = row.get(5)?;
            let on_delete: String = row.get(6)?;
            let match_option: String = row.get(7)?;
            Ok((
                id,
                seq,
                to_table,
                from_column,
                to_column,
                on_update,
                on_delete,
                match_option,
            ))
        })
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;

    // Group columns by FK id to support composite foreign keys while keeping
    // the PRAGMA encounter order deterministic.
    let mut map: std::collections::HashMap<i32, SqliteForeignKeyGroup> = std::collections::HashMap::new();
    let mut order: Vec<i32> = Vec::new();

    for (id, seq, to_table, from_column, to_column, on_update, on_delete, match_option) in rows {
        let resolved_to_column = match to_column {
            Some(column) => column,
            None => {
                let pk_columns = if let Some(columns) = referenced_pk_cache.get(&to_table) {
                    columns
                } else {
                    let columns = sqlite_primary_key_columns(conn, &to_table)?;
                    referenced_pk_cache.entry(to_table.clone()).or_insert(columns)
                };
                pk_columns.get(seq as usize).cloned().ok_or_else(|| {
                    DbError::IntrospectionFailed(format!(
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        "foreign key {table_name}_fk_{id} references {to_table} without a resolvable primary-key column at position {seq}"
                    ))
                })?
            }
        };

        let group = match map.entry(id) {
            Entry::Vacant(entry) => {
                order.push(id);
                entry.insert((to_table, Vec::new(), Vec::new(), on_update, on_delete, match_option))
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        group.1.push(from_column);
        group.2.push(resolved_to_column);
    }

    let mut foreign_keys = Vec::new();
    for id in order {
        let Some((to_table, from_columns, to_columns, on_update, on_delete, match_option)) = map.remove(&id) else {
            return Err(DbError::IntrospectionFailed(format!(
                "foreign-key grouping lost constraint {id} for table {table_name}"
            )));
        };
        foreign_keys.push(ForeignKey {
            name: format!("{table_name}_fk_{id}"),
            from_table: table_name.to_owned(),
            from_columns,
            to_table,
            to_columns,
            schema: "main".into(),
            to_schema: "main".into(),
            on_update,
            on_delete,
            match_option,
            deferrable: false,
            initially_deferred: false,
        });
    }
    Ok(foreign_keys)
}

fn introspect_check_constraints(
    conn: &rusqlite::Connection,
    table_names: &[String],
) -> Result<Vec<CheckConstraint>, DbError> {
    let mut check_constraints = Vec::new();

    for table_name in table_names {
        // Get the CREATE TABLE SQL from sqlite_master
        let mut stmt = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1")
            .map_err(crate::error::from_rusqlite)?;

        let create_sql: Option<String> = stmt
            .query_row([table_name], |row| row.get(0))
            .optional()
            .map_err(crate::error::from_rusqlite)?;

        if let Some(sql) = create_sql {
            for (constraint_idx, parsed) in parse_check_constraints(&sql).into_iter().enumerate() {
                let name = parsed
                    .name
                    .unwrap_or_else(|| format!("{table_name}_check_{constraint_idx}"));
                check_constraints.push(CheckConstraint {
                    name,
                    table_name: table_name.clone(),
                    schema: "main".into(),
                    definition: parsed.definition,
                });
            }
        }
    }

    Ok(check_constraints)
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedCheckConstraint {
    name: Option<String>,
    definition: String,
}

/// Extract CHECK definitions from a CREATE TABLE statement.
///
/// The scan is quote-, comment-, and parenthesis-depth aware. A preceding
/// `CONSTRAINT <name>` is preserved when it sits immediately before `CHECK`.
/// Unbalanced parentheses or unterminated quotes stop the scan without inventing
/// a truncated constraint.
fn parse_check_constraints(sql: &str) -> Vec<ParsedCheckConstraint> {
    let mut definitions = Vec::new();
    let mut cursor = 0;

    while let Some(check_start) = find_sql_keyword(sql, cursor, "CHECK") {
        let after_keyword = check_start + "CHECK".len();
        let open_paren = skip_sql_trivia(sql, after_keyword);
        if sql.as_bytes().get(open_paren) != Some(&b'(') {
            cursor = after_keyword;
            continue;
        }

        let Some(close_paren) = matching_parenthesis(sql, open_paren) else {
            break;
        };
        definitions.push(ParsedCheckConstraint {
            name: preceding_constraint_name(sql, check_start),
            definition: sql[open_paren + 1..close_paren].trim().to_owned(),
        });
        cursor = close_paren + 1;
    }

    definitions
}

/// If `CONSTRAINT <ident>` sits immediately before `CHECK` (trivia only between),
/// return that identifier. Otherwise the CHECK is unnamed/column-level.
fn preceding_constraint_name(sql: &str, check_start: usize) -> Option<String> {
    let mut last_match = None;
    let mut cursor = 0;
    while let Some(constraint_pos) = find_sql_keyword(sql, cursor, "CONSTRAINT") {
        if constraint_pos >= check_start {
            break;
        }
        let after_keyword = skip_sql_trivia(sql, constraint_pos + "CONSTRAINT".len());
        let Some((name, name_end)) = read_sql_identifier(sql, after_keyword) else {
            cursor = constraint_pos + 1;
            continue;
        };
        let after_name = skip_sql_trivia(sql, name_end);
        if after_name == check_start {
            last_match = Some(name);
        }
        cursor = constraint_pos + 1;
    }
    last_match
}

fn read_sql_identifier(sql: &str, start: usize) -> Option<(String, usize)> {
    let bytes = sql.as_bytes();
    if start >= bytes.len() {
        return None;
    }
    // Quoted identifiers: "name", `name`, [name]
    match bytes[start] {
        b'"' | b'`' | b'[' => {
            let end = skip_quoted(sql, start);
            if end <= start + 1 {
                return None;
            }
            let raw = &sql[start + 1..end - 1];
            if raw.is_empty() {
                return None;
            }
            Some((raw.replace("\"\"", "\"").replace("``", "`").replace("]]", "]"), end))
        }
        _ => {
            let mut end = start;
            let mut chars = sql[start..].chars();
            let first = chars.next()?;
            if !(first == '_' || first.is_alphabetic()) {
                return None;
            }
            end += first.len_utf8();
            for ch in chars {
                if !is_sql_identifier_char(ch) {
                    break;
                }
                end += ch.len_utf8();
            }
            Some((sql[start..end].to_owned(), end))
        }
    }
}

fn find_sql_keyword(sql: &str, from: usize, keyword: &str) -> Option<usize> {
    let bytes = sql.as_bytes();
    let mut cursor = from;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' | b'[' => cursor = skip_quoted(sql, cursor),
            b'-' if bytes.get(cursor + 1) == Some(&b'-') => cursor = skip_line_comment(sql, cursor),
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => cursor = skip_block_comment(sql, cursor),
            _ => {
                let ends_at = cursor + keyword.len();
                let Some(candidate) = sql.get(cursor..ends_at) else {
                    cursor += sql[cursor..].chars().next().map_or(1, char::len_utf8);
                    continue;
                };
                let before_is_boundary = sql[..cursor]
                    .chars()
                    .next_back()
                    .is_none_or(|ch| !is_sql_identifier_char(ch));
                let after_is_boundary = sql[ends_at..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !is_sql_identifier_char(ch));
                if before_is_boundary && after_is_boundary && candidate.eq_ignore_ascii_case(keyword) {
                    return Some(cursor);
                }
                cursor += sql[cursor..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    None
}

fn matching_parenthesis(sql: &str, open_paren: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let mut depth = 0_u32;
    let mut cursor = open_paren;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' | b'[' => cursor = skip_quoted(sql, cursor),
            b'-' if bytes.get(cursor + 1) == Some(&b'-') => cursor = skip_line_comment(sql, cursor),
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => cursor = skip_block_comment(sql, cursor),
            b'(' => {
                depth += 1;
                cursor += 1;
            }
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => cursor += sql[cursor..].chars().next().map_or(1, char::len_utf8),
        }
    }

    None
}

fn skip_sql_trivia(sql: &str, mut cursor: usize) -> usize {
    let bytes = sql.as_bytes();
    loop {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'-') && bytes.get(cursor + 1) == Some(&b'-') {
            cursor = skip_line_comment(sql, cursor);
        } else if bytes.get(cursor) == Some(&b'/') && bytes.get(cursor + 1) == Some(&b'*') {
            cursor = skip_block_comment(sql, cursor);
        } else {
            return cursor;
        }
    }
}

fn skip_quoted(sql: &str, start: usize) -> usize {
    let bytes = sql.as_bytes();
    let opening = bytes[start];
    let closing = if opening == b'[' { b']' } else { opening };
    let mut cursor = start + 1;

    while cursor < bytes.len() {
        if bytes[cursor] == closing {
            if bytes.get(cursor + 1) == Some(&closing) {
                cursor += 2;
            } else {
                return cursor + 1;
            }
        } else {
            cursor += sql[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }

    bytes.len()
}

fn skip_line_comment(sql: &str, start: usize) -> usize {
    sql[start..].find('\n').map_or(sql.len(), |offset| start + offset + 1)
}

fn skip_block_comment(sql: &str, start: usize) -> usize {
    sql[start + 2..]
        .find("*/")
        .map_or(sql.len(), |offset| start + offset + 4)
}

fn is_sql_identifier_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

fn introspect_views(conn: &rusqlite::Connection) -> Result<Vec<View>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'view' ORDER BY name")
        .map_err(crate::error::from_rusqlite)?;
    let views = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let definition: String = row.get(1)?;
            Ok(View {
                name,
                schema: "main".into(),
                definition,
                kind: RelationKind::View,
            })
        })
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;
    Ok(views)
}

fn introspect_triggers(conn: &rusqlite::Connection) -> Result<Vec<Trigger>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name, tbl_name, sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name")
        .map_err(crate::error::from_rusqlite)?;
    let triggers = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let table_name: String = row.get(1)?;
            let sql: Option<String> = row.get(2)?;
            let definition = sql.clone().unwrap_or_default();
            let (timing, event) = parse_sqlite_trigger_sql(&definition);
            Ok(Trigger {
                name,
                table_name,
                schema: "main".into(),
                timing,
                event,
                definition,
                function_def: String::new(),
                enabled: true,
            })
        })
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;
    Ok(triggers)
}

/// Parse timing (BEFORE/AFTER/INSTEAD OF) and event (INSERT/UPDATE/DELETE) from
/// a SQLite CREATE TRIGGER SQL body. Returns (timing, event) with empty fallbacks.
///
/// Only the header portion (before BEGIN) is inspected for the event type,
/// because the trigger body may contain DML keywords that would produce
/// false positives (e.g. an AFTER UPDATE trigger whose body does INSERT).
///
/// The search skips past SQL literals, quoted identifiers, and comments so
/// keywords embedded in names or trigger bodies cannot affect the result.
fn parse_sqlite_trigger_sql(sql: &str) -> (String, String) {
    let header_end = find_sql_keyword(sql, 0, "BEGIN").unwrap_or(sql.len());
    let header = &sql[..header_end];

    let timing = if has_sql_keyword_sequence(header, "INSTEAD", "OF") {
        "INSTEAD OF"
    } else if find_sql_keyword(header, 0, "BEFORE").is_some() {
        "BEFORE"
    } else if find_sql_keyword(header, 0, "AFTER").is_some() {
        "AFTER"
    } else {
        ""
    };

    let event = if find_sql_keyword(header, 0, "INSERT").is_some() {
        "INSERT"
    } else if find_sql_keyword(header, 0, "UPDATE").is_some() {
        "UPDATE"
    } else if find_sql_keyword(header, 0, "DELETE").is_some() {
        "DELETE"
    } else {
        ""
    };

    (timing.to_string(), event.to_string())
}

fn has_sql_keyword_sequence(sql: &str, first: &str, second: &str) -> bool {
    let Some(first_start) = find_sql_keyword(sql, 0, first) else {
        return false;
    };
    find_sql_keyword(sql, first_start + first.len(), second).is_some()
}

#[cfg(test)]
#[path = "tests/introspect_tests.rs"]
mod tests;

/// The `CREATE TABLE` text SQLite itself stores (#596).
///
/// `sqlite_master.sql` is the original statement, so it reproduces
/// `AUTOINCREMENT`, `COLLATE NOCASE`, `WITHOUT ROWID`, `STRICT`, `ON CONFLICT`
/// clauses and generated-column syntax exactly. Rebuilding the statement from
/// `PRAGMA` metadata cannot, and a clone/migration built that way silently
/// produced a different table.
fn introspect_table_definitions(
    conn: &rusqlite::Connection,
    table_names: &[String],
) -> Result<Vec<TableDefinitionSql>, DbError> {
    let mut definitions = Vec::new();
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND sql IS NOT NULL")
        .map_err(crate::error::from_rusqlite)?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(crate::error::from_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::from_rusqlite)?;
    for (name, sql) in rows {
        if table_names.iter().any(|table| table == &name) {
            definitions.push(TableDefinitionSql {
                schema: "main".into(),
                table_name: name,
                sql,
            });
        }
    }
    Ok(definitions)
}
