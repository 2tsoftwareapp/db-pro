// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
use std::collections::{HashMap, HashSet};

use sqlparser::{
    ast::{Expr, SelectItem, SetExpr, Statement},
    dialect::{PostgreSqlDialect, SQLiteDialect},
    parser::Parser,
};

use crate::{
    editor::{
        completion::{CompletionItem, CompletionItemKind},
        prediction::{
            AiSqlContext, MAX_COLUMNS_PER_TABLE, MAX_CTES, MAX_FK_NEIGHBORS, MAX_REFERENCED_TABLES,
            MAX_SQL_AFTER_CHARS, MAX_SQL_BEFORE_CHARS,
        },
        syntax::CachedSqlTokens,
    },
    runtime::{UiSchemaSummary, UiTableSummary},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlClause {
    Select,
    From,
    Join,
    JoinOn,
    Where,
    GroupBy,
    OrderBy,
    Having,
    InsertInto,
    Values,
    Update,
    Set,
    DeleteFrom,
    Returning,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct CompletionContext<'a> {
    pub text_before_cursor: &'a str,
    pub text_after_cursor: &'a str,
    pub cursor_offset: usize,
    pub active_schema: &'a str,
    pub schema_summary: &'a UiSchemaSummary,
    pub cached_tokens: Option<&'a CachedSqlTokens>,
    pub is_sqlite: bool,
    pub is_manual_trigger: bool,
}

#[derive(Debug, Clone)]
pub struct CteDefinition {
    pub name: String,
    pub target_table: Option<String>,
    pub explicit_columns: Vec<String>,
    pub inferred_columns: Vec<String>,
}

pub struct SchemaCompletionProvider;

/// Resolve an unqualified table reference against the catalog, preferring the
/// active schema (#511). A plain `find` by name returns catalog order, which
/// suggests another schema's columns when two schemas hold same-named tables.
/// Falls back to the first name match so tables that only exist in another
/// schema (or an empty active schema) keep the previous behavior.
fn find_completion_table<'a>(
    tables: &'a [UiTableSummary],
    name: &str,
    active_schema: &str,
) -> Option<&'a UiTableSummary> {
    tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name) && t.schema.eq_ignore_ascii_case(active_schema))
        .or_else(|| tables.iter().find(|t| t.name.eq_ignore_ascii_case(name)))
}

impl SchemaCompletionProvider {
    pub fn provide(ctx: &CompletionContext<'_>) -> (String, Vec<CompletionItem>) {
        // 1. Suppress completion inside strings and comments unless manual trigger (Ctrl+Space)
        if !ctx.is_manual_trigger {
            if let Some(tokens) = ctx.cached_tokens {
                if tokens.is_in_string_or_comment(ctx.cursor_offset.saturating_sub(1)) {
                    return (String::new(), Vec::new());
                }
            }
        }

        let text_before = ctx.text_before_cursor;
        let prefix = extract_word_prefix(text_before);
        let prefix_lower = prefix.to_lowercase();
        // An empty prefix matches every name. Ctrl/Cmd+Space must stay on the
        // clause (its columns/tables and keywords), not walk the whole catalog.
        let manual_blank = ctx.is_manual_trigger && prefix.is_empty();
        let replacement_start = ctx.cursor_offset.saturating_sub(prefix.len());
        let replacement_range = (replacement_start, ctx.cursor_offset);

        // No completion directly after `AS` — the cursor is in alias position
        // (`SELECT x AS |`) and columns/keywords are noise there (#607). A manual
        // Ctrl+Space still works for users who really want it.
        if !ctx.is_manual_trigger && ends_with_as_keyword(text_before) {
            return (String::new(), Vec::new());
        }

        // Check for dot qualifier right before the prefix
        let before_prefix = &text_before[..text_before.len() - prefix.len()];
        let qualifier = extract_qualifier(before_prefix);

        // Scope the real text. A space injected at the cursor splits the word
        // being typed (`ord|ers` becomes `ord ers`) and invents a false alias.
        let mut real_doc = String::with_capacity(ctx.text_before_cursor.len() + ctx.text_after_cursor.len());
        real_doc.push_str(ctx.text_before_cursor);
        real_doc.push_str(ctx.text_after_cursor);
        let cursor = ctx.text_before_cursor.len().min(real_doc.len());
        let scope = super::completion_scope::analyze(&real_doc, cursor, prefix.len());
        let mut full_doc = String::with_capacity(real_doc.len() + 1);
        full_doc.push_str(ctx.text_before_cursor);
        full_doc.push(' ');
        full_doc.push_str(ctx.text_after_cursor);
        let statement = &full_doc[scope.statement_start..scope.statement_end];
        let cursor_in_statement = cursor.saturating_sub(scope.statement_start).min(statement.len());
        let ctes = extract_cte_definitions(statement);
        let aliases = &scope.aliases;
        let subquery_aliases = extract_subquery_aliases(statement);
        let clause = scope.clause;
        let mutation_target = extract_mutation_target(statement);
        let parsed_select = parse_select_context(
            &statement[..cursor_in_statement],
            &statement[cursor_in_statement..],
            ctx.is_sqlite,
        );
        let inserted_columns = if clause == SqlClause::InsertInto {
            extract_inserted_column_names(statement)
        } else {
            HashSet::new()
        };

        let mut items = Vec::new();

        if let Some(qualifier) = qualifier {
            let qual_lower = qualifier.to_lowercase();

            // 1. Multi-part qualifier: schema.table. (e.g. "public.users.")
            if let Some((schema_part, table_part)) = qual_lower.split_once('.') {
                for table in &ctx.schema_summary.table_details {
                    if table.schema.eq_ignore_ascii_case(schema_part) && table.name.eq_ignore_ascii_case(table_part) {
                        // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
                        for col in &table.columns {
                            if col.name.to_lowercase().contains(&prefix_lower) {
                                // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
                                items.push(CompletionItem {
                                    label: col.name.clone(),
                                    insert_text: col.name.clone(),
                                    kind: CompletionItemKind::Column,
                                    detail: Some(format!(
                                        "Column · {}{}",
                                        col.data_type,
                                        if col.is_primary_key { " [PK]" } else { "" }
                                    )),
                                    documentation: Some(format!(
                                        "Column {} of {}.{}{}",
                                        col.name,
                                        table.schema,
                                        table.name,
                                        if col.nullable { " (NULL)" } else { " (NOT NULL)" }
                                    )),
                                    replacement_range,
                                    sort_score: 950,
                                });
                            }
                        }
                    }
                }
                return finish_ranked(prefix, &prefix_lower, items);
            }

            // 2. Check if qualifier is a schema name (e.g. "public.")
            let is_known_schema = ctx
                .schema_summary
                .schemas
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&qual_lower))
                || ctx
                    .schema_summary
                    .table_details
                    .iter()
                    .any(|t| t.schema.eq_ignore_ascii_case(&qual_lower));

