//! Minimal Language Server Protocol client for code completion.
//!
//! One server process per command line (clangd serves C and C++). The
//! server is started without a shell, in its own process group, with stdin
//! and stdout as the JSON-RPC channel (stderr is discarded). Two threads
//! frame messages; the owning thread only enqueues and polls, so it never
//! waits for the server:
//!
//! * outgoing messages go through a bounded queue; when it is full a
//!   completion request is skipped and a document is marked for a full
//!   resync instead of blocking or dropping edits;
//! * only responses and server requests are forwarded (diagnostics and
//!   progress notifications are dropped in the reader thread);
//! * only the most recent completion request is answered to the caller;
//!   older ones are cancelled with `$/cancelRequest` and their late
//!   responses are ignored.
//!
//! Documents are synchronized lazily with full-text `didChange` right
//! before a completion request, so typing does not send a message per key.
//! Positions use UTF-8 when the server accepts it (LSP 3.17
//! `positionEncoding`, or clangd's `offsetEncoding`), UTF-16 otherwise.
use crate::{
    config::{CompletionConfig, LspServerConfig},
    editor::{completion::strip_snippet, EditorState},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

/// Largest accepted message; a larger frame is treated as a broken server.
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
/// Largest accepted header line before the body.
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Outgoing messages queued for the writer thread.
const OUTGOING: usize = 256;
/// Incoming responses/requests queued for the owner.
const INCOMING: usize = 64;
/// Completion items kept from one response.
const MAX_ITEMS: usize = 2000;

/// Built-in server for a language id (`rust`, `c`, `cpp`, `zig`, `fortran`, `r`).
///
/// rust-analyzer runs with build scripts and procedural macros disabled:
/// opening a file must not execute project code. Enable them through
/// `completion.servers.rust.initialization_options` if you want them.
pub fn default_server(language: &str) -> Option<LspServerConfig> {
    let (command, args, init) = match language {
        "rust" => (
            "rust-analyzer",
            vec![],
            Some(json!({
                "cargo": { "buildScripts": { "enable": false } },
                "procMacro": { "enable": false },
                "checkOnSave": false
            })),
        ),
        "c" | "cpp" => ("clangd", vec!["--header-insertion=never".to_string()], None),
        "zig" => ("zls", vec![], None),
        "fortran" => ("fortls", vec![], None),
        "r" => (
            "R",
            vec![
                "--slave".to_string(),
                "-e".to_string(),
                "languageserver::run()".to_string(),
            ],
            None,
        ),
        _ => return None,
    };
    Some(LspServerConfig {
        command: command.into(),
        args,
        initialization_options: init,
    })
}

/// The server to use for `language`, honouring configuration: an override
/// replaces the default, an empty command disables the language, and
/// `completion.lsp = false` disables all servers.
pub fn server_for(language: &str, config: &CompletionConfig) -> Option<LspServerConfig> {
    if !config.lsp {
        return None;
    }
    let server = config
        .servers
        .get(language)
        .cloned()
        .or_else(|| default_server(language))?;
    (!server.command.trim().is_empty()).then_some(server)
}

/// `file://` URI of an absolute path, percent-encoding everything but
/// unreserved characters and `/` (spaces, Cyrillic, … survive).
pub fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/'); // Windows drive letter
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// LSP position (line, character) of the cursor.
pub fn cursor_position(state: &EditorState, utf8: bool) -> Option<(u32, u32)> {
    let line = state.position().ok()?.line;
    let range = state.line_range(line)?;
    let before = state.buffer().slice(range.start..state.cursor()).ok()?;
    let character = if utf8 {
        before.len()
    } else {
        before.encode_utf16().count()
    };
    Some((u32::try_from(line).ok()?, u32::try_from(character).ok()?))
}

/// Byte offset of an LSP position, clamped to the end of its line.
pub fn offset(state: &EditorState, line: u32, character: u32, utf8: bool) -> Option<usize> {
    let range = state.line_range(line as usize)?;
    let text = state.buffer().slice(range.clone()).ok()?;
    let text = text.trim_end_matches(['\n', '\r']);
    let mut units = 0usize;
    for (index, ch) in text.char_indices() {
        if units >= character as usize {
            return Some(range.start + index);
        }
        units += if utf8 { ch.len_utf8() } else { ch.len_utf16() };
    }
    Some(range.start + text.len())
}

/// LSP range in (line, character) pairs, not yet converted to bytes.
pub type LspRange = ((u32, u32), (u32, u32));

/// One completion item as the server sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Shown text.
    pub label: String,
    /// Inserted text (snippets reduced to plain text).
    pub insert: String,
    /// Kind name (`function`, `struct`, …).
    pub kind: String,
    /// Type or signature.
    pub detail: String,
    /// Matched against the typed text.
    pub filter: String,
    /// Server order.
    pub sort: String,
    /// Range to replace, if the server sent one.
    pub range: Option<LspRange>,
}

