//! End-to-end key traces through the existing parser and real document executor.
use embedide_editor::{
    vim::{Key, Mode, SelectionKind},
    Editor, EditorError, Effect, Position,
};

fn keys(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.feed(Key::Char(character)).unwrap();
    }
}
fn text(editor: &Editor) -> String {
    editor.state().buffer().contents()
}
fn typed(editor: &mut Editor, value: &str) {
    editor.feed(Key::Text(value.into())).unwrap();
    editor.feed(Key::Escape).unwrap();
}
fn query(editor: &mut Editor, prompt: char, value: &str) -> Result<Effect, EditorError> {
    editor.feed(Key::Char(prompt))?;
    editor.feed(Key::Text(value.into()))?;
    editor.feed(Key::Enter)
}

#[test]
fn unicode_vertical_movement_retains_preferred_scalar_column() {
    let mut editor = Editor::new("αβ\nx\n猫犬鳥\n".into());
    keys(&mut editor, "lj");
    assert_eq!(
        editor.state().position().unwrap(),
        Position {
            line: 1,
            scalar_column: 0
        }
    );
    keys(&mut editor, "j");
    assert_eq!(
        editor.state().position().unwrap(),
        Position {
            line: 2,
            scalar_column: 1
        }
    );
    keys(&mut editor, "kk");
    assert_eq!(editor.state().cursor(), 2);
    assert_eq!(
        editor.state().position().unwrap(),
        Position {
            line: 0,
            scalar_column: 1
        }
    );
}

#[test]
fn undo_restores_preferred_column_as_well_as_cursor_offset() {
    let mut editor = Editor::new("abc\nx\nabc".into());
    keys(&mut editor, "lljciw");
    typed(&mut editor, "new");
    keys(&mut editor, "u");
    assert_eq!(editor.state().preferred_column(), Some(2));
    keys(&mut editor, "j");
    assert_eq!(
        editor.state().position().unwrap(),
        Position {
            line: 2,
            scalar_column: 2
        }
    );
}

#[test]
fn empty_and_short_files_clamp_all_boundaries() {
    for source in ["", "\n", "a", "я", "\r\n", "a\nb"] {
        let mut editor = Editor::new(source.into());
        keys(&mut editor, "999h999k999j999lggG0^$wbe");
        let result = text(&editor);
        assert_eq!(result, source);
        assert!(result.is_char_boundary(editor.state().cursor()));
        assert!(editor.state().cursor() <= result.len());
    }
}

#[test]
fn word_punctuation_indent_and_file_motions_execute() {
    let source = "  alpha_beta ++ γδ z";
    let mut editor = Editor::new(source.into());
    keys(&mut editor, "^");
    assert_eq!(editor.state().cursor(), 2);
    keys(&mut editor, "w");
    assert_eq!(editor.state().cursor(), source.find("++").unwrap());
    keys(&mut editor, "e");
    assert_eq!(editor.state().cursor(), source.find("++").unwrap() + 1);
    keys(&mut editor, "w");
    assert_eq!(editor.state().cursor(), source.find('γ').unwrap());
    keys(&mut editor, "b");
    assert_eq!(editor.state().cursor(), source.find("++").unwrap());
    keys(&mut editor, "0$");
    assert_eq!(editor.state().cursor(), source.len() - 1);
}

#[test]
fn append_open_line_and_undo_preserve_crlf() {
    let mut editor = Editor::new("one\r\ntwo".into());
    keys(&mut editor, "A");
    typed(&mut editor, "!");
    assert_eq!(text(&editor), "one!\r\ntwo");
    keys(&mut editor, "o");
    typed(&mut editor, "new");
    assert_eq!(text(&editor), "one!\r\nnew\r\ntwo");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "one!\r\ntwo");
    assert_eq!(editor.state().cursor(), 3);
    editor.feed(Key::Ctrl('r')).unwrap();
    assert_eq!(text(&editor), "one!\r\nnew\r\ntwo");
}

#[test]
fn backspace_treats_crlf_and_unicode_scalars_as_units() {
    let mut editor = Editor::new("я\r\n猫".into());
    editor.set_cursor(4).unwrap();
    keys(&mut editor, "i");
    editor.feed(Key::Backspace).unwrap();
    editor.feed(Key::Escape).unwrap();
    assert_eq!(text(&editor), "я猫");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "я\r\n猫");
    assert_eq!(editor.state().cursor(), 4);
    keys(&mut editor, "A");
    editor.feed(Key::Backspace).unwrap();
    editor.feed(Key::Escape).unwrap();
    assert_eq!(text(&editor), "я\r\n");
}