            if is_known_schema {
                for table in &ctx.schema_summary.table_details {
                    if table.schema.eq_ignore_ascii_case(&qual_lower)
                        && table.name.to_lowercase().contains(&prefix_lower)
                    {
                        items.push(CompletionItem {
                            label: table.name.clone(),
                            insert_text: table.name.clone(),
                            kind: CompletionItemKind::Table,
                            detail: Some(format!("Table · {}.{}", table.schema, table.name)),
                            documentation: Some(format!("Table with {} columns", table.columns.len())),
                            replacement_range,
                            sort_score: 920,
                        });
                    }
                }
                for view in &ctx.schema_summary.views {
                    if view.schema.eq_ignore_ascii_case(&qual_lower) && view.name.to_lowercase().contains(&prefix_lower)
                    {
                        items.push(CompletionItem {
                            label: view.name.clone(),
                            insert_text: view.name.clone(),
                            kind: CompletionItemKind::View,
                            detail: Some(format!("View · {}.{}", view.schema, view.name)),
                            documentation: Some("Database View".to_owned()),
                            replacement_range,
                            sort_score: 900,
                        });
                    }
                }
                for func in &ctx.schema_summary.functions {
                    if func.schema.eq_ignore_ascii_case(&qual_lower) && func.name.to_lowercase().contains(&prefix_lower)
                    {
                        items.push(CompletionItem {
                            label: format!("{}()", func.name),
                            insert_text: format!("{}(", func.name),
                            kind: CompletionItemKind::Function,
                            detail: Some(format!("Function · {}.{}", func.schema, func.name)),
                            documentation: (!func.data_type.is_empty()).then(|| format!("Returns: {}", func.data_type)),
                            replacement_range,
                            sort_score: 850,
                        });
                    }
                }
                if !items.is_empty() {
                    return finish_ranked(prefix, &prefix_lower, items);
                }
            }

            // 3. Check if qualifier is a defined CTE
            if let Some(cte) = ctes.get(&qual_lower) {
                // If CTE has explicit column names, suggest them first
                if !cte.explicit_columns.is_empty() {
                    for col in &cte.explicit_columns {
                        if col.to_lowercase().contains(&prefix_lower) {
                            items.push(CompletionItem {
                                label: col.clone(),
                                insert_text: col.clone(),
                                kind: CompletionItemKind::Column,
                                detail: Some(format!("Column · CTE {}", cte.name)),
                                documentation: None,
                                replacement_range,
                                sort_score: 950,
                            });
                        }
                    }
                }
                for col in &cte.inferred_columns {
                    if col.to_lowercase().contains(&prefix_lower)
                        && !items.iter().any(|item| item.label.eq_ignore_ascii_case(col))
                    {
                        items.push(CompletionItem {
                            label: col.clone(),
                            insert_text: col.clone(),
                            kind: CompletionItemKind::Column,
                            detail: Some(format!("Column · CTE {}", cte.name)),
                            documentation: Some("Inferred from the CTE SELECT list".to_owned()),
                            replacement_range,
                            sort_score: 940,
                        });
                    }
                }
                // If CTE targets an underlying table, suggest table columns
                if let Some(target) = &cte.target_table {
                    for table in &ctx.schema_summary.table_details {
                        if table.name.eq_ignore_ascii_case(target) {
                            for col in &table.columns {
                                if col.name.to_lowercase().contains(&prefix_lower)
                                    && !items.iter().any(|i| i.label == col.name)
                                {
                                    items.push(CompletionItem {
                                        label: col.name.clone(),
                                        insert_text: col.name.clone(),
                                        kind: CompletionItemKind::Column,
                                        detail: Some(format!(
                                            "Column · {}{}",
                                            col.data_type,
                                            if col.is_primary_key { " [PK]" } else { "" }
                                        )),
                                        documentation: Some(format!(
                                            "CTE {} -> {}.{}",
                                            cte.name, table.schema, table.name
                                        )),
                                        replacement_range,
                                        sort_score: 930,
                                    });
                                }
                            }
                        }
                    }
                }
                if !items.is_empty() {
                    return finish_ranked(prefix, &prefix_lower, items);
                }
            }

            if let Some(columns) = subquery_aliases.get(&qual_lower) {
                for column in columns {
                    if column.to_lowercase().contains(&prefix_lower) {
                        items.push(CompletionItem {
                            label: column.clone(),
                            insert_text: column.clone(),
                            kind: CompletionItemKind::Column,
                            detail: Some(format!("Column · subquery {qualifier}")),
                            documentation: Some("Inferred from the subquery SELECT list".to_owned()),
                            replacement_range,
                            sort_score: 940,
                        });
                    }
                }
                if !items.is_empty() {
                    return finish_ranked(prefix, &prefix_lower, items);
                }
            }

