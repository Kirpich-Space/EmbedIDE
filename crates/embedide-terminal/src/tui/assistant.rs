//! AI assistant panel. All provider work is delegated to `embedide-ai`;
//! requests run on a worker thread and stream into the transcript. Nothing
//! is sent until the user presses Enter on a message or command.
use super::layout::{self, Row};
use embedide_ai::{
    chat,
    cli::{self, CliKind},
    complete, context, edit, AuthMode, Backend, Cancel, Message, ProviderId,
};
use embedide_editor::syntax::Highlight;
use embedide_workspace::config::AiConfig;
use std::time::Instant;
use std::{path::PathBuf, sync::mpsc, thread, time::Duration};
use unicode_width::UnicodeWidthChar;

mod agent;
mod inline;

/// What the editor shares with the assistant for one action.
#[derive(Debug, Clone, Default)]
pub struct EditorContext {
    /// Project root (for relative paths and exclusions).
    pub root: PathBuf,
    /// Active file, if saved somewhere.
    pub path: Option<PathBuf>,
    /// Buffer text.
    pub text: String,
    /// 1-based cursor line.
    pub line: usize,
    /// 1-based cursor column.
    pub column: usize,
    /// Byte offset of the cursor.
    pub offset: usize,
    /// Visual selection text, if any.
    pub selection: Option<String>,
    /// Language id of the active file.
    pub language: String,
    /// Project `[ai] exclude` patterns.
    pub exclude: Vec<String>,
    /// The buffer indents with tabs.
    pub indent_tabs: bool,
    /// Indentation width in columns.
    pub indent_width: usize,
    /// One line about the project (name, language, target) for suggestions.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    User,
    Assistant,
    Info,
    Error,
    Diff,
    /// Who speaks next ("You", "Codex CLI", …).
    Header,
    /// Quiet line under an answer (time, tokens).
    Meta,
    /// Agent activity: a command (`$ make`) or a tool (`Read src/a.c`).
    Step,
    /// Tail of a command's output.
    Output,
    /// A command succeeded.
    StepOk,
    /// A command failed.
    StepFail,
    /// A file changed.
    Changed,
    /// A refused action.
    Denied,
    /// An action waiting for the user's approval.
    Approval,
}

/// A completion ready to be accepted in the editor.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Document it belongs to.
    pub path: Option<PathBuf>,
    /// Byte offset where it inserts.
    pub offset: usize,
    /// Text to insert.
    pub text: String,
    /// Text after the cursor when the suggestion was made (typing through
    /// it keeps the suggestion only while this is unchanged).
    pub after: String,
}

enum JobEvent {
    Delta(String),
    /// A background check finished (backend status); one line per entry.
    Info(Vec<String>),
    Chat(embedide_ai::Result<embedide_ai::Response>),
    Edit(PathBuf, embedide_ai::Result<edit::Proposal>),
    Complete(
        Option<PathBuf>,
        usize,
        embedide_ai::Result<complete::Completion>,
    ),
}

struct Job {
    rx: mpsc::Receiver<JobEvent>,
    cancel: Cancel,
    streaming: bool,
    /// The worker thread; joined on shutdown so a CLI child is always stopped.
    handle: Option<thread::JoinHandle<()>>,
    /// Transcript entry the answer streams into (other notes may follow it).
    slot: Option<usize>,
    /// When the request started (spinner timer, answer time).
    started: Instant,
    /// Who answers (shown in the header and the summary line).
    who: String,
}

/// The panel's state.
pub struct Assistant {
    cfg: AiConfig,
    provider: Option<ProviderId>,
    model: Option<String>,
    auth: AuthMode,
    transcript: Vec<(Kind, String)>,
    history: Vec<Message>,
    /// Text being typed in the panel's input line.
    pub input: String,
    attachments: Vec<(String, String)>,
    job: Option<Job>,
    /// Backend status checks run beside requests (they never block chat).
    checks: Option<Job>,
    /// Animation frame last drawn (spinner, blinking caret).
    anim: u128,
    proposal: Option<(PathBuf, edit::Proposal)>,
    /// Completion awaiting Tab (accept) or Esc (reject).
    pub suggestion: Option<Suggestion>,
    engine: Option<inline::Inline>,
    /// Inline suggestions paused until then (after an auth or rate error).
    inline_paused: Option<Instant>,
    /// Inline mode set with /inline (overrides the configuration).
    inline_mode: Option<&'static str>,
    /// Indentation of the buffer suggestions are for.
    inline_tabs: bool,
    inline_width: usize,
    scroll: usize,
    /// Agent turn in progress.
    run: Option<agent::AgentRun>,
    /// Agent conversation kept between turns (CLI session).
    session: Option<embedide_ai::agent::Session>,
    /// Plain text starts an agent task instead of a chat message.
    agent_mode: bool,
    /// Policy for the next agent turn.
    policy: embedide_ai::agent::Policy,
    /// What the last agent turn changed (for /changes, /review, /revert).
    review: Option<agent::Review>,
    /// Skills activated for agent tasks.
    skills: Vec<embedide_ai::agent::SkillText>,
    /// Automatic skills the user switched off for this IDE session.
    skills_off: Vec<String>,
    /// Files the agent changed that the editor should reload.
    changed: Vec<PathBuf>,
    /// A message the host should show even when the panel is hidden.
    attention: Option<String>,
}

const HELP: &str = "Type here (this panel) and press Enter. Start with: /provider claude-code  or  /provider codex-cli \
(your installed, logged-in official CLI; no API key) — or /provider anthropic | openai | moonshot | xai with an API key. \
Chat: /status · /file (attach this file; /file a.c b.h) · /selection · /explain · /edit what to change · /diff · \
/apply · /reject · /complete · /model name · /history · /cancel · /clear · /providers. \
Agent (claude-code / codex-cli works in the project): /agent <what to do> · /mode agent|chat · /policy read-only|workspace-write|approval-required \
· y / a / n answer approvals · /changes · /review [file] · /revert file|all · /fix [command] · /new · \
/skills · /skill name · /skill off name. Nothing is sent until Enter.";

