//! Black-box tests use the freshly compiled standalone frontend executable.
mod common;
use common::Fixture;
use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(path: &std::path::Path, input: &[u8], batch: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_embedide-terminal"));
    if batch {
        command.arg("--batch");
    }
    let mut child = command
        .arg("--")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if batch {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn actual_binary_edits_saves_reopens_and_preserves_unicode_paths() {
    let f = Fixture::new();
    let path = f.write("space ; $() 猫.c", "old value\n");
    let output = run(&path, "ciwновое<Esc>\n:wq\n".as_bytes(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "новое value\n");
    let output = run(&path, b"A!<Esc>\n:wq\n", true);
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "новое value!\n");
}

#[test]
fn actual_binary_creates_empty_file_only_on_explicit_save() {
    let f = Fixture::new();
    let path = f.path("new.rs");
    assert!(run(&path, b":q\n", true).status.success());
    assert!(!path.exists());
    assert!(run(&path, b":wq\n", true).status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "");
}

#[test]
fn binary_eof_and_dirty_quit_fail_force_quit_is_explicit() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    for input in ["A!<Esc>\n", "A!<Esc>\n:q\n"] {
        let output = run(&path, input.as_bytes(), true);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsaved"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    }
    assert!(run(&path, b"A!<Esc>\n:q!\n", true).status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[test]
fn pipes_require_batch_and_malformed_arguments_are_usage_errors() {
    let f = Fixture::new();
    let path = f.path("new.rs");
    let output = run(&path, b"", false);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--batch"));
    assert!(!path.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_embedide-terminal"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