#[test]
fn line_registers_and_paste_handle_unterminated_last_line() {
    let mut editor = Editor::new("a\nb\nc".into());
    keys(&mut editor, "yyp");
    assert_eq!(text(&editor), "a\na\nb\nc");
    assert_eq!(editor.state().register().unwrap().kind, SelectionKind::Line);
    assert_eq!(editor.state().register().unwrap().text, "a\n");
    keys(&mut editor, "Gdd");
    assert_eq!(text(&editor), "a\na\nb");
    assert_eq!(editor.state().register().unwrap().text, "c");
    keys(&mut editor, "p");
    assert_eq!(text(&editor), "a\na\nb\nc");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "a\na\nb");
}

#[test]
fn character_register_and_paste_do_not_insert_line_breaks() {
    let mut editor = Editor::new("abc def".into());
    keys(&mut editor, "vlylp");
    assert_eq!(text(&editor), "ababc def");
    assert_eq!(editor.state().register().unwrap().text, "ab");
    assert_eq!(
        editor.state().register().unwrap().kind,
        SelectionKind::Character
    );
    assert_eq!(editor.state().cursor(), 3);
}

#[test]
fn reversed_visual_character_delete_is_inclusive_and_unicode_safe() {
    let mut editor = Editor::new("яβz\nlast".into());
    editor.set_cursor(2).unwrap();
    keys(&mut editor, "vhd");
    assert_eq!(text(&editor), "z\nlast");
    assert_eq!(editor.state().register().unwrap().text, "яβ");
    assert_eq!(editor.input().mode(), Mode::Normal);
    assert_eq!(editor.state().selection(), None);
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "яβz\nlast");
    assert_eq!(editor.state().cursor(), 0);
}

#[test]
fn visual_line_change_and_insert_are_one_history_transaction() {
    let mut editor = Editor::new("one\ntwo\nthree".into());
    keys(&mut editor, "Vjc");
    assert_eq!(editor.input().mode(), Mode::Insert);
    typed(&mut editor, "new");
    assert_eq!(text(&editor), "new\nthree");
    assert_eq!(editor.state().register().unwrap().text, "one\ntwo\n");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "one\ntwo\nthree");
    assert_eq!(editor.state().cursor(), 4);
    editor.feed(Key::Ctrl('r')).unwrap();
    assert_eq!(text(&editor), "new\nthree");
    assert_eq!(editor.state().cursor(), 2);
}

#[test]
fn ciw_register_grouped_undo_redo_and_counted_paste() {
    let mut editor = Editor::new("alpha beta".into());
    keys(&mut editor, "wciw");
    typed(&mut editor, "γδ");
    assert_eq!(text(&editor), "alpha γδ");
    assert_eq!(editor.state().register().unwrap().text, "beta");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "alpha beta");
    assert_eq!(editor.state().cursor(), 6);
    editor.feed(Key::Ctrl('r')).unwrap();
    assert_eq!(editor.state().cursor(), 8);
    keys(&mut editor, "2p");
    assert_eq!(text(&editor), "alpha γδbetabeta");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "alpha γδ");
}

#[test]
fn change_word_at_its_last_character_does_not_consume_next_word() {
    let mut editor = Editor::new("abc def".into());
    keys(&mut editor, "llcw");
    typed(&mut editor, "X");
    assert_eq!(text(&editor), "abX def");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "abc def");
}

#[test]
fn delete_motion_counts_and_change_lines_use_shared_ranges() {
    let mut editor = Editor::new("a b c d e f g".into());
    keys(&mut editor, "2d3w");
    assert_eq!(text(&editor), "g");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "a b c d e f g");
    let mut editor = Editor::new("one\ntwo".into());
    keys(&mut editor, "cc");
    typed(&mut editor, "new");
    assert_eq!(text(&editor), "new\ntwo");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "one\ntwo");
}