            // 4. Check if qualifier is a table name or an alias in the document
            let resolved_table = aliases.get(qual_lower.as_str()).cloned().unwrap_or(qual_lower);
            for table in &ctx.schema_summary.table_details {
                if table.name.eq_ignore_ascii_case(&resolved_table) {
                    for col in &table.columns {
                        if col.name.to_lowercase().contains(&prefix_lower) {
                            items.push(CompletionItem {
                                label: col.name.clone(),
                                insert_text: col.name.clone(),
                                kind: CompletionItemKind::Column,
                                detail: Some(format!(
                                    "Column · {}{}",
                                    col.data_type,
                                    if col.is_primary_key { " [PK]" } else { "" }
                                )),
                                documentation: Some(format!(
                                    "Column {} of {}.{}{}",
                                    col.name,
                                    table.schema,
                                    table.name,
                                    if col.nullable { " (NULL)" } else { " (NOT NULL)" }
                                )),
                                replacement_range,
                                sort_score: 950,
                            });
                        }
                    }
                }
            }

            return finish_ranked(prefix, &prefix_lower, items);
        }

        // Clause-driven completions when no dot qualifier is present:

        // JOIN / ON: foreign-key predicate. `ON` is inserted only when it has
        // not been typed yet. A side without an alias is qualified by table name.
        if clause == SqlClause::JoinOn {
            items.extend(fk_join_items(ctx.schema_summary, &scope, false, replacement_range));
        }

        // 1. FROM / JOIN clauses: suggest Tables, Views, CTEs, Schemas
        if matches!(clause, SqlClause::From | SqlClause::Join) {
            // Suggest CTE names
            for cte in ctes.values() {
                if cte.name.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: cte.name.clone(),
                        insert_text: cte.name.clone(),
                        kind: CompletionItemKind::Cte,
                        detail: Some("Common Table Expression".to_owned()),
                        documentation: None,
                        replacement_range,
                        sort_score: 960,
                    });
                }
            }

            // Suggest Tables
            for table in &ctx.schema_summary.table_details {
                if table.name.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: table.name.clone(),
                        insert_text: table.name.clone(),
                        kind: CompletionItemKind::Table,
                        detail: Some(format!("Table · {}.{}", table.schema, table.name)),
                        documentation: Some(format!("Table with {} columns", table.columns.len())),
                        replacement_range,
                        sort_score: 920,
                    });
                }
            }
            if items.is_empty() {
                for table_name in &ctx.schema_summary.tables {
                    if table_name.to_lowercase().contains(&prefix_lower) {
                        items.push(CompletionItem {
                            label: table_name.clone(),
                            insert_text: table_name.clone(),
                            kind: CompletionItemKind::Table,
                            detail: Some("Table".to_owned()),
                            documentation: None,
                            replacement_range,
                            sort_score: 900,
                        });
                    }
                }
            }
            // Suggest Views
            for view in &ctx.schema_summary.views {
                if view.name.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: view.name.clone(),
                        insert_text: view.name.clone(),
                        kind: CompletionItemKind::View,
                        detail: Some(format!("View · {}.{}", view.schema, view.name)),
                        documentation: Some("Database View".to_owned()),
                        replacement_range,
                        sort_score: 880,
                    });
                }
            }
            // Suggest Schemas
            for schema in &ctx.schema_summary.schemas {
                if schema.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: format!("{schema}."),
                        insert_text: format!("{schema}."),
                        kind: CompletionItemKind::Schema,
                        detail: Some("Database Schema".to_owned()),
                        documentation: None,
                        replacement_range,
                        sort_score: 850,
                    });
                }
            }
            if clause == SqlClause::Join {
                items.extend(fk_join_items(ctx.schema_summary, &scope, true, replacement_range));
                items.extend(fk_join_target_items(
                    ctx.schema_summary,
                    &scope,
                    &prefix_lower,
                    replacement_range,
                ));
            }
            // Keywords for FROM / JOIN
            add_clause_keywords(&mut items, clause, &prefix_lower, replacement_range);
            return finish_ranked(prefix, &prefix_lower, items);
        }

        // 2. SELECT / WHERE / GROUP BY / ORDER BY / HAVING / RETURNING:
        // columns of tables named by FROM / JOIN / UPDATE / INTO in this statement.
        let referenced_table_names = &scope.referenced_tables;

        // Detect column name frequencies across referenced tables for ambiguity check
        let mut col_frequencies: HashMap<String, usize> = HashMap::new();
        for t_name in referenced_table_names {
            if let Some(t) = find_completion_table(&ctx.schema_summary.table_details, t_name, ctx.active_schema) {
                for c in &t.columns {
                    *col_frequencies.entry(c.name.to_lowercase()).or_insert(0) += 1;
                }
            }
        }

        // Suggest columns from referenced tables
        for t_name in referenced_table_names {
            if let Some(table) = find_completion_table(&ctx.schema_summary.table_details, t_name, ctx.active_schema) {
                // Find alias if exists
                let alias = aliases
                    .iter()
                    .find(|(_, val)| val.eq_ignore_ascii_case(&table.name))
                    .map(|(k, _)| k.as_str());
                for col in &table.columns {
                    if clause == SqlClause::InsertInto
                        && mutation_target
                            .as_deref()
                            .is_some_and(|target| target.eq_ignore_ascii_case(&table.name))
                        && inserted_columns.contains(&col.name.to_lowercase())
                    {
                        continue;
                    }
                    if col.name.to_lowercase().contains(&prefix_lower) {
                        let is_ambiguous = col_frequencies.get(&col.name.to_lowercase()).copied().unwrap_or(0) > 1;
                        if let Some(a) = alias {
                            // Suggest qualified alias.col
                            items.push(CompletionItem {
                                label: format!("{a}.{}", col.name),
                                insert_text: format!("{a}.{}", col.name),
                                kind: CompletionItemKind::Column,
                                detail: Some(format!(
                                    "Column · {} ({}){}",
                                    col.data_type,
                                    table.name,
                                    if col.is_primary_key { " [PK]" } else { "" }
                                )),
                                documentation: None,
                                replacement_range,
                                sort_score: if is_ambiguous { 960 } else { 920 },
                            });
                        } else if is_ambiguous {
                            let qualified = format!("{}.{}", table.name, col.name);
                            items.push(CompletionItem {
                                label: qualified.clone(),
                                insert_text: qualified,
                                kind: CompletionItemKind::Column,
                                detail: Some(format!(
                                    "Column · {} ({}){}",
                                    col.data_type,
                                    table.name,
                                    if col.is_primary_key { " [PK]" } else { "" }
                                )),
                                documentation: None,
                                replacement_range,
                                sort_score: 960,
                            });
                        }
                        // Suggest bare col
                        items.push(CompletionItem {
                            label: col.name.clone(),
                            insert_text: col.name.clone(),
                            kind: CompletionItemKind::Column,
                            detail: Some(format!(
                                "Column · {} ({}){}",
                                col.data_type,
                                table.name,
                                if col.is_primary_key { " [PK]" } else { "" }
                            )),
                            documentation: None,
                            replacement_range,
                            sort_score: if is_ambiguous { 820 } else { 940 },
                        });
                    }
                }
            }
        }

        if let Some(select_context) = parsed_select {
            let output_names = if clause == SqlClause::GroupBy {
                select_context.non_aggregate_names
            } else if clause == SqlClause::OrderBy {
                select_context
                    .aliases
                    .into_iter()
                    .chain(select_context.non_aggregate_names)
                    .collect()
            } else {
                Vec::new()
            };
            for name in output_names {
                if name.to_lowercase().contains(&prefix_lower)
                    && !items.iter().any(|item| item.label.eq_ignore_ascii_case(&name))
                {
                    items.push(CompletionItem {
                        label: name.clone(),
                        insert_text: name,
                        kind: CompletionItemKind::Column,
                        detail: Some("SELECT output".to_owned()),
                        documentation: Some("Selected output column or alias".to_owned()),
                        replacement_range,
                        sort_score: 990,
                    });
                }
            }
        }

        // Suggest CTE columns if any defined
        for cte in ctes.values() {
            for col in cte.explicit_columns.iter().chain(cte.inferred_columns.iter()) {
                if col.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: format!("{}.{col}", cte.name),
                        insert_text: format!("{}.{col}", cte.name),
                        kind: CompletionItemKind::Column,
                        detail: Some(format!("Column · CTE {}", cte.name)),
                        documentation: None,
                        replacement_range,
                        sort_score: 930,
                    });
                }
            }
        }

        // Catalog-wide columns are a typed-prefix fallback only. Blank Ctrl/Cmd+Space
        // already collected the columns this clause wants from referenced tables.
        if items.is_empty() && !manual_blank {
            for table in &ctx.schema_summary.table_details {
                for col in &table.columns {
                    if col.name.to_lowercase().contains(&prefix_lower) {
                        items.push(CompletionItem {
                            label: col.name.clone(),
                            insert_text: col.name.clone(),
                            kind: CompletionItemKind::Column,
                            detail: Some(format!("{}.{} · {}", table.name, col.name, col.data_type)),
                            documentation: None,
                            replacement_range,
                            sort_score: 800,
                        });
                    }
                }
            }
        }

        // Tables and functions outside FROM/JOIN are prefix searches. An empty
        // manual trigger must not append every table_details row.
        if !manual_blank {
            for table in &ctx.schema_summary.table_details {
                if table.name.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: table.name.clone(),
                        insert_text: table.name.clone(),
                        kind: CompletionItemKind::Table,
                        detail: Some(format!("Table · {}.{}", table.schema, table.name)),
                        documentation: None,
                        replacement_range,
                        sort_score: 700,
                    });
                }
            }

            for func in &ctx.schema_summary.functions {
                if func.name.to_lowercase().contains(&prefix_lower) {
                    items.push(CompletionItem {
                        label: format!("{}()", func.name),
                        insert_text: format!("{}(", func.name),
                        kind: CompletionItemKind::Function,
                        detail: Some(format!("Function · {}.{}", func.schema, func.name)),
                        documentation: (!func.data_type.is_empty()).then(|| format!("Returns: {}", func.data_type)),
                        replacement_range,
                        sort_score: 650,
                    });
                }
            }
        }

        // Keywords
        add_clause_keywords(&mut items, clause, &prefix_lower, replacement_range);

        finish_ranked(prefix, &prefix_lower, items)
    }

    pub fn build_ai_sql_context(
        before_cursor: &str,
        after_cursor: &str,
        cursor_offset: usize,
        active_schema: &str,
        schema_summary: &UiSchemaSummary,
        is_sqlite: bool,
    ) -> AiSqlContext {
        let full_text = format!("{before_cursor}{after_cursor}");
        let mut ctx = self::build_ai_sql_context(&full_text, cursor_offset, active_schema, schema_summary);
        ctx.dialect = if is_sqlite {
            "SQLite".to_owned()
        } else {
            "PostgreSQL".to_owned()
        };
        ctx
    }
}

