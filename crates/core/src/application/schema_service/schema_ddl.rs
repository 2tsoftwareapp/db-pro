use crate::domain::connection::DriverType;
use crate::domain::schema::{CheckConstraint, ForeignKey, IndexOrigin, TableInfo, Trigger};
use crate::ports::SqlDialect;

use super::super::driver_dialect::DriverDialect;
use super::super::sql_builder::qualify;

struct ForeignKeyDdlGroup<'a> {
    name: &'a str,
    from_columns: Vec<&'a str>,
    to_table: &'a str,
    to_columns: Vec<&'a str>,
    to_schema: &'a str,
    on_update: &'a str,
    on_delete: &'a str,
    match_option: &'a str,
    deferrable: bool,
    initially_deferred: bool,
}

pub(super) fn build_create_table_ddl(
    info: &TableInfo,
    driver: DriverType,
    check_constraints: &[CheckConstraint],
) -> String {
    let dialect = DriverDialect::new(driver);
    let qualified = qualify_name_for_driver(driver, &info.table.schema, &info.table.name);
    let foreign_keys = group_foreign_keys_for_ddl(&info.foreign_keys);
    let definitions = table_definitions(info, driver, &dialect, check_constraints, &foreign_keys);
    let mut ddl = format!("CREATE TABLE {qualified} (\n{}\n);\n", definitions.join(",\n"));

    if driver != DriverType::Mysql {
        append_index_ddl(&mut ddl, info, driver, &dialect);
    }
    if driver == DriverType::Postgres {
        append_postgres_foreign_keys(&mut ddl, &qualified, &dialect, &foreign_keys);
    }

    ddl
}

pub(super) fn format_trigger_ddl(trigger: &Trigger) -> String {
    // Both providers report `definition` as the complete `CREATE TRIGGER`
    // statement (SQLite straight from `sqlite_master`, PostgreSQL from
    // `pg_get_triggerdef`), so it is replayed verbatim — never rebuilt from the
    // summary columns. Rebuilding dropped `FOR EACH ROW`, `WHEN (...)`,
    // `UPDATE OF col`, `REFERENCING` and `DEFERRABLE`, which silently turned a
    // row-level trigger into a statement-level one (#594).
    if trigger.is_complete_definition() {
        return format!("{};\n", trigger.definition.trim().trim_end_matches(';'));
    }

    let dialect = DriverDialect::new(DriverType::Postgres);
    let qualified = qualify(&dialect, &trigger.schema, &trigger.table_name);
    let trigger_stmt = format!(
        "CREATE TRIGGER {}\n  {} {} ON {}\n  {};\n",
        dialect.quote_identifier(&trigger.name),
        trigger.timing,
        trigger.event,
        qualified,
        trigger.definition
    );
    if trigger.function_def.is_empty() {
        trigger_stmt
    } else {
        // `pg_get_functiondef` does not terminate its statement, so the `;` is
        // added here — without it the function and the CREATE TRIGGER that
        // followed it were one syntactically invalid statement (#594).
        format!(
            "{}\n\n{}",
            trigger.function_def.trim().trim_end_matches(';'),
            trigger_statement_with_semicolon(&trigger_stmt)
        )
    }
}

fn trigger_statement_with_semicolon(statement: &str) -> String {
    let trimmed = statement.trim_end();
    if trimmed.ends_with(';') {
        statement.to_owned()
    } else {
        format!("{trimmed};\n")
    }
}

fn group_foreign_keys_for_ddl(foreign_keys: &[ForeignKey]) -> Vec<ForeignKeyDdlGroup<'_>> {
    foreign_keys
        .iter()
        .map(|fk| ForeignKeyDdlGroup {
            name: &fk.name,
            from_columns: fk.from_columns.iter().map(|s| s.as_str()).collect(),
            to_table: &fk.to_table,
            to_columns: fk.to_columns.iter().map(|s| s.as_str()).collect(),
            to_schema: &fk.to_schema,
            on_update: &fk.on_update,
            on_delete: &fk.on_delete,
            match_option: &fk.match_option,
            deferrable: fk.deferrable,
            initially_deferred: fk.initially_deferred,
        })
        .collect()
}

