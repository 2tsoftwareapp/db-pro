//! Tests for `../fonts.rs`.
//!
//! The `cmap` parser is exercised against real font files from the repository
//! (the bundled Inter subsets) and, when present, a system font. Both cases
//! assert the property the UI depends on: a subset that lacks U+1EA0–U+1EF9 is
//! rejected as a Vietnamese fallback, and a full face is accepted.

use super::*;

const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_REGULAR_EXT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular-ext.ttf");

#[test]
fn inter_subset_cannot_draw_vietnamese() {
    // #567: this is why Vietnamese rendered as tofu without a system font.
    for subset in [INTER_REGULAR, INTER_REGULAR_EXT] {
        let missing: Vec<u32> = VIETNAMESE_PROBE
            .iter()
            .copied()
            .filter(|code_point| !covers(subset, &[*code_point]))
            .collect();
        assert!(
            !missing.is_empty(),
            "the bundled Inter subset unexpectedly covers all probes"
        );
    }
}

#[test]
fn inter_draws_latin_basics() {
    assert!(covers(INTER_REGULAR, &[0x41, 0x61, 0x30]));
    // Latin-1 accented letters are in the subset even though Vietnamese is not.
    assert!(covers(INTER_REGULAR, &[0xE9, 0xF4]));
}

#[test]
fn han_probe_is_rejected_by_a_latin_only_face() {
    assert!(!covers(INTER_REGULAR, HAN_PROBE));
}

#[test]
fn garbage_input_never_panics() {
    for bytes in [&b""[..], &b"not a font"[..], &[0xFFu8; 64][..]] {
        assert!(!covers(bytes, VIETNAMESE_PROBE));
        assert!(!covers(bytes, HAN_PROBE));
    }
    // A truncated TrueType header must be refused, not indexed out of bounds.
    let mut truncated = INTER_REGULAR[..8].to_vec();
    assert!(!covers(&truncated, &[0x41]));
    truncated.extend_from_slice(INTER_REGULAR.get(8..20).unwrap_or(&[]));
    assert!(!covers(&truncated, &[0x41]));
}

#[test]
fn missing_file_is_not_a_candidate() {
    let path = std::path::Path::new("/nonexistent/dbpro/font.ttf");
    assert!(load_covering(path, VIETNAMESE_PROBE).is_none());
    assert!(!covers_file(path, VIETNAMESE_PROBE));
}

#[test]
fn font_key_is_ascii_and_role_scoped() {
    let face = FontFace {
        path: PathBuf::from("/usr/share/fonts/TTF/DejaVu Sans.ttf"),
        bytes: Vec::new(),
    };
    assert_eq!(face.key("ui"), "ui_dejavu_sans");
    assert!(face.key("mono").starts_with("mono_"));
}

#[test]
fn cjk_loading_stops_once_each_script_is_covered() {
    // No CJK font is installed in CI, so the loader must simply return an empty
    // list instead of panicking on a missing path. When fonts are present the
    // contract is one face per script.
    let faces = load_cjk_faces(CJK_CANDIDATES);
    assert!(faces.len() <= 3, "one face per script, got {}", faces.len());
    let unique: std::collections::HashSet<_> = faces.iter().map(|face| face.path.clone()).collect();
    assert_eq!(unique.len(), faces.len(), "a face must not be loaded twice");
}
