//! Tests for `../schema_completion.rs` (moved out of the source file).
use super::*;
use crate::runtime::{UiSchemaColumn, UiTableSummary};

#[test]
fn test_extract_table_aliases() {
    let sql = "SELECT u.id, u.name FROM users AS u JOIN orders o ON u.id = o.user_id";
    let aliases = extract_table_aliases(sql);
    assert_eq!(aliases.get("u").map(|s| s.as_str()), Some("users"));
    assert_eq!(aliases.get("o").map(|s| s.as_str()), Some("orders"));
}

#[test]
fn test_extract_word_prefix_multibyte_tail_does_not_panic() {
    // Regression: byte-decrement slicing panicked when the cursor sat
    // directly behind a multi-byte char (continuation byte slice).
    assert_eq!(extract_word_prefix("SELECT tên"), "tên");
    assert_eq!(extract_word_prefix("SELECT 🚀"), "");
    assert_eq!(extract_word_prefix("SELECT tên 🚀"), "");
    assert_eq!(extract_word_prefix("SELECT * FROM bảng"), "bảng");
    assert_eq!(extract_word_prefix(""), "");
    assert_eq!(extract_word_prefix("SELECT "), "");
    assert_eq!(extract_word_prefix("SELECT * FROM us"), "us");
}

#[test]
fn test_completion_resolves_alias_column() {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate
            name: "users".to_owned(),
            row_count: Some(10),
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "email".to_owned(),
                    data_type: "varchar".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };

    let ctx = CompletionContext {
        text_before_cursor: "SELECT u.em",
        text_after_cursor: " FROM users u WHERE ",
        cursor_offset: 11,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "em");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "email");
    assert_eq!(items[0].kind, CompletionItemKind::Column);
    assert_eq!(items[0].replacement_range, (9, 11)); // "em" replaced
}

#[test]
fn test_completion_suppression_inside_comment() {
    let summary = UiSchemaSummary::default();
    let mut tokens = CachedSqlTokens::new();
    let buf = crate::editor::buffer::TextBuffer::from_string("-- SELECT * FROM users\nSELECT 1;");
    tokens.get_or_recompute(&buf, crate::editor::syntax::SqlDialect::Postgres);

    let ctx = CompletionContext {
        text_before_cursor: "-- SELECT * FROM us",
        text_after_cursor: "\nSELECT 1;",
        cursor_offset: 19,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: Some(&tokens),
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert!(items.is_empty(), "Automatic completion must be suppressed in comments");
    assert_eq!(prefix, "");
}

#[test]
fn test_from_clause_suggests_tables_first() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: Some(10),
            columns: vec![],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };

    let ctx = CompletionContext {
        text_before_cursor: "SELECT * FROM us",
        text_after_cursor: " WHERE id = 1",
        cursor_offset: 16,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "us");
    assert!(!items.is_empty());
    assert_eq!(items[0].label, "users");
    assert_eq!(items[0].kind, CompletionItemKind::Table);
    assert_eq!(items[0].replacement_range, (14, 16));
    assert!(items.iter().any(|i| i.label == "USING"));
}

#[test]
fn test_schema_qualified_table_completion() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "orders".to_owned(),
            row_count: Some(10),
            columns: vec![],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };

    let ctx = CompletionContext {
        text_before_cursor: "SELECT * FROM public.ord",
        text_after_cursor: ";",
        cursor_offset: 24,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "ord");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "orders");
    assert_eq!(items[0].kind, CompletionItemKind::Table);
    assert_eq!(items[0].replacement_range, (21, 24));
}

#[test]
fn explicit_cte_columns_are_parsed_across_whitespace() {
    let ctes = extract_cte_definitions(
        "WITH visible (id, display_name) AS (SELECT id, name FROM users) SELECT * FROM visible",
    );
    let cte = ctes.get("visible").expect("CTE should be indexed");
    assert_eq!(cte.explicit_columns, ["id", "display_name"]);
    assert_eq!(cte.target_table.as_deref(), Some("users"));
}

