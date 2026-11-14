// cc-scan:allow-file LINE_TOO_LONG — SQL literals stay single-line; wrapping changes statement text
use db_pro_core::domain::connection::ConnectionHandle;
use db_pro_core::domain::error::DbError;
use db_pro_core::domain::query::{CellValue, Row};
use db_pro_core::domain::schema::{
    CheckConstraint, Column, ForeignKey, Function, Index, IndexOrigin, IntrospectResult, PrimaryKey, RelationKind,
    RoutineParameter, Schema, Table, Trigger, View,
};
use std::collections::{HashMap, HashSet};

use super::connector::SqlServerConnector;

pub struct SqlServerIntrospect;

impl SqlServerIntrospect {
    pub async fn introspect(
        connector: &SqlServerConnector,
        handle: &ConnectionHandle,
    ) -> Result<IntrospectResult, DbError> {
        let schemas = load_schemas(connector, handle).await?;
        let tables = load_tables(connector, handle).await?;
        let mut columns = load_columns(connector, handle).await?;
        let primary_keys = load_primary_keys(connector, handle).await?;
        let indexes = load_indexes(connector, handle).await?;
        let foreign_keys = load_foreign_keys(connector, handle).await?;
        let check_constraints = load_checks(connector, handle).await?;
        let views = load_views(connector, handle).await?;
        let triggers = load_triggers(connector, handle).await?;
        let functions = load_routines(connector, handle).await?;

        let primary_key_columns = primary_keys
            .iter()
            .flat_map(|primary_key| {
                primary_key.columns.iter().map(|column| {
                    (
                        primary_key.schema.clone(),
                        primary_key.table_name.clone(),
                        column.clone(),
                    )
                })
            })
            .collect::<HashSet<_>>();
        let unique_columns = indexes
            .iter()
            .filter(|index| index.unique)
            .flat_map(|index| {
                index
                    .columns
                    .iter()
                    .map(|column| (index.schema.clone(), index.table_name.clone(), column.clone()))
            })
            .collect::<HashSet<_>>();
        for column in &mut columns {
            let identity = (column.schema.clone(), column.table_name.clone(), column.name.clone());
            column.is_primary_key = primary_key_columns.contains(&identity);
            column.is_unique = unique_columns.contains(&identity);
        }

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
        })
    }
}

async fn load_schemas(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Schema>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT name FROM sys.schemas WHERE name NOT IN ('guest', 'INFORMATION_SCHEMA', 'sys') ORDER BY name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| Ok(Schema { name: text(row, 0)? }))
        .collect()
}

async fn load_tables(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Table>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name AS schema_name, t.name AS table_name, COALESCE(SUM(p.rows), 0) AS row_count \
             FROM sys.tables t JOIN sys.schemas s ON s.schema_id = t.schema_id \
             LEFT JOIN sys.partitions p ON p.object_id = t.object_id AND p.index_id IN (0, 1) \
             GROUP BY s.name, t.name ORDER BY s.name, t.name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(Table {
                schema: text(row, 0)?,
                name: text(row, 1)?,
                row_count: optional_u64(row, 2)?,
            })
        })
        .collect()
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
async fn load_columns(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Column>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name AS schema_name, t.name AS table_name, c.name AS column_name, ty.name AS data_type, \
             c.column_id, c.is_nullable, dc.definition, c.is_identity, c.is_computed, c.collation_name \
             FROM sys.columns c JOIN sys.tables t ON t.object_id = c.object_id \
             JOIN sys.schemas s ON s.schema_id = t.schema_id JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
             LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
             ORDER BY s.name, t.name, c.column_id",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .enumerate()
        .map(|(ordinal, row)| {
            Ok(Column {
                schema: text(row, 0)?,
                table_name: text(row, 1)?,
                name: text(row, 2)?,
                data_type: text(row, 3)?,
                ordinal: optional_u64(row, 4)?
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(ordinal),
                nullable: bool_value(row, 5)?,
                default: optional_text(row, 6)?,
                is_identity: bool_value(row, 7)?,
                is_generated: bool_value(row, 8)?,
                collation: optional_text(row, 9)?,
                enum_labels: Vec::new(),
                is_primary_key: false,
                is_unique: false,
            })
        })
        .collect()
}