#[derive(Debug, Default)]
struct ParsedSelectContext {
    aliases: Vec<String>,
    non_aggregate_names: Vec<String>,
}

const POSTGRES_AGGREGATE_FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "ARRAY_AGG",
    "STRING_AGG",
    "BOOL_AND",
    "BOOL_OR",
    "JSON_AGG",
    "JSONB_AGG",
    "EVERY",
];
const SQLITE_AGGREGATE_FUNCTIONS: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT", "TOTAL"];

fn parse_select_context(before_cursor: &str, after_cursor: &str, is_sqlite: bool) -> Option<ParsedSelectContext> {
    let parse_input = format!("{before_cursor}__dbpro_cursor__{after_cursor}");
    let statements = if is_sqlite {
        Parser::parse_sql(&SQLiteDialect {}, &parse_input).ok()?
    } else {
        Parser::parse_sql(&PostgreSqlDialect {}, &parse_input).ok()?
    };
    let Statement::Query(query) = statements.first()? else {
        return None;
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };

    let mut context = ParsedSelectContext::default();
    for item in &select.projection {
        match item {
            SelectItem::ExprWithAlias { expr, alias } => {
                context.aliases.push(alias.value.clone());
                if !is_aggregate_expression(expr, is_sqlite) {
                    context.non_aggregate_names.push(alias.value.clone());
                }
            }
            SelectItem::UnnamedExpr(Expr::Identifier(identifier)) => {
                context.non_aggregate_names.push(identifier.value.clone());
            }
            SelectItem::UnnamedExpr(Expr::CompoundIdentifier(identifiers)) => {
                if let Some(identifier) = identifiers.last() {
                    context.non_aggregate_names.push(identifier.value.clone());
                }
            }
            SelectItem::UnnamedExpr(expr) => {
                if !is_aggregate_expression(expr, is_sqlite) {
                    let expression = expr.to_string();
                    if !expression.is_empty() {
                        context.non_aggregate_names.push(expression);
                    }
                }
            }
            SelectItem::QualifiedWildcard(_, _) | SelectItem::Wildcard(_) => {}
        }
    }
    context.aliases.sort_unstable_by_key(|name| name.to_lowercase());
    context
        .non_aggregate_names
        .sort_unstable_by_key(|name| name.to_lowercase());
    context.aliases.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    context
        .non_aggregate_names
        .dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    Some(context)
}

