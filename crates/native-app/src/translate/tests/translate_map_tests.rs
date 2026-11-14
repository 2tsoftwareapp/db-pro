//! Tests for `../translate_map.rs` (moved out of the source file).
use super::*;
use db_pro_core::domain::query::CellValue;

// #537: a value the grid cannot hold in full must be recognisable as truncated;
// otherwise the editor stages the display prefix back over the real value.
#[test]
fn oversized_text_is_marked_as_truncated() {
    let mapped = map_cell(CellValue::Text("x".repeat(70_000)));

    let UiCell::Text(value) = &mapped else {
        panic!("text maps to UiCell::Text");
    };
    assert_eq!(value.len(), MAX_CELL_TEXT_BYTES + '…'.len_utf8());
    assert!(value.ends_with('…'));
    assert!(mapped.is_truncated());
}

#[test]
fn text_at_the_cap_is_not_marked_truncated() {
    let mapped = map_cell(CellValue::Text("x".repeat(MAX_CELL_TEXT_BYTES)));

    assert!(matches!(&mapped, UiCell::Text(value) if value.len() == MAX_CELL_TEXT_BYTES));
    assert!(!mapped.is_truncated());
}

#[test]
fn oversized_json_is_marked_as_truncated() {
    let json = format!("\"{}\"", "x".repeat(70_000));
    let mapped = map_cell(CellValue::Json(serde_json::from_str(&json).expect("valid JSON")));

    assert!(matches!(mapped, UiCell::Json(_)));
    assert!(mapped.is_truncated());
}

#[test]
fn oversized_bytes_are_marked_as_truncated() {
    let mapped = map_cell(CellValue::Bytes(vec![0x41; MAX_CELL_TEXT_BYTES / 2 + 1]));
    assert!(matches!(mapped, UiCell::Bytes(_)));
    assert!(mapped.is_truncated());

    let full = map_cell(CellValue::Bytes(vec![0x41; MAX_CELL_TEXT_BYTES / 2]));
    assert!(!full.is_truncated(), "a bytea value at the cap is not truncated");
}

/// #587: a declared text-ish column must keep its bytes, even when the content
/// looks like a date/number/UUID.
#[test]
fn text_columns_are_never_retyped_by_content_587() {
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Text("2024-01-02".to_owned()), "varchar(20)"),
        Some(CellValue::Text("2024-01-02".to_owned()))
    );
    assert_eq!(
        ui_cell_to_domain_typed(
            UiCell::Text("2f1c0a52-3d8f-4c1a-9a2f-2f9d0c7b1e11".to_owned()),
            "TEXT"
        ),
        Some(CellValue::Text("2f1c0a52-3d8f-4c1a-9a2f-2f9d0c7b1e11".to_owned()))
    );
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Number("42".to_owned()), "citext"),
        Some(CellValue::Text("42".to_owned()))
    );
    // Non-text columns keep their declared-type handling.
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Text("2024-01-02".to_owned()), "date"),
        Some(CellValue::Date("2024-01-02".to_owned()))
    );
}

/// #592: JSON columns keep their exact document text — no rounding, no key
/// reordering — so editing one key cannot silently rewrite the others.
#[test]
fn json_columns_round_trip_verbatim_592() {
    let document = r#"{"b":1,"a":0.1234567890123456789,"big":123456789012345678901234}"#;
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Text(document.to_owned()), "jsonb"),
        Some(CellValue::Text(document.to_owned()))
    );
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Json(document.to_owned()), "json"),
        Some(CellValue::Text(document.to_owned()))
    );
    assert_eq!(
        ui_cell_to_domain_typed(UiCell::Null, "jsonb"),
        Some(CellValue::Null)
    );
}
