//! Undo grouping contracts needed by modal Insert and change operators.
use embedide_editor::{EditError, TextBuffer};

#[test]
fn insert_session_undoes_and_redoes_as_one_transaction() {
    let mut buffer = TextBuffer::new("base".into());
    buffer.begin_edit_group();
    buffer.replace(4..4, "я").unwrap();
    buffer.replace(6..6, "🦀").unwrap();
    buffer.replace(4..6, "x").unwrap();
    buffer.end_edit_group();
    assert_eq!(buffer.contents(), "basex🦀");
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "base");
    assert!(!buffer.can_undo());
    assert!(!buffer.is_modified());
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "basex🦀");
    assert!(!buffer.can_redo());
}

#[test]
fn empty_and_rejected_groups_preserve_history_and_redo() {
    let mut buffer = TextBuffer::new("я".into());
    buffer.replace(2..2, "x").unwrap();
    buffer.undo();
    buffer.begin_edit_group();
    buffer.replace(0..0, "").unwrap();
    assert_eq!(buffer.replace(1..1, "bad"), Err(EditError::InvalidBoundary));
    buffer.end_edit_group();
    assert!(!buffer.can_undo());
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "яx");
}

#[test]
fn saving_closes_group_and_keeps_savepoint_reachable() {
    let mut buffer = TextBuffer::new("a".into());
    buffer.begin_edit_group();
    buffer.replace(1..1, "b").unwrap();
    buffer.mark_saved();
    buffer.replace(2..2, "c").unwrap();
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "ab");
    assert!(!buffer.is_modified());
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "a");
    assert!(buffer.is_modified());
}

#[test]
fn undo_closes_group_before_a_new_branch() {
    let mut buffer = TextBuffer::new("a".into());
    buffer.begin_edit_group();
    buffer.replace(1..1, "b").unwrap();
    buffer.replace(2..2, "c").unwrap();
    assert!(buffer.undo());
    buffer.replace(1..1, "d").unwrap();
    assert!(!buffer.can_redo());
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "a");
    assert!(!buffer.can_undo());
}

#[test]
fn repeated_begin_is_idempotent_and_next_group_is_separate() {
    let mut buffer = TextBuffer::new(String::new());
    buffer.begin_edit_group();
    buffer.replace(0..0, "a").unwrap();
    buffer.begin_edit_group();
    buffer.replace(1..1, "b").unwrap();
    buffer.end_edit_group();
    buffer.begin_edit_group();
    buffer.replace(2..2, "c").unwrap();
    buffer.end_edit_group();
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "ab");
    assert!(buffer.undo());
    assert!(buffer.is_empty());
}

#[test]
fn history_limit_counts_groups_instead_of_keystrokes() {
    let mut buffer = TextBuffer::with_history_limit(String::new(), 1);
    for text in ["abc", "def"] {
        buffer.begin_edit_group();
        for character in text.chars() {
            let end = buffer.byte_len();
            buffer.replace(end..end, &character.to_string()).unwrap();
        }
        buffer.end_edit_group();
    }
    assert!(buffer.undo());
    assert_eq!(buffer.contents(), "abc");
    assert!(!buffer.undo());
    assert!(buffer.redo());
    assert_eq!(buffer.contents(), "abcdef");
}

#[test]
fn disabled_history_stays_disabled_for_groups() {
    let mut buffer = TextBuffer::with_history_limit(String::new(), 0);
    buffer.begin_edit_group();
    buffer.replace(0..0, "a").unwrap();
    buffer.replace(1..1, "b").unwrap();
    buffer.end_edit_group();
    assert_eq!(buffer.contents(), "ab");
    assert!(!buffer.undo());
    assert!(!buffer.redo());
}
