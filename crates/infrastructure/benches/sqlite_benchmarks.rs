//! Performance benchmarks for DB Pro SQLite backend.
//!
//! Run with: `cargo bench --package db-pro-infrastructure`
//!
//! These benchmarks measure backend operations only (DB → Rust DTO → serialization).
//! Frontend rendering is NOT in scope.
//!
//! Performance budget targets (draft):
//!   SQLite connect          < 100ms typical
//!   metadata small DB       < 300ms
//!   serialize 10k rows      < 150ms
//!   cancel acknowledgement  < 200ms (measured in execution registry, not here)

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use db_pro_core::domain::connection::{ConnectionConfig, DriverType, SslMode};
use db_pro_core::ports::DbConnector;
use db_pro_infrastructure::sqlite::connector::SQLiteConnector;

// ── Helpers ────────────────────────────────────────────────────────────────

fn memory_config() -> ConnectionConfig {
    ConnectionConfig {
        name: "bench".into(),
        host: String::new(),
        port: 0,
        database: ":memory:".into(),
        username: String::new(),
        driver: DriverType::SQLite,
        ssl_mode: SslMode::Disable,
        ssh_tunnel: None,
        ssh_profile_id: None,
        ssl_root_cert_path: None,
        ssl_client_cert_path: None,
        ssl_client_key_path: None,
        query_timeout_ms: 60_000,
        max_rows: 1_000_000,
        color: None,
        tags: vec![],
        group: None,
        favorite: false,
        environment: Default::default(),
        readonly: false,
        create_if_missing: false,
    }
}

/// Create a connector + handle with the standard fixture schema (no seed data).
fn setup_empty_schema(connector: &SQLiteConnector) -> db_pro_core::domain::connection::ConnectionHandle {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let handle = rt.block_on(async {
        let h = connector.connect(&memory_config(), "").await.expect("connect");
        let schema_sql = r#"
            CREATE TABLE categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT
            );
            CREATE TABLE products (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                sku TEXT NOT NULL UNIQUE,
                price REAL NOT NULL DEFAULT 0,
                category_id INTEGER REFERENCES categories(id),
                tags TEXT DEFAULT '[]',
                metadata TEXT DEFAULT '{}',
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE orders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                total REAL NOT NULL DEFAULT 0,
                notes TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE order_items (
                order_id INTEGER NOT NULL REFERENCES orders(id),
                product_id TEXT NOT NULL REFERENCES products(id),
                quantity INTEGER NOT NULL DEFAULT 1,
                unit_price REAL NOT NULL,
                PRIMARY KEY (order_id, product_id)
            );
            CREATE INDEX idx_products_category ON products(category_id);
            CREATE INDEX idx_orders_user ON orders(user_id);
        "#;
        for stmt in schema_sql.split(';') {
            let t = stmt.trim();
            if !t.is_empty() {
                connector.execute(&h, t, &[]).await.unwrap();
            }
        }
        h
    });
    handle
}

/// Seed `n` rows into the `orders` table for bulk-query benchmarks.
fn seed_n_orders(connector: &SQLiteConnector, handle: &db_pro_core::domain::connection::ConnectionHandle, n: usize) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        connector.execute(handle, "BEGIN", &[]).await.unwrap();
        for i in 0..n {
            let sql = format!(
                "INSERT INTO orders (user_id, status, total, notes) VALUES ({}, 'pending', {:.2}, 'note-{}')",
                (i % 100) + 1,
                (i as f64) * 1.5,
                i
            );
            connector.execute(handle, &sql, &[]).await.unwrap();
        }
        connector.execute(handle, "COMMIT", &[]).await.unwrap();
    });
}

/// Seed `n` rows with JSON and BLOB data.
fn seed_n_json_blob(connector: &SQLiteConnector, handle: &db_pro_core::domain::connection::ConnectionHandle, n: usize) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Insert a category so FK references are valid.
        connector.execute(handle, "INSERT INTO categories (name) VALUES ('Bench')", &[]).await.unwrap();
        connector.execute(handle, "BEGIN", &[]).await.unwrap();
        for i in 0..n {
            let json = format!(r#"{{"id": {}, "tags": ["a", "b", "c"], "nested": {{"x": {}}}}}"#, i, i * 2);
            let sql = format!(
                // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                "INSERT INTO products (id, name, sku, price, category_id, tags, metadata) VALUES ('id-{i}', 'Product {i}', 'SKU-{i}', {i:.2}, 1, '{json}', '{json}')"
            );
            connector.execute(handle, &sql, &[]).await.unwrap();
        }
        connector.execute(handle, "COMMIT", &[]).await.unwrap();
    });
}