async fn load_primary_keys(
    connector: &SqlServerConnector,
    handle: &ConnectionHandle,
) -> Result<Vec<PrimaryKey>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name, t.name, i.name, c.name, ic.key_ordinal \
             FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id \
             JOIN sys.schemas s ON s.schema_id = t.schema_id JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
             WHERE i.is_primary_key = 1 ORDER BY s.name, t.name, ic.key_ordinal",
            &[],
        )
        .await?;
    let mut grouped = HashMap::<(String, String), PrimaryKey>::new();
    for row in &result.rows {
        let schema = text(row, 0)?;
        let table = text(row, 1)?;
        let constraint_name = text(row, 2)?;
        let column_name = text(row, 3)?;
        let key = (schema.clone(), table.clone());
        let entry = grouped.entry(key).or_insert_with(|| PrimaryKey {
            constraint_name,
            columns: Vec::new(),
            table_name: table,
            schema,
        });
        entry.columns.push(column_name);
    }
    Ok(grouped.into_values().collect())
}

// cc-scan:allow COMPLEXITY — classifier/dispatch ladder — one case per branch
async fn load_indexes(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Index>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "SELECT s.name, t.name, i.name, i.is_unique, i.is_unique_constraint, i.type_desc, ic.is_included_column, c.name, ic.key_ordinal, i.filter_definition \
             FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id \
             JOIN sys.schemas s ON s.schema_id = t.schema_id JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
             WHERE i.is_primary_key = 0 AND i.index_id > 0 ORDER BY s.name, t.name, i.name, ic.key_ordinal, ic.index_column_id",
            &[],
        )
        .await?;
    let mut grouped = HashMap::<(String, String, String), Index>::new();
    for row in &result.rows {
        let schema = text(row, 0)?;
        let table = text(row, 1)?;
        let name = text(row, 2)?;
        let unique = bool_value(row, 3)?;
        let is_unique_constraint = bool_value(row, 4)?;
        let method = text(row, 5)?;
        let is_included_column = bool_value(row, 6)?;
        let column_name = text(row, 7)?;
        let predicate = optional_text(row, 9)?;
        let key = (schema.clone(), table.clone(), name.clone());
        let entry = grouped.entry(key).or_insert_with(|| Index {
            name,
            columns: Vec::new(),
            unique,
            method,
            primary: false,
            include_columns: Vec::new(),
            predicate,
            definition: String::new(),
            origin: if is_unique_constraint {
                IndexOrigin::UniqueConstraint
            } else {
                IndexOrigin::User
            },
            table_name: table,
            schema,
        });
        if is_included_column {
            entry.include_columns.push(column_name);
        } else {
            entry.columns.push(column_name);
        }
    }
    Ok(grouped.into_values().collect())
}

async fn load_foreign_keys(
    connector: &SqlServerConnector,
    handle: &ConnectionHandle,
) -> Result<Vec<ForeignKey>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "SELECT sch.name, t.name, fk.name, c.name, rsch.name, rt.name, rc.name, fkc.constraint_column_id, fk.update_referential_action_desc, fk.delete_referential_action_desc \
             FROM sys.foreign_keys fk JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.tables t ON t.object_id = fk.parent_object_id JOIN sys.schemas sch ON sch.schema_id = t.schema_id \
             JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id \
             JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id JOIN sys.schemas rsch ON rsch.schema_id = rt.schema_id \
             JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id \
             ORDER BY sch.name, t.name, fk.name, fkc.constraint_column_id",
            &[],
        )
        .await?;
    let mut grouped = HashMap::<(String, String, String), ForeignKey>::new();
    for row in &result.rows {
        let schema = text(row, 0)?;
        let table = text(row, 1)?;
        let name = text(row, 2)?;
        let from_column = text(row, 3)?;
        let to_schema = text(row, 4)?;
        let to_table = text(row, 5)?;
        let to_column = text(row, 6)?;
        let on_update = foreign_key_action(row, 8)?;
        let on_delete = foreign_key_action(row, 9)?;
        let key = (schema.clone(), table.clone(), name.clone());
        let entry = grouped.entry(key).or_insert_with(|| ForeignKey {
            name,
            from_table: table,
            from_columns: Vec::new(),
            to_table,
            to_columns: Vec::new(),
            schema,
            to_schema,
            on_update,
            on_delete,
            ..ForeignKey::default()
        });
        entry.from_columns.push(from_column);
        entry.to_columns.push(to_column);
    }
    Ok(grouped.into_values().collect())
}

async fn load_checks(
    connector: &SqlServerConnector,
    handle: &ConnectionHandle,
) -> Result<Vec<CheckConstraint>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name, t.name, cc.name, cc.definition FROM sys.check_constraints cc \
             JOIN sys.tables t ON t.object_id = cc.parent_object_id JOIN sys.schemas s ON s.schema_id = t.schema_id \
             ORDER BY s.name, t.name, cc.name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(CheckConstraint {
                schema: text(row, 0)?,
                table_name: text(row, 1)?,
                name: text(row, 2)?,
                definition: text(row, 3)?,
            })
        })
        .collect()
}

