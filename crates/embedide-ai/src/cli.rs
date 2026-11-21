//! The providers' official command-line tools as AI backends:
//! Claude Code (`claude`) and OpenAI Codex CLI (`codex`).
//!
//! EmbedIDE never handles these tools' credentials. It runs the installed
//! CLI as a child process, which uses its own login (a Claude or ChatGPT
//! subscription, or whatever the user configured in that CLI), and reads
//! the CLI's documented machine-readable output:
//!
//! * Claude Code: `claude -p --output-format stream-json --verbose
//!   --include-partial-messages --tools "" --no-session-persistence`, the
//!   prompt on stdin. `--tools ""` gives the model no tools at all, so it
//!   can neither read nor change files nor run commands.
//! * Codex: `codex exec --json --sandbox read-only --ephemeral
//!   --skip-git-repo-check --color never -C <empty dir> -`. Codex keeps its
//!   shell tool, but its sandbox forbids writes and network access; it can
//!   still read files your user can read.
//!
//! Capabilities come from the installed tool's own `--help`; an older CLI
//! without the structured mode falls back to plain text output. Login state
//! comes from `claude auth status --json` / `codex login status`; only the
//! method and plan are kept (never e-mail, organisation or tokens).
//!
//! Each request runs in a fresh empty temporary directory, in its own
//! process group (cancellation stops the whole group), with a time limit
//! and bounded output. The child's environment drops the variables that
//! bind a process to a *parent* Claude Code session (so running `embed`
//! inside Claude Code does not attach the child to that session), and drops
//! provider API-key variables unless `ai.cli.allow_api_key` is set, so a
//! subscription request never silently turns into a billed API request.
use crate::{
    providers::Subscription, Backend, Cancel, Error, ModelInfo, ProviderId, Request, Response,
    Result, Role, Usage,
};
use embedide_workspace::config::CliConfig;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

/// Variables that bind a process to a parent Claude Code session.
const PARENT_SESSION_VARS: [&str; 11] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_HOST_SHORT_TEMP_DIRS",
    "CLAUDE_PID",
    "CLAUDE_JOB_DIR",
];
/// Time allowed for `--version`, `--help` and login-status probes.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
/// Probe output kept.
const PROBE_BYTES: usize = 256 * 1024;
/// stderr kept for error messages (the tail).
const STDERR_TAIL: usize = 16 * 1024;

/// Which official CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CliKind {
    /// Anthropic Claude Code.
    ClaudeCode,
    /// OpenAI Codex CLI.
    Codex,
}

impl CliKind {
    /// Both CLIs.
    pub const ALL: [CliKind; 2] = [Self::ClaudeCode, Self::Codex];

    /// Backend id used in the AI panel and configuration.
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex-cli",
        }
    }
    /// Executable name.
    pub fn program(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex CLI",
        }
    }
    /// Provider family whose models the CLI serves.
    pub fn provider(self) -> ProviderId {
        match self {
            Self::ClaudeCode => ProviderId::Anthropic,
            Self::Codex => ProviderId::Openai,
        }
    }
    /// The CLI for a provider, if it has one.
    pub fn for_provider(provider: ProviderId) -> Option<Self> {
        match provider.info().subscription {
            Subscription::ClaudeCode => Some(Self::ClaudeCode),
            Subscription::Codex => Some(Self::Codex),
            Subscription::NotIntegrated(_) => None,
        }
    }
    /// Parse `claude-code` / `codex-cli` (and `claude`, `codex`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "codex-cli" | "codex" => Some(Self::Codex),
            _ => None,
        }
    }
    /// Official login command, for messages.
    pub fn login_hint(self) -> &'static str {
        match self {
            Self::ClaudeCode => {
                "run `claude` once in a terminal and log in (or `claude auth login`)"
            }
            Self::Codex => "run `codex login` in a terminal",
        }
    }
    fn key_vars(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
            Self::Codex => &["OPENAI_API_KEY", "CODEX_API_KEY"],
        }
    }
}

/// What the installed CLI's `--help` says it supports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Non-interactive mode exists (`claude -p`, `codex exec`).
    pub headless: bool,
    /// Structured output (`stream-json`, `exec --json`).
    pub structured: bool,
    /// Token-level partial messages (Claude Code).
    pub partial: bool,
    /// Tools can be disabled (`--tools ""`, Claude Code).
    pub no_tools: bool,
    /// Sessions are not saved (`--no-session-persistence`, `--ephemeral`).
    pub ephemeral: bool,
}

