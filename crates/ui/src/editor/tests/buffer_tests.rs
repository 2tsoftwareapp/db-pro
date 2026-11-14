//! Tests for `../buffer.rs` (moved out of the source file).
use super::*;

#[test]
fn test_text_buffer_insert_delete_and_lines() {
    let mut buf = TextBuffer::from_string("SELECT * FROM users;\nSELECT * FROM orders;");
    assert_eq!(buf.line_count(), 2);
    assert_eq!(buf.line_at(0), Some("SELECT * FROM users;"));
    assert_eq!(buf.line_at(1), Some("SELECT * FROM orders;"));

    buf.insert(0, "-- Comment\n");
    assert_eq!(buf.line_count(), 3);
    assert_eq!(buf.line_at(0), Some("-- Comment"));
    assert_eq!(buf.line_at(1), Some("SELECT * FROM users;"));

    let (line, col) = buf.offset_to_line_col(11);
    assert_eq!(line, 1);
    assert_eq!(col, 0);

    let off = buf.line_col_to_offset(1, 7);
    assert_eq!(off, 18);
    assert_eq!(buf.slice(off, off + 6), "* FROM");
}

#[test]
fn test_text_buffer_undo_redo_snapshots() {
    let mut buf = TextBuffer::new();
    buf.insert(0, "SELECT 1;");
    assert_eq!(buf.text(), "SELECT 1;");

    buf.insert(9, "\nSELECT 2;");
    assert_eq!(buf.text(), "SELECT 1;\nSELECT 2;");

    let undo_res = buf.undo();
    assert_eq!(undo_res, Some((9, 9)));
    assert_eq!(buf.text(), "SELECT 1;");

    let redo_res = buf.redo();
    assert_eq!(redo_res, Some((19, 19)));
    assert_eq!(buf.text(), "SELECT 1;\nSELECT 2;");
}

#[test]
fn replacement_is_one_undo_step_and_restores_original_text() {
    let mut buf = TextBuffer::from_string("SELECT cust");
    buf.replace(7, 11, "customer");
    assert_eq!(buf.text(), "SELECT customer");
    assert_eq!(buf.undo(), Some((11, 7)));
    assert_eq!(buf.text(), "SELECT cust");
}

#[test]
fn test_text_buffer_typing_undo_grouping() {
    let mut buf = TextBuffer::new();
    // Type characters 'S', 'E', 'L' consecutively
    buf.type_text(
        0,
        "S",
        EditorSnapshot {
            cursor_offset: 0,
            anchor_offset: 0,
        },
        EditorSnapshot {
            cursor_offset: 1,
            anchor_offset: 1,
        },
    );
    buf.type_text(
        1,
        "E",
        EditorSnapshot {
            cursor_offset: 1,
            anchor_offset: 1,
        },
        EditorSnapshot {
            cursor_offset: 2,
            anchor_offset: 2,
        },
    );
    buf.type_text(
        2,
        "L",
        EditorSnapshot {
            cursor_offset: 2,
            anchor_offset: 2,
        },
        EditorSnapshot {
            cursor_offset: 3,
            anchor_offset: 3,
        },
    );

    assert_eq!(buf.text(), "SEL");
    // A single undo should undo the grouped typing word back to empty!
    let undo_res = buf.undo();
    assert_eq!(undo_res, Some((0, 0)));
    assert_eq!(buf.text(), "");
}

#[test]
fn test_text_buffer_utf8_boundaries() {
    let buf = TextBuffer::from_string("SELECT 'Xin chào thế giới';");
    // "SELECT 'Xin ch" is 14 bytes (0..14). 'à' is 2 bytes at offset 14..16.
    let offset = 15;
    let floor = buf.floor_char_boundary(offset);
    assert_eq!(floor, 14);
    let ceil = buf.ceil_char_boundary(offset);
    assert_eq!(ceil, 16);
    assert_eq!(buf.char_at(floor), Some('à'));
    assert_eq!(buf.prev_char(ceil), Some('à'));
}

#[test]
fn grapheme_boundaries_span_zwj_emoji() {
    let emoji = "👩\u{200d}💻";
    let buf = TextBuffer::from_string(format!("{emoji}!"));
    assert_eq!(buf.next_grapheme_boundary(0), emoji.len());
    assert_eq!(buf.prev_grapheme_boundary(buf.len_bytes()), emoji.len());
    for off in (1..emoji.len()).filter(|&o| buf.text().is_char_boundary(o)) {
        assert_eq!(buf.next_grapheme_boundary(off), emoji.len(), "from {off}");
        assert_eq!(buf.prev_grapheme_boundary(off), 0, "from {off}");
    }
}

/// The incremental line index must agree with a full rebuild after every edit
/// shape: single-char insert, newline insert, mid-line delete, multi-line
/// replace, and undo (#580).
#[test]
fn incremental_line_index_matches_full_rebuild_580() {
    fn rebuild_reference(buf: &TextBuffer) -> (Vec<usize>, usize) {
        let mut starts = vec![0usize];
        for (idx, byte) in buf.text().as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                starts.push(idx + 1);
            }
        }
        let max = (0..starts.len())
            .map(|line| buf.line_at(line).unwrap_or("").chars().count())
            .max()
            .unwrap_or(0);
        (starts, max)
    }

    let mut buf = TextBuffer::from_string("SELECT 1;\nSELECT 2;\nSELECT 3;\n");
    let snap = |offset: usize| EditorSnapshot {
        cursor_offset: offset,
        anchor_offset: offset,
    };

    let cases: Vec<Box<dyn Fn(&mut TextBuffer)>> = vec![
        Box::new(|buf: &mut TextBuffer| buf.insert_with_snapshot(8, "x", snap(8), snap(9))),
        Box::new(|buf: &mut TextBuffer| buf.type_text(9, "\n", snap(9), snap(10))),
        Box::new(|buf: &mut TextBuffer| {
            buf.delete_with_snapshot(3, 5, snap(5), snap(3));
        }),
        Box::new(|buf: &mut TextBuffer| buf.replace(20, 28, "SELECT 9;\n")),
        Box::new(|buf: &mut TextBuffer| buf.insert_with_snapshot(0, "Xin chào\n", snap(0), snap(10))),
        Box::new(|buf: &mut TextBuffer| {
            buf.undo();
        }),
    ];

    for (index, edit) in cases.iter().enumerate() {
        edit(&mut buf);
        let reference = rebuild_reference(&buf);
        assert_eq!(buf.line_starts_for_test(), reference.0, "case {index}");
        assert_eq!(
            buf.max_line_len_chars(),
            reference.1,
            "case {index}"
        );
        assert_eq!(buf.line_count(), reference.0.len(), "case {index}");
    }
}

#[test]
fn typing_in_a_long_line_updates_max_line_chars_580() {
    let mut buf = TextBuffer::from_string("short\n");
    let snap = |offset: usize| EditorSnapshot {
        cursor_offset: offset,
        anchor_offset: offset,
    };
    for index in 0..40 {
        let offset = 5 + index;
        buf.type_text(offset, "a", snap(offset), snap(offset + 1));
    }
    assert_eq!(buf.max_line_len_chars(), 45);
    assert_eq!(buf.line_count(), 2);
    buf.delete_with_snapshot(5, 45, snap(45), snap(5));
    assert_eq!(buf.max_line_len_chars(), 7);
    assert_eq!(buf.line_count(), 2);
}
