//! Tests for `../query_row_limit.rs` (moved out of the source file).
    use super::{apply_row_limit, effective_row_limit};

    /// #604: the toolbar says "limit N applied" from `effective_row_limit`, while
    /// the engine runs `apply_row_limit`. The two must agree for every shape, or
    /// the summary claims a cap the query never had.
    #[test]
    fn applied_limit_label_matches_the_sql_that_runs_604() {
        for (sql, driver) in [
            ("SELECT * FROM users", "PostgreSQL"),
            ("SELECT * FROM users", "SQLite"),
            ("SELECT * FROM users LIMIT 10", "PostgreSQL"),
            ("UPDATE users SET name = 'x'", "PostgreSQL"),
            ("SELECT 1; SELECT 2", "PostgreSQL"),
            ("VALUES (1), (2)", "PostgreSQL"),
        ] {
            let effective = effective_row_limit(sql, Some(1_000), driver);
            let rewritten = apply_row_limit(sql, Some(1_000), driver);
            let sql_grew = rewritten != sql;
            assert_eq!(
                effective.is_some(),
                sql_grew,
                "label and SQL disagree for {sql:?} ({driver}): label={effective:?} sql={rewritten:?}"
            );
            if let Some(limit) = effective {
                assert!(
                    rewritten.contains(&format!("LIMIT {limit}")) || rewritten.contains(&format!("TOP {limit}")),
                    "capped SQL must carry the announced limit: {rewritten}"
                );
            }
        }
    }

    #[test]
    fn read_statement_gets_limit() {
        assert_eq!(
            apply_row_limit("SELECT * FROM users", Some(500), "PostgreSQL"),
            "SELECT * FROM users LIMIT 500"
        );
        assert_eq!(
            apply_row_limit("SELECT * FROM users;", Some(100), "SQLite"),
            "SELECT * FROM users LIMIT 100;"
        );
    }

    #[test]
    fn limit_lands_before_a_trailing_line_comment() {
        // A `--` tail comment must not swallow the injected cap.
        assert_eq!(
            apply_row_limit("SELECT * FROM big_table -- nightly report", Some(500), "PostgreSQL"),
            "SELECT * FROM big_table LIMIT 500 -- nightly report"
        );
        assert_eq!(
            apply_row_limit("SELECT * FROM t; -- done", Some(500), "PostgreSQL"),
            "SELECT * FROM t LIMIT 500; -- done"
        );
    }

    #[test]
    fn limit_lands_before_a_trailing_block_comment() {
        assert_eq!(
            apply_row_limit("SELECT 1 /* audit */", Some(500), "PostgreSQL"),
            "SELECT 1 LIMIT 500 /* audit */"
        );
    }

    #[test]
    fn explicit_cap_wins() {
        let sql = "SELECT * FROM users LIMIT 10;";
        assert_eq!(apply_row_limit(sql, Some(500), "PostgreSQL"), sql);
        let fetch = "SELECT * FROM users FETCH FIRST 10 ROWS ONLY";
        assert_eq!(apply_row_limit(fetch, Some(500), "PostgreSQL"), fetch);
    }

    #[test]
    fn limit_inside_a_string_does_not_block_the_cap() {
        assert_eq!(
            apply_row_limit("SELECT 'no limit here' FROM t", Some(50), "PostgreSQL"),
            "SELECT 'no limit here' FROM t LIMIT 50"
        );
    }

    #[test]
    fn writes_and_ddl_pass_through() {
        for sql in ["DELETE FROM users WHERE id = 1", "UPDATE t SET x = 1", "CREATE TABLE t (a int)"] {
            assert_eq!(apply_row_limit(sql, Some(500), "PostgreSQL"), sql);
        }
    }

    #[test]
    fn locking_and_into_tails_pass_through() {
        for sql in ["SELECT * FROM t FOR UPDATE", "SELECT * INTO backup FROM t"] {
            assert_eq!(apply_row_limit(sql, Some(500), "PostgreSQL"), sql);
        }
    }

    #[test]
    fn union_still_caps_on_limit_dialects() {
        assert_eq!(
            apply_row_limit("SELECT a FROM t UNION SELECT a FROM u", Some(500), "PostgreSQL"),
            "SELECT a FROM t UNION SELECT a FROM u LIMIT 500"
        );
    }

    #[test]
    fn multi_statement_scripts_pass_through() {
        let sql = "SELECT 1; SELECT 2;";
        assert_eq!(apply_row_limit(sql, Some(500), "PostgreSQL"), sql);
    }

    #[test]
    fn no_limit_and_zero_pass_through() {
        let sql = "SELECT * FROM t";
        assert_eq!(apply_row_limit(sql, None, "PostgreSQL"), sql);
        assert_eq!(apply_row_limit(sql, Some(0), "PostgreSQL"), sql);
    }

    #[test]
    fn sqlserver_gets_top_after_select() {
        assert_eq!(
            apply_row_limit("SELECT u.id FROM users u ORDER BY u.id", Some(500), "SQL Server"),
            "SELECT TOP 500 u.id FROM users u ORDER BY u.id"
        );
        assert_eq!(
            apply_row_limit("SELECT DISTINCT role FROM users", Some(100), "sqlserver"),
            "SELECT DISTINCT TOP 100 role FROM users"
        );
        // TOP on one UNION branch would cap the wrong thing — leave it alone.
        let union = "SELECT a FROM t UNION SELECT a FROM u";
        assert_eq!(apply_row_limit(union, Some(500), "SQL Server"), union);
    }

    #[test]
    fn with_cte_caps_the_outer_read() {
        assert_eq!(
            apply_row_limit("WITH x AS (SELECT 1) SELECT * FROM x", Some(500), "PostgreSQL"),
            "WITH x AS (SELECT 1) SELECT * FROM x LIMIT 500"
        );
        assert_eq!(
            apply_row_limit("WITH x AS (SELECT 1) SELECT * FROM x", Some(500), "SQL Server"),
            "WITH x AS (SELECT 1) SELECT TOP 500 * FROM x"
        );
    }

    #[test]
    fn row_limit_note_reports_applied_cap_without_claiming_truncation() {
        assert_eq!(super::row_limit_note(Some(100)), " · limit 100 applied");
        assert_eq!(super::row_limit_note(None), "");
        assert_eq!(super::row_limit_note(Some(0)), "");
    }