impl Assistant {
    /// Panel state from the user configuration (no network).
    pub fn new(cfg: AiConfig) -> Self {
        let cli_backend = cfg.provider.as_deref().and_then(CliKind::parse);
        let provider = cli_backend
            .map(CliKind::provider)
            .or_else(|| cfg.provider.as_deref().and_then(|p| p.parse().ok()));
        let pc = provider.and_then(|p: ProviderId| cfg.providers.get(p.id()).cloned());
        let auth = if cli_backend.is_some() {
            AuthMode::Subscription
        } else {
            pc.as_ref()
                .and_then(|p| p.auth.as_deref())
                .and_then(|a| a.parse().ok())
                .unwrap_or(AuthMode::ApiKey)
        };
        let model = cfg
            .models
            .get("chat")
            .map(|m| {
                m.split_once(':')
                    .map(|(_, m)| m.to_string())
                    .unwrap_or(m.clone())
            })
            .or(pc.and_then(|p| p.model));
        let mut a = Self {
            cfg,
            provider,
            model,
            auth,
            transcript: Vec::new(),
            history: Vec::new(),
            input: String::new(),
            attachments: Vec::new(),
            job: None,
            checks: None,
            anim: 0,
            proposal: None,
            suggestion: None,
            engine: None,
            inline_paused: None,
            inline_mode: None,
            inline_tabs: false,
            inline_width: 4,
            scroll: 0,
            run: None,
            session: None,
            agent_mode: false,
            policy: Default::default(),
            review: None,
            skills: Vec::new(),
            skills_off: Vec::new(),
            changed: Vec::new(),
            attention: None,
        };
        a.agent_mode = a.cfg.agent.mode == "agent";
        a.policy = a.cfg.agent.policy.parse().unwrap_or_default();
        let start = match a.cli_backend() {
            Some(kind) => format!("Backend {} ({}). Type a message and press Enter; /help lists commands.", kind.id(), kind.name()),
            None if a.provider.is_some() => format!("Backend {}. Type a message and press Enter; /help lists commands.", a.provider_label()),
            None => "Choose a backend: /provider claude-code or /provider codex-cli (your logged-in CLI, no API key), \
or /provider anthropic|openai|moonshot|xai with an API key. /help lists commands."
                .into(),
        };
        a.note(
            Kind::Info,
            "AI is optional: editing and builds never need it.",
        );
        a.note(Kind::Info, start);
        a
    }

    fn note(&mut self, kind: Kind, text: impl Into<String>) {
        self.transcript.push((kind, text.into()));
        if self.transcript.len() > 2000 {
            self.transcript.drain(..200);
            if let Some(job) = &mut self.job {
                job.slot = job.slot.and_then(|i| i.checked_sub(200));
            }
            if let Some(run) = &mut self.run {
                run.text_slot = run.text_slot.and_then(|i| i.checked_sub(200));
                run.command_slot = run.command_slot.and_then(|i| i.checked_sub(200));
            }
        }
        self.scroll = 0;
    }

    /// One-line state for headers and the status bar.
    pub fn status(&self) -> String {
        let who = match (self.cli_backend(), &self.model) {
            (Some(kind), Some(m)) => format!("{}/{m}", kind.id()),
            (Some(kind), None) => kind.id().to_string(),
            _ => self.provider_label(),
        };
        let state = self.state();
        format!("{who} · {state}")
    }

    fn provider_label(&self) -> String {
        let who = match (&self.provider, &self.model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (Some(p), None) => format!("{p}/<no model>"),
            _ => "no provider".into(),
        };
        let auth = if self.auth == AuthMode::Subscription {
            " (subscription)"
        } else {
            ""
        };
        format!("{who}{auth}")
    }

    /// The official CLI in use, when the backend is `claude-code` or `codex-cli`.
    fn cli_backend(&self) -> Option<CliKind> {
        (self.auth == AuthMode::Subscription)
            .then_some(self.provider?)
            .and_then(CliKind::for_provider)
    }

    /// Name shown above answers.
    fn speaker(&self) -> String {
        match (self.cli_backend(), &self.provider) {
            (Some(kind), _) => kind.name().to_string(),
            (None, Some(p)) => match &self.model {
                Some(m) => format!("{} · {m}", p.info().name),
                None => p.info().name.to_string(),
            },
            (None, None) => "AI".into(),
        }
    }

    /// Animated activity for the panel title: a spinner and the elapsed time.
    fn activity(&self) -> Option<String> {
        if let Some(a) = self.agent_activity() {
            return Some(a);
        }
        let job = self.job.as_ref()?;
        let elapsed = job.started.elapsed();
        let spin = spinner(elapsed);
        let what = if job.streaming { "writing" } else { "thinking" };
        Some(format!("{spin} {what} {}s", elapsed.as_secs()))
    }

