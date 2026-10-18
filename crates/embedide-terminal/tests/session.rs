//! Real terminal-transport -> parser -> executor -> file-layer contracts.
mod common;
use common::Fixture;
use embedide_editor::{vim::Mode, Editor, EditorError, Position};
use embedide_terminal::{open, FrontendError, LineEditor};
use std::{
    fs,
    io::{self, Cursor, Write},
};

#[test]
fn open_missing_file_edit_save_quit_and_reopen() {
    let f = Fixture::new();
    let path = f.path("new 猫.rs");
    let mut session = LineEditor::new(open(&path).unwrap());
    assert!(!path.exists());
    let report = session
        .run(
            &mut Cursor::new("ihello 猫<Esc>\n:wq\n"),
            &mut Vec::new(),
            true,
        )
        .unwrap();
    assert!(report.closed);
    assert!(!report.modified);
    assert_eq!(open(&path).unwrap().state().buffer().contents(), "hello 猫");
}

#[test]
fn navigation_registers_history_search_save_and_clean_quit() {
    let f = Fixture::new();
    let path = f.write("main.c", "one\n猫 two\nthree");
    let mut session = LineEditor::new(open(&path).unwrap());
    assert!(!session.editor().state().is_modified());
    session.submit("j0yy").unwrap();
    assert_eq!(
        session.editor().state().position().unwrap(),
        Position {
            line: 1,
            scalar_column: 0
        }
    );
    session.submit("p").unwrap();
    let pasted = "one\n猫 two\n猫 two\nthree";
    assert_eq!(session.editor().state().buffer().contents(), pasted);
    session.submit("dd").unwrap();
    assert_eq!(
        session.editor().state().buffer().contents(),
        "one\n猫 two\nthree"
    );
    session.submit("u").unwrap();
    assert_eq!(session.editor().state().buffer().contents(), pasted);
    session.submit("<C-r>").unwrap();
    session.submit("gg/three").unwrap();
    assert_eq!(session.editor().state().position().unwrap().line, 2);
    session.submit("ciwZ<Esc>").unwrap();
    session.submit(":w").unwrap();
    assert!(session.message().starts_with("Saved"));
    assert_eq!(
        open(&path).unwrap().state().buffer().contents(),
        "one\n猫 two\nZ"
    );
    session.submit(":q").unwrap();
    assert!(session.editor().state().is_closed());
}

#[test]
fn crlf_insert_newline_and_unicode_session_are_one_undo_transaction() {
    let f = Fixture::new();
    let path = f.write("main.c", "one\r\ntwo\r\n");
    let mut session = LineEditor::new(open(&path).unwrap());
    session.submit("A!<Enter>猫e\u{301}<Esc>").unwrap();
    let expected = "one!\r\n猫e\u{301}\r\ntwo\r\n";
    assert_eq!(session.editor().state().buffer().contents(), expected);
    session.submit("u").unwrap();
    assert_eq!(
        session.editor().state().buffer().contents(),
        "one\r\ntwo\r\n"
    );
    session.submit("<C-r>").unwrap();
    session.submit(":wq").unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), expected);
}

#[test]
fn interactive_unsafe_quit_displays_error_and_force_quit_preserves_disk() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut session = LineEditor::new(open(&path).unwrap());
    let mut output = Vec::new();
    let report = session
        .run(
            &mut Cursor::new("A!<Esc>\n:q\n<Esc>:q!\n"),
            &mut output,
            false,
        )
        .unwrap();
    assert!(report.closed && report.modified);
    assert!(String::from_utf8(output)
        .unwrap()
        .contains("unsaved changes"));
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[test]
fn batch_unsafe_quit_stops_with_error_and_retains_buffer() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut session = LineEditor::new(open(&path).unwrap());
    let error = session
        .run(&mut Cursor::new("A!<Esc>\n:q\n"), &mut Vec::new(), true)
        .unwrap_err();
    assert!(matches!(
        error,
        FrontendError::Editor(EditorError::UnsavedChanges)
    ));
    assert!(!session.editor().state().is_closed());
    assert_eq!(session.editor().state().buffer().contents(), "original!");
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[test]
fn external_change_rejects_save_and_remains_visible_and_dirty() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut session = LineEditor::new(open(&path).unwrap());
    session.submit("A local<Esc>").unwrap();
    fs::write(&path, "external").unwrap();
    assert!(matches!(
        session.submit(":w"),
        Err(FrontendError::Editor(EditorError::FileChanged))
    ));
    assert!(session.editor().state().is_modified());
    assert!(session.message().contains("changed externally"));
    session.submit("<Esc>:q!").unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "external");
}

#[test]
fn read_errors_are_never_retried_as_empty_documents() {
    let f = Fixture::new();
    let path = f.path("bad.bin");
    fs::write(&path, [0xff]).unwrap();
    assert!(
        matches!(open(&path), Err(EditorError::Io(ref e)) if e.kind() == io::ErrorKind::InvalidData)
    );
    assert!(open(&f.path("absent/new.c")).is_err());
    assert!(open(&f.0).is_err());
    assert_eq!(fs::read(path).unwrap(), [0xff]);
}