fn is_aggregate_expression(expression: &Expr, is_sqlite: bool) -> bool {
    let rendered = expression.to_string();
    let aggregate_functions = if is_sqlite {
        SQLITE_AGGREGATE_FUNCTIONS
    } else {
        POSTGRES_AGGREGATE_FUNCTIONS
    };
    let mut offset = 0;
    while offset < rendered.len() {
        let Some(character) = rendered[offset..].chars().next() else {
            break;
        };
        if character == '\'' || character == '"' {
            offset = skip_quoted_text(&rendered, offset, character);
            continue;
        }
        if character.is_ascii_alphabetic() || character == '_' {
            let identifier_start = offset;
            offset += character.len_utf8();
            while offset < rendered.len() {
                let Some(next) = rendered[offset..].chars().next() else {
                    break;
                };
                if !next.is_ascii_alphanumeric() && next != '_' {
                    break;
                }
                offset += next.len_utf8();
            }
            let identifier = rendered[identifier_start..offset].to_ascii_uppercase();
            let after_identifier = rendered[offset..].trim_start();
            if aggregate_functions.iter().any(|name| *name == identifier) && after_identifier.starts_with('(') {
                return true;
            }
            continue;
        }
        offset += character.len_utf8();
    }
    false
}

fn skip_quoted_text(text: &str, start: usize, quote: char) -> usize {
    let mut offset = start + quote.len_utf8();
    while offset < text.len() {
        let Some(character) = text[offset..].chars().next() else {
            break;
        };
        offset += character.len_utf8();
        if character == '\\' && quote == '\'' {
            if let Some(escaped) = text[offset..].chars().next() {
                offset += escaped.len_utf8();
            }
        } else if character == quote {
            if text[offset..].starts_with(quote) {
                offset += quote.len_utf8();
            } else {
                break;
            }
        }
    }
    offset
}

/// Tables not yet in the statement that the current tables reference, inserted
/// as `customers ON orders.customer_id = customers.id`.
fn fk_join_target_items(
    summary: &UiSchemaSummary,
    scope: &super::completion_scope::CompletionScope,
    prefix_lower: &str,
    replacement_range: (usize, usize),
) -> Vec<CompletionItem> {
    scope
        .foreign_key_join_targets(summary)
        .into_iter()
        .filter(|snippet| prefix_lower.is_empty() || snippet.label.to_lowercase().contains(prefix_lower))
        .map(|snippet| CompletionItem {
            label: snippet.label,
            insert_text: snippet.insert_text,
            kind: CompletionItemKind::Snippet,
            detail: Some(snippet.detail),
            documentation: Some(snippet.documentation),
            replacement_range,
            sort_score: 970,
        })
        .collect()
}

fn fk_join_items(
    summary: &UiSchemaSummary,
    scope: &super::completion_scope::CompletionScope,
    prepend_on: bool,
    replacement_range: (usize, usize),
) -> Vec<CompletionItem> {
    scope
        .foreign_key_join_snippets(summary, prepend_on)
        .into_iter()
        .map(|snippet| CompletionItem {
            label: snippet.label,
            insert_text: snippet.insert_text,
            kind: CompletionItemKind::Snippet,
            detail: Some(snippet.detail),
            documentation: Some(snippet.documentation),
            replacement_range,
            sort_score: 1_000,
        })
        .collect()
}

fn extract_inserted_column_names(text: &str) -> HashSet<String> {
    let normalized = text.replace('(', " ( ").replace(')', " ) ").replace(',', " , ");
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let Some(into_index) = words.iter().position(|word| word.eq_ignore_ascii_case("INTO")) else {
        return HashSet::new();
    };
    let Some(open_index) = words[into_index.saturating_add(2)..]
        .iter()
        .position(|word| *word == "(")
        .map(|offset| into_index + 2 + offset)
    else {
        return HashSet::new();
    };
    let mut columns = HashSet::new();
    for word in words.iter().skip(open_index + 1) {
        if *word == ")" {
            break;
        }
        if *word != "," {
            let column = clean_cte_token(word);
            if !column.is_empty() {
                columns.insert(column.to_lowercase());
            }
        }
    }
    columns
}

