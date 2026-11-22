use super::*;

/// Transaction-control keywords fall to the classifier's fail-safe `Write` default,
/// so a script that manages its own transaction is never treated as read-only. This
/// pins the contract the multi-statement execution path depends on: `BEGIN`/`COMMIT`/
/// `ROLLBACK` are not mutations, but they must not hide one either.
#[test]
fn transaction_control_batches_classify_as_write_and_never_hide_a_mutation() {
    assert_eq!(
        classify_script_safety("BEGIN; UPDATE users SET name = 'x' WHERE id = 1; COMMIT;"),
        Some(StatementSafety::Write)
    );
    assert_eq!(
        classify_script_safety("BEGIN; SELECT 1; ROLLBACK;"),
        Some(StatementSafety::Write)
    );
    assert_eq!(
        classify_script_safety("SELECT 1; SELECT 2;"),
        Some(StatementSafety::Read)
    );
    // A destructive statement inside its own transaction is still destructive, so the
    // run-all confirmation gate sees it.
    assert_eq!(
        classify_script_safety("BEGIN; DROP TABLE users; COMMIT;"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn classify_select_is_read() {
    assert_eq!(classify_statement_safety("SELECT 1"), Some(StatementSafety::Read));
}

#[test]
fn classify_insert_is_write() {
    assert_eq!(
        classify_statement_safety("INSERT INTO t VALUES (1)"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn classify_update_with_where_is_write() {
    assert_eq!(
        classify_statement_safety("UPDATE t SET x = 1 WHERE id = 2"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn classify_update_without_where_is_destructive() {
    assert_eq!(
        classify_statement_safety("UPDATE t SET x = 1"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn classify_delete_with_where_is_write() {
    assert_eq!(
        classify_statement_safety("DELETE FROM t WHERE id = 1"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn classify_delete_without_where_is_destructive() {
    assert_eq!(
        classify_statement_safety("DELETE FROM t"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn delete_identifier_containing_where_is_still_destructive() {
    assert_eq!(
        classify_statement_safety(r#"DELETE FROM \"somewhere\""#),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_statement_safety("DELETE FROM t -- WHERE id = 1"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_statement_safety("DELETE FROM t RETURNING $tag$ WHERE $tag$"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn delete_and_update_with_tautology_where_are_destructive() {
    for sql in [
        "DELETE FROM t WHERE true",
        "DELETE FROM t WHERE 1 = 1",
        "DELETE FROM t WHERE 1=1",
        "DELETE FROM t WHERE ( 1 = 1 )",
        "DELETE FROM t WHERE '' = ''",
        "DELETE FROM t WHERE 1 = 1 -- always",
        "UPDATE t SET x = 1 WHERE true",
        "UPDATE t SET x = 1 WHERE 1 = 1",
    ] {
        assert_eq!(
            classify_statement_safety(sql),
            Some(StatementSafety::Destructive),
            "{sql}"
        );
    }
}

#[test]
fn delete_and_update_with_real_where_stay_writes() {
    for sql in [
        "DELETE FROM t WHERE id = 1",
        "DELETE FROM t WHERE name = 'x' RETURNING id",
        "UPDATE t SET x = 1 WHERE id = 2",
        "UPDATE t SET x = 1 WHERE true AND id = 2",
    ] {
        assert_eq!(classify_statement_safety(sql), Some(StatementSafety::Write), "{sql}");
    }
}

#[test]
fn select_into_is_ddl_not_read() {
    assert_eq!(
        classify_statement_safety("SELECT * INTO backup FROM users"),
        Some(StatementSafety::Ddl)
    );
    assert_eq!(
        classify_statement_safety("SELECT id FROM t"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn privilege_and_maintenance_verbs_are_gated() {
    for sql in [
        "GRANT SELECT ON t TO r",
        "REVOKE ALL ON t FROM r",
        "SECURITY LABEL FOR p ON t IS 'x'",
    ] {
        assert_eq!(classify_statement_safety(sql), Some(StatementSafety::Ddl), "{sql}");
    }
    for sql in ["VACUUM FULL t", "CLUSTER t USING idx", "REINDEX TABLE t", "DISCARD ALL"] {
        assert_eq!(
            classify_statement_safety(sql),
            Some(StatementSafety::Destructive),
            "{sql}"
        );
    }
}

#[test]
fn classify_drop_is_destructive() {
    assert_eq!(
        classify_statement_safety("DROP TABLE t"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn classify_truncate_is_destructive() {
    assert_eq!(
        classify_statement_safety("TRUNCATE TABLE t"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn classify_merge_delete_as_destructive_but_update_as_write() {
    assert_eq!(
        classify_statement_safety("MERGE INTO target USING source ON target.id = source.id WHEN MATCHED THEN DELETE"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_statement_safety(
            "MERGE INTO target USING source ON target.id = source.id WHEN MATCHED THEN UPDATE SET value = source.value"
        ),
        Some(StatementSafety::Write)
    );
    assert_eq!(
        classify_statement_safety(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "MERGE INTO target USING (SELECT 'DELETE' AS action) source ON target.id = 1 WHEN MATCHED THEN UPDATE SET value = 1"
        ),
        Some(StatementSafety::Write)
    );
}

#[test]
fn classify_opaque_server_side_execution_as_destructive() {
    for sql in [
        "DO $body$ BEGIN DELETE FROM users; END $body$",
        "CALL cleanup_expired_rows()",
        "EXECUTE prepared_cleanup",
    ] {
        assert_eq!(classify_statement_safety(sql), Some(StatementSafety::Destructive));
        assert!(validate_against_policy(
            sql,
            &ConnectionSafetyPolicy {
                read_only: false,
                allow_ddl: true,
                allow_destructive: false,
                max_rows: None,
                query_timeout_ms: None,
            }
        )
        .is_err());
    }
}

#[test]
fn classify_create_is_ddl() {
    assert_eq!(
        classify_statement_safety("CREATE TABLE t (id INT)"),
        Some(StatementSafety::Ddl)
    );
}

#[test]
fn classify_alter_is_ddl() {
    assert_eq!(
        classify_statement_safety("ALTER TABLE t ADD COLUMN x INT"),
        Some(StatementSafety::Ddl)
    );
}

#[test]
fn classify_with_select_is_read() {
    assert_eq!(
        classify_statement_safety("WITH cte AS (SELECT id FROM t) SELECT * FROM cte"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn classify_with_update_is_write() {
    assert_eq!(
        classify_statement_safety("WITH cte AS (SELECT id FROM t) UPDATE t SET x = 1 WHERE id = 1"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn read_only_policy_rejects_write() {
    let policy = ConnectionSafetyPolicy::read_only();
    assert!(validate_against_policy("SELECT 1", &policy).is_ok());
    assert!(validate_against_policy("INSERT INTO t VALUES (1)", &policy).is_err());
    assert!(validate_against_policy("UPDATE t SET x = 1", &policy).is_err());
    assert!(validate_against_policy("DELETE FROM t WHERE id = 1", &policy).is_err());
}

#[test]
fn full_access_policy_allows_everything() {
    let policy = ConnectionSafetyPolicy::full_access();
    assert!(validate_against_policy("SELECT 1", &policy).is_ok());
    assert!(validate_against_policy("INSERT INTO t VALUES (1)", &policy).is_ok());
    assert!(validate_against_policy("DROP TABLE t", &policy).is_ok());
    assert!(validate_against_policy("CREATE TABLE t (id INT)", &policy).is_ok());
}

#[test]
fn no_ddl_policy_rejects_create_alter() {
    let policy = ConnectionSafetyPolicy {
        read_only: false,
        allow_ddl: false,
        allow_destructive: true,
        max_rows: None,
        query_timeout_ms: None,
    };
    assert!(validate_against_policy("SELECT 1", &policy).is_ok());
    assert!(validate_against_policy("CREATE TABLE t (id INT)", &policy).is_err());
    assert!(validate_against_policy("ALTER TABLE t ADD COLUMN x INT", &policy).is_err());
}

#[test]
fn empty_sql_is_rejected() {
    let policy = ConnectionSafetyPolicy::full_access();
    assert!(validate_against_policy("  ", &policy).is_err());
}

#[test]
fn leading_comments_stripped() {
    assert_eq!(
        classify_statement_safety("-- comment\nSELECT 1"),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_statement_safety("/* block */ DROP TABLE t"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn explain_plain_select_is_read() {
    assert_eq!(
        classify_statement_safety("EXPLAIN SELECT * FROM t"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn explain_analyze_substrings_in_query_do_not_change_plain_explain_safety() {
    assert_eq!(
        classify_statement_safety("EXPLAIN SELECT 'ANALYZE' AS analyze_column"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn explain_analyze_allows_comments_between_keywords() {
    assert_eq!(
        classify_statement_safety("EXPLAIN /* options */ ANALYZE /* query */ DELETE FROM t WHERE id = 1"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn explain_analyze_select_is_read() {
    assert_eq!(
        classify_statement_safety("EXPLAIN ANALYZE SELECT * FROM t"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn explain_analyze_delete_is_write() {
    // EXPLAIN ANALYZE actually executes the statement — must NOT be Read.
    assert_eq!(
        classify_statement_safety("EXPLAIN ANALYZE DELETE FROM users WHERE id = 1"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn explain_analyze_insert_is_write() {
    assert_eq!(
        classify_statement_safety("EXPLAIN ANALYZE INSERT INTO t VALUES (1)"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn explain_parenthesized_analyze_classifies_the_inner_statement() {
    assert_eq!(
        classify_statement_safety("EXPLAIN (ANALYZE true, FORMAT JSON) DELETE FROM t WHERE id = 1"),
        Some(StatementSafety::Write)
    );
    assert_eq!(
        classify_statement_safety("EXPLAIN (FORMAT JSON) DELETE FROM t"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn explain_analyze_with_unicode_query_text_does_not_panic() {
    assert_eq!(
        classify_statement_safety("EXPLAIN ANALYZE SELECT 'straße'"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn readonly_rejects_explain_analyze_delete() {
    let policy = ConnectionSafetyPolicy::read_only();
    assert!(validate_against_policy("EXPLAIN ANALYZE DELETE FROM users WHERE id = 1", &policy).is_err());
}

#[test]
fn mutating_cte_with_delete_is_write() {
    // Data-modifying CTE: even though outer query is SELECT, the CTE mutates.
    assert_eq!(
        classify_statement_safety("WITH deleted AS (DELETE FROM users WHERE id = 1 RETURNING *) SELECT * FROM deleted"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn cte_classifier_respects_lexical_boundaries_and_comment_separators() {
    assert_eq!(
        classify_statement_safety("WITH deleted AS (DELETE/* comment */FROM users RETURNING *) SELECT * FROM deleted"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_statement_safety(
            "WITH cte (delete_col) AS (SELECT \"DELETE FROM users\", $$UPDATE users$$) SELECT * FROM cte"
        ),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_statement_safety(r#"WITH cte AS (SELECT E'prefix\' DELETE FROM users') SELECT * FROM cte"#),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_statement_safety(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "WITH deleted AS (DELETE FROM users USING (SELECT id FROM audit WHERE id > 0) a RETURNING *) SELECT * FROM deleted"
        ),
        Some(StatementSafety::Destructive)
    );
    assert!(validate_against_policy(
        "WITH deleted AS (DELETE/* comment */FROM users RETURNING *) SELECT * FROM deleted",
        &ConnectionSafetyPolicy::read_only()
    )
    .is_err());
}

#[test]
fn destructive_delete_inside_cte_remains_destructive() {
    assert_eq!(
        classify_statement_safety("WITH deleted AS (DELETE FROM users RETURNING *) SELECT * FROM deleted"),
        Some(StatementSafety::Destructive)
    );

    let policy = ConnectionSafetyPolicy {
        read_only: false,
        allow_ddl: true,
        allow_destructive: false,
        max_rows: None,
        query_timeout_ms: None,
    };
    assert!(validate_against_policy(
        "WITH deleted AS (DELETE FROM users RETURNING *) SELECT * FROM deleted",
        &policy
    )
    .is_err());
}

#[test]
fn cte_delete_requires_a_real_where_keyword() {
    assert_eq!(
        classify_statement_safety("WITH cte AS (SELECT 1) DELETE FROM users -- WHERE id = 1"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_statement_safety("WITH cte AS (SELECT 1) DELETE FROM users WHERE id = 1"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn mutating_cte_with_insert_is_write() {
    assert_eq!(
        classify_statement_safety("WITH moved AS (INSERT INTO t2 SELECT * FROM t1 RETURNING *) SELECT * FROM moved"),
        Some(StatementSafety::Write)
    );
}

#[test]
fn readonly_rejects_mutating_cte() {
    let policy = ConnectionSafetyPolicy::read_only();
    assert!(validate_against_policy(
        "WITH deleted AS (DELETE FROM users RETURNING *) SELECT * FROM deleted",
        &policy
    )
    .is_err());
}

#[test]
fn split_statements_basic() {
    let stmts = split_statements("SELECT 1; SELECT 2");
    assert_eq!(stmts, vec!["SELECT 1", "SELECT 2"]);
}

#[test]
fn split_statements_trailing_semicolon() {
    let stmts = split_statements("SELECT 1; SELECT 2;");
    assert_eq!(stmts, vec!["SELECT 1", "SELECT 2"]);
}

#[test]
fn split_statements_respects_quotes() {
    let stmts = split_statements("SELECT ';'; SELECT 2");
    assert_eq!(stmts, vec!["SELECT ';'", "SELECT 2"]);
}

#[test]
fn split_statements_respects_comments_and_dollar_quotes() {
    let stmts = split_statements("DO $body$ BEGIN PERFORM 1; /* nested ; comment */ PERFORM 2; END $body$; SELECT 2");
    assert_eq!(stmts.len(), 2);
    assert!(stmts[0].contains("PERFORM 1;"));
    assert_eq!(stmts[1], "SELECT 2");
}

#[test]
fn split_statements_drops_comment_only_tail() {
    assert_eq!(split_statements("SELECT 1; -- trailing comment\n"), vec!["SELECT 1"]);
}

#[test]
fn split_statements_does_not_treat_identifier_dollar_signs_as_quotes() {
    let stmts = split_statements("SELECT foo$bar$; SELECT 2");
    assert_eq!(stmts, vec!["SELECT foo$bar$", "SELECT 2"]);
}

#[test]
fn split_statements_empty() {
    let stmts = split_statements("  ");
    assert!(stmts.is_empty());
}

#[test]
fn script_safety_takes_the_most_dangerous_statement() {
    assert_eq!(
        classify_script_safety("SELECT 1; DROP TABLE t;"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_script_safety("SELECT 1; UPDATE t SET a = 1 WHERE id = 1"),
        Some(StatementSafety::Write)
    );
    assert_eq!(
        classify_script_safety("DROP TABLE t; SELECT 1"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(
        classify_script_safety("SELECT 1; CREATE TABLE t (id int); SELECT 2"),
        Some(StatementSafety::Ddl)
    );
    assert_eq!(
        classify_script_safety("SELECT 1; SELECT 2;"),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_script_safety("DROP TABLE t"),
        Some(StatementSafety::Destructive)
    );
    assert_eq!(classify_script_safety(""), None);
    assert_eq!(classify_script_safety(";"), None);
    assert_eq!(classify_script_safety("-- only a comment"), None);
}

#[test]
fn script_safety_ignores_semicolons_inside_literals_and_comments() {
    assert_eq!(
        classify_script_safety("SELECT ';'; SELECT 2"),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_script_safety("SELECT 1 -- ; not a statement\n"),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_script_safety("DO $body$ BEGIN PERFORM 1; END $body$"),
        Some(StatementSafety::Destructive)
    );
}

#[test]
fn transaction_control_verbs_are_recognised_in_every_form() {
    for (statement, verb) in [
        ("BEGIN", "BEGIN"),
        ("BEGIN;", "BEGIN"),
        ("begin", "BEGIN"),
        ("BEGIN TRANSACTION", "BEGIN"),
        ("BEGIN IMMEDIATE", "BEGIN"),
        ("START TRANSACTION", "START TRANSACTION"),
        ("COMMIT", "COMMIT"),
        ("COMMIT WORK", "COMMIT"),
        ("END", "END"),
        ("ROLLBACK", "ROLLBACK"),
        ("ROLLBACK TO SAVEPOINT before_insert", "ROLLBACK"),
        ("ABORT", "ABORT"),
        ("SAVEPOINT s1", "SAVEPOINT"),
        ("RELEASE SAVEPOINT s1", "RELEASE"),
        ("-- note\nCOMMIT", "COMMIT"),
        ("/* note */ COMMIT", "COMMIT"),
    ] {
        assert_eq!(
            transaction_control_verb(statement),
            Some(verb),
            "statement: {statement}"
        );
    }
}

#[test]
fn transaction_control_detection_does_not_match_inside_a_statement() {
    for statement in [
        "",
        "   ",
        "SELECT 1",
        // A keyword that merely starts with a control keyword.
        "BEGINNING",
        "COMMITTED",
        "ENDLESS",
        // Control words inside a statement are not transaction control.
        "SELECT 'begin' AS word",
        "SELECT 1 AS commit",
        "SELECT commit FROM audit_log",
        "INSERT INTO t (begin) VALUES (1)",
        // A PL/pgSQL body: the statement is a `DO`, not a `BEGIN`.
        "DO $body$ BEGIN PERFORM 1; END $body$",
        // `START` alone is not `START TRANSACTION`.
        "START",
    ] {
        assert_eq!(transaction_control_verb(statement), None, "statement: {statement}");
    }
}

// #477: `standard_conforming_strings` is on by default in PostgreSQL, so `\`
// inside `'…'` is a literal backslash, not an escape. `'x\'` closes the string
// and the tail is a real second statement — splitting it as one would let a
// destructive statement ride through `reject_multi_statement`.
#[test]
fn split_statements_postgres_backslash_does_not_escape_quote() {
    let stmts = split_statements("SELECT 'x\\'; SELECT 2");
    assert_eq!(stmts.len(), 2, "{stmts:?}");

    let stmts = split_statements_for_driver("SELECT 'x\\'; SELECT 2", DriverType::Postgres);
    assert_eq!(stmts.len(), 2, "{stmts:?}");

    // SQLite, SQL Server, and every non-MySQL driver share the standard reading.
    for driver in [DriverType::SQLite, DriverType::SqlServer] {
        assert_eq!(split_statements_for_driver("SELECT 'x\\'; SELECT 2", driver).len(), 2);
    }
}

// MySQL's default sql_mode keeps `\'` as an escaped quote, so the same text
// stays a single statement there.
#[test]
fn split_statements_mysql_backslash_escapes_quote() {
    let stmts = split_statements_for_driver("SELECT 'x\\'; SELECT 2", DriverType::Mysql);
    assert_eq!(stmts.len(), 1, "{stmts:?}");
    assert_eq!(stmts[0], "SELECT 'x\\'; SELECT 2");

    let stmts = split_statements_for_driver("SELECT 'it\\'s ok'; SELECT 2", DriverType::Mysql);
    assert_eq!(stmts.len(), 2, "{stmts:?}");
}

// `E'…'` literals opt into backslash escapes on their own, whatever the driver.
#[test]
fn split_statements_escape_string_literal() {
    let stmts = split_statements("SELECT E'x\\''; SELECT 2");
    assert_eq!(stmts.len(), 2, "{stmts:?}");
    assert_eq!(stmts[0], "SELECT E'x\\''");

    // Lowercase prefix, and content after the escape-string's closing quote.
    let stmts = split_statements("SELECT e'a\\'b' AS s; SELECT 2");
    assert_eq!(stmts.len(), 2, "{stmts:?}");

    // `E` glued to an identifier is not an escape-string prefix — the `'…'`
    // runs as a standard string with no backslash escapes.
    let stmts = split_statements("SELECT someE'x\\'; SELECT 2");
    assert_eq!(stmts.len(), 2, "{stmts:?}");
}

#[test]
fn script_safety_sees_statements_after_a_closed_backslash_string() {
    assert_eq!(
        classify_script_safety("SELECT 'x\\'; DROP TABLE important; --"),
        Some(StatementSafety::Destructive)
    );
    // On MySQL the same text is a single read; the follow-up only fires where
    // the string actually closes.
    assert_eq!(
        classify_script_safety_for_driver("SELECT 'x\\'; DROP TABLE important; --", DriverType::Mysql),
        Some(StatementSafety::Read)
    );
}

// --- #586: BEGIN … END bodies are one statement -------------------------------

#[test]
fn sqlite_trigger_body_splits_as_one_statement() {
    let sql = "CREATE TRIGGER trg AFTER INSERT ON t BEGIN UPDATE t SET b = 'x'; SELECT 1; END;";
    let statements = split_statements_for_driver(sql, DriverType::SQLite);
    assert_eq!(statements.len(), 1, "{statements:?}");
    assert!(statements[0].ends_with("END"), "{statements:?}");
    // The `END` fragment that used to fall out of the splitter read as
    // transaction control, which is what refused the whole batch (#586).
    assert_eq!(transaction_control_verb(&statements[0]), None);
}

#[test]
fn sqlite_trigger_with_nested_case_and_control_flow_is_one_statement() {
    let sql = concat!(
        "CREATE TRIGGER trg AFTER UPDATE ON t BEGIN ",
        "UPDATE t SET kind = CASE WHEN new.a > 0 THEN 'pos' ELSE 'neg' END; ",
        "INSERT INTO log(v) SELECT v FROM (SELECT 1 AS v); ",
        "END; ",
        "SELECT 1;"
    );
    let statements = split_statements_for_driver(sql, DriverType::SQLite);
    assert_eq!(statements.len(), 2, "{statements:?}");
    assert_eq!(statements[1], "SELECT 1");
}

#[test]
fn temporary_trigger_spelling_opens_a_body() {
    let sql = "CREATE TEMPORARY TRIGGER trg AFTER INSERT ON t BEGIN INSERT INTO log VALUES (1); END; SELECT 1";
    let statements = split_statements_for_driver(sql, DriverType::SQLite);
    assert_eq!(statements.len(), 2, "{statements:?}");
}

#[test]
fn postgres_begin_atomic_function_body_is_one_statement() {
    let sql = "CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END; SELECT 3;";
    let statements = split_statements_for_driver(sql, DriverType::Postgres);
    assert_eq!(statements.len(), 2, "{statements:?}");
    assert!(statements[0].contains("SELECT 2"), "{statements:?}");
}

#[test]
fn plain_begin_still_does_not_swallow_later_statements() {
    // A bare `BEGIN;` is transaction control, never a block opener, so the
    // `END;` that follows still closes the transaction as its own statement.
    let statements = split_statements("BEGIN; UPDATE t SET a = 1; END;");
    assert_eq!(statements.len(), 3, "{statements:?}");
    assert_eq!(transaction_control_verb(&statements[0]), Some("BEGIN"));
    assert_eq!(transaction_control_verb(&statements[2]), Some("END"));
}

#[test]
fn create_table_column_named_case_does_not_open_a_block() {
    let sql = "CREATE TABLE t (case int, if int); SELECT 1";
    let statements = split_statements(sql);
    assert_eq!(statements.len(), 2, "{statements:?}");
}

#[test]
fn non_create_statements_keep_every_semicolon_boundary() {
    let statements = split_statements("SELECT 'a;b'; INSERT INTO t VALUES (1); SELECT CASE WHEN true THEN 1 ELSE 2 END");
    assert_eq!(statements.len(), 3, "{statements:?}");
}

#[test]
fn body_units_keep_a_trigger_body_whole() {
    let trigger = "CREATE TRIGGER trg AFTER INSERT ON t BEGIN UPDATE t SET b = 'x'; INSERT INTO log VALUES (1); END";
    let units = split_statement_body_for_driver(trigger, DriverType::SQLite);
    assert_eq!(units.len(), 3, "{units:?}");
    assert!(units[0].starts_with("CREATE TRIGGER"));
    assert_eq!(units[1], "INSERT INTO log VALUES (1)");
    assert_eq!(units[2], "END");
}

#[test]
fn body_units_leave_a_plain_statement_untouched() {
    let sql = "SELECT 1; SELECT 2";
    assert_eq!(split_statement_body_for_driver(sql, DriverType::SQLite), vec![sql.to_owned()]);
}

#[test]
fn body_units_split_after_a_closed_case_expression() {
    let trigger = concat!(
        "CREATE TRIGGER trg AFTER INSERT ON t BEGIN ",
        "UPDATE t SET kind = CASE WHEN new.a > 0 THEN 'pos' ELSE 'neg' END; ",
        "INSERT INTO log VALUES (1); ",
        "END"
    );
    let units = split_statement_body_for_driver(trigger, DriverType::SQLite);
    assert_eq!(units.len(), 3, "{units:?}");
    assert!(units[0].ends_with("ELSE 'neg' END"), "{units:?}");
    assert_eq!(units[1], "INSERT INTO log VALUES (1)");
    assert_eq!(units[2], "END");
}

#[test]
fn select_with_row_lock_is_not_a_read() {
    // A read-only connection must not be able to hold row locks (#588).
    for sql in [
        "SELECT * FROM users FOR UPDATE",
        "SELECT * FROM users FOR NO KEY UPDATE",
        "SELECT * FROM users FOR SHARE",
        "SELECT * FROM users FOR KEY SHARE",
        "SELECT id FROM (SELECT id FROM users FOR UPDATE) locked",
    ] {
        assert_eq!(classify_statement_safety(sql), Some(StatementSafety::Write), "{sql}");
    }
    assert_eq!(
        classify_statement_safety("SELECT * FROM users WHERE name = 'for update'"),
        Some(StatementSafety::Read)
    );
    assert_eq!(
        classify_statement_safety("SELECT * FROM users ORDER BY id"),
        Some(StatementSafety::Read)
    );
}

#[test]
fn begin_atomic_keeps_the_function_body_together() {
    let sql = "CREATE FUNCTION f() RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT 1; SELECT 2; END;";
    let statements = split_statements_for_driver(sql, DriverType::Postgres);
    assert_eq!(statements.len(), 1, "{statements:?}");
    assert_eq!(transaction_control_verb(&statements[0]), None);
}