#[test]
fn non_edits_and_identical_replacements_do_not_fill_history() {
    let mut editor = Editor::new("a".into());
    keys(&mut editor, "i");
    editor.feed(Key::Escape).unwrap();
    keys(&mut editor, "yy0h");
    editor.replace(0..1, "a").unwrap();
    assert!(!editor.state().buffer().can_undo());
    assert_eq!(editor.state().buffer().revision(), 0);
}

#[test]
fn empty_delete_preserves_the_register_and_empty_line_paste_is_linewise() {
    let mut editor = Editor::new("keep".into());
    keys(&mut editor, "ddxP");
    assert_eq!(text(&editor), "keep");
    let mut editor = Editor::new(String::new());
    keys(&mut editor, "yyp");
    assert_eq!(text(&editor), "\n");
    assert_eq!(editor.state().register().unwrap().kind, SelectionKind::Line);
}

#[test]
fn counted_line_end_and_visual_search_keep_valid_selection_endpoints() {
    let mut editor = Editor::new("abc\ndef\nghi".into());
    keys(&mut editor, "2$");
    assert_eq!(
        editor.state().position().unwrap(),
        Position {
            line: 1,
            scalar_column: 2
        }
    );
    keys(&mut editor, "ggv");
    query(&mut editor, '/', "def").unwrap();
    assert_eq!(editor.input().mode(), Mode::Visual);
    assert_eq!(editor.state().selection().unwrap().head, 4);
    keys(&mut editor, "d");
    assert_eq!(text(&editor), "ef\nghi");
}

#[test]
fn new_branch_discards_redo_and_insert_cursor_movement_splits_groups() {
    let mut editor = Editor::new("a".into());
    keys(&mut editor, "A");
    typed(&mut editor, "b");
    keys(&mut editor, "uA");
    typed(&mut editor, "c");
    assert_eq!(text(&editor), "ac");
    assert!(!editor.state().buffer().can_redo());
    keys(&mut editor, "Ai");
    editor.feed(Key::Left).unwrap();
    typed(&mut editor, "X");
    assert_eq!(text(&editor), "acXi");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "aci");
}

#[test]
fn literal_search_wraps_counts_and_preserves_last_direction() {
    let source = "cat dog cat 猫 cat";
    let mut editor = Editor::new(source.into());
    query(&mut editor, '/', "cat").unwrap();
    assert_eq!(editor.state().cursor(), 8);
    keys(&mut editor, "n");
    assert_eq!(editor.state().cursor(), source.rfind("cat").unwrap());
    keys(&mut editor, "N2n");
    assert_eq!(editor.state().cursor(), 0);
    query(&mut editor, '?', "猫").unwrap();
    assert_eq!(editor.state().cursor(), source.find('猫').unwrap());
    assert_eq!(editor.state().search_history().count(), 2);
}

#[test]
fn failed_search_keeps_document_and_prompt_for_correction() {
    let mut editor = Editor::new("abc".into());
    let before = editor.state().buffer().revision();
    assert!(matches!(
        query(&mut editor, '/', "missing"),
        Err(EditorError::SearchNotFound(_))
    ));
    assert_eq!(text(&editor), "abc");
    assert_eq!(editor.state().cursor(), 0);
    assert_eq!(editor.state().buffer().revision(), before);
    assert_eq!(editor.input().mode(), Mode::Command);
    assert_eq!(editor.input().command_text(), Some("missing"));
    assert_eq!(editor.state().search_history().count(), 0);
}

#[test]
fn literal_match_can_span_piece_boundaries() {
    let mut editor = Editor::new("abCDxy".into());
    editor.replace(2..4, "ef").unwrap();
    query(&mut editor, '/', "befx").unwrap();
    assert_eq!(editor.state().cursor(), 1);
    assert_eq!(text(&editor), "abefxy");
}

#[test]
fn invalid_utf8_coordinates_and_unsupported_counts_do_not_edit() {
    let mut editor = Editor::new("я猫".into());
    assert!(editor.set_cursor(1).is_err());
    assert!(editor.replace(1..1, "bad").is_err());
    keys(&mut editor, "3");
    assert!(matches!(
        editor.feed(Key::Char('i')),
        Err(EditorError::Unsupported(_))
    ));
    assert_eq!(text(&editor), "я猫");
    assert_eq!(editor.input().mode(), Mode::Normal);
    assert!(!editor.state().buffer().can_undo());
    let mut editor = Editor::new("a\nb\nc".into());
    keys(&mut editor, "999");
    assert!(matches!(
        editor.feed(Key::Char('p')),
        Err(EditorError::EmptyRegister)
    ));
    keys(&mut editor, "j");
    assert_eq!(editor.state().position().unwrap().line, 1);
}