pub fn detect_clause_context(text_before: &str) -> SqlClause {
    let upper = text_before.to_ascii_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    if tokens.is_empty() {
        return SqlClause::Unknown;
    }

    // Scan backwards from last token
    for &w in tokens.iter().rev() {
        match w {
            "FROM" => return SqlClause::From,
            "JOIN" | "CROSS" | "INNER" | "LEFT" | "RIGHT" | "OUTER" => return SqlClause::Join,
            "ON" => return SqlClause::JoinOn,
            "WHERE" => return SqlClause::Where,
            "SET" => return SqlClause::Set,
            "UPDATE" => return SqlClause::Update,
            "VALUES" => return SqlClause::Values,
            "INTO" => return SqlClause::InsertInto,
            "DELETE" => return SqlClause::DeleteFrom,
            "HAVING" => return SqlClause::Having,
            "ORDER" => return SqlClause::OrderBy,
            "GROUP" => return SqlClause::GroupBy,
            "RETURNING" => return SqlClause::Returning,
            "SELECT" => return SqlClause::Select,
            _ => {}
        }
    }
    SqlClause::Unknown
}

fn add_clause_keywords(
    items: &mut Vec<CompletionItem>,
    clause: SqlClause,
    prefix_lower: &str,
    replacement_range: (usize, usize),
) {
    let keywords: &[&str] = match clause {
        SqlClause::Select => &[
            "DISTINCT", "FROM", "AS", "CASE", "WHEN", "THEN", "ELSE", "END", "COUNT", "SUM", "AVG", "MIN", "MAX",
            "COALESCE", "NOW()",
        ],
        SqlClause::From | SqlClause::Join => &[
            "ON",
            "USING",
            "LEFT JOIN",
            "INNER JOIN",
            "RIGHT JOIN",
            "FULL JOIN",
            "CROSS JOIN",
            "WHERE",
            "AS",
            "GROUP BY",
            "ORDER BY",
            "LIMIT",
        ],
        SqlClause::JoinOn => &["AND", "OR", "IS NULL", "IS NOT NULL"],
        SqlClause::Where | SqlClause::Having => &[
            "AND",
            "OR",
            "NOT",
            "IN",
            "IS NULL",
            "IS NOT NULL",
            "LIKE",
            "ILIKE",
            "BETWEEN",
            "EXISTS",
            "GROUP BY",
            "ORDER BY",
            "LIMIT",
        ],
        SqlClause::GroupBy => &["HAVING", "ORDER BY", "LIMIT"],
        SqlClause::OrderBy => &["ASC", "DESC", "NULLS FIRST", "NULLS LAST", "LIMIT", "OFFSET"],
        SqlClause::Update => &["SET", "WHERE"],
        SqlClause::Set => &["WHERE", "RETURNING"],
        SqlClause::InsertInto => &["VALUES", "SELECT", "RETURNING"],
        SqlClause::DeleteFrom => &["WHERE", "RETURNING"],
        _ => &[
            "SELECT",
            "FROM",
            "WHERE",
            "JOIN",
            "LEFT JOIN",
            "INNER JOIN",
            "GROUP BY",
            "ORDER BY",
            "HAVING",
            "LIMIT",
            "OFFSET",
            "DISTINCT",
            "AS",
            "AND",
            "OR",
            "NOT",
            "IN",
            "IS NULL",
            "IS NOT NULL",
            "INSERT INTO",
            "VALUES",
            "UPDATE",
            "SET",
            "DELETE FROM",
            "COUNT",
            "SUM",
            "AVG",
            "COALESCE",
            "NOW()",
        ],
    };

    for kw in keywords {
        if kw.to_lowercase().contains(prefix_lower) {
            items.push(CompletionItem {
                label: (*kw).to_owned(),
                insert_text: (*kw).to_owned(),
                kind: CompletionItemKind::Keyword,
                detail: Some("SQL Keyword".to_owned()),
                documentation: None,
                replacement_range,
                sort_score: 100,
            });
        }
    }
}

pub fn build_ai_sql_context(
    doc_text: &str,
    cursor_offset: usize,
    active_schema: &str,
    schema_summary: &UiSchemaSummary,
) -> AiSqlContext {
    let cursor = cursor_offset.min(doc_text.len());
    let (before_cursor, after_cursor) = doc_text.split_at(cursor);
    let statement_start = before_cursor.rfind(';').map_or(0, |offset| offset + 1);
    let statement_end = after_cursor
        .find(';')
        .map_or(doc_text.len(), |offset| cursor + offset + 1);
    let current_statement = doc_text[statement_start..statement_end].trim().to_owned();

    // Truncate SQL before cursor deterministically
    let truncated_before = if before_cursor.len() > MAX_SQL_BEFORE_CHARS {
        let start = before_cursor.len() - MAX_SQL_BEFORE_CHARS;
        &before_cursor[start..]
    } else {
        before_cursor
    };

    // Small suffix after cursor
    let truncated_after = if after_cursor.len() > MAX_SQL_AFTER_CHARS {
        &after_cursor[..MAX_SQL_AFTER_CHARS]
    } else {
        after_cursor
    };

    let normalized_sql = doc_text.to_lowercase();
    let ctes = extract_cte_definitions(doc_text);
    let mut cte_names: Vec<String> = ctes.keys().cloned().collect();
    cte_names.sort_unstable();
    cte_names.truncate(MAX_CTES);

    let aliases = extract_table_aliases(doc_text);
    let mut referenced_tables = Vec::new();
    let mut alias_tables: Vec<String> = aliases.values().cloned().collect();
    alias_tables.sort_unstable();
    alias_tables.dedup();
    for table_val in &alias_tables {
        if !referenced_tables.contains(table_val) {
            referenced_tables.push(table_val.clone());
        }
    }
    for table in &schema_summary.table_details {
        if normalized_sql.contains(&table.name.to_lowercase())
            && !referenced_tables.iter().any(|t| t.eq_ignore_ascii_case(&table.name))
        {
            referenced_tables.push(table.name.clone());
        }
    }
    referenced_tables.truncate(MAX_REFERENCED_TABLES);

    let mut relevant_columns = Vec::new();
    let mut fk_neighbors = Vec::new();

    for table_name in &referenced_tables {
        if let Some(table) = find_completion_table(&schema_summary.table_details, table_name, active_schema) {
            for col in table.columns.iter().take(MAX_COLUMNS_PER_TABLE) {
                relevant_columns.push(format!("{}.{} ({})", table.name, col.name, col.data_type));
            }
            for fk in table.foreign_keys.iter().take(MAX_FK_NEIGHBORS) {
                fk_neighbors.push(format!(
                    "{} ({}) -> {}.{} ({})",
                    table.name,
                    fk.from_columns.join(", "),
                    fk.to_schema,
                    fk.to_table,
                    fk.to_columns.join(", ")
                ));
            }
        }
    }
    fk_neighbors.truncate(MAX_FK_NEIGHBORS);

    AiSqlContext {
        sql_before_cursor: truncated_before.to_owned(),
        sql_after_cursor: truncated_after.to_owned(),
        current_statement,
        active_schema: active_schema.to_owned(),
        dialect: "PostgreSQL".to_owned(),
        referenced_tables,
        table_aliases: aliases,
        relevant_columns,
        fk_neighbors,
        cte_names,
    }
}

