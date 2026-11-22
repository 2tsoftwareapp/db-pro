//! Tests for `../format.rs` (moved out of the source file).
use super::*;

#[test]
fn truncate_ellipsis_preserves_unicode_boundaries() {
    assert_eq!(truncate_ellipsis("short", 10), "short");
    assert_eq!(truncate_ellipsis("hello world", 5), "hell…");
    assert_eq!(truncate_ellipsis("tiếng việt unicode", 6), "tiếng…");
    assert_eq!(truncate_ellipsis("", 5), "");
}

#[test]
fn format_bytes_uses_human_readable_units() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1024 * 1024 * 5), "5.0 MB");
}

#[test]
fn format_duration_millis_uses_compact_units() {
    assert_eq!(format_duration_millis(45), "45ms");
    assert_eq!(format_duration_millis(1500), "1.50s");
    assert_eq!(format_duration_millis(65000), "1m 5s");
}

#[test]
fn format_count_with_suffix_uses_plural_for_non_one_counts() {
    assert_eq!(format_count_with_suffix(1, "row", "rows"), "1 row");
    assert_eq!(format_count_with_suffix(0, "row", "rows"), "0 rows");
    assert_eq!(format_count_with_suffix(5, "table", "tables"), "5 tables");
}

#[test]
fn format_page_range_covers_known_and_unknown_totals() {
    assert_eq!(format_page_range(0, 50, Some(200)), "Rows 1–50 of 200");
    assert_eq!(format_page_range(50, 50, Some(200)), "Rows 51–100 of 200");
    assert_eq!(format_page_range(180, 50, Some(200)), "Rows 181–200 of 200");
    assert_eq!(format_page_range(0, 25, None), "Rows 1–25");
    assert_eq!(format_page_range(0, 0, Some(0)), "0 rows");
}

#[test]
fn format_percentage_handles_zero_denominator() {
    assert_eq!(format_percentage(985, 1000), "98.5%");
    assert_eq!(format_percentage(1, 1), "100.0%");
    assert_eq!(format_percentage(0, 0), "0.0%");
}

#[test]
fn fmt_opt_hides_rust_debug_syntax() {
    // #569: the UI must never show `Some(…)` / `None`.
    assert_eq!(fmt_opt(Some(5321)), "5321");
    assert_eq!(fmt_opt(None::<u64>), VALUE_ABSENT);
    assert_eq!(fmt_opt(Some("logical")), "logical");
    assert!(!fmt_opt(None::<u64>).contains("None"));
    assert!(!fmt_opt(Some(1)).contains("Some"));
}

#[test]
fn fmt_opt_duration_millis_scales_units() {
    assert_eq!(fmt_opt_duration_millis(None), VALUE_ABSENT);
    assert_eq!(fmt_opt_duration_millis(Some(812)), "812ms");
    assert_eq!(fmt_opt_duration_millis(Some(5_320)), "5.32s");
    assert_eq!(fmt_opt_duration_millis(Some(134_000)), "2m 14s");
}

#[test]
fn format_shown_total_reports_truncation_only_when_capped() {
    assert_eq!(format_shown_total(20, 137), "Showing 20 of 137");
    assert_eq!(format_shown_total(7, 7), "7");
    assert_eq!(format_shown_total(0, 0), "0");
}