    fn state(&self) -> &'static str {
        if self.run.is_some() {
            return "agent running";
        }
        match &self.job {
            Some(j) if j.streaming => "streaming",
            Some(_) => "waiting",
            None if self.suggestion.is_some() => "suggestion ready",
            None if self.proposal.is_some() => "edit proposed",
            None if self.agent_mode => "agent mode",
            None => "idle",
        }
    }

    /// Whether a request is running (for the event-loop interval).
    pub fn busy(&self) -> bool {
        self.job.is_some()
            || self.checks.is_some()
            || self.engine.as_ref().is_some_and(|e| e.busy())
            || self.run.is_some()
    }

    fn backend(&self) -> Result<(Box<dyn Backend>, String), String> {
        let p = self.provider.ok_or("No provider: /provider anthropic|openai|moonshot|xai, or set ai.provider in config.toml")?;
        let model = match (&self.model, self.auth) {
            (Some(m), _) => m.clone(),
            (None, AuthMode::Subscription) => "default".into(),
            (None, AuthMode::ApiKey) => {
                return Err(format!(
                    "No model for {p}: /model <id> (EmbedIDE does not guess model names)"
                ))
            }
        };
        embedide_ai::backend(p, self.auth, &self.cfg)
            .map(|b| (b, model))
            .map_err(|e| e.to_string())
    }

    /// Attach the editor buffer (not the file on disk), after the project's
    /// exclusion rules and the built-in secret patterns have been applied.
    fn gather(&self, ctx: &EditorContext) -> Result<(String, String), String> {
        let path = ctx
            .path
            .clone()
            .ok_or("The current buffer has no file; save it first")?;
        let c = context::gather(Some(&ctx.root), &[path], &ctx.exclude, usize::MAX)
            .map_err(|e| e.to_string())?;
        if let Some(r) = c.refused.first() {
            return Err(format!("{} is not sent: {}", r.path, r.reason));
        }
        let shown = c
            .files
            .first()
            .map(|f| f.path.clone())
            .ok_or("The file could not be read")?;
        let mut take = ctx.text.len().min(self.cfg.max_context_bytes);
        while !ctx.text.is_char_boundary(take) {
            take -= 1;
        }
        let truncated = take < ctx.text.len();
        Ok((
            format!("{shown}{}", if truncated { " (truncated)" } else { "" }),
            format!("File: {shown}\n```\n{}\n```\n\n", &ctx.text[..take]),
        ))
    }

    /// Handle Enter in the input line.
    pub fn submit(&mut self, ctx: &EditorContext) {
        let line = std::mem::take(&mut self.input);
        let line = line.trim().to_string();
        if line.is_empty() {
            return;
        }
        let (cmd, arg) = line
            .split_once(' ')
            .map(|(c, a)| (c, a.trim()))
            .unwrap_or((line.as_str(), ""));
        // `/provider <codex-cli>` as copied from a usage line: drop the brackets.
        let arg = arg
            .strip_prefix('<')
            .and_then(|a| a.strip_suffix('>'))
            .unwrap_or(arg)
            .trim();
        if self.agent_command(cmd, arg, &line, ctx) {
            return;
        }
        match cmd {
            "/help" => self.note(Kind::Info, HELP),
            "/status" => {
                let s = self.status();
                self.note(Kind::Info, s);
                if let Some(kind) = self.cli_backend() {
                    self.check_backends(vec![kind]);
                }
            }
            "/providers" | "/backends" => {
                let mut lines = vec!["AI backends (select with /provider <id>):".to_string()];
                for p in ProviderId::ALL {
                    let key = match embedide_ai::auth::key_source(p) {
                        embedide_ai::auth::KeySource::Missing => "no API key",
                        _ => "API key available",
                    };
                    lines.push(format!(
                        "  {:<11} {} API ({}), billed per request — {key}",
                        p.id(),
                        p.info().name,
                        p.info().models_family
                    ));
                }
                for kind in CliKind::ALL {
                    lines.push(format!(
                        "  {:<11} official {} CLI with its own login (subscription) — checking…",
                        kind.id(),
                        kind.name()
                    ));
                }
                for line in lines {
                    self.note(Kind::Info, line);
                }
                self.check_backends(CliKind::ALL.to_vec());
            }
            "/provider" if CliKind::parse(arg).is_some() => {
                let kind = CliKind::parse(arg).expect("checked");
                self.provider = Some(kind.provider());
                self.auth = AuthMode::Subscription;
                self.model = self
                    .cfg
                    .providers
                    .get(kind.id())
                    .and_then(|c| c.model.clone());
                self.note(
                    Kind::Info,
                    format!(
                        "Backend {} ({}): requests go to your installed official CLI and use its login; no API key is used. Checking…",
                        kind.id(),
                        kind.name()
                    ),
                );
                self.check_backends(vec![kind]);
            }
            "/selection" => {
                self.input = "/sel".into();
                self.submit(ctx);
            }
            "/file" if !arg.is_empty() => {
                let paths: Vec<PathBuf> =
                    arg.split_whitespace().map(|p| ctx.root.join(p)).collect();
                match context::gather(
                    Some(&ctx.root),
                    &paths,
                    &ctx.exclude,
                    self.cfg.max_context_bytes,
                ) {
                    Ok(c) => {
                        for r in &c.refused {
                            self.note(Kind::Error, format!("{} is not sent: {}", r.path, r.reason));
                        }
                        if !c.files.is_empty() {
                            let label = c
                                .files
                                .iter()
                                .map(|f| {
                                    format!(
                                        "{}{}",
                                        f.path,
                                        if f.truncated { " (truncated)" } else { "" }
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            self.note(Kind::Info, format!("Attached {label} for the next message"));
                            self.attachments.push((label, c.text));
                        }
                    }
                    Err(e) => self.note(Kind::Error, e.to_string()),
                }
            }
            "/diff" => match &self.proposal {
                Some((_, p)) => {
                    let diff = p.diff.clone();
                    self.note(Kind::Diff, diff);
                    self.note(
                        Kind::Info,
                        "/apply puts it into the editor buffer (undoable), /reject discards it",
                    );
                }
                None => self.note(
                    Kind::Error,
                    "No proposed edit (/edit <instruction> asks for one)",
                ),
            },
            "/history" => {
                if self.history.is_empty() {
                    self.note(Kind::Info, "No conversation yet");
                }
                let recent: Vec<String> = self
                    .history
                    .iter()
                    .rev()
                    .take(10)
                    .rev()
                    .map(|m| {
                        let who = if m.role == embedide_ai::Role::User {
                            "you"
                        } else {
                            "ai"
                        };
                        let first: String = m
                            .content
                            .lines()
                            .last()
                            .unwrap_or("")
                            .chars()
                            .take(160)
                            .collect();
                        format!("{who}: {first} ({} bytes)", m.content.len())
                    })
                    .collect();
                for line in recent {
                    self.note(Kind::Info, line);
                }
            }
            "/clear" => {
                self.transcript.clear();
                self.history.clear();
                self.attachments.clear();
                self.note(Kind::Info, "Conversation cleared");
            }
            "/cancel" => self.cancel(),
            "/provider" => match arg.parse::<ProviderId>() {
                Ok(p) => {
                    self.provider = Some(p);
                    let pc = self.cfg.providers.get(p.id());
                    self.model = pc.and_then(|c| c.model.clone());
                    // An API provider means API-key access unless configured otherwise.
                    self.auth = pc
                        .and_then(|c| c.auth.as_deref())
                        .and_then(|a| a.parse().ok())
                        .unwrap_or(AuthMode::ApiKey);
                    self.note(
                        Kind::Info,
                        format!(
                            "Provider {} ({}), model {}",
                            p.info().name,
                            p.info().models_family,
                            self.model.as_deref().unwrap_or("not set: /model <id>")
                        ),
                    );
                }
                Err(e) => self.note(Kind::Error, e),
            },
            "/model"
                if !arg.is_empty()
                    && !arg.starts_with('-')
                    && !arg.contains(char::is_whitespace) =>
            {
                self.model = Some(arg.to_string());
                self.note(Kind::Info, format!("Model {arg}"));
            }
            "/auth" => match arg.parse::<AuthMode>() {
                Ok(a) => {
                    self.auth = a;
                    self.note(Kind::Info, format!("Authentication: {arg}"));
                }
                Err(e) => self.note(Kind::Error, e),
            },
            "/file" => match self.gather(ctx) {
                Ok((label, text)) => {
                    self.note(Kind::Info, format!("Attached {label} for the next message"));
                    self.attachments.push((label, text));
                }
                Err(e) => self.note(Kind::Error, e),
            },
            "/sel" => match &ctx.selection {
                Some(sel) if !sel.is_empty() => {
                    let name = ctx
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "buffer".into());
                    let label = format!("selection from {name} ({} bytes)", sel.len());
                    self.note(Kind::Info, format!("Attached {label}"));
                    self.attachments.push((
                        label,
                        format!(
                            "Selected code from {name}:\n```{}\n{sel}\n```\n\n",
                            ctx.language
                        ),
                    ));
                }
                _ => self.note(
                    Kind::Error,
                    "No Visual selection in the editor (select with v / V first)",
                ),
            },
            "/explain" => {
                if ctx.selection.as_deref().is_some_and(|s| !s.is_empty()) {
                    self.input = "/sel".into();
                    self.submit(ctx);
                } else if self.attachments.is_empty() {
                    if let Ok((label, text)) = self.gather(ctx) {
                        self.attachments.push((label, text));
                    }
                }
                self.chat(if arg.is_empty() {
                    "Explain this code: what it does, and any bugs or risks."
                } else {
                    arg
                });
            }
            "/edit" if !arg.is_empty() => self.start_edit(ctx, arg),
            "/apply" | "/reject" => self.note(Kind::Error, "No proposed edit"),
            "/complete" => self.start_completion(ctx),
            "/inline" => self.inline_command(arg),
            c if c.starts_with('/') => self.note(
                Kind::Error,
                format!("Unknown command {c}. /help lists commands"),
            ),
            _ if self.agent_mode => self.start_agent(ctx, line.clone(), None),
            _ => self.chat(&line),
        }
    }

    fn chat(&mut self, text: &str) {
        if self.job.is_some() {
            return self.note(Kind::Error, "A request is already running (/cancel)");
        }
        let (backend, model) = match self.backend() {
            Ok(b) => b,
            Err(e) => return self.note(Kind::Error, e),
        };
        let attach: String = self.attachments.iter().map(|(_, t)| t.as_str()).collect();
        let labels: Vec<String> = self.attachments.drain(..).map(|(l, _)| l).collect();
        let content = if attach.is_empty() {
            text.to_string()
        } else {
            format!("{attach}\n{text}")
        };
        let mut messages = self.history.clone();
        messages.push(Message::user(content.clone()));
        let req = embedide_ai::Request {
            model,
            system: Some(chat::SYSTEM.into()),
            messages,
            max_tokens: self.cfg.max_tokens,
            temperature: None,
            stream: true,
        };
        let who = self.speaker();
        self.note(Kind::Header, "You");
        self.note(Kind::User, text.to_string());
        self.note(
            Kind::Meta,
            format!(
                "sent {} bytes{}",
                content.len() + chat::SYSTEM.len(),
                if labels.is_empty() {
                    String::new()
                } else {
                    format!(" with {}", labels.join(", "))
                }
            ),
        );
        self.history.push(Message::user(content));
        self.note(Kind::Header, who.clone());
        self.note(Kind::Assistant, String::new());
        let slot = Some(self.transcript.len() - 1);
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let tx2 = tx.clone();
            let r = backend.send(&req, &c2, &mut |d| {
                let _ = tx2.send(JobEvent::Delta(d.to_string()));
            });
            let _ = tx.send(JobEvent::Chat(r));
        });
        self.job = Some(Job {
            rx,
            cancel,
            streaming: false,
            handle: Some(handle),
            slot,
            started: Instant::now(),
            who,
        });
    }

    fn start_edit(&mut self, ctx: &EditorContext, instruction: &str) {
        if self.job.is_some() {
            return self.note(Kind::Error, "A request is already running (/cancel)");
        }
        let Some(path) = ctx.path.clone() else {
            return self.note(Kind::Error, "Save the buffer to a file first");
        };
        let (label, _) = match self.gather(ctx) {
            Ok(x) => x,
            Err(e) => return self.note(Kind::Error, e),
        };
        if label.ends_with("(truncated)") {
            return self.note(
                Kind::Error,
                "File is larger than ai.max_context_bytes; AI edit needs the whole file",
            );
        }
        let (backend, model) = match self.backend() {
            Ok(b) => b,
            Err(e) => return self.note(Kind::Error, e),
        };
        let shown = label.clone();
        let text = ctx.text.clone();
        let instr = instruction.to_string();
        let max = self.cfg.max_tokens;
        self.note(Kind::User, format!("/edit {instruction}"));
        self.note(
            Kind::Info,
            format!("→ {} bytes ({shown}) to {}", text.len(), backend.label()),
        );
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let r = edit::propose(backend.as_ref(), &shown, &text, &instr, &model, max, &c2);
            let _ = tx.send(JobEvent::Edit(path, r));
        });
        self.job = Some(Job {
            rx,
            cancel,
            streaming: false,
            handle: Some(handle),
            slot: None,
            started: Instant::now(),
            who: String::new(),
        });
    }

    /// Ask for a completion at the cursor (explicit, one request).
    pub fn start_completion(&mut self, ctx: &EditorContext) {
        if self.job.is_some() {
            return self.note(Kind::Error, "A request is already running (/cancel)");
        }
        if ctx.path.is_some() {
            if let Err(e) = self.gather(ctx) {
                return self.note(Kind::Error, e);
            }
        }
        let (backend, model) = match self.backend() {
            Ok(b) => b,
            Err(e) => return self.note(Kind::Error, e),
        };
        let req = self.completion_request(ctx);
        let (path, offset) = (ctx.path.clone(), ctx.offset);
        self.note(
            Kind::Info,
            format!(
                "→ completion at {}:{} via {}",
                ctx.line,
                ctx.column,
                backend.label()
            ),
        );
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let r = complete::complete(backend.as_ref(), &req, &model, &c2);
            let _ = tx.send(JobEvent::Complete(path, offset, r));
        });
        self.job = Some(Job {
            rx,
            cancel,
            streaming: false,
            handle: Some(handle),
            slot: None,
            started: Instant::now(),
            who: String::new(),
        });
    }

    /// Inspect the official CLIs in the background (it runs `--version`,
    /// `--help` and their login-status commands, which take a moment).
    fn check_backends(&mut self, kinds: Vec<CliKind>) {
        if self.checks.is_some() {
            return;
        }
        let cfg = self.cfg.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let lines = kinds
                .into_iter()
                .map(|kind| {
                    let configured = cfg
                        .providers
                        .get(kind.id())
                        .or_else(|| cfg.providers.get(kind.provider().id()))
                        .and_then(|c| c.cli.clone());
                    match cli::locate(kind, configured.as_deref()) {
                        Ok(program) => format!(
                            "  {:<11} {}",
                            kind.id(),
                            cli::inspect(kind, &program, &cfg.cli).summary()
                        ),
                        Err(e) => format!("  {:<11} {e}", kind.id()),
                    }
                })
                .collect();
            let _ = tx.send(JobEvent::Info(lines));
        });
        self.checks = Some(Job {
            rx,
            cancel: Cancel::default(),
            streaming: false,
            handle: Some(handle),
            slot: None,
            started: Instant::now(),
            who: String::new(),
        });
    }

    /// Stop any running request and wait (bounded) until its worker has
    /// stopped the CLI child, so no process outlives the IDE.
    pub fn shutdown(&mut self) {
        self.dismiss();
        self.shutdown_agent();
        self.checks = None; // probes end on their own time limit
        if let Some(mut job) = self.job.take() {
            job.cancel.cancel();
            if let Some(handle) = job.handle.take() {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while !handle.is_finished() && std::time::Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                if handle.is_finished() {
                    let _ = handle.join();
                }
            }
        }
    }

    /// A message for the transcript from the host.
    pub fn info(&mut self, text: impl Into<String>) {
        self.note(Kind::Info, text);
    }

    /// Cancel the running request.
    pub fn cancel(&mut self) {
        self.cancel_agent();
        if let Some(job) = self.job.take() {
            job.cancel.cancel();
            self.note(Kind::Info, "Cancelled");
        }
        self.dismiss();
    }

    /// The proposal waiting for /apply, if any.
    pub fn take_proposal_if(&mut self, apply: bool) -> Option<(PathBuf, edit::Proposal)> {
        let p = self.proposal.take();
        match (&p, apply) {
            (Some(_), true) => {}
            (Some(_), false) => self.note(Kind::Info, "Proposal rejected"),
            (None, _) => self.note(Kind::Error, "No proposed edit"),
        }
        if apply {
            p
        } else {
            None
        }
    }

    /// Whether an edit proposal is waiting.
    pub fn has_proposal(&self) -> bool {
        self.proposal.is_some()
    }

    /// Report a failure from the host (e.g. applying an edit).
    pub fn error(&mut self, text: impl Into<String>) {
        self.note(Kind::Error, text);
    }

    /// Poll worker results. Returns whether the panel changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = self.tick_agent();
        // Spinner and caret advance every 100 ms while a request runs; the
        // screen diff sends only the cells that changed.
        if let Some(job) = &self.job {
            let frame = job.started.elapsed().as_millis() / 100;
            if frame != self.anim {
                self.anim = frame;
                changed = true;
            }
        }
        if let Some(checks) = &self.checks {
            match checks.rx.try_recv() {
                Ok(JobEvent::Info(lines)) => {
                    self.checks = None;
                    for line in lines {
                        self.note(Kind::Info, line);
                    }
                    changed = true;
                }
                Ok(_) | Err(mpsc::TryRecvError::Disconnected) => self.checks = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        changed |= self.tick_inline();
        let Some(job) = &mut self.job else {
            return changed;
        };
        let mut finished = None;
        loop {
            match job.rx.try_recv() {
                Ok(JobEvent::Delta(d)) => {
                    job.streaming = true;
                    if let Some((Kind::Assistant, t)) =
                        job.slot.and_then(|i| self.transcript.get_mut(i))
                    {
                        t.push_str(&d);
                    }
                    changed = true;
                }
                Ok(other) => {
                    finished = Some(other);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(JobEvent::Chat(Err(embedide_ai::Error::Network(
                        "worker stopped".into(),
                    ))));
                    break;
                }
            }
        }
        if let Some(ev) = finished {
            let (slot, started, who) = match self.job.take() {
                Some(j) => (j.slot, j.started, j.who),
                None => (None, Instant::now(), String::new()),
            };
            changed = true;
            match ev {
                JobEvent::Chat(Ok(r)) => {
                    if let Some((Kind::Assistant, t)) =
                        slot.and_then(|i| self.transcript.get_mut(i))
                    {
                        if t.is_empty() {
                            t.push_str(&r.text);
                        }
                    }
                    self.history.push(Message::assistant(r.text));
                    let tokens = match (r.usage.input_tokens, r.usage.output_tokens) {
                        (Some(i), Some(o)) => format!(" · tokens {i} in, {o} out"),
                        _ => String::new(),
                    };
                    self.note(
                        Kind::Meta,
                        format!("{who} · {:.1} s{tokens}", started.elapsed().as_secs_f32()),
                    );
                }
                JobEvent::Chat(Err(e)) => {
                    if let Some(i) = slot.filter(|&i| {
                        matches!(self.transcript.get(i), Some((Kind::Assistant, t)) if t.is_empty())
                    }) {
                        self.transcript.remove(i);
                    }
                    self.history.pop();
                    self.note(Kind::Error, e.to_string());
                }
                JobEvent::Edit(path, Ok(p)) => {
                    if p.diff.is_empty() {
                        self.note(Kind::Info, "No changes proposed");
                    } else {
                        self.note(Kind::Diff, p.diff.clone());
                        self.note(Kind::Info, "Review the diff: /apply puts it into the editor buffer (undoable), /reject discards it");
                        self.proposal = Some((path, p));
                    }
                }
                JobEvent::Edit(_, Err(e)) => self.note(Kind::Error, e.to_string()),
                JobEvent::Complete(path, offset, Ok(c)) => {
                    if c.insertion.is_empty() {
                        self.note(Kind::Info, "No completion suggested");
                    } else {
                        self.note(
                            Kind::Info,
                            "Suggestion shown in the editor: Tab accepts, Esc rejects",
                        );
                        self.suggestion = Some(Suggestion {
                            path,
                            offset,
                            text: c.insertion,
                            after: String::new(),
                        });
                    }
                }
                JobEvent::Complete(_, _, Err(e)) => self.note(Kind::Error, e.to_string()),
                JobEvent::Info(lines) => {
                    for line in lines {
                        self.note(Kind::Info, line);
                    }
                }
                JobEvent::Delta(_) => {}
            }
        }
        changed
    }

    /// Scroll the transcript (positive = older).
    pub fn scroll(&mut self, delta: isize) {
        self.scroll = (self.scroll as isize + delta).max(0) as usize;
    }

    /// Render into `width` × `height`; returns rows and the input cursor.
    pub fn rows(
        &self,
        width: usize,
        height: usize,
        focused: bool,
    ) -> (Vec<Row>, Option<(u16, u16)>) {
        let mut rows = Vec::with_capacity(height);
        if height == 0 || width < 4 {
            return (rows, None);
        }
        let who = match self.cli_backend() {
            Some(kind) => kind.id().to_string(),
            None => self.provider_label(),
        };
        let state = self.activity().unwrap_or_else(|| self.state().to_string());
        rows.push(layout::label(
            &format!("{}AI · {who} · {state}", if focused { "> " } else { "" }),
            width,
            true,
        ));
        let lines = self.transcript_lines(width);
        let body_h = height.saturating_sub(2);
        let end = lines.len().saturating_sub(self.scroll.min(lines.len()));
        let start = end.saturating_sub(body_h);
        for line in &lines[start..end] {
            let mut row = Row::default();
            for (text, style) in line {
                row.push_styled(text, false, *style);
            }
            rows.push(row);
        }
        rows.resize_with(height.saturating_sub(1), Row::default);
        // Input line: a prompt, the text (its tail when long), or a hint.
        let visible: String = {
            let chars: Vec<char> = self.input.chars().collect();
            let max = width.saturating_sub(3);
            let skip = chars.len().saturating_sub(max);
            chars[skip..].iter().collect()
        };
        let mut input = Row::default();
        input.push_styled("› ", false, Some(Highlight::Keyword));
        if visible.is_empty() {
            let hint = if self.approval_pending() {
                "y allow · a allow similar · n deny · /cancel stops the agent".to_string()
            } else if self.run.is_some() {
                "/cancel stops the agent (and every command it started)".to_string()
            } else if self.job.is_some() {
                "/cancel stops the answer".to_string()
            } else if self.provider.is_none() {
                "/provider claude-code  or  /provider codex-cli".to_string()
            } else if self.agent_mode {
                format!("Task for the {who} agent · Enter runs · /mode chat · /help")
            } else {
                format!("Message {who} · Enter sends · /agent <what to do> edits files · /help")
            };
            let hint: String = hint.chars().take(width.saturating_sub(2)).collect();
            input.push_styled(&hint, false, Some(Highlight::Comment));
        } else {
            input.push(&visible, false);
        }
        rows.push(input);
        let cursor = focused.then(|| {
            (
                (2 + visible
                    .chars()
                    .map(|c| c.width().unwrap_or(1))
                    .sum::<usize>())
                .min(width - 1) as u16,
                (height - 1) as u16,
            )
        });
        (rows, cursor)
    }

    /// The conversation as styled, wrapped lines: a header per speaker,
    /// indented bodies, code blocks and diffs coloured, quiet notes.
    fn transcript_lines(&self, width: usize) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let streaming = self
            .job
            .as_ref()
            .and_then(|j| j.slot)
            .or_else(|| self.run.as_ref().and_then(|r| r.text_slot));
        let started = self
            .job
            .as_ref()
            .map(|j| j.started)
            .or_else(|| self.run.as_ref().map(|r| r.started));
        let blink = started.is_some_and(|s| s.elapsed().as_millis() / 450 % 2 == 0);
        let running_step = self.run.as_ref().and_then(|r| r.command_slot);
        for (index, (kind, text)) in self.transcript.iter().enumerate() {
            match kind {
                Kind::Header => {
                    if !out.is_empty() {
                        out.push(Vec::new());
                    }
                    let style = if text == "You" {
                        Highlight::Keyword
                    } else {
                        Highlight::Function
                    };
                    out.push(vec![("▍".into(), Some(style)), (text.clone(), Some(style))]);
                }
                Kind::User => {
                    for raw in text.lines() {
                        wrap_into(&mut out, "  ", raw, None, width);
                    }
                }
                Kind::Assistant if text.is_empty() => {
                    if streaming == Some(index) {
                        let spin = self.activity().unwrap_or_default();
                        out.push(vec![(format!("  {spin}"), Some(Highlight::Comment))]);
                    } else {
                        out.push(vec![("  (no answer)".into(), Some(Highlight::Comment))]);
                    }
                }
                Kind::Assistant => {
                    let mut code = false;
                    for raw in text.lines() {
                        let fence = raw.trim_start().strip_prefix("```");
                        if let Some(lang) = fence {
                            code = !code;
                            let mark = if code {
                                format!(
                                    "  ╭─ {}",
                                    if lang.trim().is_empty() {
                                        "code"
                                    } else {
                                        lang.trim()
                                    }
                                )
                            } else {
                                "  ╰─".to_string()
                            };
                            out.push(vec![(mark, Some(Highlight::Comment))]);
                        } else if code {
                            wrap_into(&mut out, "  │ ", raw, Some(Highlight::String), width);
                        } else if raw.trim_start().starts_with('#') {
                            wrap_into(
                                &mut out,
                                "  ",
                                raw.trim_start_matches('#').trim(),
                                Some(Highlight::Type),
                                width,
                            );
                        } else {
                            wrap_into(&mut out, "  ", raw, None, width);
                        }
                    }
                    if streaming == Some(index) && blink {
                        if let Some(last) = out.last_mut() {
                            last.push(("▍".into(), Some(Highlight::Function)));
                        }
                    }
                }
                Kind::Info => wrap_into(&mut out, "  · ", text, Some(Highlight::Comment), width),
                Kind::Meta => wrap_into(&mut out, "  ", text, Some(Highlight::Comment), width),
                Kind::Error => wrap_into(&mut out, "  ✗ ", text, Some(Highlight::Constant), width),
                Kind::Step => {
                    let style = if text.starts_with("$ ") {
                        Highlight::Keyword
                    } else {
                        Highlight::Function
                    };
                    wrap_into(&mut out, "  ", text, Some(style), width);
                    if running_step == Some(index) {
                        if let (Some(last), Some(s)) = (out.last_mut(), started) {
                            last.push((
                                format!(" {}", spinner(s.elapsed())),
                                Some(Highlight::Comment),
                            ));
                        }
                    }
                }
                Kind::Output => {
                    for raw in text.lines() {
                        wrap_into(&mut out, "    │ ", raw, Some(Highlight::Comment), width);
                    }
                }
                Kind::StepOk => wrap_into(&mut out, "    ✓ ", text, Some(Highlight::String), width),
                Kind::StepFail => {
                    wrap_into(&mut out, "    ✗ ", text, Some(Highlight::Constant), width)
                }
                Kind::Changed => wrap_into(&mut out, "  ✎ ", text, Some(Highlight::Type), width),
                Kind::Denied => wrap_into(&mut out, "  ⊘ ", text, Some(Highlight::Constant), width),
                Kind::Approval => {
                    for (i, raw) in text.lines().enumerate() {
                        let lead = if i == 0 { "  ⚠ " } else { "    " };
                        wrap_into(&mut out, lead, raw, Some(Highlight::Preprocessor), width);
                    }
                }
                Kind::Diff => {
                    for raw in text.lines() {
                        let style = match raw.as_bytes().first() {
                            Some(b'+') if !raw.starts_with("+++") => Highlight::String,
                            Some(b'-') if !raw.starts_with("---") => Highlight::Constant,
                            Some(b'@') => Highlight::Preprocessor,
                            _ => Highlight::Comment,
                        };
                        wrap_into(&mut out, "", raw, Some(style), width);
                    }
                }
            }
        }
        if !self.skills.is_empty() {
            let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
            wrap_into(
                &mut out,
                "  + ",
                &format!("skills for agent tasks: {}", names.join(", ")),
                Some(Highlight::Comment),
                width,
            );
        }
        if !self.attachments.is_empty() {
            let names: Vec<&str> = self.attachments.iter().map(|(l, _)| l.as_str()).collect();
            wrap_into(
                &mut out,
                "  + ",
                &format!("attached for the next message: {}", names.join(", ")),
                Some(Highlight::Comment),
                width,
            );
        }
        out
    }
}