impl Caps {
    /// Capabilities from `claude --help` or `codex exec --help` text.
    pub fn from_help(kind: CliKind, help: &str) -> Self {
        let has = |flag: &str| help.contains(flag);
        match kind {
            CliKind::ClaudeCode => Self {
                headless: has("--print"),
                structured: has("stream-json") && has("--verbose"),
                partial: has("--include-partial-messages"),
                no_tools: has("--tools "),
                ephemeral: has("--no-session-persistence"),
            },
            CliKind::Codex => Self {
                headless: has("--sandbox"),
                structured: has("--json"),
                partial: false,
                no_tools: false,
                ephemeral: has("--ephemeral"),
            },
        }
    }
}

/// Login state as the CLI reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    /// Logged in: method (`claude.ai`, `ChatGPT`, `API key`, …) and plan, if shown.
    LoggedIn {
        /// How the CLI authenticates.
        method: String,
        /// Subscription plan, when the CLI reports one.
        plan: Option<String>,
    },
    /// The CLI says it is not logged in.
    LoggedOut,
    /// Could not be determined (with the reason).
    Unknown(String),
}

/// Everything known about one installed CLI.
#[derive(Debug, Clone)]
pub struct CliInfo {
    /// Which CLI.
    pub kind: CliKind,
    /// Executable used.
    pub program: PathBuf,
    /// Version string, e.g. `2.1.322`.
    pub version: Option<String>,
    /// Supported modes.
    pub caps: Caps,
    /// Login state.
    pub auth: AuthState,
}

impl CliInfo {
    /// Usable for requests (a headless mode exists).
    pub fn supported(&self) -> bool {
        self.caps.headless
    }
    /// One line for the AI panel; never contains secrets.
    pub fn summary(&self) -> String {
        let version = self.version.as_deref().unwrap_or("unknown version");
        let state = if !self.supported() {
            format!(
                "unsupported: this {} has no non-interactive mode; update it",
                self.kind.name()
            )
        } else {
            match &self.auth {
                AuthState::LoggedIn { method, plan } => format!(
                    "ready, logged in via {method}{}",
                    plan.as_ref().map(|p| format!(" ({p})")).unwrap_or_default()
                ),
                AuthState::LoggedOut => format!("not logged in: {}", self.kind.login_hint()),
                AuthState::Unknown(why) => format!("login state unknown ({why})"),
            }
        };
        let mode = if self.caps.structured {
            "streaming"
        } else {
            "plain text"
        };
        format!(
            "{} {version} at {} — {state}; {mode} output",
            self.kind.name(),
            self.program.display()
        )
    }
}

/// Find the CLI: an explicit path from configuration, otherwise `PATH`.
pub fn locate(kind: CliKind, configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = configured {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(Error::Config(format!(
                "{} is configured at {}, which does not exist",
                kind.name(),
                path.display()
            )))
        };
    }
    let name = kind.program().to_string();
    std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join(&name)).find(|p| p.is_file()))
        .ok_or_else(|| {
            Error::Config(format!(
                "{} (`{}`) was not found on PATH. Install it and log in with it, or set its path (ai.providers.{}.cli)",
                kind.name(),
                kind.program(),
                kind.provider().id()
            ))
        })
}

/// `major.minor.patch` from a version line such as `2.1.322 (Claude Code)`
/// or `codex-cli 0.163.0`.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|word| {
            let mut parts = word.split('.').map(|p| p.parse::<u32>().ok());
            let major = parts.next()??;
            let minor = parts.next()??;
            Some((major, minor, parts.next().flatten().unwrap_or(0)))
        })
}

/// The child environment: no parent-session binding, no API keys unless allowed.
pub(crate) fn command(kind: CliKind, program: &Path, settings: &CliConfig) -> Command {
    let mut cmd = Command::new(program);
    for var in PARENT_SESSION_VARS {
        cmd.env_remove(var);
    }
    if !settings.allow_api_key {
        for var in kind.key_vars() {
            cmd.env_remove(var);
        }
    }
    cmd.env("NO_COLOR", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

/// Stop the child's whole process group and reap it.
pub(crate) fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{killpg, Signal},
            unistd::Pid,
        };
        if let Ok(pid) = i32::try_from(child.id()) {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
                let deadline = Instant::now() + Duration::from_millis(500);
                while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
                    thread::sleep(Duration::from_millis(10));
                }
            }
            // Always: a launcher (codex.js) may exit while the real CLI in
            // its group is still shutting down "gracefully".
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Read up to `limit` bytes on a thread; the rest is drained and dropped.
pub(crate) fn collect(
    mut reader: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let room = limit.saturating_sub(kept.len());
            kept.extend_from_slice(&buf[..n.min(room)]);
        }
        kept
    })
}

