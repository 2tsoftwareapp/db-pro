//! Tests for `../schema_diff.rs` (moved out of the source file).
use super::*;
use crate::domain::schema::{Column, Index, IndexOrigin, Table};

#[test]
fn qualified_name_preserves_dotted_schema_and_table_names() {
    let qualified = qualify_key("tenant.prod", "orders.archive");
    assert_eq!(qualified, ("tenant.prod".into(), "orders.archive".into()));
    assert_eq!(display_qualified_name(&qualified), "\"tenant.prod\".\"orders.archive\"");
}

#[test]
fn empty_schema_dotted_table_is_compared_by_its_literal_name() {
    let mut source = IntrospectResult::empty();
    source.tables.push(Table {
        name: "my.table".into(),
        schema: "".into(),
        row_count: None,
    });
    source.columns.push(Column {
        name: "id".into(),
        data_type: "INTEGER".into(),
        nullable: false,
        default: None,
        is_primary_key: true,
        table_name: "my.table".into(),
        schema: "".into(),
        ..Default::default()
    });

    let mut target = source.clone();
    target.columns[0].data_type = "TEXT".into();

    let diff = compare_introspect_results(&source, &target);
    assert_eq!(diff.column_diffs.len(), 1);
    assert_eq!(diff.column_diffs[0].schema, "");
    assert_eq!(diff.column_diffs[0].table, "my.table");
    assert_eq!(diff.column_diffs[0].type_mismatches[0].column, "id");
}

#[test]
fn dotted_schema_names_do_not_collide_with_dotted_table_names() {
    let mut source = IntrospectResult::empty();
    source.tables.push(Table {
        name: "orders".into(),
        schema: "tenant.prod".into(),
        row_count: None,
    });

    let mut target = IntrospectResult::empty();
    target.tables.push(Table {
        name: "prod.orders".into(),
        schema: "tenant".into(),
        row_count: None,
    });

    let diff = compare_introspect_results(&source, &target);
    assert_eq!(diff.tables_only_in_source, vec!["\"tenant.prod\".orders"]);
    assert_eq!(diff.tables_only_in_target, vec!["tenant.\"prod.orders\""]);
}

#[test]
fn schema_diff_orders_set_based_results_deterministically() {
    let mut source = IntrospectResult::empty();
    source.tables = vec![
        Table {
            name: "users".into(),
            schema: "public".into(),
            row_count: None,
        },
        Table {
            name: "accounts".into(),
            schema: "public".into(),
            row_count: None,
        },
    ];
    source.indexes = vec![
        Index {
            name: "users_z_idx".into(),
            columns: vec![],
            unique: false,
            origin: IndexOrigin::User,
            table_name: "users".into(),
            schema: "public".into(),
            ..Default::default()
        },
        Index {
            name: "users_a_idx".into(),
            columns: vec![],
            unique: false,
            origin: IndexOrigin::User,
            table_name: "users".into(),
            schema: "public".into(),
            ..Default::default()
        },
    ];

    let mut target = IntrospectResult::empty();
    target.tables.push(Table {
        name: "orders".into(),
        schema: "public".into(),
        row_count: None,
    });

    let diff = compare_introspect_results(&source, &target);
    assert_eq!(diff.tables_only_in_source, vec!["public.accounts", "public.users"]);
    assert_eq!(diff.tables_only_in_target, vec!["public.orders"]);
    assert_eq!(
        diff.indexes_only_in_source,
        vec!["public.users_a_idx", "public.users_z_idx"]
    );
}

#[test]
fn live_schema_compare_includes_indexes_foreign_keys_and_view_bodies() {
    let source_code = include_str!("../schema_diff.rs");
    let start = source_code
        .find("pub async fn diff_schemas")
        .expect("live compare entry");
    let body = &source_code[start..];
    let end = body.find("\n}\n").expect("end of diff_schemas");
    let body = &body[..end];
    assert!(body.contains("self.introspect(source_id"));
    assert!(body.contains("self.introspect(target_id"));
    assert!(body.contains("compare_introspect_results"));

    let mut source = IntrospectResult::empty();
    source.indexes.push(Index {
        name: "orders_customer_idx".into(),
        columns: vec!["customer_id".into()],
        unique: false,
        definition: "CREATE INDEX orders_customer_idx ON orders (customer_id)".into(),
        origin: IndexOrigin::User,
        table_name: "orders".into(),
        schema: "public".into(),
        ..Default::default()
    });
    source.foreign_keys.push(crate::domain::schema::ForeignKey {
        name: "orders_customer_fk".into(),
        from_table: "orders".into(),
        from_columns: vec!["customer_id".into()],
        to_table: "customers".into(),
        schema: "public".into(),
        to_schema: "public".into(),
        to_columns: vec!["id".into()],
        on_update: "NO ACTION".into(),
        on_delete: "CASCADE".into(),
        ..Default::default()
    });
    source.views.push(crate::domain::schema::View {
        name: "open_orders".into(),
        schema: "public".into(),
        definition: "SELECT id FROM orders WHERE open".into(),
        kind: Default::default(),
    });

    let mut target = IntrospectResult::empty();
    target.indexes.push(Index {
        name: "orders_customer_idx".into(),
        columns: vec!["customer_id".into(), "created_at".into()],
        unique: false,
        definition: "CREATE INDEX orders_customer_idx ON orders (customer_id, created_at)".into(),
        origin: IndexOrigin::User,
        table_name: "orders".into(),
        schema: "public".into(),
        ..Default::default()
    });
    target.views.push(crate::domain::schema::View {
        name: "open_orders".into(),
        schema: "public".into(),
        definition: "SELECT id, created_at FROM orders WHERE open".into(),
        kind: Default::default(),
    });

    let diff = compare_schemas(&source, &target);
    assert!(
        diff.index_definition_mismatches
            .iter()
            .any(|line| line.contains("orders_customer_idx") && line.contains("created_at")),
        "{:?}",
        diff.index_definition_mismatches
    );
    assert!(
        diff.foreign_keys_only_in_source
            .iter()
            .any(|line| line.contains("orders_customer_fk")
                && line.contains("customer_id")
                && line.contains("CASCADE")),
        "{:?}",
        diff.foreign_keys_only_in_source
    );
    assert!(diff.foreign_keys_only_in_target.is_empty());
    assert_eq!(diff.view_body_mismatches.len(), 1);
    assert_eq!(diff.view_body_mismatches[0].name, "open_orders");
    assert!(diff.view_body_mismatches[0]
        .source_body
        .contains("SELECT id FROM orders"));
    assert!(diff.view_body_mismatches[0].target_body.contains("created_at"));
}
