//! Tests for `../query_helpers.rs` (moved out of the source file).
    use super::format_query_document;
    use crate::editor::{SelectionRange, SqlDialect};
    use crate::query::QueryDocument;

    #[test]
    fn format_query_without_selection_formats_the_whole_document() {
        let sql = "select id, full_name from shop.customers where note = 'x' order by 1";
        let mut doc = QueryDocument::new("q", "Query", sql);
        doc.selection = SelectionRange::point(0);
        let changed = format_query_document(&mut doc, SqlDialect::Postgres);
        assert!(changed, "empty selection must format instead of silently doing nothing");
        assert!(
            doc.text().contains("FROM"),
            "whole document is formatted, not just a selection: {}",
            doc.text()
        );
        assert_ne!(doc.text(), sql);
    }

    #[test]
    fn format_query_without_selection_keeps_caret_on_its_line() {
        let sql = "select id from shop.customers where id = 1";
        let mut doc = QueryDocument::new("q", "Query", sql);
        let caret = sql.find("where").expect("where keyword");
        doc.selection = SelectionRange::point(caret);
        doc.cursor.set_offset(&doc.buffer, caret);
        let changed = format_query_document(&mut doc, SqlDialect::Postgres);
        assert!(changed);
        let (line, _) = doc.buffer.offset_to_line_col(doc.cursor.offset);
        assert_eq!(line, 2, "caret lands on the WHERE line of the formatted document");
    }

    #[test]
    fn format_query_without_selection_reports_no_change_when_clean() {
        let sql = "SELECT 1\nFROM t";
        let mut doc = QueryDocument::new("q", "Query", sql);
        doc.selection = SelectionRange::point(0);
        doc.cursor.set_offset(&doc.buffer, 0);
        let changed = format_query_document(&mut doc, SqlDialect::Postgres);
        assert!(!changed, "an already well-formed document reports no change");
        assert_eq!(doc.text(), sql);
    }

    #[test]
    fn format_query_selection_does_not_reformat_the_rest_of_the_buffer() {
        let sql = "SELECT 1 FROM t; SELECT 2 FROM u";
        let mut doc = QueryDocument::new("q", "Query", sql);
        let start = sql.find("SELECT 2").expect("second statement");
        doc.selection = SelectionRange::new(start, sql.len());
        format_query_document(&mut doc, SqlDialect::Postgres);
        assert!(
            doc.text().starts_with("SELECT 1 FROM t; "),
            "text before the selection stays put: {}",
            doc.text()
        );
        assert!(
            doc.text().ends_with("SELECT 2\nFROM u"),
            "only the selection is formatted: {}",
            doc.text()
        );
    }