fn table_definitions(
    info: &TableInfo,
    driver: DriverType,
    dialect: &dyn SqlDialect,
    check_constraints: &[CheckConstraint],
    foreign_keys: &[ForeignKeyDdlGroup<'_>],
) -> Vec<String> {
    let mut definitions: Vec<_> = info
        .columns
        .iter()
        .map(|column| format_column_definition(column, dialect))
        .collect();

    if let Some(ref pk) = info.primary_key {
        definitions.push(format!(
            "    PRIMARY KEY ({})",
            quote_columns(dialect, &pk.columns.iter().map(String::as_str).collect::<Vec<_>>())
        ));
    }
    definitions.extend(
        check_constraints
            .iter()
            .map(|constraint| format_check_constraint(constraint, dialect)),
    );

    if driver == DriverType::Mysql {
        // Declare existing indexes before FKs so MySQL reuses supporting indexes
        // rather than auto-creating one and colliding with a later CREATE INDEX.
        definitions.extend(
            info.indexes
                .iter()
                .filter(|index| !index.primary)
                .map(|index| format_mysql_index(index, dialect)),
        );
    }
    if driver != DriverType::Postgres {
        definitions.extend(
            foreign_keys
                .iter()
                .map(|foreign_key| format_foreign_key(foreign_key, driver, dialect)),
        );
    }
    if driver == DriverType::SQLite {
        definitions.extend(
            info.indexes
                .iter()
                .filter(|index| index.origin == IndexOrigin::UniqueConstraint)
                .map(|index| format_sqlite_unique_constraint(index, dialect)),
        );
    }

    definitions
}

fn format_column_definition(column: &crate::domain::schema::Column, dialect: &dyn SqlDialect) -> String {
    let mut definition = format!("    {} {}", dialect.quote_identifier(&column.name), column.data_type);
    if column.is_identity {
        definition.push_str(" GENERATED BY DEFAULT AS IDENTITY");
    } else if column.is_generated {
        if let Some(ref expression) = column.default {
            definition.push_str(&format!(" GENERATED ALWAYS AS ({expression}) STORED"));
        }
    }
    if !column.nullable {
        definition.push_str(" NOT NULL");
    }
    if !column.is_identity && !column.is_generated {
        if let Some(ref default) = column.default {
            definition.push_str(&format!(" DEFAULT {default}"));
        }
    }
    definition
}

fn format_check_constraint(constraint: &CheckConstraint, dialect: &dyn SqlDialect) -> String {
    format!(
        "    CONSTRAINT {} {}",
        dialect.quote_identifier(&constraint.name),
        check_constraint_definition(constraint),
    )
}

fn format_foreign_key(foreign_key: &ForeignKeyDdlGroup<'_>, driver: DriverType, dialect: &dyn SqlDialect) -> String {
    let to_qualified = qualify_name_for_driver(driver, foreign_key.to_schema, foreign_key.to_table);
    let from_columns = quote_columns(dialect, &foreign_key.from_columns);
    let to_columns = quote_columns(dialect, &foreign_key.to_columns);
    let actions = format_foreign_key_actions(foreign_key);
    format!(
        "    CONSTRAINT {} FOREIGN KEY ({from_columns}) REFERENCES {to_qualified} ({to_columns}){actions}",
        dialect.quote_identifier(foreign_key.name),
    )
}

fn format_foreign_key_actions(foreign_key: &ForeignKeyDdlGroup<'_>) -> String {
    let mut clauses = String::new();
    if !foreign_key.match_option.is_empty()
        && foreign_key.match_option != "SIMPLE"
        && foreign_key.match_option != "NONE"
    {
        clauses.push_str(&format!(" MATCH {}", foreign_key.match_option));
    }
    if !foreign_key.on_update.is_empty() && foreign_key.on_update != "NO ACTION" {
        clauses.push_str(&format!(" ON UPDATE {}", foreign_key.on_update));
    }
    if !foreign_key.on_delete.is_empty() && foreign_key.on_delete != "NO ACTION" {
        clauses.push_str(&format!(" ON DELETE {}", foreign_key.on_delete));
    }
    if foreign_key.deferrable {
        clauses.push_str(" DEFERRABLE");
        if foreign_key.initially_deferred {
            clauses.push_str(" INITIALLY DEFERRED");
        }
    }
    clauses
}

fn format_sqlite_unique_constraint(index: &crate::domain::schema::Index, dialect: &dyn SqlDialect) -> String {
    let columns = quote_columns(dialect, &index.columns.iter().map(String::as_str).collect::<Vec<_>>());
    format!("    UNIQUE ({columns})")
}

fn format_mysql_index(index: &crate::domain::schema::Index, dialect: &dyn SqlDialect) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let name = dialect.quote_identifier(&index.name);
    let columns = quote_columns(dialect, &index.columns.iter().map(String::as_str).collect::<Vec<_>>());
    format!("    {unique}INDEX {name} ({columns})")
}

