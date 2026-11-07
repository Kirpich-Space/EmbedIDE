//! AI assistant panel. All provider work is delegated to `embedide-ai`;
//! requests run on a worker thread and stream into the transcript. Nothing
//! is sent until the user presses Enter on a message or command.
use super::layout::{self, Row};
use embedide_ai::{chat, complete, context, edit, AuthMode, Backend, Cancel, Message, ProviderId};
use embedide_workspace::config::AiConfig;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use unicode_width::UnicodeWidthChar;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    User,
    Assistant,
    Info,
    Error,
    Diff,
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
}

enum JobEvent {
    Delta(String),
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
    proposal: Option<(PathBuf, edit::Proposal)>,
    /// Completion awaiting Tab (accept) or Esc (reject).
    pub suggestion: Option<Suggestion>,
    engine: Option<(complete::Engine, Option<PathBuf>, usize)>,
    scroll: usize,
}

const HELP: &str = "Type a message and press Enter. Commands: /file attach current file · /sel attach selection · \
/explain · /edit <instruction> · /apply · /reject · /complete · /provider <anthropic|openai|moonshot|xai> · \
/model <id> · /auth <api-key|subscription> · /cancel · /clear · /status · /help. Nothing is sent until Enter.";

impl Assistant {
    /// Panel state from the user configuration (no network).
    pub fn new(cfg: AiConfig) -> Self {
        let provider = cfg.provider.as_deref().and_then(|p| p.parse().ok());
        let pc = provider.and_then(|p: ProviderId| cfg.providers.get(p.id()).cloned());
        let auth = pc
            .as_ref()
            .and_then(|p| p.auth.as_deref())
            .and_then(|a| a.parse().ok())
            .unwrap_or(AuthMode::ApiKey);
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
            proposal: None,
            suggestion: None,
            engine: None,
            scroll: 0,
        };
        a.note(
            Kind::Info,
            "AI is optional; editing and builds never use it. ".to_string() + HELP,
        );
        a
    }

    fn note(&mut self, kind: Kind, text: impl Into<String>) {
        self.transcript.push((kind, text.into()));
        if self.transcript.len() > 500 {
            self.transcript.drain(..100);
        }
        self.scroll = 0;
    }

    /// One-line state for headers and the status bar.
    pub fn status(&self) -> String {
        let who = match (&self.provider, &self.model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (Some(p), None) => format!("{p}/<no model>"),
            _ => "no provider".into(),
        };
        let state = match &self.job {
            Some(j) if j.streaming => "streaming",
            Some(_) => "waiting",
            None if self.suggestion.is_some() => "suggestion ready",
            None if self.proposal.is_some() => "edit proposed",
            None => "idle",
        };
        let auth = if self.auth == AuthMode::Subscription {
            " (subscription)"
        } else {
            ""
        };
        format!("{who}{auth} · {state}")
    }

    /// Whether a request is running (for the event-loop interval).
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.engine.is_some()
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
        match cmd {
            "/help" => self.note(Kind::Info, HELP),
            "/status" => {
                let s = self.status();
                self.note(Kind::Info, s);
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
                    self.model = self.cfg.providers.get(p.id()).and_then(|c| c.model.clone());
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
            c if c.starts_with('/') => self.note(
                Kind::Error,
                format!("Unknown command {c}. /help lists commands"),
            ),
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
        self.note(Kind::User, text.to_string());
        self.note(
            Kind::Info,
            format!(
                "→ {} bytes to {}{}",
                content.len() + chat::SYSTEM.len(),
                backend.label(),
                if labels.is_empty() {
                    String::new()
                } else {
                    format!(" with {}", labels.join(", "))
                }
            ),
        );
        self.history.push(Message::user(content));
        self.note(Kind::Assistant, String::new());
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
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
        thread::spawn(move || {
            let r = edit::propose(backend.as_ref(), &shown, &text, &instr, &model, max, &c2);
            let _ = tx.send(JobEvent::Edit(path, r));
        });
        self.job = Some(Job {
            rx,
            cancel,
            streaming: false,
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
        thread::spawn(move || {
            let r = complete::complete(backend.as_ref(), &req, &model, &c2);
            let _ = tx.send(JobEvent::Complete(path, offset, r));
        });
        self.job = Some(Job {
            rx,
            cancel,
            streaming: false,
        });
    }

    fn completion_request(&self, ctx: &EditorContext) -> complete::CompletionRequest {
        complete::CompletionRequest {
            language: ctx.language.clone(),
            path: ctx.path.as_ref().map(|p| {
                p.strip_prefix(&ctx.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            }),
            text: ctx.text.clone(),
            line: ctx.line,
            column: ctx.column,
            related: Vec::new(),
            max_tokens: self.cfg.max_tokens.min(256),
        }
    }

    /// Whether automatic inline suggestions are enabled (`ai.inline = true`).
    pub fn inline_enabled(&self) -> bool {
        self.cfg.inline
    }

    /// Typing in Insert mode: queue a debounced suggestion (only with `ai.inline = true`).
    pub fn on_edit(&mut self, ctx: &EditorContext) {
        self.suggestion = None;
        if !self.cfg.inline || ctx.path.is_none() || self.gather(ctx).is_err() {
            return;
        }
        if self.engine.is_none() {
            match self.backend() {
                Ok((b, model)) => {
                    let engine = complete::Engine::new(
                        Arc::from(b),
                        model,
                        Duration::from_millis(self.cfg.completion_debounce_ms),
                    );
                    self.engine = Some((engine, None, 0));
                }
                Err(e) => {
                    self.cfg.inline = false;
                    return self.note(Kind::Error, format!("Inline completion disabled: {e}"));
                }
            }
        }
        let req = self.completion_request(ctx);
        if let Some((engine, path, offset)) = &mut self.engine {
            engine.submit(req);
            *path = ctx.path.clone();
            *offset = ctx.offset;
        }
    }

    /// Drop a pending suggestion (cursor moved, mode changed, Esc).
    pub fn dismiss(&mut self) {
        self.suggestion = None;
        if let Some((engine, _, _)) = &self.engine {
            engine.cancel();
        }
    }

    /// Cancel the running request.
    pub fn cancel(&mut self) {
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
            (Some(_), true) => self.note(
                Kind::Info,
                "Applied to the editor buffer (undo with u; save with :w)",
            ),
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
        let mut changed = false;
        if let Some((engine, path, offset)) = &self.engine {
            if let Some((_, r)) = engine.poll() {
                if let Ok(c) = r {
                    if !c.insertion.is_empty() {
                        self.suggestion = Some(Suggestion {
                            path: path.clone(),
                            offset: *offset,
                            text: c.insertion,
                        });
                    }
                }
                changed = true;
            }
        }
        let Some(job) = &mut self.job else {
            return changed;
        };
        let mut finished = None;
        loop {
            match job.rx.try_recv() {
                Ok(JobEvent::Delta(d)) => {
                    job.streaming = true;
                    if let Some((Kind::Assistant, t)) = self.transcript.last_mut() {
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
            self.job = None;
            changed = true;
            match ev {
                JobEvent::Chat(Ok(r)) => {
                    if let Some((Kind::Assistant, t)) = self.transcript.last_mut() {
                        if t.is_empty() {
                            t.push_str(&r.text);
                        }
                    }
                    self.history.push(Message::assistant(r.text));
                    if let (Some(i), Some(o)) = (r.usage.input_tokens, r.usage.output_tokens) {
                        self.note(Kind::Info, format!("tokens: {i} in, {o} out"));
                    }
                }
                JobEvent::Chat(Err(e)) => {
                    if matches!(self.transcript.last(), Some((Kind::Assistant, t)) if t.is_empty())
                    {
                        self.transcript.pop();
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
                        });
                    }
                }
                JobEvent::Complete(_, _, Err(e)) => self.note(Kind::Error, e.to_string()),
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
        rows.push(layout::label(
            &format!("{}AI · {}", if focused { "> " } else { "" }, self.status()),
            width,
            true,
        ));
        let mut lines: Vec<String> = Vec::new();
        for (kind, text) in &self.transcript {
            let prefix = match kind {
                Kind::User => "you: ",
                Kind::Assistant => "ai: ",
                Kind::Info => "· ",
                Kind::Error => "! ",
                Kind::Diff => "",
            };
            let body = if *kind == Kind::Assistant && text.is_empty() {
                "…"
            } else {
                text.as_str()
            };
            for (i, raw) in body.lines().enumerate() {
                let first = if i == 0 {
                    prefix
                } else if *kind == Kind::Diff {
                    ""
                } else {
                    "  "
                };
                wrap(&format!("{first}{raw}"), width, &mut lines);
            }
            if body.is_empty() {
                lines.push(prefix.to_string());
            }
        }
        if !self.attachments.is_empty() {
            lines.push(format!(
                "· attached for next message: {}",
                self.attachments
                    .iter()
                    .map(|(l, _)| l.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let body_h = height.saturating_sub(2);
        let end = lines.len().saturating_sub(self.scroll.min(lines.len()));
        let start = end.saturating_sub(body_h);
        for l in &lines[start..end] {
            rows.push(layout::label(l, width, false));
        }
        rows.resize_with(height.saturating_sub(1), Row::default);
        let visible: String = {
            let chars: Vec<char> = self.input.chars().collect();
            let max = width.saturating_sub(3);
            let skip = chars.len().saturating_sub(max);
            chars[skip..].iter().collect()
        };
        rows.push(layout::label(&format!("> {visible}"), width, focused));
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
}

fn wrap(text: &str, width: usize, out: &mut Vec<String>) {
    let mut line = String::new();
    let mut w = 0;
    for ch in text.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            w = 0;
        }
        line.push(ch);
        w += cw;
    }
    out.push(line);
}