/// Run a short probe command with a time limit.
pub(crate) fn probe_output(
    kind: CliKind,
    program: &Path,
    args: &[&str],
    settings: &CliConfig,
) -> Option<(ExitStatus, String)> {
    let mut cmd = command(kind, program, settings);
    cmd.args(args)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(&mut cmd).ok()?;
    let out = collect(child.stdout.take()?, PROBE_BYTES);
    let err = collect(child.stderr.take()?, PROBE_BYTES);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < PROBE_TIMEOUT => thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(&mut child);
                return None;
            }
        }
    };
    let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    text.push_str(&String::from_utf8_lossy(&err.join().unwrap_or_default()));
    Some((status, text))
}

fn caps_cache() -> &'static Mutex<HashMap<PathBuf, Caps>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Caps>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Capabilities of the installed CLI (cached per executable).
pub fn caps(kind: CliKind, program: &Path, settings: &CliConfig) -> Caps {
    if let Some(caps) = caps_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(program).copied())
    {
        return caps;
    }
    let args: &[&str] = match kind {
        CliKind::ClaudeCode => &["--help"],
        CliKind::Codex => &["exec", "--help"],
    };
    let Some((_, help)) = probe_output(kind, program, args, settings) else {
        return Caps::default(); // not cached: the next request probes again
    };
    let caps = Caps::from_help(kind, &help);
    if let Ok(mut cache) = caps_cache().lock() {
        cache.insert(program.to_path_buf(), caps);
    }
    caps
}

/// Spawn, retrying briefly when the executable was just written
/// (ETXTBSY: another thread's fork still held it open for writing).
pub(crate) fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 20 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

/// Parse `claude auth status --json` / `codex login status` output.
pub fn parse_auth(kind: CliKind, success: bool, text: &str) -> AuthState {
    match kind {
        CliKind::ClaudeCode => {
            let json = text
                .find('{')
                .and_then(|i| serde_json::from_str::<Value>(&text[i..]).ok());
            match json {
                Some(v) if v["loggedIn"] == true => AuthState::LoggedIn {
                    method: v["authMethod"].as_str().unwrap_or("unknown").to_string(),
                    plan: v["subscriptionType"].as_str().map(str::to_string),
                },
                Some(v) if v["loggedIn"] == false => AuthState::LoggedOut,
                _ => AuthState::Unknown("`claude auth status` gave no status".into()),
            }
        }
        CliKind::Codex => {
            let line = text
                .lines()
                .map(str::trim)
                .find(|l| {
                    l.to_ascii_lowercase().contains("logged in")
                        || l.to_ascii_lowercase().contains("not logged")
                })
                .unwrap_or("");
            let lower = line.to_ascii_lowercase();
            if lower.contains("not logged in") || (!success && lower.is_empty()) {
                AuthState::LoggedOut
            } else if let Some(rest) = lower.strip_prefix("logged in using ") {
                let method = if rest.contains("chatgpt") {
                    "ChatGPT".to_string()
                } else if rest.contains("api key") {
                    "API key".to_string()
                } else {
                    redact(rest.trim())
                };
                AuthState::LoggedIn { method, plan: None }
            } else {
                AuthState::Unknown("`codex login status` gave no status".into())
            }
        }
    }
}

/// Inspect an installed CLI: version, capabilities and login state.
pub fn inspect(kind: CliKind, program: &Path, settings: &CliConfig) -> CliInfo {
    let version = probe_output(kind, program, &["--version"], settings)
        .and_then(|(_, text)| parse_version(&text).map(|(a, b, c)| format!("{a}.{b}.{c}")));
    let caps = caps(kind, program, settings);
    let args: &[&str] = match kind {
        CliKind::ClaudeCode => &["auth", "status", "--json"],
        CliKind::Codex => &["login", "status"],
    };
    let auth = match probe_output(kind, program, args, settings) {
        Some((status, text)) => parse_auth(kind, status.success(), &text),
        None => AuthState::Unknown("the status command did not answer in time".into()),
    };
    CliInfo {
        kind,
        program: program.to_path_buf(),
        version,
        caps,
        auth,
    }
}