/// Something the owner should know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Initialized; completion requests are accepted now.
    Ready,
    /// Server published diagnostics (indicates workspace indexing progressed).
    Indexed,
    /// Answer to the latest completion request.
    Completion {
        /// Id returned by [`Client::complete`].
        request: u64,
        /// Items, in server order.
        items: Vec<Item>,
        /// The server wants to be asked again as the user types.
        incomplete: bool,
    },
    /// The server failed or exited; the client is unusable.
    Stopped(String),
}

enum Incoming {
    Message(Value),
    Indexed,
    Closed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initializing,
    Ready,
    Stopped,
}

struct Document {
    version: i64,
    /// Buffer revision last sent, None when a resync is needed.
    synced: Option<u64>,
}

/// A running language server.
pub struct Client {
    name: String,
    child: Child,
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Receiver<Incoming>,
    phase: Phase,
    utf8: bool,
    next_id: u64,
    initialize_id: u64,
    latest: Option<u64>,
    documents: BTreeMap<PathBuf, Document>,
    trigger_characters: Vec<String>,
}

fn frame(value: &Value) -> Vec<u8> {
    let body = value.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Read one framed message; None at end of stream.
fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader
            .by_ref()
            .take(MAX_HEADER_BYTES as u64 + 1)
            .read_line(&mut line)?
            == 0
        {
            return Ok(None);
        }
        if line.len() > MAX_HEADER_BYTES {
            return Err(std::io::Error::other("oversized LSP header line"));
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let length = length
        .filter(|&n| n <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| std::io::Error::other("missing or oversized Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| std::io::Error::other(format!("invalid JSON: {e}")))
}

fn writer_thread(mut stdin: ChildStdin, outgoing: Receiver<Vec<u8>>) {
    for message in outgoing {
        if stdin
            .write_all(&message)
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
    // Dropping stdin closes the pipe: servers exit on EOF.
}

fn reader_thread(stdout: impl Read, incoming: SyncSender<Incoming>) {
    let mut reader = BufReader::new(stdout);
    let reason = loop {
        match read_message(&mut reader) {
            Ok(Some(message)) => {
                // Forward publishDiagnostics as a non-blocking readiness signal
                // (servers like rust-analyzer finish workspace indexing after initialize);
                // other notifications are dropped.
                if message.get("id").is_none() {
                    if message.get("method").and_then(Value::as_str)
                        == Some("textDocument/publishDiagnostics")
                    {
                        let _ = incoming.try_send(Incoming::Indexed);
                    }
                    continue;
                }
                if incoming.send(Incoming::Message(message)).is_err() {
                    return;
                }
            }
            Ok(None) => break "exited".to_string(),
            Err(e) => break format!("protocol error: {e}"),
        }
    };
    let _ = incoming.send(Incoming::Closed(reason));
}

fn kind_name(kind: u64) -> &'static str {
    const NAMES: [&str; 25] = [
        "text",
        "method",
        "function",
        "constructor",
        "field",
        "variable",
        "class",
        "interface",
        "module",
        "property",
        "unit",
        "value",
        "enum",
        "keyword",
        "snippet",
        "color",
        "file",
        "reference",
        "folder",
        "member",
        "constant",
        "struct",
        "event",
        "operator",
        "type",
    ];
    kind.checked_sub(1)
        .and_then(|i| NAMES.get(i as usize))
        .copied()
        .unwrap_or("")
}

fn range_of(value: &Value) -> Option<LspRange> {
    let point = |p: &Value| -> Option<(u32, u32)> {
        Some((
            u32::try_from(p.get("line")?.as_u64()?).ok()?,
            u32::try_from(p.get("character")?.as_u64()?).ok()?,
        ))
    };
    Some((point(value.get("start")?)?, point(value.get("end")?)?))
}

/// Parse a `textDocument/completion` result (`CompletionList`, an item
/// array, or null). Malformed items are skipped.
pub fn parse_completion(result: &Value) -> (Vec<Item>, bool) {
    let (list, incomplete, defaults) = match result {
        Value::Array(items) => (items.as_slice(), false, None),
        Value::Object(map) => (
            map.get("items")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            map.get("isIncomplete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            map.get("itemDefaults"),
        ),
        _ => (&[][..], false, None),
    };
    let default_range = defaults
        .and_then(|d| d.get("editRange"))
        .and_then(|r| r.get("insert").and_then(range_of).or_else(|| range_of(r)));
    let default_format = defaults
        .and_then(|d| d.get("insertTextFormat"))
        .and_then(Value::as_u64);
    let items = list
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
            let format = item
                .get("insertTextFormat")
                .and_then(Value::as_u64)
                .or(default_format);
            let edit = item.get("textEdit");
            let (raw, range) = match edit {
                Some(edit) => (
                    edit.get("newText")?.as_str()?.to_string(),
                    edit.get("insert")
                        .and_then(range_of)
                        .or_else(|| edit.get("range").and_then(range_of)),
                ),
                None => (
                    text("textEditText")
                        .or_else(|| text("insertText"))
                        .unwrap_or_else(|| label.clone()),
                    default_range,
                ),
            };
            let insert = if format == Some(2) {
                strip_snippet(&raw)
            } else {
                raw
            };
            let suffix = item
                .get("labelDetails")
                .and_then(|d| d.get("detail"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let detail = text("detail").unwrap_or_else(|| {
                item.get("labelDetails")
                    .and_then(|d| d.get("description"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            });
            Some(Item {
                filter: text("filterText").unwrap_or_else(|| label.trim().to_string()),
                sort: text("sortText").unwrap_or_default(),
                kind: kind_name(item.get("kind").and_then(Value::as_u64).unwrap_or(0)).into(),
                label: format!("{}{suffix}", label.trim()),
                detail: detail.lines().next().unwrap_or("").trim().to_string(),
                insert,
                range,
            })
        })
        .take(MAX_ITEMS)
        .collect();
    (items, incomplete)
}

/// Version number of a versioned clangd name: `clangd21` and `clangd-21` give 21, anything else None.
fn clangd_version(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("clangd")?;
    rest.strip_prefix('-').unwrap_or(rest).parse().ok()
}

/// The newest versioned clangd among `path_dirs` (`clangd21`, `clangd-21`) and the LLVM install directories
/// below `llvm_roots` (`/usr/local/llvm21/bin/clangd`, `/usr/lib/llvm-21/bin/clangd`).
fn newest_clangd(path_dirs: &[PathBuf], llvm_roots: &[PathBuf]) -> Option<PathBuf> {
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for dir in path_dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if let Some(v) = name.to_str().and_then(clangd_version) {
                if entry.path().is_file() {
                    found.push((v, entry.path()));
                }
            }
        }
    }
    for root in llvm_roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let digits = name.to_str().and_then(|n| n.strip_prefix("llvm"));
            if let Some(v) = digits.and_then(|d| d.trim_start_matches('-').parse::<u32>().ok()) {
                let candidate = entry.path().join("bin/clangd");
                if candidate.is_file() {
                    found.push((v, candidate));
                }
            }
        }
    }
    found.into_iter().max_by_key(|(v, _)| *v).map(|(_, p)| p)
}

/// The program to start for the configured `command`. A bare `clangd` that is not on PATH is looked up under
/// the names packages give it: FreeBSD installs `clangd21` and `/usr/local/llvm21/bin/clangd`, Debian
/// `clangd-21` and `/usr/lib/llvm-21/bin/clangd`. The newest version found wins; anything else is used as given.
fn resolve_program(command: &str) -> std::ffi::OsString {
    if command != "clangd" {
        return command.into();
    }
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if path_dirs.iter().any(|d| d.join("clangd").is_file()) {
        return command.into();
    }
    let roots = [
        PathBuf::from("/usr/local"),
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/lib64"),
    ];
    newest_clangd(&path_dirs, &roots).map_or_else(|| command.into(), Into::into)
}

impl Client {
    /// Start `server` for the workspace `root` and send `initialize`.
    /// A missing executable is an error naming the configuration key.
    pub fn start(language: &str, server: &LspServerConfig, root: &Path) -> crate::Result<Self> {
        let mut command = Command::new(resolve_program(&server.command));
        command
            .args(&server.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|e| {
            crate::Error(format!(
                "{} could not start ({e}); install it or set completion.servers.{language}.command",
                server.command
            ))
        })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (to_writer, outgoing) = mpsc::sync_channel(OUTGOING);
        let (to_owner, incoming) = mpsc::sync_channel(INCOMING);
        thread::spawn(move || writer_thread(stdin, outgoing));
        thread::spawn(move || reader_thread(stdout, to_owner));
        let name = Path::new(&server.command)
            .file_name()
            .map_or(server.command.clone(), |n| n.to_string_lossy().into_owned());
        let mut client = Self {
            name,
            child,
            outgoing: Some(to_writer),
            incoming,
            phase: Phase::Initializing,
            utf8: false,
            next_id: 1,
            initialize_id: 0,
            latest: None,
            documents: BTreeMap::new(),
            trigger_characters: Vec::new(),
        };
        let root_uri = file_uri(root);
        let folder = root
            .file_name()
            .map_or_else(|| "workspace".into(), |n| n.to_string_lossy().into_owned());
        client.initialize_id = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "EmbedIDE", "version": env!("CARGO_PKG_VERSION") },
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": folder }],
                "initializationOptions": server.initialization_options,
                "capabilities": {
                    "general": { "positionEncodings": ["utf-8", "utf-16"] },
                    "offsetEncoding": ["utf-8", "utf-16"],
                    "textDocument": {
                        "synchronization": { "dynamicRegistration": false },
                        "completion": {
                            "contextSupport": true,
                            "completionItem": {
                                "snippetSupport": false,
                                "insertReplaceSupport": true,
                                "labelDetailsSupport": true
                            },
                            "completionList": { "itemDefaults": ["editRange", "insertTextFormat"] }
                        }
                    },
                    "window": { "workDoneProgress": false }
                }
            }),
        )
        .ok_or_else(|| crate::Error(format!("{} did not accept input", server.command)))?;
        Ok(client)
    }

    /// Program name, for status messages.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ready for completion requests.
    pub fn is_ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Stopped (crashed, exited, or failed to initialize).
    pub fn is_stopped(&self) -> bool {
        self.phase == Phase::Stopped
    }

    /// Characters after which the server offers member completion (`.`, `::`, `->`).
    pub fn trigger_characters(&self) -> &[String] {
        &self.trigger_characters
    }

    fn send(&mut self, message: Value) -> bool {
        let Some(outgoing) = &self.outgoing else {
            return false;
        };
        match outgoing.try_send(frame(&message)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => {
                self.phase = Phase::Stopped;
                false
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Option<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .then_some(id)
    }

    fn notify(&mut self, method: &str, params: Value) -> bool {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Bring the server's copy of `path` up to date (full text).
    fn sync(&mut self, path: &Path, language: &str, state: &EditorState) -> bool {
        let revision = state.buffer().revision();
        let uri = file_uri(path);
        match self.documents.get(path).map(|d| (d.version, d.synced)) {
            Some((_, Some(synced))) if synced == revision => true,
            Some((version, _)) => {
                let version = version + 1;
                let sent = self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": version },
                        "contentChanges": [{ "text": state.buffer().contents() }]
                    }),
                );
                self.documents.insert(
                    path.to_path_buf(),
                    Document {
                        version,
                        synced: sent.then_some(revision),
                    },
                );
                sent
            }
            None => {
                let sent = self.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": {
                        "uri": uri, "languageId": language, "version": 1,
                        "text": state.buffer().contents()
                    }}),
                );
                if sent {
                    self.documents.insert(
                        path.to_path_buf(),
                        Document {
                            version: 1,
                            synced: Some(revision),
                        },
                    );
                }
                sent
            }
        }
    }

    /// Ask for completions at the cursor of `state` (the document at
    /// `path`). Returns the request id, or None when the server is not
    /// ready or its queue is full. Supersedes any earlier request.
    pub fn complete(
        &mut self,
        path: &Path,
        language: &str,
        state: &EditorState,
        trigger: Option<char>,
    ) -> Option<u64> {
        if self.phase != Phase::Ready || !self.sync(path, language, state) {
            return None;
        }
        let (line, character) = cursor_position(state, self.utf8)?;
        if let Some(old) = self.latest.take() {
            self.notify("$/cancelRequest", json!({ "id": old }));
        }
        let context = match trigger {
            Some(ch) => json!({ "triggerKind": 2, "triggerCharacter": ch.to_string() }),
            None => json!({ "triggerKind": 1 }),
        };
        let id = self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": file_uri(path) },
                "position": { "line": line, "character": character },
                "context": context
            }),
        )?;
        self.latest = Some(id);
        Some(id)
    }

    /// Positions this server expects (true: UTF-8 bytes, false: UTF-16 units).
    pub fn utf8(&self) -> bool {
        self.utf8
    }

    /// Tell the server a document was closed.
    pub fn close(&mut self, path: &Path) {
        if self.documents.remove(path).is_some() {
            let uri = file_uri(path);
            self.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri } }),
            );
        }
    }

    /// Process queued server messages; never blocks.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let message = match self.incoming.try_recv() {
                Ok(Incoming::Message(message)) => message,
                Ok(Incoming::Indexed) => {
                    if self.phase == Phase::Ready {
                        events.push(Event::Indexed);
                    }
                    continue;
                }
                Ok(Incoming::Closed(reason)) => {
                    if self.phase != Phase::Stopped {
                        self.phase = Phase::Stopped;
                        events.push(Event::Stopped(format!("{} {reason}", self.name)));
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            };
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                // A request from the server: answer so it never waits on us.
                let id = message["id"].clone();
                let result = match method {
                    "workspace/configuration" => {
                        let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; count])
                    }
                    _ => Value::Null,
                };
                self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                continue;
            }
            let Some(id) = message.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if id == self.initialize_id && self.phase == Phase::Initializing {
                if let Some(error) = message.get("error") {
                    self.phase = Phase::Stopped;
                    events.push(Event::Stopped(format!(
                        "{} refused to initialize: {}",
                        self.name,
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error")
                    )));
                    continue;
                }
                let result = &message["result"];
                let encoding = result["capabilities"]["positionEncoding"]
                    .as_str()
                    .or_else(|| result["offsetEncoding"].as_str())
                    .unwrap_or("utf-16");
                self.utf8 = encoding == "utf-8";
                self.trigger_characters = result["capabilities"]["completionProvider"]
                    ["triggerCharacters"]
                    .as_array()
                    .map(|t| {
                        t.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                self.notify("initialized", json!({}));
                self.phase = Phase::Ready;
                events.push(Event::Ready);
                continue;
            }
            if Some(id) != self.latest {
                continue; // superseded or unrelated
            }
            self.latest = None;
            let (items, incomplete) = match message.get("result") {
                Some(result) => parse_completion(result),
                None => (Vec::new(), false), // error response: no items
            };
            events.push(Event::Completion {
                request: id,
                items,
                incomplete,
            });
        }
        events
    }
}

