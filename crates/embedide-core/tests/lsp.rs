//! LSP client against a scripted mock server (no real language server needed).
use embedide_core::{
    config::{CompletionConfig, LspServerConfig},
    lsp::{self, Client, Event},
    Editor,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Fixture {
    dir: PathBuf,
    log: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("embed lsp ж {name} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log.jsonl");
        Self { dir, log }
    }
    fn server(&self, mode: &str) -> LspServerConfig {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_lsp.py");
        LspServerConfig {
            command: "python3".into(),
            args: vec![
                script.display().to_string(),
                mode.into(),
                self.log.display().to_string(),
            ],
            initialization_options: Some(serde_json::json!({ "mock": true })),
        }
    }
    fn received(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn methods(&self) -> Vec<String> {
        self.received()
            .iter()
            .filter_map(|m| m["method"].as_str().map(str::to_string))
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn editor(path: &Path, text: &str, cursor: usize) -> Editor {
    let mut editor = Editor::create(path).unwrap();
    editor.replace(0..0, text).unwrap();
    // Completion happens in Insert mode, where the cursor may follow the last character.
    editor
        .feed(embedide_core::editor::vim::Key::Char('i'))
        .unwrap();
    editor.set_cursor(cursor).unwrap();
    editor
}

/// Poll until `pred` accepts an event (10 s limit).
fn wait(client: &mut Client, mut pred: impl FnMut(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in client.poll() {
            if pred(&event) {
                return event;
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for an event");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ready(client: &mut Client) {
    wait(client, |e| *e == Event::Ready);
}

#[test]
fn initialize_open_change_complete_and_shutdown() {
    let f = Fixture::new("flow");
    let path = f.dir.join("main file.rs");
    let mut client = Client::start("rust", &f.server("normal"), &f.dir).unwrap();
    assert!(!client.is_ready());
    let doc = editor(&path, "fn main() {\n    pr\n}\n", 18);
    assert_eq!(
        client.complete(&path, "rust", doc.state(), None),
        None,
        "not ready yet"
    );
    ready(&mut client);
    assert!(client.utf8(), "utf-8 negotiated");
    assert_eq!(client.trigger_characters(), [".", ":"]);
    let id = client.complete(&path, "rust", doc.state(), None).unwrap();
    let Event::Completion {
        request,
        items,
        incomplete,
    } = wait(&mut client, |e| matches!(e, Event::Completion { .. }))
    else {
        unreachable!()
    };
    assert_eq!((request, incomplete), (id, false));
    assert_eq!(items.len(), 2, "the malformed item is skipped");
    assert_eq!(items[0].label, "println!");
    assert_eq!(items[0].insert, "println!()", "snippet reduced to text");
    assert_eq!(items[0].kind, "function");
    assert_eq!(items[0].range, Some(((1, 4), (1, 6))));
    assert_eq!(items[1].kind, "variable");
    // Same revision: no second didOpen/didChange; an edit sends one full didChange.
    client.complete(&path, "rust", doc.state(), None).unwrap();
    let mut doc = doc;
    doc.replace(18..18, "i").unwrap();
    client
        .complete(&path, "rust", doc.state(), Some('.'))
        .unwrap();
    wait(&mut client, |e| matches!(e, Event::Completion { .. }));
    client.close(&path);
    drop(client);
    let received = f.received();
    let init = &received[0];
    assert_eq!(init["method"], "initialize");
    assert_eq!(init["params"]["initializationOptions"]["mock"], true);
    assert!(
        init["params"]["rootUri"]
            .as_str()
            .unwrap()
            .contains("%20%D0%B6%20"),
        "spaces and Cyrillic are percent-encoded"
    );
    let methods = f.methods();
    assert_eq!(
        methods
            .iter()
            .filter(|m| *m == "textDocument/didOpen")
            .count(),
        1
    );
    assert_eq!(
        methods
            .iter()
            .filter(|m| *m == "textDocument/didChange")
            .count(),
        1
    );
    let change = received
        .iter()
        .find(|m| m["method"] == "textDocument/didChange")
        .unwrap();
    assert_eq!(change["params"]["textDocument"]["version"], 2);
    assert_eq!(
        change["params"]["contentChanges"][0]["text"],
        "fn main() {\n    pri\n}\n"
    );
    let last = received
        .iter()
        .rev()
        .find(|m| m["method"] == "textDocument/completion")
        .unwrap();
    assert_eq!(last["params"]["context"]["triggerCharacter"], ".");
    // The server's workspace/configuration request was answered with one null per item.
    let answer = received.iter().find(|m| m["id"] == "cfg-1").unwrap();
    assert_eq!(answer["result"], serde_json::json!([null, null]));
    for expected in ["initialized", "textDocument/didClose", "shutdown", "exit"] {
        assert!(
            methods.iter().any(|m| m == expected),
            "missing {expected}: {methods:?}"
        );
    }
}

#[test]
fn stale_responses_are_ignored_and_superseded_requests_cancelled() {
    let f = Fixture::new("stale");
    let path = f.dir.join("a.c");
    let mut client = Client::start("c", &f.server("reverse"), &f.dir).unwrap();
    ready(&mut client);
    let mut doc = editor(&path, "in", 2);
    let first = client.complete(&path, "c", doc.state(), None).unwrap();
    doc.replace(2..2, "t").unwrap();
    let second = client.complete(&path, "c", doc.state(), None).unwrap();
    // The mock answers the second request first, then the stale first one.
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && seen.is_empty() {
        for event in client.poll() {
            if let Event::Completion { request, items, .. } = event {
                seen.push((request, items[1].detail.clone()));
            }
        }
    }
    std::thread::sleep(Duration::from_millis(200));
    for event in client.poll() {
        if let Event::Completion { request, .. } = event {
            seen.push((request, String::new()));
        }
    }
    assert_eq!(
        seen,
        vec![(second, "u32 at 0:3".to_string())],
        "first={first}"
    );
    drop(client);
    let cancel = f
        .received()
        .into_iter()
        .find(|m| m["method"] == "$/cancelRequest")
        .unwrap();
    assert_eq!(cancel["params"]["id"], first);
}

#[test]
fn utf16_servers_get_utf16_positions_and_ranges_convert_back() {
    let f = Fixture::new("utf16");
    let path = f.dir.join("u.rs");
    let mut client = Client::start("rust", &f.server("utf16"), &f.dir).unwrap();
    ready(&mut client);
    assert!(!client.utf8());
    // "😀" is 4 bytes / 2 UTF-16 units, "ж" 2 bytes / 1 unit.
    let text = "let 😀ж = pr";
    let doc = editor(&path, text, text.len());
    client.complete(&path, "rust", doc.state(), None).unwrap();
    wait(&mut client, |e| matches!(e, Event::Completion { .. }));
    drop(client);
    let request = f
        .received()
        .into_iter()
        .find(|m| m["method"] == "textDocument/completion")
        .unwrap();
    assert_eq!(request["params"]["position"]["character"], 12);
    let state = doc.state();
    assert_eq!(lsp::offset(state, 0, 12, false), Some(text.len()));
    assert_eq!(lsp::offset(state, 0, 6, false), Some("let 😀".len()));
    assert_eq!(lsp::offset(state, 0, 8, true), Some("let 😀".len()));
    assert_eq!(
        lsp::offset(state, 0, 999, true),
        Some(text.len()),
        "clamped to the line"
    );
    assert_eq!(
        lsp::cursor_position(state, true),
        Some((0, text.len() as u32))
    );
}

#[test]
fn crashes_and_protocol_errors_stop_the_client_without_panicking() {
    for mode in ["crash", "garbage"] {
        let f = Fixture::new(mode);
        let path = f.dir.join("z.zig");
        let mut client = Client::start("zig", &f.server(mode), &f.dir).unwrap();
        let stopped = wait(&mut client, |e| matches!(e, Event::Stopped(_)));
        assert!(client.is_stopped(), "{mode}: {stopped:?}");
        let doc = editor(&path, "x", 1);
        assert_eq!(client.complete(&path, "zig", doc.state(), None), None);
    }
}

#[test]
fn missing_servers_name_the_setting_and_configuration_selects_servers() {
    let f = Fixture::new("missing");
    let server = LspServerConfig {
        command: "embedide-no-such-language-server".into(),
        ..Default::default()
    };
    let error = Client::start("rust", &server, &f.dir)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("completion.servers.rust.command"), "{error}");
    let mut config = CompletionConfig::default();
    assert_eq!(
        lsp::server_for("rust", &config).unwrap().command,
        "rust-analyzer"
    );
    assert_eq!(lsp::server_for("cpp", &config).unwrap().command, "clangd");
    assert_eq!(lsp::server_for("zig", &config).unwrap().command, "zls");
    assert_eq!(
        lsp::server_for("fortran", &config).unwrap().command,
        "fortls"
    );
    let r_lsp = lsp::server_for("r", &config).unwrap();
    assert_eq!(r_lsp.command, "R");
    assert!(r_lsp.args.iter().any(|a| a.contains("languageserver::run")));
    assert!(lsp::server_for("asm", &config).is_none());
    let ra = lsp::server_for("rust", &config).unwrap();
    assert_eq!(
        ra.initialization_options.unwrap()["cargo"]["buildScripts"]["enable"],
        false,
        "build scripts are not run by default"
    );
    config
        .servers
        .insert("c".into(), LspServerConfig::default());
    assert!(
        lsp::server_for("c", &config).is_none(),
        "empty command disables"
    );
    config.lsp = false;
    assert!(lsp::server_for("rust", &config).is_none());
}

#[cfg(unix)]
#[test]
fn dropping_the_client_stops_the_server_process() {
    let f = Fixture::new("cleanup");
    let mut client = Client::start("rust", &f.server("normal"), &f.dir).unwrap();
    ready(&mut client);
    let pids = std::process::Command::new("pgrep")
        .args(["-f", &f.log.display().to_string()])
        .output()
        .unwrap();
    assert!(!pids.stdout.is_empty(), "mock server is running");
    drop(client);
    let pids = std::process::Command::new("pgrep")
        .args(["-f", &f.log.display().to_string()])
        .output()
        .unwrap();
    assert!(
        pids.stdout.is_empty(),
        "server survived: {:?}",
        String::from_utf8_lossy(&pids.stdout)
    );
}

#[test]
fn completion_results_of_every_shape_parse() {
    let (items, incomplete) = lsp::parse_completion(&serde_json::json!({
        "isIncomplete": true,
        "itemDefaults": {
            "editRange": { "insert": { "start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 3} },
                           "replace": { "start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 5} } },
            "insertTextFormat": 2
        },
        "items": [
            { "label": " vec!", "labelDetails": { "detail": "(…)" }, "textEditText": "vec![${1:x}]", "kind": 3 },
            { "label": "Some", "textEdit": { "newText": "Some", "insert": { "start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2} },
                                             "replace": { "start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 4} } } },
            { "label": 7 }
        ]
    }));
    assert!(incomplete);
    assert_eq!(items.len(), 2);
    assert_eq!(
        (items[0].label.as_str(), items[0].insert.as_str()),
        ("vec!(…)", "vec![x]")
    );
    assert_eq!(
        items[0].range,
        Some(((0, 1), (0, 3))),
        "itemDefaults insert range"
    );
    assert_eq!(
        items[1].range,
        Some(((0, 0), (0, 2))),
        "insert, not replace, range"
    );
    assert_eq!(
        lsp::parse_completion(&serde_json::json!([{ "label": "a" }]))
            .0
            .len(),
        1
    );
    assert!(lsp::parse_completion(&Value::Null).0.is_empty());
}