/// Remove things that look like credentials from text shown to the user.
pub fn redact(text: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|word| {
            let bare = word.trim();
            let secretish = bare.starts_with("sk-")
                || bare.starts_with("sk_")
                || bare.starts_with("eyJ")
                || (bare.len() >= 32
                    && bare
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)));
            if secretish {
                word.replace(bare, "[redacted]")
            } else {
                word.to_string()
            }
        })
        .collect()
}

/// Model output without terminal control sequences (a reply must never
/// move the cursor or change colours in the IDE).
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // CSI … final byte, OSC … BEL/ST, or a single escape.
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if ch == '\n' || ch == '\t' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

/// Turn a CLI error message into an [`Error`] with an actionable hint.
pub fn classify(kind: CliKind, message: &str, code: Option<i32>) -> Error {
    let message: String = redact(sanitize(message).trim()).chars().take(600).collect();
    let lower = message.to_ascii_lowercase();
    let auth = [
        "not logged in",
        "please run /login",
        "login",
        "log in",
        "authenticat",
        "unauthorized",
        "invalid api key",
        "401",
        "token has expired",
        "expired",
    ];
    let limit = [
        "rate limit",
        "rate_limit",
        "usage limit",
        "limit reached",
        "429",
        "quota",
        "too many requests",
    ];
    if limit.iter().any(|p| lower.contains(p)) {
        return Error::RateLimited(None, format!("{}: {message}", kind.name()));
    }
    if auth.iter().any(|p| lower.contains(p)) {
        return Error::Auth(format!(
            "{}: {message} — {}",
            kind.name(),
            kind.login_hint()
        ));
    }
    let code = code.and_then(|c| u16::try_from(c).ok()).unwrap_or(1);
    Error::Provider(
        code,
        if message.is_empty() {
            format!("{} exited without an answer", kind.name())
        } else {
            format!("{}: {message}", kind.name())
        },
    )
}

/// The conversation as one prompt (the CLIs take a single prompt).
fn prompt_text(kind: CliKind, req: &Request) -> String {
    let mut s = String::new();
    if let Some(sys) = &req.system {
        s.push_str(sys);
        s.push_str("\n\n");
    }
    if kind == CliKind::Codex {
        s.push_str("Answer in text only. Do not run commands and do not modify files; the user reviews every change in EmbedIDE.\n\n");
    }
    for m in &req.messages {
        s.push_str(match m.role {
            Role::User => "User: ",
            Role::Assistant => "Assistant: ",
        });
        s.push_str(&m.content);
        s.push_str("\n\n");
    }
    s
}

/// What a request produced so far.
#[derive(Debug, Default)]
struct Outcome {
    text: String,
    streamed: bool,
    usage: Usage,
    error: Option<String>,
    model: Option<String>,
}

/// Apply one structured output line; returns the text to stream, if any.
fn event(kind: CliKind, line: &[u8], out: &mut Outcome) -> Option<String> {
    let value: Value = serde_json::from_slice(line).ok()?;
    let usage = |u: &Value| Usage {
        input_tokens: u["input_tokens"].as_u64(),
        output_tokens: u["output_tokens"].as_u64(),
    };
    match kind {
        CliKind::ClaudeCode => match value["type"].as_str()? {
            "stream_event" => {
                let ev = &value["event"];
                if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                    let text = sanitize(ev["delta"]["text"].as_str()?);
                    out.text.push_str(&text);
                    out.streamed = true;
                    return Some(text);
                }
                None
            }
            "assistant" => {
                out.model = value["message"]["model"]
                    .as_str()
                    .map(str::to_string)
                    .or(out.model.take());
                if out.streamed {
                    return None;
                }
                let text: String = value["message"]["content"]
                    .as_array()?
                    .iter()
                    .filter(|c| c["type"] == "text")
                    .filter_map(|c| c["text"].as_str())
                    .collect();
                let text = sanitize(&text);
                out.text.push_str(&text);
                Some(text)
            }
            "result" => {
                out.usage = usage(&value["usage"]);
                if value["is_error"] == true
                    || value["subtype"].as_str().is_some_and(|s| s != "success")
                {
                    out.error = Some(value["result"].as_str().map(str::to_string).unwrap_or_else(
                        || value["subtype"].as_str().unwrap_or("error").to_string(),
                    ));
                } else if out.text.is_empty() {
                    let text = sanitize(value["result"].as_str().unwrap_or(""));
                    out.text.push_str(&text);
                    return Some(text);
                }
                None
            }
            _ => None,
        },
        CliKind::Codex => match value["type"].as_str()? {
            "item.completed" if value["item"]["type"] == "agent_message" => {
                let mut text = sanitize(value["item"]["text"].as_str()?);
                if !out.text.is_empty() {
                    text.insert_str(0, "\n\n");
                }
                out.text.push_str(&text);
                Some(text)
            }
            "turn.completed" => {
                out.usage = usage(&value["usage"]);
                None
            }
            "turn.failed" => {
                out.error = Some(
                    value["error"]["message"]
                        .as_str()
                        .unwrap_or("turn failed")
                        .to_string(),
                );
                None
            }
            "error" => {
                out.error = Some(value["message"].as_str().unwrap_or("error").to_string());
                None
            }
            _ => None,
        },
    }
}

