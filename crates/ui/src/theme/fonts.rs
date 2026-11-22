//! Font discovery and glyph-coverage checks for the native shell.
//!
//! Two audit findings drive this module:
//!
//! * **#567** — the bundled Inter files are subsets that miss most Vietnamese
//!   letters (the whole `Latin Extended Additional` block, U+1EA0–U+1EF9), and
//!   the system font fallback used a handful of hard-coded paths that do not
//!   exist on Fedora, Arch, NixOS or inside Flatpak. Vietnamese text therefore
//!   rendered as tofu unless a font happened to sit at one of those paths.
//! * **#584** — `install_fonts` read *every* CJK face it could find (tens of MB
//!   on Windows, several faces on macOS), synchronously on the main thread
//!   before the first frame, and kept all of them in RAM.
//!
//! The fix in both cases is the same primitive: before a font is accepted as a
//! fallback, its `cmap` table is parsed and the code points the script actually
//! needs are checked. A face that cannot draw `ạ` is not a Vietnamese fallback,
//! so the search continues; and once a face covers a script, no further face for
//! that script is loaded at all.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Enough coverage probes to reject a subset font but stay cheap: `ạ ệ ỡ ơ ư`.
pub(crate) const VIETNAMESE_PROBE: &[u32] = &[0x1EA1, 0x1EC7, 0x1EDF, 0x01A1, 0x01B0];
/// `漢 中 国`.
pub(crate) const HAN_PROBE: &[u32] = &[0x6F22, 0x4E2D, 0x56FD];
/// `あ ア` — hiragana and katakana.
pub(crate) const KANA_PROBE: &[u32] = &[0x3042, 0x30A2];
/// `가 한`.
pub(crate) const HANGUL_PROBE: &[u32] = &[0xAC00, 0xD55C];

/// A font file that was read into memory and accepted for a fallback role.
pub(crate) struct FontFace {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl FontFace {
    /// A stable egui font-data key derived from the file name, so two faces that
    /// happen to share a stem still get distinct keys.
    pub fn key(&self, role: &str) -> String {
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| "font".to_owned());
        let stem: String = stem
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
            .collect();
        format!("{role}_{stem}")
    }
}

/// Candidate UI sans faces, in preference order.
///
/// The list covers the paths used by Debian/Ubuntu, Fedora (where DejaVu lives
/// under `/usr/share/fonts/dejavu-sans-fonts`), Arch (`/usr/share/fonts/TTF`),
/// openSUSE, Alpine, NixOS, Flatpak runtimes and the three desktop platforms.
/// Each candidate is only accepted when it actually covers Vietnamese, so a path
/// that exists but ships a subset font is skipped instead of producing tofu.
pub(crate) const SANS_CANDIDATES: &[&str] = &[
    // Debian / Ubuntu
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    // Fedora / RHEL
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
    "/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
    // Arch / Manjaro / openSUSE
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/NotoSans-Regular.ttf",
    // Alpine / musl
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    // NixOS and Flatpak runtimes
    "/run/current-system/sw/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/run/current-system/sw/share/fonts/noto/NotoSans-Regular.ttf",
    "/run/host/fonts/DejaVuSans.ttf",
    "/run/host/fonts/NotoSans-Regular.ttf",
    // macOS
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/SFNS.ttf",
    "/System/Library/Fonts/SFCompact.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    // Windows
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\tahoma.ttf",
];

/// Candidate CJK faces, grouped by the script they are asked to cover.
///
/// A single Noto Sans CJK / PingFang / MS Gothic face covers Han, kana *and*
/// hangul, so the loader takes one face per script and stops as soon as the
/// scripts are covered — see [`load_cjk_faces`].
pub(crate) const CJK_CANDIDATES: &[&str] = &[
    // Linux — one Noto CJK face covers Han + kana + hangul
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/run/current-system/sw/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    // Windows
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\malgun.ttf",
    "C:\\Windows\\Fonts\\yugothr.ttc",
];

/// Candidate monospace faces, in preference order.
pub(crate) const MONO_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation-mono/LiberationMono-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
    "/run/current-system/sw/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Monaco.ttf",
    "C:\\Windows\\Fonts\\cascadiacode.ttf",
    "C:\\Windows\\Fonts\\consola.ttf",
];

/// Read a font file and keep it only when it covers every probe code point.
///
/// Reading is done through a `Read + Seek` handle so the `cmap` table can be
/// inspected without keeping a face that will be rejected.
pub(crate) fn load_covering(path: &Path, probe: &[u32]) -> Option<FontFace> {
    if !covers_file(path, probe) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(FontFace {
        path: path.to_path_buf(),
        bytes,
    })
}

/// First candidate that exists *and* covers `probe`.
pub(crate) fn first_covering(candidates: &[&str], probe: &[u32]) -> Option<FontFace> {
    candidates
        .iter()
        .find_map(|candidate| load_covering(Path::new(candidate), probe))
}