#[test]
fn large_multiline_file_keeps_index_consistent_after_edit_and_undo() {
    let mut source = "αβ\t猫\n".repeat(100_000);
    source.pop();
    let mut editor = Editor::new(source.clone());
    keys(&mut editor, "100000G");
    assert_eq!(editor.state().position().unwrap().line, 99_999);
    keys(&mut editor, "gg50000jdd");
    assert_eq!(editor.state().line_count(), 99_999);
    keys(&mut editor, "u");
    assert_eq!(text(&editor), source);
    assert_eq!(editor.state().line_count(), 100_000);
}

#[test]
fn deterministic_replacements_match_string_and_undo_restores_cursor() {
    let mut editor = Editor::new("seed\r\nя🦀".into());
    let mut seed = 0x5eed_u64;
    for index in 0..300 {
        let before = text(&editor);
        let before_cursor = editor.state().cursor();
        let boundaries: Vec<_> = before
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(before.len()))
            .collect();
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = boundaries[(seed >> 32) as usize % boundaries.len()];
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = boundaries[(seed >> 32) as usize % boundaries.len()];
        let range = a.min(b)..a.max(b);
        let replacement = ["я", "🦀\r\n", "", "e\u{301}", "a\nb"][index % 5];
        let mut expected = before.clone();
        expected.replace_range(range.clone(), replacement);
        let revision = editor.state().buffer().revision();
        editor.replace(range, replacement).unwrap();
        assert_eq!(text(&editor), expected);
        assert!(expected.is_char_boundary(editor.state().cursor()));
        let cursor = editor.state().cursor();
        assert_eq!(
            editor.state().line_count(),
            expected.bytes().filter(|b| *b == b'\n').count() + 1
        );
        assert_eq!(
            editor.state().position().unwrap().line,
            expected[..cursor].bytes().filter(|b| *b == b'\n').count()
        );
        if editor.state().buffer().revision() != revision {
            keys(&mut editor, "u");
            assert_eq!(text(&editor), before);
            assert_eq!(editor.state().cursor(), before_cursor);
            editor.feed(Key::Ctrl('r')).unwrap();
            assert_eq!(text(&editor), expected);
            assert_eq!(editor.state().cursor(), cursor);
        }
    }
}

#[test]
fn mixed_key_sequences_preserve_text_cursor_selection_and_history_invariants() {
    let mut editor = Editor::new("αβ\r\n\t猫 e\u{301}\n\nlast".into());
    let mut seed = 0x5eed_u64;
    let commands = [
        "h", "j", "k", "l", "w", "b", "e", "gg", "G", "0", "^", "$", "3j", "3k", "2w", "2b", "2e",
        "x", "2x", "dw", "db", "de", "d$", "dh", "dl", "dd", "2dd", "yy", "2yy", "yiw", "p", "P",
        "ciw", "cw", "cc", "i", "a", "A", "o", "O", "vld", "vhy", "Vjd", "Vky", "vwc", "Vjc",
    ];
    for step in 0..1500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let command = commands[(seed >> 32) as usize % commands.len()];
        let before = text(&editor);
        let mut before_cursor = editor.state().cursor();
        let revision = editor.state().buffer().revision();
        for character in command.chars() {
            // Visual motions precede the editing transaction. Undo restores the
            // cursor at the operator, rather than before entering Visual.
            if editor.input().mode() == Mode::Visual && matches!(character, 'd' | 'c' | 'y') {
                before_cursor = editor.state().cursor();
            }
            if let Err(error) = editor.feed(Key::Char(character)) {
                assert!(
                    matches!(error, EditorError::EmptyRegister),
                    "{command}: {error}"
                );
            }
        }
        if editor.input().mode() == Mode::Insert {
            editor
                .feed(Key::Text(["я", "\r\n猫", "e\u{301}", ""][step % 4].into()))
                .unwrap();
        }
        editor.feed(Key::Escape).unwrap();
        let after = text(&editor);
        let cursor = editor.state().cursor();
        assert!(after.is_char_boundary(cursor), "step {step}: {command}");
        assert_eq!(editor.input().mode(), Mode::Normal);
        assert!(editor.state().selection().is_none());
        let expected_row = after[..cursor].bytes().filter(|b| *b == b'\n').count();
        let row_start = after[..cursor].rfind('\n').map_or(0, |i| i + 1);
        assert_eq!(
            editor.state().position().unwrap(),
            Position {
                line: expected_row,
                scalar_column: after[row_start..cursor].chars().count(),
            }
        );
        assert_eq!(
            editor.state().line_count(),
            after.bytes().filter(|b| *b == b'\n').count() + 1
        );
        assert!(
            !(cursor > 0
                && after.as_bytes().get(cursor) == Some(&b'\n')
                && after.as_bytes()[cursor - 1] == b'\r'),
            "cursor split CRLF after {command}"
        );
        if editor.state().buffer().revision() != revision {
            keys(&mut editor, "u");
            assert_eq!(text(&editor), before, "undo step {step}: {command}");
            assert_eq!(
                editor.state().cursor(),
                before_cursor,
                "undo cursor: {command}"
            );
            editor.feed(Key::Ctrl('r')).unwrap();
            assert_eq!(text(&editor), after, "redo step {step}: {command}");
            assert_eq!(editor.state().cursor(), cursor, "redo cursor: {command}");
        }
    }
}

