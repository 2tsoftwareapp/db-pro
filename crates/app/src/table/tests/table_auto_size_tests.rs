//! Tests for the column auto-size helpers in `../table_data_state.rs`.

use super::{measured_text_width, AUTO_SIZE_UNIT_WIDTH};

#[test]
fn ascii_measurement_is_proportional_to_length() {
    let short = measured_text_width("abc");
    let long = measured_text_width("abcdef");
    assert!((long - short * 2.0).abs() < 0.01);
    assert!((measured_text_width("a") - AUTO_SIZE_UNIT_WIDTH).abs() < 0.01);
}

#[test]
fn narrow_and_wide_ascii_differ() {
    // #575: `chars * 7` treated every glyph as identical width.
    assert!(measured_text_width("iiii") < measured_text_width("mmmm"));
    assert!(measured_text_width("WWWW") > measured_text_width("llll"));
}

#[test]
fn cjk_glyphs_measure_at_two_cells() {
    // A Chinese column name used to be sized as if every glyph were Latin.
    let cjk = measured_text_width("数据库");
    let latin = measured_text_width("abcd");
    assert!(cjk > latin * 1.8, "CJK {cjk} should be ~2x Latin {latin}");
}

#[test]
fn vietnamese_diacritics_do_not_collapse_the_estimate() {
    // `chars().count()` counts precomposed Vietnamese letters as one char, and
    // decomposed sequences must not be double counted by combining marks.
    let precomposed = measured_text_width("tiếng Việt");
    let decomposed = measured_text_width("tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t");
    assert!(precomposed > 0.0);
    assert!(
        (precomposed - decomposed).abs() < auto_unit_tolerance(),
        "precomposed {precomposed} vs decomposed {decomposed}"
    );
}

fn auto_unit_tolerance() -> f32 {
    // One combining mark cluster may shift the estimate by less than a whole cell.
    AUTO_SIZE_UNIT_WIDTH * 2.5
}

#[test]
fn empty_text_has_zero_width() {
    assert_eq!(measured_text_width(""), 0.0);
}