/// First candidate that exists, regardless of coverage.
///
/// Used only as a last resort: a monospace face that misses a few Vietnamese
/// probes still beats dropping the monospace family entirely (which would make
/// the SQL editor render in Inter's proportional metrics). The glyph itself then
/// comes from the UI fallback face.
pub(crate) fn first_existing(candidates: &[&str]) -> Option<FontFace> {
    candidates.iter().find_map(|candidate| {
        let path = Path::new(candidate);
        let bytes = std::fs::read(path).ok()?;
        Some(FontFace {
            path: path.to_path_buf(),
            bytes,
        })
    })
}

/// One face per CJK script, never reading the same file twice.
///
/// #584: this used to load every CJK face found and push each into all three
/// families. Han, kana and hangul are now treated as separate requirements, and
/// the loop stops as soon as all three are satisfied — which on every desktop
/// platform happens after a single Noto CJK / PingFang / MS Gothic face.
pub(crate) fn load_cjk_faces(candidates: &[&str]) -> Vec<FontFace> {
    let mut wanted: Vec<(&'static str, &[u32])> = vec![
        ("han", HAN_PROBE),
        ("kana", KANA_PROBE),
        ("hangul", HANGUL_PROBE),
    ];
    let mut faces: Vec<FontFace> = Vec::new();
    for candidate in candidates {
        if wanted.is_empty() {
            break;
        }
        let path = Path::new(candidate);
        if let Some(already) = faces.iter().position(|face| face.path == path) {
            // Same file reached through two candidate spellings: probe the bytes
            // we already have instead of reading tens of MB again.
            wanted.retain(|(_, probe)| !covers(&faces[already].bytes, probe));
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut covers_any = false;
        wanted.retain(|(_, probe)| {
            if covers(&bytes, probe) {
                covers_any = true;
                false
            } else {
                true
            }
        });
        if covers_any {
            faces.push(FontFace {
                path: path.to_path_buf(),
                bytes,
            });
        }
    }
    faces
}

/// Whether the font at `path` covers every `probe` code point.
///
/// Only the `cmap` table is read, so a rejected face costs a header read instead
/// of the tens of MB its glyph data would need (#584).
pub(crate) fn covers_file(path: &Path, probe: &[u32]) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|meta| meta.len()) else {
        return false;
    };
    // A subset font is still tens of KB; anything smaller is not worth parsing.
    if len < 1_000 {
        return false;
    }
    let Some((cmap_offset, cmap_length)) = cmap_location(&mut file) else {
        return false;
    };
    if cmap_length == 0 || cmap_length > MAX_CMAP {
        return false;
    }
    let mut cmap = vec![0u8; cmap_length as usize];
    if file.seek(SeekFrom::Start(cmap_offset)).is_err() || file.read_exact(&mut cmap).is_err() {
        return false;
    }
    covers_cmap(&cmap, probe)
}

/// Offset and length of the `cmap` table record, for a single face or the first
/// face of a TrueType collection (egui only ever uses face 0).
fn cmap_location<R: Read + Seek>(reader: &mut R) -> Option<(u64, u32)> {
    let (base, table_count) = table_directory(reader)?;
    let mut records = vec![0u8; table_count * 16];
    reader.read_exact(&mut records).ok()?;
    records.chunks_exact(16).find_map(|record| {
        if &record[0..4] != b"cmap" {
            return None;
        }
        let offset = read_u32(record, 8)? as u64;
        let length = read_u32(record, 12)?;
        Some((base + offset, length))
    })
}

/// Byte offset of the table directory and its record count.
fn table_directory<R: Read + Seek>(reader: &mut R) -> Option<(u64, usize)> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut header = [0u8; 12];
    reader.read_exact(&mut header).ok()?;
    if &header[0..4] != b"ttcf" {
        return some_table_count(&header, 0, 4);
    }
    // Collection header: `ttcf`, version, numFonts, then one offset per face.
    // Only face 0 is probed because egui only ever uses face 0.
    let mut offset_bytes = [0u8; 4];
    reader.seek(SeekFrom::Start(12)).ok()?;
    reader.read_exact(&mut offset_bytes).ok()?;
    let base = read_u32(&offset_bytes, 0)? as u64;
    reader.seek(SeekFrom::Start(base)).ok()?;
    let mut face_header = [0u8; 12];
    reader.read_exact(&mut face_header).ok()?;
    some_table_count(&face_header, base, 4)
}

fn some_table_count(header: &[u8; 12], base: u64, count_offset: usize) -> Option<(u64, usize)> {
    let table_count = read_u16(header, count_offset)? as usize;
    if table_count == 0 || table_count > MAX_TABLES {
        return None;
    }
    Some((base, table_count))
}

/// Upper bound on the `cmap` table size we are willing to parse.
const MAX_CMAP: u32 = 1 << 20;
/// Upper bound on the table-directory record count; real fonts stay far below.
const MAX_TABLES: usize = 512;