#[test]
fn simple_cte_select_list_is_available_for_qualified_completion() {
    let summary = UiSchemaSummary::default();
    let sql = "WITH x AS (SELECT id, email FROM users) SELECT x.";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: "",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(labels.contains(&"id"));
    assert!(labels.contains(&"email"));
}

#[test]
fn update_set_completion_prioritizes_update_target_columns() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: None,
            columns: vec![UiSchemaColumn {
                name: "email".to_owned(),
                // cc-scan:allow DUPLICATE_BLOCK — coincidental boilerplate, not a real clone
                data_type: "text".to_owned(),
                nullable: true,
                is_primary_key: false,
            }],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };
    let sql = "UPDATE users SET em";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: "",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("email"));
}

#[test]
fn insert_completion_excludes_columns_already_entered() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: None,
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "email".to_owned(),
                    data_type: "text".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };
    let sql = "INSERT INTO users (id, em";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: ") VALUES (1, 'a@example.com')",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("email"));
    assert!(!items.iter().any(|item| item.label == "id"));
}

#[test]
fn order_by_prioritizes_select_aliases_from_ast() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: None,
            columns: vec![UiSchemaColumn {
                name: "name".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
                is_primary_key: false,
            }],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };
    let sql = "SELECT name AS display_name, count(*) AS total FROM users ORDER BY ";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: "",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("display_name"));
    assert!(items.iter().any(|item| item.label == "total"));
}

#[test]
fn group_by_prioritizes_non_aggregate_projection_from_ast() {
    let summary = UiSchemaSummary::default();
    let sql = "SELECT status, count(*) AS total FROM users GROUP BY ";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: "",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("status"));
    assert!(!items.iter().any(|item| item.label == "total"));
}

#[test]
fn group_by_keeps_scalar_functions_and_excludes_nested_aggregates() {
    let parsed = parse_select_context(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "SELECT LOWER(name) AS normalized, DATE(created_at) AS day, ARRAY_AGG(id) AS ids, COALESCE(SUM(total), 0) AS total_sum, first_name || last_name AS full_name FROM users GROUP BY ",
            "",
            false,
        )
        .expect("valid select context");

    assert!(parsed.non_aggregate_names.iter().any(|name| name == "normalized"));
    assert!(parsed.non_aggregate_names.iter().any(|name| name == "day"));
    assert!(parsed.non_aggregate_names.iter().any(|name| name == "full_name"));
    assert!(!parsed.non_aggregate_names.iter().any(|name| name == "ids"));
    assert!(!parsed.non_aggregate_names.iter().any(|name| name == "total_sum"));

    let sqlite = parse_select_context(
        "SELECT GROUP_CONCAT(name) AS names, TOTAL(amount) AS total, DATE(created_at) AS day FROM users GROUP BY ",
        "",
        true,
    )
    .expect("valid SQLite select context");
    assert!(!sqlite.non_aggregate_names.iter().any(|name| name == "names"));
    assert!(!sqlite.non_aggregate_names.iter().any(|name| name == "total"));
    assert!(sqlite.non_aggregate_names.iter().any(|name| name == "day"));
}

#[test]
fn join_on_prioritizes_matching_foreign_key_condition() {
    let summary = UiSchemaSummary {
        table_details: vec![
            UiTableSummary {
                schema: "public".to_owned(),
                name: "users".to_owned(),
                row_count: None,
                columns: vec![UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                }],
                foreign_keys: vec![],
            },
            UiTableSummary {
                schema: "public".to_owned(),
                name: "orders".to_owned(),
                row_count: None,
                columns: vec![UiSchemaColumn {
                    name: "user_id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    is_primary_key: false,
                }],
                foreign_keys: vec![crate::runtime::UiSchemaForeignKey {
                    name: "orders_user_id_fkey".to_owned(),
                    from_columns: vec!["user_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: "users".to_owned(),
                    to_columns: vec!["id".to_owned()],
                }],
            },
        ],
        ..Default::default()
    };
    let sql = "SELECT * FROM users u JOIN orders o ON ";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: "",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("o.user_id = u.id"));
    assert_eq!(items.first().map(|item| item.kind), Some(CompletionItemKind::Snippet));
}