impl Drop for Client {
    /// `shutdown` + `exit`, close stdin, give the server a moment, then
    /// stop its whole process group and reap it.
    fn drop(&mut self) {
        if self.phase == Phase::Ready {
            let _ = self.request("shutdown", Value::Null);
            let _ = self.notify("exit", Value::Null);
        }
        self.outgoing = None;
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return; // exited by itself (and was reaped): its pid may be reused now
            }
            thread::sleep(Duration::from_millis(10));
        }
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{killpg, Signal},
                unistd::Pid,
            };
            if let Ok(pid) = i32::try_from(self.child.id()) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod clangd_discovery_tests {
    use super::{clangd_version, newest_clangd, read_message, MAX_HEADER_BYTES};
    use std::{fs, io::Cursor, path::PathBuf};

    #[test]
    fn oversized_lsp_header_line_is_rejected() {
        let huge = "X-Junk: ".to_string() + &"a".repeat(MAX_HEADER_BYTES + 16) + "\r\n\r\n";
        let mut cursor = Cursor::new(huge.as_bytes());
        let err = read_message(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("oversized"), "{err}");
    }

    fn touch(path: &PathBuf) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn versioned_names_are_recognised() {
        assert_eq!(clangd_version("clangd21"), Some(21));
        assert_eq!(clangd_version("clangd-19"), Some(19));
        assert_eq!(clangd_version("clangd"), None);
        assert_eq!(clangd_version("clangd-tidy"), None);
        assert_eq!(clangd_version("clang21"), None);
    }

    #[test]
    fn the_newest_clangd_is_found_in_path_and_in_llvm_install_directories() {
        let base = std::env::temp_dir().join(format!("embedide-clangd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let bin = base.join("bin");
        touch(&bin.join("clangd19"));
        touch(&bin.join("clangd-15"));
        touch(&bin.join("clang-format21")); // not a clangd
        assert_eq!(
            newest_clangd(std::slice::from_ref(&bin), &[]),
            Some(bin.join("clangd19"))
        );
        // an LLVM directory with a newer version wins; one without bin/clangd is ignored
        touch(&base.join("llvm21/bin/clangd"));
        fs::create_dir_all(base.join("llvm22/bin")).unwrap();
        touch(&base.join("lib/llvm-20/bin/clangd"));
        assert_eq!(
            newest_clangd(std::slice::from_ref(&bin), std::slice::from_ref(&base)),
            Some(base.join("llvm21/bin/clangd"))
        );
        assert_eq!(
            newest_clangd(&[], &[base.join("lib")]),
            Some(base.join("lib/llvm-20/bin/clangd"))
        );
        assert_eq!(newest_clangd(&[], &[]), None);
        let _ = fs::remove_dir_all(&base);
    }
}