#[test]
fn word_operators_preserve_the_final_line_ending_and_next_line_indent() {
    for (source, command, expected, register) in [
        ("one two\nthree\n", "wdw", "one \nthree\n", "two"),
        ("one two\r\n  три\r\n", "wdw", "one \r\n  три\r\n", "two"),
        ("one two  \n  three\n", "wdw", "one \n  three\n", "two  "),
        ("one two\nthree four\n", "d2w", "\nthree four\n", "one two"),
        ("one two\nthree four\n", "wd2w", "one four\n", "two\nthree "),
        ("one\n\nthree\n", "d2w", "three\n", "one\n\n"),
    ] {
        let mut editor = Editor::new(source.into());
        keys(&mut editor, command);
        assert_eq!(text(&editor), expected, "{command} in {source:?}");
        assert_eq!(editor.state().register().unwrap().text, register);
        keys(&mut editor, "u");
        assert_eq!(text(&editor), source);
        editor.feed(Key::Ctrl('r')).unwrap();
        assert_eq!(text(&editor), expected);
    }
}

#[test]
fn word_forward_counts_empty_lines_and_yank_and_change_share_word_endpoints() {
    for source in ["one\n\nthree", "one\r\n\r\nthree"] {
        let mut editor = Editor::new(source.into());
        keys(&mut editor, "w");
        assert_eq!(editor.state().position().unwrap().line, 1);
        keys(&mut editor, "w");
        assert_eq!(editor.state().position().unwrap().line, 2);
    }
    let mut editor = Editor::new("one two\nthree".into());
    keys(&mut editor, "wyw");
    assert_eq!(editor.state().register().unwrap().text, "two");
    assert_eq!(text(&editor), "one two\nthree");
    assert!(!editor.state().buffer().can_undo());
    let mut editor = Editor::new("one   \r\n  two".into());
    keys(&mut editor, "3lcw");
    typed(&mut editor, "X");
    assert_eq!(text(&editor), "oneX\r\n  two");
    keys(&mut editor, "u");
    assert_eq!(text(&editor), "one   \r\n  two");
}

#[test]
fn indexed_line_ranges_preserve_utf8_and_exclude_lf_crlf_terminators() {
    let mut editor = Editor::new("\u{feff}a\r\n猫\n\nz\r".into());
    for (line, expected) in ["\u{feff}a", "猫", "", "z\r"].iter().enumerate() {
        assert_eq!(
            editor
                .state()
                .buffer()
                .slice(editor.state().line_range(line).unwrap())
                .unwrap(),
            *expected
        );
    }
    assert!(editor.state().line_range(4).is_none());
    editor
        .replace(0..editor.state().buffer().byte_len(), "\n")
        .unwrap();
    assert_eq!(editor.state().line_range(0), Some(0..0));
    assert_eq!(editor.state().line_range(1), Some(1..1));
    assert!(editor.state().line_range(usize::MAX).is_none());
}