#[test]
fn subquery_alias_exposes_simple_select_columns() {
    let summary = UiSchemaSummary::default();
    let sql = "SELECT u.em";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: " FROM (SELECT id, email FROM users) u WHERE ",
        cursor_offset: 11,
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: true,
    };
    let (_, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("email"));
}

#[test]
fn test_cte_alias_resolution_and_column_completion() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "public".to_owned(),
            name: "users".to_owned(),
            row_count: Some(10),
            columns: vec![
                UiSchemaColumn {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "email".to_owned(),
                    data_type: "text".to_owned(),
                    nullable: true,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };

    let sql = "WITH active_users AS (SELECT * FROM users) SELECT active_users.em";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: " FROM active_users",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "em");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "email");
    assert_eq!(items[0].kind, CompletionItemKind::Column);
    assert_eq!(items[0].detail, Some("Column · text".to_owned()));
    assert_eq!(items[0].replacement_range, (63, 65));
}

#[test]
fn test_multi_segment_schema_table_column_completion() {
    let summary = UiSchemaSummary {
        table_details: vec![UiTableSummary {
            schema: "analytics".to_owned(),
            name: "events".to_owned(),
            row_count: Some(500),
            columns: vec![
                UiSchemaColumn {
                    name: "event_id".to_owned(),
                    data_type: "uuid".to_owned(),
                    nullable: false,
                    is_primary_key: true,
                },
                UiSchemaColumn {
                    name: "payload".to_owned(),
                    data_type: "jsonb".to_owned(),
                    nullable: true,
                    is_primary_key: false,
                },
            ],
            foreign_keys: vec![],
        }],
        ..Default::default()
    };

    let sql = "SELECT analytics.events.pay";
    let ctx = CompletionContext {
        text_before_cursor: sql,
        text_after_cursor: " FROM analytics.events",
        cursor_offset: sql.len(),
        active_schema: "public",
        schema_summary: &summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: false,
    };

    let (prefix, items) = SchemaCompletionProvider::provide(&ctx);
    assert_eq!(prefix, "pay");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "payload");
    assert_eq!(items[0].kind, CompletionItemKind::Column);
    assert_eq!(items[0].replacement_range, (24, 27));
}