pub(crate) struct CliBackend {
    kind: CliKind,
    program: PathBuf,
    settings: CliConfig,
}

/// A backend for the CLI of `provider` (Anthropic → Claude Code, OpenAI → Codex).
pub(crate) fn backend(
    provider: ProviderId,
    configured: Option<PathBuf>,
    settings: CliConfig,
) -> Result<Box<dyn Backend>> {
    let kind =
        CliKind::for_provider(provider).ok_or_else(|| match provider.info().subscription {
            Subscription::NotIntegrated(why) => Error::Config(format!(
                "Subscription mode is not available for {}: {why}",
                provider.info().name
            )),
            _ => Error::Config("no CLI".into()),
        })?;
    let program = locate(kind, configured.as_deref())?;
    Ok(Box::new(CliBackend {
        kind,
        program,
        settings,
    }))
}

impl CliBackend {
    fn args(&self, caps: Caps, req: &Request, work: &Path) -> Result<Vec<String>> {
        if req.model.starts_with('-') {
            return Err(Error::Config("Model id cannot start with '-'".into()));
        }
        let model = (!req.model.is_empty() && req.model != "default").then(|| req.model.clone());
        let mut args: Vec<String> = Vec::new();
        match self.kind {
            CliKind::ClaudeCode => {
                args.push("-p".into());
                if caps.structured {
                    args.extend(["--output-format", "stream-json", "--verbose"].map(String::from));
                    if caps.partial {
                        args.push("--include-partial-messages".into());
                    }
                } else {
                    args.extend(["--output-format", "text"].map(String::from));
                }
                if caps.no_tools {
                    args.extend(["--tools".into(), String::new()]);
                }
                if caps.ephemeral {
                    args.push("--no-session-persistence".into());
                }
                if let Some(m) = model {
                    args.extend(["--model".into(), m]);
                }
            }
            CliKind::Codex => {
                args.extend(
                    ["exec", "--skip-git-repo-check", "--sandbox", "read-only"].map(String::from),
                );
                if caps.structured {
                    args.push("--json".into());
                } else {
                    args.push("--output-last-message".into());
                    args.push(work.join("last-message.txt").display().to_string());
                }
                if caps.ephemeral {
                    args.push("--ephemeral".into());
                }
                args.extend([
                    "--color".into(),
                    "never".into(),
                    "-C".into(),
                    work.display().to_string(),
                ]);
                if let Some(m) = model {
                    args.extend(["-m".into(), m]);
                }
                args.push("-".into());
            }
        }
        Ok(args)
    }
}