/// `true` when every `probe` code point has a glyph in the parsed `cmap` table.
pub(crate) fn covers(bytes: &[u8], probe: &[u32]) -> bool {
    let mut cursor = std::io::Cursor::new(bytes);
    let Some((cmap_offset, cmap_length)) = cmap_location(&mut cursor) else {
        return false;
    };
    let start = cmap_offset as usize;
    let end = start.saturating_add(cmap_length as usize).min(bytes.len());
    if start >= end {
        return false;
    }
    covers_cmap(&bytes[start..end], probe)
}

/// Scan the encoding records of a `cmap` table (subtables of format 4, 6 and 12).
fn covers_cmap(cmap: &[u8], probe: &[u32]) -> bool {
    if cmap.len() < 4 {
        return false;
    }
    let table_count = read_u16(cmap, 2).unwrap_or(0) as usize;
    if table_count == 0 {
        return false;
    }
    // All subtables are tried: a face can carry a BMP-only subtable plus a
    // full-repertoire format 12 subtable, and a probe code point may live in
    // either.
    let mut subtables: Vec<usize> = Vec::new();
    for index in 0..table_count {
        let record = 4 + index * 8;
        if record + 8 > cmap.len() {
            break;
        }
        let offset = read_u32(cmap, record + 4).unwrap_or(0) as usize;
        if offset + 2 <= cmap.len() && !subtables.contains(&offset) {
            subtables.push(offset);
        }
    }
    probe.iter().all(|code_point| {
        subtables
            .iter()
            .any(|offset| subtable_has_glyph(cmap, *offset, *code_point))
    })
}

fn subtable_has_glyph(cmap: &[u8], offset: usize, code_point: u32) -> bool {
    let Some(format) = read_u16(cmap, offset) else {
        return false;
    };
    match format {
        4 => format4_has_glyph(cmap, offset, code_point),
        6 => format6_has_glyph(cmap, offset, code_point),
        12 => format12_has_glyph(cmap, offset, code_point),
        _ => false,
    }
}

fn format4_has_glyph(cmap: &[u8], offset: usize, code_point: u32) -> bool {
    if code_point > 0xFFFF {
        return false;
    }
    let Some(seg_count_x2) = read_u16(cmap, offset + 6) else {
        return false;
    };
    let seg_count = seg_count_x2 as usize / 2;
    if seg_count == 0 {
        return false;
    }
    let end_codes = offset + 14;
    let start_codes = end_codes + seg_count * 2 + 2; // + reservedPad
    for segment in 0..seg_count {
        let Some(end) = read_u16(cmap, end_codes + segment * 2) else {
            return false;
        };
        if (code_point as u16) > end {
            continue;
        }
        let Some(start) = read_u16(cmap, start_codes + segment * 2) else {
            return false;
        };
        if (code_point as u16) < start {
            return false;
        }
        let delta_position = start_codes + seg_count * 2 + segment * 2;
        let range_position = delta_position + seg_count * 2;
        let Some(delta) = read_i16(cmap, delta_position) else {
            return false;
        };
        let Some(range_offset) = read_u16(cmap, range_position) else {
            return false;
        };
        if range_offset == 0 {
            // Glyph id 0 is `.notdef`: the mapping exists but draws no glyph.
            return (code_point as i32 + delta as i32) != 0;
        }
        let glyph_position = range_position + segment * 2 + range_offset as usize;
        let glyph_offset = glyph_position + (code_point as usize - start as usize) * 2;
        let Some(glyph) = read_u16(cmap, glyph_offset) else {
            return false;
        };
        if glyph == 0 {
            return false;
        }
        return glyph.wrapping_add(delta as u16) != 0;
    }
    false
}

fn format6_has_glyph(cmap: &[u8], offset: usize, code_point: u32) -> bool {
    let Some(first) = read_u16(cmap, offset + 6) else {
        return false;
    };
    let Some(count) = read_u16(cmap, offset + 8) else {
        return false;
    };
    let first = first as u32;
    if code_point < first || code_point >= first + count as u32 {
        return false;
    }
    let index = (code_point - first) as usize;
    read_u16(cmap, offset + 10 + index * 2).is_some_and(|glyph| glyph != 0)
}

fn format12_has_glyph(cmap: &[u8], offset: usize, code_point: u32) -> bool {
    let Some(group_count) = read_u32(cmap, offset + 12) else {
        return false;
    };
    if group_count == 0 || group_count > 65_536 {
        return false;
    }
    let mut low = 0u32;
    let mut high = group_count - 1;
    // Groups are sorted by `startCharCode`, so binary search the range.
    while low <= high {
        let middle = low + (high - low) / 2;
        let group = offset + 16 + middle as usize * 12;
        let Some(start) = read_u32(cmap, group) else {
            return false;
        };
        let Some(end) = read_u32(cmap, group + 4) else {
            return false;
        };
        if code_point < start {
            if middle == 0 {
                return false;
            }
            high = middle - 1;
        } else if code_point > end {
            low = middle + 1;
        } else {
            return true;
        }
    }
    false
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(read_u16(bytes, offset)? as i16)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
#[path = "tests/fonts_tests.rs"]
mod tests;