#[test]
fn readonly_save_failure_keeps_editor_dirty_and_can_be_quit_explicitly() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let mut session = LineEditor::new(open(&path).unwrap());
    session.submit("A!<Esc>").unwrap();
    assert!(session.submit(":wq").is_err());
    assert!(session.editor().state().is_modified());
    assert!(!session.editor().state().is_closed());
    session.submit("<Esc>:q!").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    // On Windows a read-only fixture cannot be removed by remove_dir_all.
    #[cfg(windows)]
    {
        let mut p = fs::metadata(&path).unwrap().permissions();
        p.set_readonly(false);
        fs::set_permissions(&path, p).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn symlink_restriction_is_preserved_by_the_frontend() {
    let f = Fixture::new();
    let target = f.write("target.c", "unchanged");
    let link = f.path("link.c");
    std::os::unix::fs::symlink(target, &link).unwrap();
    assert!(matches!(open(&link), Err(EditorError::SymbolicLink)));
}

#[test]
fn eof_with_unsaved_text_is_an_error_without_automatic_save() {
    let f = Fixture::new();
    let path = f.write("main.c", "old");
    let mut session = LineEditor::new(open(&path).unwrap());
    assert!(matches!(
        session.run(&mut Cursor::new("A!<Esc>\n"), &mut Vec::new(), true),
        Err(FrontendError::UnsavedInput)
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "old");
    assert!(session.editor().state().is_modified());
}

#[test]
fn invalid_utf8_and_oversized_key_lines_do_not_partially_edit() {
    for bytes in [
        vec![0xff, b'\n'],
        format!("i{}\n", "x".repeat(64 * 1024)).into_bytes(),
    ] {
        let mut session = LineEditor::new(Editor::new(String::new()));
        assert!(session
            .run(&mut Cursor::new(bytes), &mut Vec::new(), true)
            .is_err());
        assert!(session.editor().state().buffer().is_empty());
        assert_eq!(session.editor().input().mode(), Mode::Normal);
    }
}

#[test]
fn literal_angle_brackets_special_keys_and_physical_escape_sequences() {
    let mut session = LineEditor::new(Editor::new(String::new()));
    session
        .submit("i#include <stdint.h> a << 2; <lt>Esc>猫<Esc>")
        .unwrap();
    assert_eq!(
        session.editor().state().buffer().contents(),
        "#include <stdint.h> a << 2; <Esc>猫"
    );
    session.submit("\u{1b}[D").unwrap();
    let before = session.editor().state().buffer().contents();
    let position = session.editor().state().cursor();
    assert!(session.submit("iBAD\u{1b}[15~").is_err());
    assert_eq!(session.editor().state().buffer().contents(), before);
    assert_eq!(session.editor().state().cursor(), position);
    assert_eq!(session.editor().input().mode(), Mode::Normal);
}

#[test]
fn rendering_is_bounded_has_a_cursor_and_never_emits_source_escape_codes() {
    let mut session = LineEditor::new(Editor::new(format!(
        "\u{1b}[31m\t猫\n{}",
        "a\n".repeat(100_000)
    )));
    let mut output = Vec::new();
    session.draw(&mut output).unwrap();
    let rendered = String::from_utf8(output).unwrap();
    assert!(rendered.len() < 6000);
    assert!(!rendered.contains('\u{1b}'));
    assert!(rendered.contains("▏"));
    assert!(rendered.contains("[Normal]"));
    assert!(rendered.contains("keys>"));
    session.submit("G").unwrap();
    let mut output = Vec::new();
    session.draw(&mut output).unwrap();
    assert!(output.len() < 6000);
}

#[test]
fn empty_buffer_and_long_unicode_line_render_without_document_materialization() {
    for source in [String::new(), "猫".repeat(100_000)] {
        let mut editor = Editor::new(source);
        editor
            .set_cursor(editor.state().buffer().byte_len())
            .unwrap();
        let session = LineEditor::new(editor);
        let mut output = Vec::new();
        session.draw(&mut output).unwrap();
        assert!(output.len() < 6000);
        assert!(String::from_utf8(output).unwrap().contains("▏"));
    }
}

#[test]
fn output_failure_returns_error_and_does_not_touch_the_file() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "test sink"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut session = LineEditor::new(open(&path).unwrap());
    let error = session
        .run(&mut Cursor::new("iBAD<Esc>\n:wq\n"), &mut Broken, false)
        .unwrap_err();
    assert!(matches!(error, FrontendError::Io(ref e) if e.kind() == io::ErrorKind::BrokenPipe));
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[test]
fn shell_ex_commands_are_rejected_without_execution() {
    let f = Fixture::new();
    let mut session = LineEditor::new(open(&f.path("new.c")).unwrap());
    assert!(session
        .submit(&format!(":!touch {}", f.path("marker").display()))
        .is_err());
    assert!(!f.path("marker").exists());
    assert!(!f.path("new.c").exists());
}
