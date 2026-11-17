//! Project frame and routing contracts using real editor documents.
#![cfg(feature = "tui")]
mod common;
use common::Fixture;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use embedide_terminal::tui::{
    layout::Frame,
    screen::{Screen, Theme},
    workspace::{Focus, Workspace},
};
use std::fs;
use unicode_width::UnicodeWidthStr;
fn key(app: &mut Workspace, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn ctrl(app: &mut Workspace, ch: char) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(ch),
        KeyModifiers::CONTROL,
    )));
}
fn text(app: &mut Workspace, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn frame_text(frame: &Frame) -> String {
    frame
        .rows
        .iter()
        .map(|r| r.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn workspace_starts_without_project_or_tool_execution_and_opens_explorer_selection() {
    let f = Fixture::new();
    f.write("a.c", "int main() {}");
    let mut app = Workspace::open(&f.0, 100, 28).unwrap();
    assert_eq!(app.focus(), Focus::Explorer);
    assert!(frame_text(&app.frame().unwrap()).contains("EXPLORER"));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus(), Focus::Editor);
    assert_eq!(app.editor().state().buffer().contents(), "int main() {}");
    assert!(app.command("build").is_err());
    assert!(!app.is_closed());
}
#[test]
fn each_document_keeps_history_and_modified_state_and_quit_can_be_cancelled() {
    let f = Fixture::new();
    let first = f.write("a.c", "first");
    let second = f.write("b.c", "second");
    let mut app = Workspace::open(&f.0, 100, 28).unwrap();
    app.open_file(&first).unwrap();
    text(&mut app, "iA");
    key(&mut app, KeyCode::Esc);
    app.open_file(&second).unwrap();
    text(&mut app, "iB");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.document_count(), 2);
    key(&mut app, KeyCode::F(10));
    assert_eq!(app.editor().state().buffer().contents(), "Afirst");
    text(&mut app, "u");
    assert_eq!(app.editor().state().buffer().contents(), "first");
    key(&mut app, KeyCode::F(10));
    assert_eq!(app.editor().state().buffer().contents(), "Bsecond");
    ctrl(&mut app, 'q');
    assert!(frame_text(&app.frame().unwrap()).contains("Discard ALL"));
    key(&mut app, KeyCode::Esc);
    assert!(!app.is_closed());
    ctrl(&mut app, 'w');
    key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.document_count(), 2);
    ctrl(&mut app, 'w');
    key(&mut app, KeyCode::Char('y'));
    assert_eq!(app.document_count(), 1);
    assert_eq!(fs::read_to_string(second).unwrap(), "second");
}
#[test]
fn palette_receives_text_before_editor_and_normal_typing_never_starts_a_build() {
    let f = Fixture::new();
    let path = f.write("main.c", "source");
    let mut app = Workspace::open(&f.0, 90, 24).unwrap();
    app.open_file(&path).unwrap();
    text(&mut app, "ibuild flash debug");
    key(&mut app, KeyCode::Esc);
    assert_eq!(
        app.editor().state().buffer().contents(),
        "build flash debugsource"
    );
    ctrl(&mut app, 'k');
    text(&mut app, "show problems");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus(), Focus::Panel);
    assert!(frame_text(&app.frame().unwrap()).contains("PROBLEMS"));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus(), Focus::Editor);
}
#[test]
fn workspace_frames_are_cell_bounded_and_tiny_resize_never_panics() {
    let f = Fixture::new();
    let path = f.write("猫.c", "a\t猫e\u{301}👩‍💻");
    let mut app = Workspace::open(&f.0, 120, 30).unwrap();
    app.open_file(&path).unwrap();
    for (width, height) in [
        (0, 0),
        (1, 1),
        (2, 2),
        (10, 5),
        (30, 10),
        (80, 24),
        (120, 30),
        (65535, 65535),
    ] {
        app.handle(Event::Resize(width, height));
        for command in ["help", "explorer", "problems"] {
            app.command(command).unwrap();
            let frame = app.frame().unwrap();
            assert!(frame.rows.len() <= usize::from(height.min(256)));
            for row in &frame.rows {
                let text: String = row.runs.iter().map(|r| r.text.as_str()).collect();
                assert!(
                    UnicodeWidthStr::width(text.as_str())
                        <= usize::from(width.min(1024)).saturating_sub(1),
                    "{width} {height} {text:?}"
                );
                assert!(!text.contains('\x1b'));
            }
            if let Some((x, y)) = frame.cursor {
                assert!(x < width && y < height);
            }
        }
    }
}
#[test]
fn theme_change_invalidates_screen_once_and_identical_frames_produce_no_output() {
    let f = Fixture::new();
    let mut app = Workspace::open(&f.0, 100, 24).unwrap();
    let frame = app.frame().unwrap();
    let mut screen = Screen::default();
    let mut output = Vec::new();
    screen.draw(&frame, &mut output).unwrap();
    output.clear();
    assert!(!screen.draw(&frame, &mut output).unwrap());
    screen.set_theme(Theme::Light);
    assert!(screen.draw(&frame, &mut output).unwrap());
    assert!(!output.is_empty());
}
#[test]
fn flash_confirmation_cancel_never_executes_the_configured_backend() {
    let f = Fixture::new();
    let mut elf = [0; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[5] = 1;
    elf[18] = 40;
    fs::write(f.path("firmware.elf"), elf).unwrap();
    f.write("embedide.json", r#"{"version":1,"native":{"targets":[{"name":"Fixture Board","chip":"RP2040","probe":"1234:5678:EXACT","artifact":"firmware.elf","compiler_target":"thumbv6m-none-eabi"}],"tools":{"probe-rs":"must-not-execute"}}}"#);
    let mut app = Workspace::open(&f.0, 100, 28).unwrap();
    app.command("flash").unwrap();
    let preview = frame_text(&app.frame().unwrap());
    assert!(preview.contains("must-not-execute"));
    assert!(preview.contains("--verify"));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.notification(), "Cancelled");
    assert!(!app.tick());
}
#[cfg(unix)]
#[test]
fn editing_remains_live_during_background_build_and_diagnostics_jump_to_source() {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let source = f.write("main.c", "one\ntwo\nthree");
    f.write("Makefile", "unused:");
    let compiler = f.write(
        "fake compiler",
        "#!/bin/sh\nprintf 'main.c:2:1: error: fixture diagnostic\\n'\nsleep 0.2\nexit 2\n",
    );
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o700)).unwrap();
    f.write(
        "embedide.json",
        &serde_json::json!({"version":1,"native":{"tools":{"make":compiler}}}).to_string(),
    );
    let mut app = Workspace::open(&f.0, 100, 28).unwrap();
    app.open_file(&source).unwrap();
    app.command("build").unwrap();
    assert_eq!(app.focus(), Focus::Editor);
    text(&mut app, "iX");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.editor().state().buffer().contents(), "Xone\ntwo\nthree");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) && !app.notification().contains("Finished:") {
        app.tick();
        thread::sleep(Duration::from_millis(10));
    }
    assert!(app.notification().contains("Some(2)"));
    app.command("problems").unwrap();
    assert!(frame_text(&app.frame().unwrap()).contains("fixture diagnostic"));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus(), Focus::Editor);
    assert_eq!(app.editor().state().position().unwrap().line, 1);
    assert!(app.editor().state().is_modified());
}