/// One display line: styled pieces.
type Line = Vec<(String, Option<Highlight>)>;

/// Braille spinner frame for an elapsed time.
fn spinner(elapsed: Duration) -> char {
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    SPIN[(elapsed.as_millis() / 100) as usize % SPIN.len()]
}

/// Wrap `text` to `width` cells at spaces (a word longer than the line is
/// split); continuation lines are indented like the first.
fn wrap_into(
    out: &mut Vec<Line>,
    indent: &str,
    text: &str,
    style: Option<Highlight>,
    width: usize,
) {
    let cells = |t: &str| t.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>();
    let indent_w = cells(indent);
    let room = width.saturating_sub(indent_w).max(1);
    let lead_style = if indent.trim().is_empty() {
        None
    } else {
        style
    };
    let mut first = true;
    let mut push = |line: String, out: &mut Vec<Line>| {
        let lead = if first {
            indent.to_string()
        } else {
            " ".repeat(indent_w)
        };
        out.push(vec![(lead, lead_style), (line, style)]);
        first = false;
    };
    let mut line = String::new();
    for word in text.split_inclusive(' ') {
        if cells(&line) + cells(word.trim_end()) > room && !line.trim().is_empty() {
            push(std::mem::take(&mut line).trim_end().to_string(), out);
        }
        let mut word = word;
        // A word that does not fit on a line of its own is split.
        while cells(word.trim_end()) > room {
            let mut cut = 0;
            let mut used = 0;
            for (i, c) in word.char_indices() {
                let w = c.width().unwrap_or(0);
                if used + w > room {
                    break;
                }
                used += w;
                cut = i + c.len_utf8();
            }
            if !line.is_empty() {
                push(std::mem::take(&mut line).trim_end().to_string(), out);
            }
            push(word[..cut].to_string(), out);
            word = &word[cut..];
        }
        line.push_str(word);
    }
    push(line.trim_end().to_string(), out);
}

