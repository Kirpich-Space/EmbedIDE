//! Headless tests of actual crossterm events, shared Editor and terminal-cell layout.
#![cfg(feature = "tui")]
mod common;
use common::Fixture;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use embedide_editor::{vim::Mode, Editor};
use embedide_terminal::{
    open,
    tui::{layout::Frame, screen::Screen, TuiEditor},
};
use std::fs;
use unicode_width::UnicodeWidthStr;

fn key(app: &mut TuiEditor, code: KeyCode) {
    assert!(app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
}
fn keys(app: &mut TuiEditor, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn escape(app: &mut TuiEditor) {
    key(app, KeyCode::Esc);
}
fn ex(app: &mut TuiEditor, command: &str) {
    keys(app, &format!(":{command}"));
    key(app, KeyCode::Enter);
}
fn redo(app: &mut TuiEditor) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
}
fn row(frame: &Frame, line: usize) -> String {
    frame.rows[line]
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect()
}
fn contents(app: &TuiEditor) -> String {
    app.editor().state().buffer().contents()
}

#[test]
fn keys_apply_immediately_without_enter_and_insert_is_one_history_transaction() {
    let mut app = TuiEditor::new(Editor::new("abc".into()), 80, 24);
    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.editor().input().mode(), Mode::Insert);
    keys(&mut app, "猫x");
    assert_eq!(contents(&app), "猫xabc");
    escape(&mut app);
    keys(&mut app, "u");
    assert_eq!(contents(&app), "abc");
    redo(&mut app);
    assert_eq!(contents(&app), "猫xabc");
}

#[test]
fn grapheme_motions_wide_characters_and_tabs_map_to_distinct_cells() {
    let mut app = TuiEditor::new(Editor::new("a\t猫e\u{301}👩‍💻z".into()), 40, 8);
    let mut positions = Vec::new();
    for _ in 0..8 {
        positions.push(app.frame().unwrap().cursor.unwrap().0);
        keys(&mut app, "l");
    }
    assert_eq!(positions, [3, 4, 7, 9, 10, 12, 12, 12]);
    let frame = app.frame().unwrap();
    assert!(row(&frame, 0).contains("a   猫e\u{301}👩‍💻z"));
}

#[test]
fn viewport_scrolls_both_axes_resizes_and_keeps_cursor_visible() {
    let source = format!("{}\n", "猫".repeat(100)).repeat(50);
    let mut app = TuiEditor::new(Editor::new(source), 20, 7);
    keys(&mut app, "40j50l");
    let frame = app.frame().unwrap();
    assert!(app.viewport().top > 0 && app.viewport().left > 0);
    let (x, y) = frame.cursor.unwrap();
    assert!(x < 19 && y < 5);
    app.handle(Event::Resize(12, 4));
    let frame = app.frame().unwrap();
    let (x, y) = frame.cursor.unwrap();
    assert!(x < 11 && y < 2);
    for line in 0..frame.rows.len() {
        assert!(UnicodeWidthStr::width(row(&frame, line).as_str()) <= 11);
    }
    keys(&mut app, "gg0");
    app.frame().unwrap();
    assert_eq!(app.viewport().top, 0);
    assert_eq!(app.viewport().left, 0);
}

#[test]
fn empty_tiny_and_extreme_viewports_are_bounded() {
    let mut app = TuiEditor::new(Editor::new(String::new()), 80, 24);
    for (width, height) in [(0, 0), (1, 1), (2, 2), (3, 3), (65535, 65535)] {
        app.handle(Event::Resize(width, height));
        let frame = app.frame().unwrap();
        assert!(frame.rows.len() <= 256);
        if let Some((x, y)) = frame.cursor {
            assert!(x < width && y < height);
        }
        if width < 2 || height < 3 {
            assert!(frame.cursor.is_none());
        }
    }
}

#[test]
fn long_line_uses_escaped_streaming_fallback_and_orphan_combining_marks_are_visible() {
    let mut app = TuiEditor::new(Editor::new("猫".repeat(10_000)), 40, 5);
    let frame = app.frame().unwrap();
    assert!(row(&frame, 0).contains("\\u{732b}"));
    assert!(!row(&frame, 0).contains('猫'));
    keys(&mut app, "$");
    assert!(app.frame().unwrap().cursor.unwrap().0 < 39);
    let mut app = TuiEditor::new(Editor::new("\u{301}x\u{1b}[31m".into()), 80, 5);
    let frame = app.frame().unwrap();
    assert!(row(&frame, 0).contains("\\u{301}"));
    assert!(!row(&frame, 0).contains('\u{1b}'));
}

#[test]
fn clipping_never_prints_half_a_wide_character_or_overflows_cells() {
    for width in 2..18 {
        let mut app = TuiEditor::new(Editor::new("a猫b\t👩‍💻z".into()), width, 5);
        for _ in 0..9 {
            let frame = app.frame().unwrap();
            for line in 0..frame.rows.len() {
                assert!(UnicodeWidthStr::width(row(&frame, line).as_str()) < usize::from(width));
            }
            keys(&mut app, "l");
        }
    }
}