async fn load_views(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<View>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name, v.name, COALESCE(sm.definition, '') FROM sys.views v \
             JOIN sys.schemas s ON s.schema_id = v.schema_id LEFT JOIN sys.sql_modules sm ON sm.object_id = v.object_id \
             ORDER BY s.name, v.name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(View {
                schema: text(row, 0)?,
                name: text(row, 1)?,
                definition: text(row, 2)?,
                kind: RelationKind::View,
            })
        })
        .collect()
}

async fn load_triggers(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Trigger>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name, t.name, tr.name, tr.is_disabled, COALESCE(sm.definition, '') \
             FROM sys.triggers tr JOIN sys.tables t ON t.object_id = tr.parent_id \
             JOIN sys.schemas s ON s.schema_id = t.schema_id LEFT JOIN sys.sql_modules sm ON sm.object_id = tr.object_id \
             WHERE tr.parent_class = 1 ORDER BY s.name, t.name, tr.name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(Trigger {
                schema: text(row, 0)?,
                table_name: text(row, 1)?,
                name: text(row, 2)?,
                enabled: !bool_value(row, 3)?,
                definition: text(row, 4)?,
                timing: "".into(),
                event: "".into(),
                function_def: String::new(),
            })
        })
        .collect()
}

async fn load_routines(connector: &SqlServerConnector, handle: &ConnectionHandle) -> Result<Vec<Function>, DbError> {
    let result = connector
        .query_uncapped(
            handle,
            "SELECT s.name, o.name, o.type_desc, COALESCE(sm.definition, ''), o.object_id \
             FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id \
             LEFT JOIN sys.sql_modules sm ON sm.object_id = o.object_id \
             WHERE o.type IN ('FN', 'IF', 'TF', 'P', 'PC') ORDER BY s.name, o.name",
            &[],
        )
        .await?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(Function {
                schema: text(row, 0)?,
                name: text(row, 1)?,
                routine_type: text(row, 2)?,
                definition: text(row, 3)?,
                data_type: String::new(),
                identity_arguments: text(row, 4)?,
                language: "T-SQL".into(),
                volatility: String::new(),
                security_definer: false,
                parameters: Vec::<RoutineParameter>::new(),
            })
        })
        .collect()
}

fn cell(row: &Row, index: usize) -> Result<&CellValue, DbError> {
    row.0
        .get(index)
        .ok_or_else(|| DbError::IntrospectionFailed(format!("SQL Server catalog row is missing column {index}")))
}

fn text(row: &Row, index: usize) -> Result<String, DbError> {
    match cell(row, index)? {
        CellValue::Text(value) | CellValue::Uuid(value) | CellValue::Decimal(value) => Ok(value.clone()),
        CellValue::Int64(value) => Ok(value.to_string()),
        CellValue::Null => Ok(String::new()),
        value => Err(DbError::IntrospectionFailed(format!(
            "expected text at catalog column {index}, got {value:?}"
        ))),
    }
}

fn foreign_key_action(row: &Row, index: usize) -> Result<String, DbError> {
    match cell(row, index)? {
        CellValue::Text(value) => Ok(value.replace('_', " ")),
        _ => text(row, index),
    }
}

fn optional_text(row: &Row, index: usize) -> Result<Option<String>, DbError> {
    match cell(row, index)? {
        CellValue::Null => Ok(None),
        _ => Ok(Some(text(row, index)?)),
    }
}

fn bool_value(row: &Row, index: usize) -> Result<bool, DbError> {
    match cell(row, index)? {
        CellValue::Bool(value) => Ok(*value),
        CellValue::Int64(value) => Ok(*value != 0),
        CellValue::Null => Ok(false),
        value => Err(DbError::IntrospectionFailed(format!(
            "expected boolean at catalog column {index}, got {value:?}"
        ))),
    }
}

fn optional_u64(row: &Row, index: usize) -> Result<Option<u64>, DbError> {
    match cell(row, index)? {
        CellValue::Null => Ok(None),
        CellValue::Int64(value) if *value >= 0 => Ok(Some(*value as u64)),
        CellValue::Decimal(value) => value
            .parse::<u64>()
            .map(Some)
            .map_err(|error| DbError::IntrospectionFailed(format!("invalid unsigned catalog value {value}: {error}"))),
        value => Err(DbError::IntrospectionFailed(format!(
            "expected integer at catalog column {index}, got {value:?}"
        ))),
    }
}

#[cfg(test)]
#[path = "tests/introspect_tests.rs"]
mod tests;