/// An empty private directory removed on drop.
struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        for _ in 0..32 {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("embedide-ai-{}-{stamp:x}-{n}", std::process::id(),));
            match builder.create(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(Error::Config(format!(
                        "Cannot create {}: {e}",
                        dir.display()
                    )))
                }
            }
        }
        Err(Error::Config(
            "Cannot create a private temporary directory for the AI CLI".into(),
        ))
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Backend for CliBackend {
    fn label(&self) -> String {
        format!("{} ({})", self.kind.name(), self.program.display())
    }

    fn send(
        &self,
        req: &Request,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Response> {
        if crate::offline() {
            return Err(Error::Offline);
        }
        let caps = caps(self.kind, &self.program, &self.settings);
        if !caps.headless {
            return Err(Error::Config(format!(
                "{} at {} has no non-interactive mode (checked its --help); update it",
                self.kind.name(),
                self.program.display()
            )));
        }
        let work = WorkDir::new()?;
        let args = self.args(caps, req, &work.0)?;
        let mut cmd = command(self.kind, &self.program, &self.settings);
        cmd.args(&args)
            .current_dir(&work.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(&mut cmd)
            .map_err(|e| Error::Config(format!("Cannot run {}: {e}", self.program.display())))?;
        let prompt = prompt_text(self.kind, req);
        let mut stdin = child.stdin.take().expect("piped stdin");
        // A CLI that exits early closes the pipe: a broken pipe is not an error here.
        let writer = thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let limit = self.settings.max_output_bytes.max(4096);
        let (tx, lines) = mpsc::sync_channel::<Vec<u8>>(256);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut total = 0usize;
            loop {
                let mut line = Vec::new();
                match reader
                    .by_ref()
                    .take((limit - total.min(limit)) as u64 + 1)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) | Err(_) => return false,
                    Ok(n) => {
                        total += n;
                        if total > limit {
                            return true; // overflow: the caller stops the CLI
                        }
                        if tx.send(line).is_err() {
                            return false;
                        }
                    }
                }
            }
        });
        let err_tail = thread::spawn(move || {
            let mut tail = Vec::new();
            let mut buf = [0u8; 8192];
            let mut stderr = stderr;
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > STDERR_TAIL {
                    tail.drain(..tail.len() - STDERR_TAIL);
                }
            }
            tail
        });
        let start = Instant::now();
        let timeout = Duration::from_secs(self.settings.timeout_secs.max(1));
        let mut out = Outcome::default();
        let mut plain = Vec::new();
        let finished = loop {
            if cancel.is_cancelled() {
                stop(&mut child);
                return Err(Error::Cancelled);
            }
            if start.elapsed() > timeout {
                stop(&mut child);
                return Err(Error::Network(format!(
                    "{} did not finish within {} s (ai.cli.timeout_secs)",
                    self.kind.name(),
                    timeout.as_secs()
                )));
            }
            match lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    if caps.structured {
                        if let Some(delta) = event(self.kind, &line, &mut out) {
                            if !delta.is_empty() {
                                on_delta(&delta);
                            }
                        }
                    } else {
                        plain.extend_from_slice(&line);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break true,
            }
        };
        let overflow = finished && reader.join().unwrap_or(false);
        if overflow {
            stop(&mut child);
            return Err(Error::Provider(
                1,
                format!(
                    "{} produced more than {} bytes (ai.cli.max_output_bytes); stopped",
                    self.kind.name(),
                    limit
                ),
            ));
        }
        let status = loop {
            if cancel.is_cancelled() || start.elapsed() > timeout {
                stop(&mut child);
                return Err(if cancel.is_cancelled() {
                    Error::Cancelled
                } else {
                    Error::Network(format!("{} did not exit", self.kind.name()))
                });
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(e) => {
                    stop(&mut child);
                    return Err(Error::Network(e.to_string()));
                }
            }
        };
        let _ = writer.join();
        let stderr = String::from_utf8_lossy(&err_tail.join().unwrap_or_default()).into_owned();
        if !caps.structured {
            let text = match self.kind {
                CliKind::Codex => std::fs::read_to_string(work.0.join("last-message.txt"))
                    .ok()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| String::from_utf8_lossy(&plain).into_owned()),
                CliKind::ClaudeCode => String::from_utf8_lossy(&plain).into_owned(),
            };
            out.text = sanitize(text.trim());
            if status.success() && !out.text.is_empty() {
                on_delta(&out.text);
            }
        }
        if let Some(error) = out.error.take() {
            return Err(classify(self.kind, &error, status.code()));
        }
        if !status.success() {
            let message = if out.text.trim().is_empty() {
                stderr.trim()
            } else {
                out.text.trim()
            };
            return Err(classify(self.kind, message, status.code()));
        }
        if out.text.trim().is_empty() {
            return Err(classify(self.kind, stderr.trim(), status.code()));
        }
        Ok(Response {
            text: out.text.trim().to_string(),
            usage: out.usage,
            stop_reason: None,
            model: out.model,
        })
    }

    fn models(&self) -> Result<Vec<ModelInfo>> {
        Err(Error::Config(format!(
            "{} chooses its own default model; pass one it accepts with /model <id> (EmbedIDE does not list them)",
            self.kind.name()
        )))
    }
}