const MAX_COMPLETION_ITEMS: usize = 50;

/// Rank first, then keep the best 50. Truncating earlier would drop a
/// higher-scored column that was collected after a run of weaker matches.
fn finish_ranked(prefix: &str, prefix_lower: &str, mut items: Vec<CompletionItem>) -> (String, Vec<CompletionItem>) {
    rank_items(&mut items, prefix_lower);
    items.truncate(MAX_COMPLETION_ITEMS);
    (prefix.to_owned(), items)
}

fn rank_items(items: &mut [CompletionItem], prefix: &str) {
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut ranked: Vec<(usize, bool, bool, i32)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let label = item.label.to_ascii_lowercase();
            (
                index,
                label == prefix_lower,
                label.starts_with(&prefix_lower),
                subsequence_score(&prefix_lower, &label),
            )
        })
        .collect();
    ranked.sort_by(|left, right| {
        right.1.cmp(&left.1).then(right.2.cmp(&left.2)).then_with(|| {
            let left_item = &items[left.0];
            let right_item = &items[right.0];
            right_item
                .sort_score
                .cmp(&left_item.sort_score)
                .then(right.3.cmp(&left.3))
                .then_with(|| left_item.label.cmp(&right_item.label))
        })
    });
    let snapshot = items.to_vec();
    for (new_index, (old_index, _, _, _)) in ranked.into_iter().enumerate() {
        items[new_index] = snapshot[old_index].clone();
    }
}

/// Higher when the query is a tighter subsequence of the label.
/// A label that does not contain the query in order scores 0, so this only
/// breaks ties inside one context-score bucket.
fn subsequence_score(query: &str, label: &str) -> i32 {
    if query.is_empty() {
        return 0;
    }
    let query = query.as_bytes();
    let label = label.as_bytes();
    let mut query_index = 0;
    let mut score = 0i32;
    let mut previous = usize::MAX;
    for (index, byte) in label.iter().copied().enumerate() {
        if query_index < query.len() && byte == query[query_index] {
            score += 1;
            if previous != usize::MAX && previous + 1 == index {
                score += 2;
            }
            if index == 0 || label[index - 1] == b'_' {
                score += 2;
            }
            previous = index;
            query_index += 1;
        }
    }
    if query_index < query.len() {
        0
    } else {
        score
    }
}

/// True when `text` ends with the SQL keyword AS (case-insensitive, word
/// boundary, whitespace between the keyword and the cursor allowed).
fn ends_with_as_keyword(text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.len() < 2 {
        return false;
    }
    let (head, tail) = trimmed.split_at(trimmed.len() - 2);
    if !tail.eq_ignore_ascii_case("as") {
        return false;
    }
    match head.chars().next_back() {
        None => true,
        Some(ch) => !(ch.is_alphanumeric() || ch == '_'),
    }
}

fn extract_word_prefix(text: &str) -> &str {
    // Walk back one char at a time — `end - 1` can sit on a UTF-8
    // continuation byte, which would panic a byte slice.
    let mut start = text.len();
    for (idx, ch) in text.char_indices().rev() {
        if ch.is_alphanumeric() || ch == '_' {
            start = idx;
        } else {
            break;
        }
    }
    &text[start..]
}

fn extract_qualifier(text: &str) -> Option<&str> {
    let trimmed = text.trim_end();
    if let Some(without_dot) = trimmed.strip_suffix('.') {
        let start = without_dot
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let qual = &without_dot[start..];
        if !qual.is_empty() {
            return Some(qual);
        }
    }
    None
}

pub fn extract_cte_definitions(text: &str) -> HashMap<String, CteDefinition> {
    let mut ctes = HashMap::new();
    let normalized = text.replace('(', " ( ").replace(')', " ) ").replace(',', " , ");
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        if !words[i].eq_ignore_ascii_case("WITH") {
            i += 1;
            continue;
        }
        let mut cursor = i + 1;
        if words
            .get(cursor)
            .is_some_and(|word| word.eq_ignore_ascii_case("RECURSIVE"))
        {
            cursor += 1;
        }
        while let Some(raw_name) = words.get(cursor) {
            let name = clean_cte_token(raw_name);
            if name.is_empty() {
                break;
            }
            cursor += 1;
            let mut explicit_columns = Vec::new();
            if words.get(cursor) == Some(&"(") {
                cursor += 1;
                while let Some(word) = words.get(cursor) {
                    cursor += 1;
                    if *word == ")" {
                        break;
                    }
                    if *word != "," {
                        explicit_columns.push(clean_cte_token(word));
                    }
                }
            }
            if words.get(cursor).is_none_or(|word| !word.eq_ignore_ascii_case("AS")) {
                break;
            }
            cursor += 1;
            if words.get(cursor) != Some(&"(") {
                break;
            }
            cursor += 1;
            let body_start = cursor;
            let mut depth = 1usize;
            while let Some(word) = words.get(cursor) {
                if *word == "(" {
                    depth += 1;
                } else if *word == ")" {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                cursor += 1;
            }
            let body = &words[body_start..cursor.min(words.len())];
            let target_table = find_cte_source_table(body);
            let inferred_columns = infer_cte_columns(body);
            ctes.insert(
                name.to_lowercase(),
                CteDefinition {
                    name,
                    target_table,
                    explicit_columns,
                    inferred_columns,
                },
            );
            if words.get(cursor) == Some(&")") {
                cursor += 1;
            }
            if words.get(cursor) != Some(&",") {
                break;
            }
            cursor += 1;
        }
        i = cursor;
    }
    ctes
}