/// Create a "large schema" database with many tables and columns.
fn setup_large_schema(connector: &SQLiteConnector) -> db_pro_core::domain::connection::ConnectionHandle {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let handle = rt.block_on(async {
        let h = connector.connect(&memory_config(), "").await.expect("connect");
        // Create 50 tables, each with 20 columns.
        for t in 0..50 {
            let mut cols = Vec::new();
            cols.push("id INTEGER PRIMARY KEY".to_string());
            for c in 0..19 {
                cols.push(format!("col_{c} TEXT"));
            }
            let sql = format!("CREATE TABLE bench_t{t} ({})", cols.join(", "));
            connector.execute(&h, &sql, &[]).await.unwrap();
            // Insert a few rows.
            for r in 0..5 {
                let vals = (0..19)
                    .map(|c| format!("'val_{t}_{c}_{r}'"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let insert = format!("INSERT INTO bench_t{t} VALUES ({r}, {vals})");
                connector.execute(&h, &insert, &[]).await.unwrap();
            }
        }
        h
    });
    handle
}

// ── Benchmarks ─────────────────────────────────────────────────────────────

fn bench_connect(c: &mut Criterion) {
    c.bench_function("sqlite_connect_and_disconnect", |b| {
        b.iter(|| {
            let connector = SQLiteConnector::new();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let handle = rt.block_on(async { connector.connect(&memory_config(), "").await.unwrap() });
            rt.block_on(connector.disconnect(&handle)).unwrap();
        })
    });
}

fn bench_introspection_small(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_empty_schema(&connector);

    c.bench_function("introspect_small_db", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| rt.block_on(async { black_box(connector.introspect(&handle).await.unwrap()) }))
    });
}

fn bench_introspection_large(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_large_schema(&connector);

    c.bench_function("introspect_large_schema", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| rt.block_on(async { black_box(connector.introspect(&handle).await.unwrap()) }))
    });
}

fn bench_query_10k_rows(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_empty_schema(&connector);
    seed_n_orders(&connector, &handle, 10_000);

    let mut group = c.benchmark_group("query_rows");
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("select_10k", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async { black_box(connector.query(&handle, "SELECT * FROM orders", &[]).await.unwrap()) })
        })
    });
    group.finish();
}

fn bench_query_100k_rows(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_empty_schema(&connector);
    seed_n_orders(&connector, &handle, 100_000);

    let mut group = c.benchmark_group("query_rows");
    group.throughput(Throughput::Elements(100_000));
    group.sample_size(10);
    group.bench_function("select_100k", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async { black_box(connector.query(&handle, "SELECT * FROM orders", &[]).await.unwrap()) })
        })
    });
    group.finish();
}

fn bench_query_json_blob(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_empty_schema(&connector);
    seed_n_json_blob(&connector, &handle, 5_000);

    let mut group = c.benchmark_group("query_json_blob");
    group.throughput(Throughput::Elements(5_000));
    group.bench_function("select_json_metadata_5k", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                black_box(
                    connector
                        .query(&handle, "SELECT name, tags, metadata FROM products", &[])
                        .await
                        .unwrap(),
                )
            })
        })
    });
    group.finish();
}

fn bench_serialize_large_text(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let handle = rt.block_on(async {
        let h = connector.connect(&memory_config(), "").await.unwrap();
        connector
            .execute(&h, "CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT)", &[])
            .await
            .unwrap();
        // Insert 1000 rows with ~10KB text each.
        connector.execute(&h, "BEGIN", &[]).await.unwrap();
        for i in 0..1000 {
            let body = format!("{} ", i).repeat(500); // ~4-5KB per row
            let sql = format!("INSERT INTO docs VALUES ({i}, '{body}')");
            connector.execute(&h, &sql, &[]).await.unwrap();
        }
        connector.execute(&h, "COMMIT", &[]).await.unwrap();
        h
    });

    let mut group = c.benchmark_group("serialize_large_text");
    group.throughput(Throughput::Elements(1_000));
    group.bench_function("select_1k_large_text", |b| {
        b.iter(|| rt.block_on(async { black_box(connector.query(&handle, "SELECT * FROM docs", &[]).await.unwrap()) }))
    });
    group.finish();
}

fn bench_explain(c: &mut Criterion) {
    let connector = SQLiteConnector::new();
    let handle = setup_empty_schema(&connector);
    seed_n_orders(&connector, &handle, 1_000);

    c.bench_function("explain_query", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                black_box(
                    connector
                        .explain(&handle, "SELECT * FROM orders WHERE user_id = 42", &[], false)
                        .await
                        .unwrap(),
                )
            })
        })
    });
}

criterion_group!(
    benches,
    bench_connect,
    bench_introspection_small,
    bench_introspection_large,
    bench_query_10k_rows,
    bench_query_100k_rows,
    bench_query_json_blob,
    bench_serialize_large_text,
    bench_explain,
);
criterion_main!(benches);
