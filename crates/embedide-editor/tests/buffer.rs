//! Contract checks independent of input handling and rendering.
use embedide_editor::{EditError, TextBuffer};
use std::io::{self, Write};

#[test]
fn replaces_across_original_and_inserted_pieces() {
    let mut buffer = TextBuffer::new("abcdef".into());
    buffer.replace(3..3, "XYZ").unwrap();
    buffer.replace(1..7, "!").unwrap();
    assert_eq!(buffer.contents(), "a!ef");
    assert_eq!(buffer.slice(1..3).unwrap(), "!e");
    assert_eq!(buffer.byte_len(), 4);
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "abcXYZdef");
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "a!ef");
}

#[test]
fn rejects_invalid_ranges_without_changing_text_or_history() {
    let mut buffer = TextBuffer::new("Aя🦀Z".into());
    let reversed_start = 4;
    for (range, expected) in [
        (2..3, EditError::InvalidBoundary),
        (1..4, EditError::InvalidBoundary),
        (9..9, EditError::InvalidRange),
        (reversed_start..1, EditError::InvalidRange),
    ] {
        assert_eq!(buffer.replace(range.clone(), "bad"), Err(expected));
        assert_eq!(buffer.slice(range), Err(expected));
        assert_eq!(buffer.contents(), "Aя🦀Z");
        assert!(!buffer.can_undo());
        assert!(!buffer.is_modified());
    }
    buffer.replace(1..7, "中").unwrap();
    assert_eq!(buffer.contents(), "A中Z");
}

#[test]
fn empty_buffer_and_deleting_everything_are_undoable() {
    let mut buffer = TextBuffer::new(String::new());
    assert!(buffer.is_empty());
    assert_eq!(buffer.chunks().count(), 0);
    assert_eq!(buffer.slice(0..0).unwrap(), "");
    buffer.replace(0..0, "hello").unwrap();
    buffer.replace(0..5, "").unwrap();
    assert!(buffer.is_empty());
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "hello");
    assert!(buffer.undo());
    assert!(buffer.is_empty());
    assert!(!buffer.is_modified());
    assert!(!buffer.undo());
    assert!(buffer.redo());
    assert!(buffer.redo());
    assert!(!buffer.redo());
}

#[test]
fn savepoint_survives_undo_and_redo_and_distinguishes_branches() {
    let mut buffer = TextBuffer::new("a".into());
    buffer.replace(1..1, "b").unwrap();
    buffer.mark_saved();
    assert!(!buffer.is_modified());
    buffer.replace(2..2, "c").unwrap();
    assert!(buffer.is_modified());
    assert!(buffer.undo());
    assert!(!buffer.is_modified());
    assert!(buffer.undo());
    assert!(buffer.is_modified());
    assert!(buffer.redo());
    assert!(!buffer.is_modified());
    assert!(buffer.undo());
    buffer.replace(1..1, "b").unwrap();
    assert_eq!(buffer.contents(), "ab");
    assert!(buffer.is_modified());
    assert!(!buffer.can_redo());
}

#[test]
fn empty_edit_keeps_redo_branch() {
    let mut buffer = TextBuffer::new("a".into());
    buffer.replace(1..1, "b").unwrap();
    buffer.undo();
    buffer.replace(0..0, "").unwrap();
    assert!(!buffer.is_modified());
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "ab");
}

#[test]
fn history_limit_evicts_old_transactions() {
    let mut buffer = TextBuffer::with_history_limit(String::new(), 2);
    for text in ["a", "b", "c"] {
        buffer
            .replace(buffer.byte_len()..buffer.byte_len(), text)
            .unwrap();
    }
    assert!(buffer.undo());
    assert!(buffer.undo());
    assert!(!buffer.undo());
    assert_eq!(buffer.contents(), "a");
    assert!(buffer.redo());
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "abc");
}

#[test]
fn history_can_be_disabled() {
    let mut buffer = TextBuffer::with_history_limit("a".into(), 0);
    buffer.replace(0..1, "b").unwrap();
    assert_eq!(buffer.contents(), "b");
    assert!(buffer.is_modified());
    assert!(!buffer.undo());
    assert!(!buffer.redo());
}

#[test]
fn preserves_bom_crlf_and_trailing_newline_during_streaming() {
    let mut buffer = TextBuffer::new("\u{feff}one\r\ntwo\r\n".into());
    buffer.replace(3..6, "три").unwrap();
    let mut bytes = Vec::new();
    buffer.write_to(&mut bytes).unwrap();
    assert_eq!(bytes, "\u{feff}три\r\ntwo\r\n".as_bytes());
    assert!(
        buffer.is_modified(),
        "streaming is not a successful file save"
    );
}

#[test]
fn writer_errors_are_propagated_without_marking_saved() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::WriteZero, "test sink"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = TextBuffer::new("a".into());
    buffer.replace(1..1, "b").unwrap();
    assert_eq!(
        buffer.write_to(&mut Broken).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert!(buffer.is_modified());
}

#[test]
fn adjacent_typing_coalesces_added_storage() {
    let mut buffer = TextBuffer::new("start".into());
    for _ in 0..100 {
        let end = buffer.byte_len();
        buffer.replace(end..end, "x").unwrap();
    }
    assert_eq!(buffer.chunks().count(), 2);
    assert_eq!(buffer.contents(), format!("start{}", "x".repeat(100)));
}

#[test]
fn deterministic_unicode_edits_match_string_and_restore_every_revision() {
    let mut buffer = TextBuffer::with_history_limit("initial\r\n".into(), 1024);
    let mut model = buffer.contents();
    let mut revisions = vec![model.clone()];
    let mut seed = 0x5eed_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as usize
    };
    for step in 0..1000 {
        let mut boundaries: Vec<_> = model.char_indices().map(|(i, _)| i).collect();
        boundaries.push(model.len());
        let a = boundaries[random() % boundaries.len()];
        let b = boundaries[random() % boundaries.len()];
        let range = a.min(b)..a.max(b);
        let text = ["x", "я", "🦀", "e\u{301}", "\r\n"][step % 5];
        let changed = &model[range.clone()] != text;
        let revision = buffer.revision();
        buffer.replace(range.clone(), text).unwrap();
        model.replace_range(range, text);
        assert_eq!(buffer.contents(), model, "edit {step}");
        assert_eq!(buffer.byte_len(), model.len());
        assert_eq!(buffer.slice(0..buffer.byte_len()).unwrap(), model);
        assert_eq!(buffer.revision() != revision, changed);
        if changed {
            revisions.push(model.clone());
        }
    }
    for expected in revisions[..revisions.len() - 1].iter().rev() {
        assert!(buffer.undo());
        assert_eq!(&buffer.contents(), expected);
    }
    assert!(!buffer.undo());
    assert!(!buffer.is_modified());
    for expected in &revisions[1..] {
        assert!(buffer.redo());
        assert_eq!(&buffer.contents(), expected);
    }
    assert!(!buffer.redo());
}