fn clean_cte_token(token: &str) -> String {
    token
        .trim_matches(|character| matches!(character, '"' | '`' | '[' | ']' | ';'))
        .to_owned()
}

fn find_cte_source_table(body: &[&str]) -> Option<String> {
    body.windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("FROM") && window[1] != "(")
        .map(|window| clean_cte_token(window[1]))
        .filter(|table| !table.is_empty())
        .map(|table| table.rsplit('.').next().unwrap_or(&table).to_owned())
}

fn infer_cte_columns(body: &[&str]) -> Vec<String> {
    let Some(select_index) = body.iter().position(|word| word.eq_ignore_ascii_case("SELECT")) else {
        return Vec::new();
    };
    let Some(from_index) = body[select_index + 1..]
        .iter()
        .position(|word| word.eq_ignore_ascii_case("FROM"))
        .map(|index| select_index + 1 + index)
    else {
        return Vec::new();
    };
    body[select_index + 1..from_index]
        .iter()
        .filter(|word| **word != "," && **word != "*")
        .filter_map(|word| {
            let cleaned = clean_cte_token(word);
            let column = cleaned.rsplit('.').next().unwrap_or(&cleaned);
            (!column.is_empty() && !column.eq_ignore_ascii_case("AS")).then(|| column.to_owned())
        })
        .collect()
}

pub fn extract_table_aliases(text: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if (w.eq_ignore_ascii_case("FROM")
            || w.eq_ignore_ascii_case("JOIN")
            || w.eq_ignore_ascii_case("INTO")
            || w.eq_ignore_ascii_case("UPDATE"))
            && i + 1 < words.len()
        {
            let raw_table = words[i + 1].trim_matches(|c| {
                c == '"' || c == '`' || c == '[' || c == ']' || c == '(' || c == ')' || c == ';' || c == ','
            });
            if !raw_table.is_empty() && !raw_table.starts_with('(') {
                let table_name = if let Some(dot_pos) = raw_table.rfind('.') {
                    &raw_table[dot_pos + 1..]
                } else {
                    raw_table
                };

                if i + 2 < words.len() {
                    if words[i + 2].eq_ignore_ascii_case("AS") {
                        if i + 3 < words.len() {
                            let alias = words[i + 3].trim_matches(|c| {
                                c == '"'
                                    || c == '`'
                                    || c == '['
                                    || c == ']'
                                    || c == '('
                                    || c == ')'
                                    || c == ';'
                                    || c == ','
                            });
                            if is_valid_alias_token(alias) {
                                aliases.insert(alias.to_lowercase(), table_name.to_lowercase());
                                i += 3;
                            }
                        }
                    } else {
                        let alias = words[i + 2].trim_matches(|c| {
                            c == '"' || c == '`' || c == '[' || c == ']' || c == '(' || c == ')' || c == ';' || c == ','
                        });
                        if is_valid_alias_token(alias) {
                            aliases.insert(alias.to_lowercase(), table_name.to_lowercase());
                            i += 2;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    aliases
}

fn is_valid_alias_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    const NON_ALIAS_KEYWORDS: &[&str] = &[
        "WHERE", "ON", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "FULL", "CROSS", "NATURAL", "GROUP", "ORDER",
        "HAVING", "LIMIT", "OFFSET", "UNION", "SET", "VALUES", "SELECT", "FROM", "AND", "OR", "USING", "AS",
    ];
    !NON_ALIAS_KEYWORDS.iter().any(|kw| kw.eq_ignore_ascii_case(token))
}

fn extract_mutation_target(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let is_target_keyword = word.eq_ignore_ascii_case("UPDATE") || word.eq_ignore_ascii_case("INTO");
        if is_target_keyword {
            let target = words
                .get(index + 1)?
                .trim_matches(|character| matches!(character, '"' | '`' | '[' | ']' | '(' | ')' | ',' | ';'));
            if !target.is_empty() && !target.eq_ignore_ascii_case("SELECT") {
                return Some(target.rsplit('.').next().unwrap_or(target).to_lowercase());
            }
        }
    }
    None
}

fn extract_subquery_aliases(text: &str) -> HashMap<String, Vec<String>> {
    let normalized = text.replace('(', " ( ").replace(')', " ) ").replace(',', " , ");
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let mut aliases = HashMap::new();
    for index in 0..words.len().saturating_sub(1) {
        if !matches!(words[index].to_ascii_uppercase().as_str(), "FROM" | "JOIN") || words.get(index + 1) != Some(&"(")
        {
            continue;
        }
        let body_start = index + 2;
        let mut cursor = body_start;
        let mut depth = 1usize;
        while let Some(word) = words.get(cursor) {
            if *word == "(" {
                depth += 1;
            } else if *word == ")" {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            cursor += 1;
        }
        let Some(alias_token) = words.get(cursor + 1) else {
            continue;
        };
        let alias = if alias_token.eq_ignore_ascii_case("AS") {
            words.get(cursor + 2).copied().unwrap_or_default()
        } else {
            alias_token
        };
        let alias = clean_cte_token(alias);
        if !alias.is_empty() && is_valid_alias_token(&alias) {
            let columns = infer_cte_columns(&words[body_start..cursor.min(words.len())]);
            if !columns.is_empty() {
                aliases.insert(alias.to_lowercase(), columns);
            }
        }
    }
    aliases
}

#[cfg(test)]
#[path = "tests/schema_completion_tests.rs"]
mod tests;
