//! Tests for `../explorer_schema_object_folders_view.rs` (moved out of the source file).
use super::trigger_toggle_menu_label;

#[test]
fn trigger_toggle_menu_offers_only_supported_inverse_action() {
    // #568: the labels come from the locale catalogue, not literals.
    assert_eq!(
        trigger_toggle_menu_label(true, true).as_deref(),
        Some(t!("explorer.disable_trigger").as_ref())
    );
    assert_eq!(
        trigger_toggle_menu_label(false, true).as_deref(),
        Some(t!("explorer.enable_trigger").as_ref())
    );
    assert_eq!(trigger_toggle_menu_label(true, false), None);
}
/// #583: the culling rule shared by Tables/Views/Functions/Triggers must keep
/// on-screen rows (including partially clipped ones) and drop the rest.
#[test]
fn explorer_row_culling_matches_the_clip_rect_583() {
    let clip = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(280.0, 400.0));
    assert!(!crate::app::explorer_row_is_offscreen(100.0, 26.0, clip));
    assert!(!crate::app::explorer_row_is_offscreen(380.0, 26.0, clip));
    assert!(!crate::app::explorer_row_is_offscreen(90.0, 26.0, clip), "partially visible above");
    assert!(crate::app::explorer_row_is_offscreen(0.0, 26.0, clip));
    assert!(crate::app::explorer_row_is_offscreen(420.0, 26.0, clip));
}
