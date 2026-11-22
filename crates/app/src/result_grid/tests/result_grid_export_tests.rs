//! Tests for `../result_grid_export.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn quote_sql_identifier_escapes_embedded_quotes() {
        assert_eq!(quote_sql_identifier("customers"), "\"customers\"");
        assert_eq!(quote_sql_identifier("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn qualified_sql_name_quotes_each_part() {
        assert_eq!(
            qualified_sql_name("main", "customers"),
            "\"main\".\"customers\""
        );
        assert_eq!(qualified_sql_name("", "customers"), "\"customers\"");
    }

    #[test]
    fn qualified_sql_name_preserves_literal_dot_in_table_name() {
        assert_eq!(
            qualified_sql_name("main", "HumanResources.Department"),
            "\"main\".\"HumanResources.Department\""
        );
    }

    #[test]
    fn qualified_sql_name_escapes_quotes_in_both_parts() {
        assert_eq!(
            qualified_sql_name("sch\"ema", "ta\"ble"),
            "\"sch\"\"ema\".\"ta\"\"ble\""
        );
    }

    // #260: generated row-limited snippets must not leak T-SQL-incompatible `LIMIT`.
    #[test]
    fn select_top_snippet_emits_limit_for_postgres_sqlite_and_mysql() {
        for driver in ["postgres", "postgresql", "sqlite", "mysql"] {
            let snippet = select_top_snippet(driver, "\"public\".\"customers\"", 100);
            assert_eq!(snippet, "SELECT *\nFROM \"public\".\"customers\"\nLIMIT 100;", "{driver}: {snippet}");
        }
    }

    #[test]
    fn select_top_snippet_emits_top_for_sql_server() {
        for driver in ["sqlserver", "sql server", "mssql"] {
            let snippet = select_top_snippet(driver, "\"dbo\".\"customers\"", 100);
            assert_eq!(snippet, "SELECT TOP (100) *\nFROM \"dbo\".\"customers\";", "{driver}: {snippet}");
            assert!(!snippet.contains("LIMIT"), "{driver}: {snippet}");
        }
    }

    #[test]
    fn unique_json_keys_suffixes_duplicate_column_names() {
        let columns = vec![
            crate::UiColumn { name: "id".to_owned(), data_type: "int".to_owned(), nullable: false },
            crate::UiColumn { name: "id".to_owned(), data_type: "int".to_owned(), nullable: false },
            crate::UiColumn { name: "id".to_owned(), data_type: "text".to_owned(), nullable: true },
            crate::UiColumn { name: "name".to_owned(), data_type: "text".to_owned(), nullable: false },
        ];
        assert_eq!(unique_json_keys(&columns), vec!["id", "id_2", "id_3", "name"]);
    }

    #[test]
    fn format_result_json_keeps_all_duplicate_named_columns() {
        let result = UiQueryResult {
            columns: vec![
                crate::UiColumn { name: "x".to_owned(), data_type: "int".to_owned(), nullable: false },
                crate::UiColumn { name: "x".to_owned(), data_type: "int".to_owned(), nullable: false },
            ],
            rows: vec![vec![UiCell::Number("1".to_owned()), UiCell::Number("2".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };
        let json = format_result_json(&result);
        assert!(json.contains("\"x\": 1"), "{json}");
        assert!(json.contains("\"x_2\": 2"), "{json}");
    }

    // #429: cells that Excel/LibreOffice would evaluate as formulas must be emitted as
    // `'`-prefixed literal text instead.
    #[test]
    fn delimited_export_escapes_formula_leading_cells() {
        assert_eq!(
            escape_delimited_field("=HYPERLINK(\"http://evil\",\"x\")", ','),
            "\"'=HYPERLINK(\"\"http://evil\"\",\"\"x\"\")\""
        );
        assert_eq!(escape_delimited_field("=1+1", ','), "'=1+1");
        assert_eq!(escape_delimited_field("+SUM(A1)", ','), "'+SUM(A1)");
        assert_eq!(escape_delimited_field("@SUM(1)", ','), "'@SUM(1)");
        assert_eq!(escape_delimited_field("\t=1", ','), "'\t=1");
        // A `-` value that parses as a number stays a number — otherwise every negative
        // export would silently become text.
        assert_eq!(escape_delimited_field("-42", ','), "-42");
        assert_eq!(escape_delimited_field("-1+1", ','), "'-1+1");
        assert_eq!(escape_delimited_field("plain", ','), "plain");
    }

    #[test]
    fn csv_header_and_number_cells_get_formula_guard() {
        let result = UiQueryResult {
            columns: vec![crate::UiColumn {
                name: "=evil".to_owned(),
                data_type: "text".to_owned(),
                nullable: false,
            }],
            rows: vec![vec![UiCell::Number("-1+1".to_owned())]],
            row_count: 1,
            duration_ms: 0,
        };
        assert_eq!(format_result_delimited(&result, ","), "'=evil\n'-1+1\n");
    }

    // #487: NULL must not collapse into the same field as `''` — the Postgres `\copy`
    // sentinel `\N` keeps the distinction through a delimited round-trip.
    #[test]
    fn delimited_cells_distinguish_null_from_empty_text() {
        assert_eq!(format_cell_delimited(&UiCell::Null, '\t'), "\\N");
        assert_eq!(format_cell_delimited(&UiCell::Text(String::new()), '\t'), "");
        assert_eq!(format_cell_delimited(&UiCell::Null, ','), "\\N");

        let result = UiQueryResult {
            columns: vec![
                crate::UiColumn { name: "a".to_owned(), data_type: "text".to_owned(), nullable: true },
                crate::UiColumn { name: "b".to_owned(), data_type: "text".to_owned(), nullable: true },
            ],
            rows: vec![vec![UiCell::Null, UiCell::Text(String::new())]],
            row_count: 1,
            duration_ms: 0,
        };
        assert_eq!(format_result_delimited(&result, "\t"), "a\tb\n\\N\t\n");
    }

    fn dialect_result() -> UiQueryResult {
        UiQueryResult {
            columns: vec![
                crate::UiColumn { name: "id".to_owned(), data_type: "int".to_owned(), nullable: false },
                crate::UiColumn { name: "path".to_owned(), data_type: "text".to_owned(), nullable: true },
                crate::UiColumn { name: "ok".to_owned(), data_type: "bool".to_owned(), nullable: true },
            ],
            rows: vec![vec![
                UiCell::Number("1".to_owned()),
                UiCell::Text("C:\\temp\\O'Brien".to_owned()),
                UiCell::Boolean(true),
            ]],
            row_count: 1,
            duration_ms: 0,
        }
    }

    // #538: SQL export must quote identifiers and escape literals for the connection's driver,
    // not always emit PostgreSQL syntax.
    #[test]
    fn sql_insert_export_uses_mysql_quoting_and_backslash_escaping() {
        let insert = format_result_sql_insert(&dialect_result(), "people", "mysql");

        assert!(insert.contains("INSERT INTO `people` (`id`, `path`, `ok`)"), "{insert}");
        assert!(insert.contains("'C:\\\\temp\\\\O''Brien'"), "{insert}");
        assert!(insert.contains("TRUE"), "{insert}");
        assert!(!insert.contains('"'), "MySQL must not receive ANSI-quoted identifiers: {insert}");
    }

    #[test]
    fn sql_insert_export_uses_sql_server_quoting_and_bit_literals() {
        let insert = format_result_sql_insert(&dialect_result(), "people", "sql server");

        assert!(insert.contains("INSERT INTO [people] ([id], [path], [ok])"), "{insert}");
        // Backslashes are literal in T-SQL; only the quote is doubled.
        assert!(insert.contains("'C:\\temp\\O''Brien'"), "{insert}");
        assert!(insert.contains(", 1)"), "SQL Server has no TRUE literal: {insert}");
    }

    #[test]
    fn sql_insert_export_keeps_ansi_quoting_for_postgres_and_sqlite() {
        for driver in ["postgres", "sqlite"] {
            let insert = format_result_sql_insert(&dialect_result(), "people", driver);

            assert!(insert.contains("INSERT INTO \"people\" (\"id\", \"path\", \"ok\")"), "{driver}: {insert}");
            assert!(insert.contains("'C:\\temp\\O''Brien'"), "{driver}: {insert}");
            assert!(insert.contains("TRUE"), "{driver}: {insert}");
        }
    }

    #[test]
    fn sql_copy_export_quotes_for_postgres() {
        let copy = format_result_copy(&dialect_result(), "people", "postgres");

        assert!(copy.starts_with("COPY \"people\" (\"id\", \"path\", \"ok\") FROM stdin;"), "{copy}");
        assert!(copy.ends_with("\\.\n"), "{copy}");
    }

    // #590: `NaN`/`Infinity` parse as `f64` but are not bare SQL literals.
    #[test]
    fn copy_as_sql_quotes_non_finite_numbers_590() {
        for value in ["NaN", "inf", "-inf", "Infinity", "-Infinity"] {
            assert_eq!(
                cell_sql_literal(&UiCell::Number(value.to_owned())),
                format!("'{value}'"),
                "{value} must be quoted"
            );
        }
        for value in ["0", "-1.5", "1e9", "1000000000000000000"] {
            assert_eq!(
                cell_sql_literal(&UiCell::Number(value.to_owned())),
                value,
                "{value} stays a numeric literal"
            );
        }
    }

    // #593: the clipboard path is raw — phone numbers, indented strings, bare
    // `-` and `@user` must not gain a `'`, while file exports stay Excel-safe.
    #[test]
    fn clipboard_csv_is_raw_while_file_export_stays_excel_safe_593() {
        for value in ["+84 912 345 678", " leading space", "-", "@user", "-abc"] {
            assert_eq!(
                format_cell_csv(&UiCell::Text(value.to_owned())),
                value,
                "{value:?} must round-trip through the clipboard"
            );
            assert_eq!(
                escape_delimited_field_with(value, ',', db_pro_core::application::DelimitedSafety::Raw),
                value,
                "{value:?}"
            );
        }
        assert_eq!(
            format_cell_csv(&UiCell::Text("=HYPERLINK(\"http://evil\")".to_owned())),
            "=HYPERLINK(\"http://evil\")"
        );
        // File exports still neutralise real formulas.
        assert_eq!(
            escape_delimited_field("=1+1", ','),
            "'=1+1",
            "Excel-safe default is unchanged"
        );
    }