fn append_index_ddl(ddl: &mut String, info: &TableInfo, driver: DriverType, dialect: &dyn SqlDialect) {
    let index_target = qualify_name_for_driver(driver, &info.table.schema, &info.table.name);
    for index in &info.indexes {
        if index.primary || (driver == DriverType::SQLite && index.origin == IndexOrigin::UniqueConstraint) {
            continue;
        }
        if driver == DriverType::Postgres && !index.definition.is_empty() {
            ddl.push_str(&index.definition);
            ddl.push_str(";\n");
            continue;
        }
        let unique = if index.unique { "UNIQUE " } else { "" };
        // Index names are local to the target table's schema, not schema-qualified.
        let index_name = dialect.quote_identifier(&index.name);
        let cols = quote_columns(dialect, &index.columns.iter().map(String::as_str).collect::<Vec<_>>());
        ddl.push_str(&format!(
            "CREATE {unique}INDEX {index_name} ON {index_target} ({cols});\n"
        ));
    }
}

fn append_postgres_foreign_keys(
    ddl: &mut String,
    qualified_table: &str,
    dialect: &dyn SqlDialect,
    foreign_keys: &[ForeignKeyDdlGroup<'_>],
) {
    for foreign_key in foreign_keys {
        let to_qualified = qualify(dialect, foreign_key.to_schema, foreign_key.to_table);
        let from_columns = quote_columns(dialect, &foreign_key.from_columns);
        let to_columns = quote_columns(dialect, &foreign_key.to_columns);
        ddl.push_str(&format!(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "ALTER TABLE {qualified_table} ADD CONSTRAINT {} FOREIGN KEY ({from_columns}) REFERENCES {to_qualified} ({to_columns}){};\n",
            dialect.quote_identifier(foreign_key.name),
            format_foreign_key_actions(foreign_key),
        ));
    }
}

fn qualify_name_for_driver(driver: DriverType, schema: &str, name: &str) -> String {
    let dialect = DriverDialect::new(driver);
    if driver == DriverType::SQLite {
        dialect.quote_identifier(name)
    } else {
        qualify(&dialect, schema, name)
    }
}

fn quote_columns(dialect: &dyn SqlDialect, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| dialect.quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_constraint_definition(constraint: &CheckConstraint) -> String {
    if constraint
        .definition
        .trim_start()
        .to_ascii_uppercase()
        .starts_with("CHECK")
    {
        constraint.definition.clone()
    } else {
        format!("CHECK ({})", constraint.definition)
    }
}