#[test]
fn panels_work_without_the_alt_key_for_terminals_that_cannot_send_it() {
    // The FreeBSD console sends Alt+1 as a plain "1" (its keymap has no Alt prefix), so panels also answer to
    // the Normal-mode leader (Space) and Ctrl+] instead of Alt+digit and Alt+digit+Delete.
    let f = Fixture::new();
    f.write("a.c", "int main() {}");
    let mut app = Workspace::open(&f.0, 100, 28).unwrap();
    key(&mut app, KeyCode::Enter); // open a.c: editor focus, Normal mode
    assert_eq!(app.focus(), Focus::Editor);
    text(&mut app, " 1"); // Space 1: explorer
    assert_eq!(app.focus(), Focus::Explorer);
    ctrl(&mut app, ']'); // Ctrl+] moves focus on
    assert_ne!(app.focus(), Focus::Explorer);
    // back in the editor: Space 2 opens the AI panel, Space A hides it again
    for _ in 0..4 {
        if app.focus() == Focus::Editor {
            break;
        }
        ctrl(&mut app, ']');
    }
    assert_eq!(app.focus(), Focus::Editor);
    text(&mut app, " 2");
    assert_eq!(app.focus(), Focus::Assistant);
    ctrl(&mut app, ']');
    assert_eq!(app.focus(), Focus::Editor);
    text(&mut app, " A");
    assert!(
        !frame_text(&app.frame().unwrap()).contains("AI ·"),
        "the AI panel is hidden"
    );
    // and the explorer: Space E hides it
    text(&mut app, " E");
    assert!(!frame_text(&app.frame().unwrap()).contains("EXPLORER"));
    // the legacy-terminal encoding of Ctrl+] (0x1d) arrives as Ctrl+5
    text(&mut app, " 1");
    assert_eq!(app.focus(), Focus::Explorer);
    ctrl(&mut app, '5');
    assert_ne!(app.focus(), Focus::Explorer);
}
