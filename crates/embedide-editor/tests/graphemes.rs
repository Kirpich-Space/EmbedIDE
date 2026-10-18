//! Extended grapheme regressions across original/inserted piece boundaries.
use embedide_editor::{vim::Key, Editor, TextBuffer};
use unicode_segmentation::UnicodeSegmentation;

fn keys(editor: &mut Editor, text: &str) {
    for ch in text.chars() {
        editor.feed(Key::Char(ch)).unwrap();
    }
}
#[test]
fn horizontal_delete_backspace_and_visual_selection_use_whole_clusters() {
    let source = "e\u{301}👩‍💻🇫🇷猫z";
    let mut editor = Editor::new(source.into());
    keys(&mut editor, "lx");
    assert_eq!(editor.state().buffer().contents(), "e\u{301}🇫🇷猫z");
    keys(&mut editor, "u0vld");
    assert_eq!(editor.state().buffer().contents(), "🇫🇷猫z");
    keys(&mut editor, "u0li");
    editor.feed(Key::Backspace).unwrap();
    assert_eq!(editor.state().buffer().contents(), "👩‍💻🇫🇷猫z");
    editor.feed(Key::Delete).unwrap();
    assert_eq!(editor.state().buffer().contents(), "🇫🇷猫z");
    editor.feed(Key::Escape).unwrap();
    keys(&mut editor, "u");
    assert_eq!(editor.state().buffer().contents(), source);
}
#[test]
fn piece_boundaries_and_regional_indicator_context_match_unicode_segmentation() {
    let mut buffer = TextBuffer::new("az🇫🇷🇩🇪\r\nक्\u{200d}ष".into());
    buffer.replace(1..1, "\u{301}👩").unwrap();
    buffer.replace(7..7, "\u{200d}💻").unwrap();
    let text = buffer.contents();
    let mut boundaries: Vec<_> = text.grapheme_indices(true).map(|(i, _)| i).collect();
    boundaries.push(text.len());
    for pair in boundaries.windows(2) {
        assert_eq!(buffer.next_grapheme(pair[0]).unwrap(), pair[1]);
        assert_eq!(buffer.previous_grapheme(pair[1]).unwrap(), pair[0]);
    }
    for (offset, _) in text.char_indices() {
        let expected = *boundaries.iter().rev().find(|b| **b <= offset).unwrap();
        assert_eq!(buffer.grapheme_start(offset).unwrap(), expected);
    }
}
#[test]
fn api_coordinates_snap_to_graphemes_and_vertical_motion_keeps_logical_cluster_column() {
    let mut editor = Editor::new("e\u{301}👩‍💻x\n猫ab\ne\u{301}👩‍💻x".into());
    editor.set_cursor(1).unwrap();
    assert_eq!(editor.state().cursor(), 0);
    keys(&mut editor, "ljj");
    assert_eq!(editor.state().position().unwrap().line, 2);
    assert_eq!(editor.state().cursor(), "e\u{301}👩‍💻x\n猫ab\ne\u{301}".len());
}
#[test]
fn newline_copies_leading_indent_and_undo_restores_one_insert_session() {
    let mut editor = Editor::new("  \tvalue".into());
    keys(&mut editor, "A");
    editor.feed(Key::Enter).unwrap();
    keys(&mut editor, "next");
    editor.feed(Key::Escape).unwrap();
    assert_eq!(editor.state().buffer().contents(), "  \tvalue\n  \tnext");
    keys(&mut editor, "u");
    assert_eq!(editor.state().buffer().contents(), "  \tvalue");
}
