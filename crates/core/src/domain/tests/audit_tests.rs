//! Tests for `../audit.rs` (moved out of the source file).
use super::*;
use std::io::Write;

#[test]
fn redact_password_and_uri() {
    let (out, changed) = redact_secrets("password=s3cret postgresql://u:hunter2@host/db");
    assert!(changed);
    assert!(!out.contains("s3cret"));
    assert!(!out.contains("hunter2"));
    assert!(out.contains("***"));
}

#[test]
fn redact_sql_credential_literals() {
    let (out, changed) = redact_secrets("CREATE ROLE bob PASSWORD 'hunter2' LOGIN;");
    assert!(changed);
    assert_eq!(out, "CREATE ROLE bob PASSWORD '***' LOGIN;");

    let (out, changed) = redact_secrets(r#"ALTER USER app IDENTIFIED BY "s3cr3t" WITH VALID UNTIL '2030-01-01';"#);
    assert!(changed, "double-quoted literal must be redacted: {out}");
    assert_eq!(
        out,
        r#"ALTER USER app IDENTIFIED BY "***" WITH VALID UNTIL '2030-01-01';"#
    );
}

#[test]
fn redact_sql_credential_literals_redacts_every_occurrence() {
    let (out, changed) = redact_secrets("CREATE USER a PASSWORD 'one1'; CREATE USER b PASSWORD 'two2';");
    assert!(changed);
    assert_eq!(out.matches("***").count(), 2, "{out}");
    assert!(!out.contains("one1") && !out.contains("two2"), "{out}");
}

#[test]
fn redact_sql_credential_literals_keeps_ordinary_sql_alone() {
    for statement in [
        "SELECT password_hash FROM users WHERE name = 'bob'",
        "CREATE TABLE my_password_store (value text)",
        "SELECT 'password is a word' AS note",
        "ALTER ROLE bob PASSWORD ''",
    ] {
        let (out, changed) = redact_secrets(statement);
        assert!(!changed, "{statement} must be left alone, got {out}");
        assert_eq!(out, statement);
    }
}

#[test]
fn parse_csv_quoted_commas() {
    let cols = parse_csv_line(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            r#"2024-01-01 00:00:00.000 UTC,"u","db",1,"127.0.0.1",s,1,"SELECT",t,v,0,"LOG","00000","hello, world",,,,,,,,,,"app","client",,"0""#,
        )
        .unwrap();
    assert_eq!(cols[13], "hello, world");
}

#[test]
fn page_reads_newest_without_full_materialization() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("db-pro-audit-{}.csv", std::process::id()));
    {
        let mut tmp = File::create(&path).unwrap();
        for i in 0..20 {
            writeln!(
                    tmp,
                    // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                    r#"2024-01-01 00:00:{i:02}.000 UTC,"u","db",{i},"127.0.0.1",s,{i},"SELECT",t,v,0,"LOG","00000","msg {i}",,,,,,,,,,"app","client",,"0""#
                )
                .unwrap();
        }
        tmp.flush().unwrap();
    }
    let page = read_csvlog_page(&path, &AuditFilter::default(), 5, 64 * 1024).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(page.events.len(), 5);
    assert!(page.events.last().unwrap().message.contains("msg 19"));
    assert_eq!(page.source.kind, AuditSourceKind::PostgresCsvLog);
}

#[test]
fn filter_and_tag_parse() {
    let path = csvlog_path_from_tags(&["favorite".into(), "audit:csvlog=/var/log/pg/log.csv".into()]);
    assert_eq!(path.unwrap(), PathBuf::from("/var/log/pg/log.csv"));
    let ev = AuditEvent {
        id: "1".into(),
        timestamp: None,
        username: Some("alice".into()),
        database: Some("app".into()),
        severity: Some("LOG".into()),
        command_tag: Some("SELECT".into()),
        application_name: None,
        client_addr: None,
        message: "ok".into(),
        query: Some("select 1".into()),
        detail: None,
        redacted: false,
    };
    assert!(AuditFilter {
        username: "ali".into(),
        ..AuditFilter::default()
    }
    .matches(&ev));
    assert!(!AuditFilter {
        database: "other".into(),
        ..AuditFilter::default()
    }
    .matches(&ev));
}