#[cfg(test)]
mod usage_tests {
    use super::*;
    use embedide_workspace::config::ProviderConfig;
    use unicode_width::UnicodeWidthStr;

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| r.runs.iter().map(|x| x.text.as_str()).collect::<String>())
            .collect()
    }

    /// The AI panel with a mock Claude Code agent and a disposable workspace.
    #[cfg(unix)]
    fn agent_fixture(name: &str) -> (Assistant, EditorContext, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "embedide-panel-agent-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ws = dir.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("main.c"), "int main(void) { return 0; }\n").unwrap();
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../embedide-ai/tests/fixtures/mock_agent.py");
        let program = dir.join("claude");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nexec python3 '{}' claude edit '{}' \"$@\"\n",
                script.display(),
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cfg = AiConfig {
            provider: Some("claude-code".into()),
            ..AiConfig::default()
        };
        cfg.providers.insert(
            "claude-code".into(),
            ProviderConfig {
                cli: Some(program),
                ..Default::default()
            },
        );
        let ctx = EditorContext {
            root: ws.clone(),
            ..Default::default()
        };
        (Assistant::new(cfg), ctx, dir)
    }

    #[cfg(unix)]
    fn tick_until(a: &mut Assistant, what: &str, done: impl Fn(&Assistant) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(a) {
            a.tick();
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {:#?}",
                a.transcript
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_task_shows_steps_asks_for_approval_and_can_be_reviewed_and_reverted() {
        let (mut a, ctx, dir) = agent_fixture("flow");
        a.input = "/agent implement add".into();
        a.submit(&ctx);
        assert!(a.agent_running());
        // Reading /etc/hostname is outside the workspace: asked, and denied with n.
        tick_until(&mut a, "first approval", Assistant::approval_pending);
        let shown = text(&a.rows(100, 40, true).0).join("\n");
        assert!(
            shown.contains("⚠ Approval needed: Read /etc/hostname"),
            "{shown}"
        );
        assert!(
            shown.contains("approval needed"),
            "title shows the wait: {shown}"
        );
        assert!(shown.contains("y allow · a allow similar"), "{shown}");
        assert_eq!(
            a.take_attention()
                .as_deref()
                .map(|m| m.starts_with("AI agent needs approval")),
            Some(true)
        );
        // Typed text cannot slip past a pending approval.
        a.input = "hello".into();
        a.submit(&ctx);
        assert!(a.approval_pending());
        a.input = "n".into();
        a.submit(&ctx);
        // `rm -rf build` asks next; allow it.
        tick_until(&mut a, "second approval", Assistant::approval_pending);
        a.answer(embedide_ai::agent::Decision::Allow);
        tick_until(&mut a, "the end", |a| !a.agent_running());
        let shown = text(&a.rows(120, 200, false).0).join("\n");
        for needle in [
            "▍Agent · Claude Code · workspace-write",
            "Write add.c",
            "$ gcc -o app main.c add.c && ./app",
            "✓ ok",
            "$ make broken",
            "│ make: *** No rule to make target 'broken'",
            "✗ exit 2",
            "⊘ Read: The user refused this action",
            "Allowed: $ rm -rf build",
            "■ done · 1 file(s) changed · 3 command(s) (1 failed) · 2 approval(s) · 1 refused",
            "✎ add.c (added)",
        ] {
            assert!(shown.contains(needle), "missing {needle:?} in\n{shown}");
        }
        assert_eq!(
            shown.matches("Created add.c; built.").count(),
            1,
            "summary once: {shown}"
        );
        let changed = a.take_changed_files();
        assert_eq!(changed, [ctx.root.canonicalize().unwrap().join("add.c")]);
        a.input = "/review".into();
        a.submit(&ctx);
        assert!(a
            .transcript
            .iter()
            .any(|(k, t)| *k == Kind::Diff && t.contains("+int add")));
        a.input = "/revert all".into();
        a.submit(&ctx);
        assert!(!ctx.root.join("add.c").exists());
        // The next task continues the CLI session.
        a.input = "/agent".into();
        a.submit(&ctx);
        let status = &a.transcript.iter().rev().nth(1).unwrap().1;
        assert!(status.contains("1 turn(s)"), "{status}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn agent_mode_routes_plain_text_and_cancel_stops_the_task() {
        let (mut a, ctx, dir) = agent_fixture("mode");
        a.input = "/mode agent".into();
        a.submit(&ctx);
        a.input = "/policy read-only".into();
        a.submit(&ctx);
        a.input = "inspect the code".into();
        a.submit(&ctx);
        assert!(a.agent_running(), "plain text started an agent task");
        tick_until(&mut a, "approval", Assistant::approval_pending);
        a.input = "/cancel".into();
        a.submit(&ctx);
        tick_until(&mut a, "stop", |a| !a.agent_running());
        let shown: String = a.transcript.iter().map(|(_, t)| format!("{t}\n")).collect();
        assert!(shown.contains("Agent · Claude Code · read-only"), "{shown}");
        assert!(shown.contains("Agent stopped"), "{shown}");
        assert!(
            !ctx.root.join("add.c").exists(),
            "read-only changed nothing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    fn run_denying(a: &mut Assistant, ctx: &EditorContext, task: &str) {
        a.input = task.into();
        a.submit(ctx);
        assert!(a.agent_running(), "{task}");
        let deadline = Instant::now() + Duration::from_secs(20);
        while a.agent_running() {
            a.tick();
            if a.approval_pending() {
                a.answer(embedide_ai::agent::Decision::Deny(String::new()));
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn agents_get_built_in_instructions_and_project_skills_once_per_session() {
        let (mut a, ctx, dir) = agent_fixture("builtin");
        a.input = "/agent task".into();
        a.submit(&ctx);
        assert!(!a.agent_running(), "a placeholder is not a task");
        assert!(a.transcript.last().unwrap().1.contains("Describe the task"));
        a.input = "/skills".into();
        a.submit(&ctx);
        let listed: String = a.transcript.iter().map(|(_, t)| format!("{t}\n")).collect();
        assert!(
            listed.contains("● embedide-agent (built-in, auto)"),
            "{listed}"
        );
        assert!(
            listed.contains("● c-cpp (built-in, auto)"),
            "main.c makes it a C project: {listed}"
        );
        assert!(listed.contains("○ rust-crate (built-in)"), "{listed}");
        run_denying(&mut a, &ctx, "/agent implement add");
        let shown: String = a.transcript.iter().map(|(_, t)| format!("{t}\n")).collect();
        assert!(
            shown.contains("skills: embedide-agent (built-in), c-cpp (built-in)"),
            "{shown}"
        );
        run_denying(&mut a, &ctx, "/agent and now test it");
        let prompts: Vec<String> = std::fs::read_to_string(dir.join("prompts"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0]
            .contains("EmbedIDE working instructions (from EmbedIDE itself; follow them):"));
        assert!(prompts[0].contains("<instructions name=\"embedide-agent\">"));
        assert!(prompts[0].contains("timeout 10 ./app"));
        assert!(prompts[0].contains("<instructions name=\"c-cpp\">"));
        assert!(
            !prompts[1].contains("<instructions"),
            "a resumed session is not sent them again"
        );
        // Switched off: a new session starts without it.
        a.input = "/skill off c-cpp".into();
        a.submit(&ctx);
        a.input = "/new".into();
        a.submit(&ctx);
        run_denying(&mut a, &ctx, "/agent third");
        let third = std::fs::read_to_string(dir.join("prompts")).unwrap();
        let third: String = serde_json::from_str(third.lines().nth(2).unwrap()).unwrap();
        assert!(third.contains("<instructions name=\"embedide-agent\">"));
        assert!(!third.contains("<instructions name=\"c-cpp\">"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skills_are_listed_activated_and_reported() {
        let dir =
            std::env::temp_dir().join(format!("embedide-panel-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let skill = dir.join(".embedide/skills/c-style");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: c-style\ndescription: C style rules\nallowed-tools: Bash\n---\nUse stdint types.\n",
        )
        .unwrap();
        let ctx = EditorContext {
            root: dir.clone(),
            ..Default::default()
        };
        let mut a = Assistant::new(AiConfig::default());
        a.input = "/skills".into();
        a.submit(&ctx);
        assert!(a
            .transcript
            .iter()
            .any(|(_, t)| t.contains("○ c-style (project) — C style rules")));
        a.input = "/skill c-style".into();
        a.submit(&ctx);
        assert_eq!(a.skills.len(), 1);
        assert!(a.skills[0].body.starts_with("Use stdint types."));
        assert!(a
            .transcript
            .iter()
            .any(|(_, t)| t.contains("cannot grant permissions")));
        let shown = text(&a.rows(100, 30, false).0).join("\n");
        assert!(shown.contains("skills for agent tasks: c-style"), "{shown}");
        a.input = "/skill nope".into();
        a.submit(&ctx);
        assert!(a
            .transcript
            .last()
            .unwrap()
            .1
            .contains("Available: c-style"));
        a.input = "/skill off c-style".into();
        a.submit(&ctx);
        assert!(a.skills.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_chat_shows_speakers_code_blocks_diffs_and_wraps_at_words() {
        let mut a = Assistant::new(AiConfig::default());
        a.transcript.clear();
        a.note(Kind::Header, "You");
        a.note(Kind::User, "explain the loop please");
        a.note(Kind::Header, "Codex CLI");
        a.note(Kind::Assistant, "Sure:\n```c\nfor (;;) {}\n```\n# Summary\nIt never ends and keeps the processor busy forever");
        a.note(Kind::Meta, "Codex CLI · 1.2 s");
        a.note(Kind::Diff, "--- a/x.c\n+++ b/x.c\n-old\n+new");
        let (rows, _) = a.rows(40, 30, true);
        let lines = text(&rows);
        assert!(
            lines.contains(&"▍You".to_string()) && lines.contains(&"▍Codex CLI".to_string()),
            "{lines:#?}"
        );
        assert!(
            lines.contains(&"  ╭─ c".to_string()) && lines.contains(&"  │ for (;;) {}".to_string())
        );
        assert!(lines.contains(&"  Summary".to_string()));
        assert!(
            lines
                .iter()
                .all(|l| UnicodeWidthStr::width(l.as_str()) <= 40),
            "{lines:#?}"
        );
        // Wrapped at a space, never inside a word.
        assert!(
            lines.contains(&"  It never ends and keeps the processor".to_string()),
            "{lines:#?}"
        );
        assert!(lines.contains(&"  busy forever".to_string()));
        // Styles: code green, diff + green and - red, notes grey.
        let style_of = |needle: &str| {
            rows.iter()
                .flat_map(|r| r.runs.iter())
                .find(|x| x.text.contains(needle))
                .and_then(|x| x.style)
        };
        assert_eq!(style_of("for (;;)"), Some(Highlight::String));
        assert_eq!(style_of("+new"), Some(Highlight::String));
        assert_eq!(style_of("-old"), Some(Highlight::Constant));
        assert_eq!(style_of("1.2 s"), Some(Highlight::Comment));
        // The empty input shows a hint.
        assert!(lines.last().unwrap().starts_with("› "));
    }

    #[test]
    fn a_running_request_animates_a_spinner_timer_and_caret() {
        let mut a = Assistant::new(AiConfig::default());
        a.note(Kind::Header, "Codex CLI");
        a.note(Kind::Assistant, String::new());
        let index = a.transcript.len() - 1;
        let slot = Some(index);
        let (_tx, rx) = mpsc::channel();
        a.job = Some(Job {
            rx,
            cancel: Cancel::default(),
            streaming: false,
            handle: None,
            slot,
            started: Instant::now() - Duration::from_millis(2_050),
            who: "Codex CLI".into(),
        });
        let (rows, _) = a.rows(60, 20, false);
        let lines = text(&rows);
        assert!(lines[0].contains("thinking 2s"), "{}", lines[0]);
        assert!(
            lines.iter().any(|l| l.trim_start().contains("thinking 2s")),
            "spinner in the answer slot"
        );
        let first = a.anim;
        assert!(a.tick(), "a new frame is drawn");
        std::thread::sleep(Duration::from_millis(120));
        assert!(a.tick() && a.anim != first, "frames advance every 100 ms");
        // Streaming text gets a blinking caret.
        a.job.as_mut().unwrap().streaming = true;
        a.transcript[index].1 = "Hello".into();
        let shows_caret = (0..12).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            text(&a.rows(60, 20, false).0)
                .iter()
                .any(|l| l == "  Hello▍")
        });
        assert!(shows_caret);
        assert!(a.rows(60, 20, false).0.len() == 20);
        assert!(text(&a.rows(60, 20, false).0)[0].contains("writing"));
    }

    #[test]
    fn provider_names_copied_with_angle_brackets_still_work() {
        let mut cfg = AiConfig::default();
        // A CLI path that does not exist: the status check fails fast, no CLI runs.
        cfg.providers.insert(
            "codex-cli".into(),
            ProviderConfig {
                cli: Some("/nonexistent/codex".into()),
                ..Default::default()
            },
        );
        let mut a = Assistant::new(cfg);
        a.input = "/provider <codex-cli>".into();
        a.submit(&EditorContext::default());
        assert!(a.status().starts_with("codex-cli"), "{}", a.status());
        a.input = "/provider <anthropic>".into();
        a.submit(&EditorContext::default());
        assert!(a.status().starts_with("anthropic"), "{}", a.status());
        assert!(
            HELP.contains("/provider codex-cli"),
            "the help shows a command that can be typed as is"
        );
    }
}