#[test]
fn command_and_search_prompts_need_real_enter_and_unicode_prompt_scrolls() {
    let f = Fixture::new();
    let path = f.write("main.c", "cat dog cat");
    let mut app = TuiEditor::new(open(&path).unwrap(), 16, 5);
    keys(&mut app, "/cat");
    assert_eq!(app.editor().state().cursor(), 0);
    assert_eq!(app.editor().input().mode(), Mode::Command);
    assert!(row(&app.frame().unwrap(), 4).starts_with("/cat"));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.editor().state().cursor(), 8);
    keys(&mut app, "N");
    assert_eq!(app.editor().state().cursor(), 0);
    keys(&mut app, "?cat");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.editor().state().cursor(), 8);
    keys(&mut app, "/");
    keys(&mut app, &"猫".repeat(60));
    let frame = app.frame().unwrap();
    assert!(frame.cursor.unwrap().0 < 15);
    assert_eq!(frame.cursor.unwrap().1, 4);
    escape(&mut app);
    keys(&mut app, "A!");
    escape(&mut app);
    keys(&mut app, ":w");
    assert_eq!(fs::read_to_string(&path).unwrap(), "cat dog cat");
    key(&mut app, KeyCode::Enter);
    assert_eq!(fs::read_to_string(path).unwrap(), "cat dog cat!");
}

#[test]
fn visual_registers_counted_motions_and_linewise_operations_reuse_engine() {
    let mut app = TuiEditor::new(Editor::new("one\ntwo\nthree".into()), 80, 8);
    keys(&mut app, "Vj");
    let frame = app.frame().unwrap();
    assert!(frame.rows[0].runs.iter().any(|run| run.selected));
    assert!(frame.rows[1].runs.iter().any(|run| run.selected));
    keys(&mut app, "yG0p");
    assert_eq!(contents(&app), "one\ntwo\nthree\none\ntwo");
    keys(&mut app, "gg2dd");
    assert_eq!(contents(&app), "three\none\ntwo");
    keys(&mut app, "u");
    assert_eq!(contents(&app), "one\ntwo\nthree\none\ntwo");
}

#[test]
fn bracketed_paste_is_literal_and_release_alt_and_function_keys_are_predictable() {
    let mut app = TuiEditor::new(Editor::new("original".into()), 80, 8);
    app.handle(Event::Paste(":q!dd".into()));
    assert_eq!(contents(&app), "original");
    let event = KeyEvent::new_with_kind(
        KeyCode::Char('i'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    assert!(!app.handle(Event::Key(event)));
    assert_eq!(app.editor().input().mode(), Mode::Normal);
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('i'),
        KeyModifiers::ALT,
    )));
    assert_eq!(app.editor().input().mode(), Mode::Normal);
    keys(&mut app, "i");
    app.handle(Event::Paste(":q!\r\n猫".into()));
    escape(&mut app);
    assert_eq!(contents(&app), ":q!\r\n猫original");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::F(12),
        KeyModifiers::NONE,
    )));
    assert!(!app.editor().state().is_closed());
}

#[test]
fn failed_quit_and_failed_save_preserve_buffer_and_history() {
    let f = Fixture::new();
    let path = f.write("main.c", "original\r\n");
    let mut app = TuiEditor::new(open(&path).unwrap(), 80, 8);
    keys(&mut app, "A!");
    escape(&mut app);
    ex(&mut app, "q");
    assert!(!app.editor().state().is_closed());
    assert!(app.editor().state().is_modified());
    assert!(row(&app.frame().unwrap(), 6).contains("unsaved"));
    escape(&mut app);
    fs::write(&path, "external").unwrap();
    ex(&mut app, "wq");
    assert!(!app.editor().state().is_closed());
    assert!(app.editor().state().is_modified());
    escape(&mut app);
    keys(&mut app, "u");
    assert_eq!(contents(&app), "original\r\n");
    redo(&mut app);
    assert_eq!(contents(&app), "original!\r\n");
    ex(&mut app, "q!");
    assert!(app.editor().state().is_closed());
    assert_eq!(fs::read_to_string(path).unwrap(), "external");
}

#[test]
fn ctrl_c_cancels_input_but_never_discards_or_quits() {
    let mut app = TuiEditor::new(Editor::new(String::new()), 80, 8);
    keys(&mut app, "iHello");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.editor().input().mode(), Mode::Normal);
    assert_eq!(contents(&app), "Hello");
    assert!(!app.editor().state().is_closed());
}

#[test]
fn identical_frames_emit_no_bytes_and_motions_do_not_repaint_unchanged_body_rows() {
    let mut app = TuiEditor::new(Editor::new("abc\ndef".into()), 80, 8);
    let mut screen = Screen::default();
    let mut out = Vec::new();
    let frame = app.frame().unwrap();
    assert!(screen.draw(&frame, &mut out).unwrap());
    out.clear();
    assert!(!screen.draw(&app.frame().unwrap(), &mut out).unwrap());
    assert!(out.is_empty());
    keys(&mut app, "l");
    screen.draw(&app.frame().unwrap(), &mut out).unwrap();
    assert!(!String::from_utf8(out).unwrap().contains("abc"));
}

#[test]
fn errors_have_priority_in_narrow_status_rows_and_unsupported_chords_cancel_counts() {
    let mut app = TuiEditor::new(
        Editor::new("one\ntwo\nthree\nfour\nfive\nsix".into()),
        20,
        5,
    );
    keys(&mut app, "5");
    app.handle(Event::Key(KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT)));
    keys(&mut app, "j");
    assert_eq!(app.editor().state().position().unwrap().line, 1);
    keys(&mut app, "/missing");
    key(&mut app, KeyCode::Enter);
    assert!(row(&app.frame().unwrap(), 3).starts_with("Error:"));
    assert!(row(&app.frame().unwrap(), 4).contains("missing"));
}