#[test]
fn export_csv_includes_header() {
    let csv = export_events_csv(&[]);
    assert!(csv.starts_with("timestamp,"));
}

#[test]
fn event_ids_differ_when_only_the_message_tail_differs() {
    // Regression (#490): ids built from timestamp|pid|session|first-48-chars
    // of the message collided for rows sharing that prefix.
    let line = |tail: &str| {
        format!(
            r#"2024-01-01 00:00:00.000 UTC,"u","db",1,"127.0.0.1",s,1,"SELECT",t,v,0,"LOG","00000","{}{tail}",,,,,,,,,,"app","client",,"0""#,
            "x".repeat(64)
        )
    };
    let a = parse_postgres_csv_line(&line("insert into a")).unwrap();
    let b = parse_postgres_csv_line(&line("insert into b")).unwrap();
    assert_ne!(a.id, b.id);
}

#[test]
fn event_id_is_stable_for_the_same_line() {
    let raw = r#"2024-01-01 00:00:00.000 UTC,"u","db",1,"127.0.0.1",s,1,"SELECT",t,v,0,"LOG","00000","stable msg",,,,,,,,,,"app","client",,"0""#;
    let a = parse_postgres_csv_line(raw).unwrap();
    let b = parse_postgres_csv_line(raw).unwrap();
    assert_eq!(a.id, b.id);
}

#[test]
fn identical_log_lines_get_distinct_ids_within_a_file() {
    // Byte-identical lines can repeat (retry loops); the file offset suffix
    // keeps their ids unique for selection/bookmark/export (#490).
    let dir = std::env::temp_dir();
    let path = dir.join(format!("db-pro-audit-dup-{}.csv", std::process::id()));
    {
        let mut tmp = File::create(&path).unwrap();
        for _ in 0..3 {
            writeln!(
                    tmp,
                    // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                    r#"2024-01-01 00:00:00.000 UTC,"u","db",1,"127.0.0.1",s,1,"SELECT",t,v,0,"LOG","00000","same message",,,,,,,,,,"app","client",,"0""#
                )
                .unwrap();
        }
        tmp.flush().unwrap();
    }
    let page = read_csvlog_page(&path, &AuditFilter::default(), 10, 64 * 1024).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(page.events.len(), 3);
    let ids: std::collections::BTreeSet<&str> = page.events.iter().map(|event| event.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
}

/// #599: query history must not keep credentials verbatim.
#[test]
fn history_redaction_masks_every_credential_form_599() {
    use crate::application::history_sql_for_test;

    for (sql, must_not_contain) in [
        ("CREATE ROLE app LOGIN PASSWORD 'S3cr3t!'", "S3cr3t!"),
        ("ALTER USER x WITH PASSWORD 'hunter2'", "hunter2"),
        (
            "CREATE USER MAPPING FOR u SERVER s OPTIONS (user 'u', password 'mappingpw')",
            "mappingpw",
        ),
        (
            "SELECT dblink_connect('host=db password=dblinkpw');",
            "dblinkpw",
        ),
        (
            "CREATE SUBSCRIPTION sub CONNECTION 'host=db password=subpw' PUBLICATION p;",
            "subpw",
        ),
    ] {
        let redacted = history_sql_for_test(sql);
        assert!(
            !redacted.contains(must_not_contain),
            "{must_not_contain} survived: {redacted}"
        );
    }

    // Statements without secrets are stored unchanged so history stays recognisable.
    let plain = "SELECT id, name FROM users WHERE id = 42;";
    assert_eq!(history_sql_for_test(plain), plain);
}