#[test]
fn test_build_ai_sql_context_extracts_references() {
    let summary = UiSchemaSummary {
        table_details: vec![
            UiTableSummary {
                schema: "public".to_owned(),
                name: "users".to_owned(),
                row_count: Some(10),
                columns: vec![
                    UiSchemaColumn {
                        name: "id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    UiSchemaColumn {
                        name: "name".to_owned(),
                        data_type: "text".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: vec![],
            },
            UiTableSummary {
                schema: "public".to_owned(),
                name: "orders".to_owned(),
                row_count: Some(20),
                columns: vec![
                    UiSchemaColumn {
                        name: "order_id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    UiSchemaColumn {
                        name: "user_id".to_owned(),
                        data_type: "integer".to_owned(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                foreign_keys: vec![crate::runtime::UiSchemaForeignKey {
                    name: "fk_orders_users".to_owned(),
                    from_columns: vec!["user_id".to_owned()],
                    to_schema: "public".to_owned(),
                    to_table: "users".to_owned(),
                    to_columns: vec!["id".to_owned()],
                }],
            },
        ],
        ..Default::default()
    };

    let sql = "SELECT u.name, o.order_id FROM users u JOIN orders o ON u.id = o.user_id WHERE u.";
    let ai_ctx = build_ai_sql_context(sql, sql.len(), "public", &summary);

    assert_eq!(ai_ctx.active_schema, "public");
    assert!(ai_ctx.referenced_tables.contains(&"users".to_owned()));
    assert!(ai_ctx.referenced_tables.contains(&"orders".to_owned()));
    assert!(ai_ctx.relevant_columns.iter().any(|c| c.starts_with("users.name")));
    assert!(ai_ctx.relevant_columns.iter().any(|c| c.starts_with("orders.order_id")));
    assert_eq!(ai_ctx.fk_neighbors.len(), 1);
    assert!(ai_ctx.fk_neighbors[0].contains("orders (user_id) -> public.users (id)"));
}

#[test]
fn ai_context_contains_only_the_current_statement() {
    let sql = "SELECT 1; SELECT u.id FROM users u WHERE u.";
    let ai_ctx = build_ai_sql_context(sql, sql.len(), "public", &UiSchemaSummary::default());
    assert_eq!(ai_ctx.current_statement, "SELECT u.id FROM users u WHERE u.");
    assert_eq!(ai_ctx.sql_before_cursor, sql);
}

fn text_column(name: &str) -> UiSchemaColumn {
    UiSchemaColumn {
        name: name.to_owned(),
        data_type: "text".to_owned(),
        nullable: true,
        is_primary_key: false,
    }
}

fn bare_table(name: &str, columns: Vec<UiSchemaColumn>) -> UiTableSummary {
    UiTableSummary {
        schema: "public".to_owned(),
        name: name.to_owned(),
        row_count: None,
        columns,
        foreign_keys: vec![],
    }
}

fn provide_at(sql: &str, summary: &UiSchemaSummary, manual: bool) -> (String, Vec<CompletionItem>) {
    provide_at_schema(sql, summary, manual, "public")
}

fn provide_at_schema(
    sql: &str,
    summary: &UiSchemaSummary,
    manual: bool,
    active_schema: &str,
) -> (String, Vec<CompletionItem>) {
    provide_split_schema(&sql[..sql.len()], "", summary, manual, active_schema)
}

fn provide_split_schema(
    before: &str,
    after: &str,
    summary: &UiSchemaSummary,
    manual: bool,
    active_schema: &str,
) -> (String, Vec<CompletionItem>) {
    let ctx = CompletionContext {
        text_before_cursor: before,
        text_after_cursor: after,
        cursor_offset: before.len(),
        active_schema,
        schema_summary: summary,
        cached_tokens: None,
        is_sqlite: false,
        is_manual_trigger: manual,
    };
    SchemaCompletionProvider::provide(&ctx)
}

fn duplicate_users_summary() -> UiSchemaSummary {
    // cc-scan:allow DUPLICATE_BLOCK — coincidental fixture boilerplate
    let tenant_a = UiTableSummary {
        schema: "tenant_a".to_owned(),
        name: "users".to_owned(),
        row_count: None,
        columns: vec![text_column("id"), text_column("tenant_a_only")],
        foreign_keys: vec![],
    };
    let tenant_b = UiTableSummary {
        schema: "tenant_b".to_owned(),
        name: "users".to_owned(),
        row_count: None,
        columns: vec![text_column("id"), text_column("tenant_b_only")],
        foreign_keys: vec![],
    };
    UiSchemaSummary {
        table_details: vec![tenant_a, tenant_b],
        ..Default::default()
    }
}

#[test]
fn completion_prefers_active_schema_for_duplicate_table_511() {
    let summary = duplicate_users_summary();
    let (_, items) = provide_split_schema("SELECT ", " FROM users", &summary, true, "tenant_b");
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"tenant_b_only"),
        "must suggest active-schema columns, got {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("tenant_a_only")),
        "must not suggest other-schema columns, got {labels:?}"
    );
}

#[test]
fn completion_falls_back_when_active_schema_has_no_match_511() {
    let summary = duplicate_users_summary();
    let (_, items) = provide_split_schema("SELECT ", " FROM users", &summary, true, "tenant_c");
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"tenant_a_only"),
        "fallback keeps first catalog match, got {labels:?}"
    );
}

#[test]
fn ai_context_prefers_active_schema_for_duplicate_table_511() {
    let summary = duplicate_users_summary();
    let sql = "SELECT * FROM users";
    let ctx = build_ai_sql_context(sql, sql.len(), "tenant_b", &summary);
    assert!(
        ctx.relevant_columns.iter().any(|c| c.contains("tenant_b_only")),
        "got {:?}",
        ctx.relevant_columns
    );
    assert!(
        !ctx.relevant_columns.iter().any(|c| c.contains("tenant_a_only")),
        "got {:?}",
        ctx.relevant_columns
    );
}

#[test]
fn completion_manual_empty_prefix_does_not_return_the_catalog() {
    let mut tables = Vec::with_capacity(51);
    for index in 0..50 {
        let columns = (0..40).map(|col| text_column(&format!("c{index}_{col}"))).collect();
        tables.push(bare_table(&format!("tbl{index:03}"), columns));
    }
    tables.push(bare_table("users", vec![text_column("id"), text_column("email")]));
    tables.push(bare_table("zzz_other", vec![text_column("zzz_col")]));
    let summary = UiSchemaSummary {
        table_details: tables,
        ..Default::default()
    };

    let (_, bare_select) = provide_at("SELECT ", &summary, true);
    assert!(bare_select.len() <= 50, "returned {}", bare_select.len());
    assert!(bare_select.len() < 1000, "returned {}", bare_select.len());
    assert!(
        bare_select.iter().all(|item| item.kind == CompletionItemKind::Keyword),
        "blank SELECT should stay on clause keywords, got {:?}",
        bare_select.iter().map(|item| item.label.as_str()).collect::<Vec<_>>()
    );
    assert!(bare_select.iter().any(|item| item.label == "FROM"));

    let (_, where_items) = provide_at("SELECT * FROM users WHERE ", &summary, true);
    assert!(where_items.len() <= 50, "returned {}", where_items.len());
    assert!(where_items.iter().any(|item| item.label == "email"));
    assert!(where_items.iter().any(|item| item.kind == CompletionItemKind::Keyword));
    assert!(!where_items
        .iter()
        .any(|item| item.label == "zzz_other" || item.label == "zzz_col"));
    assert!(!where_items.iter().any(|item| item.label.starts_with("c0_")));
}

#[test]
fn completion_result_length_is_at_most_50() {
    let mut columns = vec![text_column("name")];
    for index in 0..60 {
        columns.push(text_column(&format!("c{index:03}")));
    }
    let wide = UiSchemaSummary {
        table_details: vec![bare_table("users", columns)],
        ..Default::default()
    };
    let sql = "SELECT name AS display_name FROM users ORDER BY ";
    let (_, items) = provide_at(sql, &wide, true);
    assert!(items.len() <= 50, "returned {}", items.len());
    assert_eq!(items.len(), 50);
    // display_name scores above the 60 columns collected before it, so the cap has to run after rank.
    assert_eq!(items.first().map(|item| item.label.as_str()), Some("display_name"));

    let mut tables = Vec::with_capacity(61);
    for index in 0..60 {
        tables.push(bare_table(&format!("m_{index:03}"), vec![]));
    }
    tables.push(bare_table("aaa_table", vec![]));
    let catalog = UiSchemaSummary {
        table_details: tables,
        ..Default::default()
    };
    let (_, from_items) = provide_at("SELECT * FROM ", &catalog, true);
    assert_eq!(from_items.len(), 50);
    assert!(from_items.iter().all(|item| item.kind == CompletionItemKind::Table));
    assert_eq!(from_items[0].label, "aaa_table");
}

#[test]
fn completion_is_suppressed_directly_after_as_alias_position() {
    let summary = duplicate_users_summary();
    // Cursor right after the `AS ` keyword — alias position must not suggest
    // columns/keywords (#607).
    let (prefix, items) = provide_at("select id as ", &summary, false);
    assert!(items.is_empty(), "alias position must not offer completions");
    assert_eq!(prefix, "");

    let (prefix, items) = provide_at("select 1 AS\n", &summary, false);
    assert!(items.is_empty(), "alias position after newline must not offer completions");
    assert_eq!(prefix, "");
}

#[test]
fn completion_still_offers_keywords_inside_expressions_without_as() {
    let summary = duplicate_users_summary();
    // Sanity: the AS suppression must not leak into unrelated contexts.
    let (_, items) = provide_at("select ", &summary, false);
    assert!(!items.is_empty(), "SELECT list itself should still resolve completions");
}

#[test]
fn manual_completion_after_as_still_opens() {
    let summary = duplicate_users_summary();
    // Ctrl+Space is an explicit ask — it must keep working even after AS.
    let (_, items) = provide_at("select id as ", &summary, true);
    assert!(!items.is_empty(), "manual trigger must override the AS suppression");
}
