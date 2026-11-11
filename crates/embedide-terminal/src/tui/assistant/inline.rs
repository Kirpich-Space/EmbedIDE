//! Inline AI suggestions (ghost text) in the editor.
//!
//! * `manual` (default): only Alt+\ (`ai_suggest`) or `:complete` asks.
//! * `auto`: a debounced request after typing stops, only at the end of a
//!   line (or before closing brackets), never while another panel has focus.
//! * `off`: never.
//!
//! One request at a time; a newer one cancels the older, results for a
//! moved cursor are dropped, the same context is answered from a cache, and
//! typing the start of a suggestion keeps the rest without a new request.
//! Auth and rate-limit errors pause automatic suggestions instead of
//! retrying. Nothing is written to the buffer before Tab.
use super::{Assistant, EditorContext, Kind, Suggestion};
use embedide_ai::{cli::CliKind, complete, AuthMode, Error, ProviderId};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// The suggestion service and what it was asked last.
pub(super) struct Inline {
    engine: complete::Engine,
    key: String,
    label: String,
    path: Option<PathBuf>,
    offset: usize,
    after: String,
    manual: bool,
    started: Instant,
    errors: u32,
}

impl Inline {
    pub(super) fn busy(&self) -> bool {
        self.engine.busy()
    }
}

/// Text after the cursor remembered with a suggestion.
fn after_head(text: &str, offset: usize) -> String {
    text.get(offset..).unwrap_or("").chars().take(32).collect()
}

impl Assistant {
    /// Effective inline mode: `off`, `manual` or `auto`.
    pub fn inline_mode(&self) -> &'static str {
        match self.inline_mode {
            Some(m) => m,
            None => match self.cfg.completion_mode() {
                "off" => "off",
                "auto" => "auto",
                _ => "manual",
            },
        }
    }

    /// Whether suggestions are requested while typing.
    pub fn inline_enabled(&self) -> bool {
        self.inline_mode() == "auto" && self.inline_paused.is_none_or(|t| Instant::now() >= t)
    }

    pub(super) fn inline_command(&mut self, arg: &str) {
        match arg {
            "off" | "manual" | "auto" => {
                self.inline_mode = Some(match arg {
                    "off" => "off",
                    "auto" => "auto",
                    _ => "manual",
                });
                self.inline_paused = None;
                if arg == "off" {
                    self.dismiss();
                }
                self.note(
                    Kind::Info,
                    match arg {
                        "off" => "Inline AI suggestions off",
                        "auto" => "Inline AI suggestions while typing (debounced; Alt+\\ still asks at once)",
                        _ => "Inline AI suggestions on request: Alt+\\ in Insert mode (Tab accepts, Esc dismisses)",
                    },
                );
            }
            "" => {
                let backend = match self.completion_backend() {
                    Ok((_, model, label)) => format!("{label} ({model})"),
                    Err(e) => e,
                };
                let paused = match self.inline_paused {
                    Some(t) if t > Instant::now() => format!(
                        " · paused for {} s after an error",
                        (t - Instant::now()).as_secs()
                    ),
                    _ => String::new(),
                };
                self.note(
                    Kind::Info,
                    format!(
                        "Inline suggestions: {} · backend {backend} · debounce {} ms · timeout {} s · context {}+{} chars{paused}. /inline off|manual|auto",
                        self.inline_mode(),
                        self.cfg.completion_debounce_ms,
                        self.cfg.completion.timeout_secs,
                        self.cfg.completion.prefix_chars,
                        self.cfg.completion.suffix_chars,
                    ),
                );
            }
            other => self.note(
                Kind::Error,
                format!("/inline off|manual|auto (not {other})"),
            ),
        }
    }

    /// The backend for suggestions: `ai.completion.provider` or the chat backend.
    fn completion_backend(
        &self,
    ) -> Result<(Box<dyn embedide_ai::Backend>, String, String), String> {
        let cfg = &self.cfg;
        let Some(id) = cfg.completion.provider.as_deref() else {
            let (b, model) = self.backend()?;
            let model = cfg.completion.model.clone().unwrap_or(model);
            let label = b.label();
            return Ok((b, model, label));
        };
        let (provider, auth, model) = match CliKind::parse(id) {
            Some(kind) => (
                kind.provider(),
                AuthMode::Subscription,
                cfg.completion
                    .model
                    .clone()
                    .or_else(|| cfg.providers.get(kind.id()).and_then(|p| p.model.clone()))
                    .unwrap_or_else(|| "default".into()),
            ),
            None => {
                let p: ProviderId = id
                    .parse()
                    .map_err(|e| format!("ai.completion.provider: {e}"))?;
                let pc = cfg.providers.get(p.id());
                let auth = pc
                    .and_then(|c| c.auth.as_deref())
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(AuthMode::ApiKey);
                let model = cfg
                    .completion
                    .model
                    .clone()
                    .or_else(|| pc.and_then(|c| c.model.clone()))
                    .ok_or_else(|| {
                        format!("No model for suggestions with {p}: set ai.completion.model")
                    })?;
                (p, auth, model)
            }
        };
        let b = embedide_ai::backend(provider, auth, cfg).map_err(|e| e.to_string())?;
        let label = b.label();
        Ok((b, model, label))
    }

    pub(super) fn completion_request(&self, ctx: &EditorContext) -> complete::CompletionRequest {
        let prefix = self.cfg.completion.prefix_chars.max(200);
        let suffix = self.cfg.completion.suffix_chars;
        // Declarations outside the window that is sent anyway
        let start = {
            let before = &ctx.text[..ctx.offset.min(ctx.text.len())];
            let count = before.chars().count();
            before
                .char_indices()
                .nth(count.saturating_sub(prefix))
                .map_or(0, |(i, _)| i)
        };
        let end = ctx
            .text
            .get(ctx.offset..)
            .and_then(|a| a.char_indices().nth(suffix).map(|(i, _)| ctx.offset + i))
            .unwrap_or(ctx.text.len());
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
            max_tokens: self.cfg.max_tokens.min(512),
            symbols: complete::symbols(&ctx.text, &ctx.language, start..end, 40),
            project: ctx.project.clone(),
            prefix_chars: prefix,
            suffix_chars: suffix,
        }
    }

    fn ensure_engine(&mut self) -> Result<(), String> {
        let (backend, model, label) = self.completion_backend()?;
        let key = format!("{label}/{model}");
        if self.engine.as_ref().is_some_and(|e| e.key == key) {
            return Ok(());
        }
        let engine = complete::Engine::with_options(
            Arc::from(backend),
            model,
            complete::Options {
                debounce: Duration::from_millis(self.cfg.completion_debounce_ms),
                timeout: Duration::from_secs(self.cfg.completion.timeout_secs),
                cache_entries: self.cfg.completion.cache_entries,
            },
        );
        self.engine = Some(Inline {
            engine,
            key,
            label,
            path: None,
            offset: 0,
            after: String::new(),
            manual: false,
            started: Instant::now(),
            errors: 0,
        });
        Ok(())
    }

    fn queue(&mut self, ctx: &EditorContext, manual: bool) -> Result<(), String> {
        if ctx.path.is_some() {
            self.gather(ctx)?;
        }
        self.ensure_engine()?;
        self.inline_tabs = ctx.indent_tabs;
        self.inline_width = ctx.indent_width.max(1);
        let req = self.completion_request(ctx);
        let inline = self.engine.as_mut().expect("engine");
        if manual {
            inline.engine.submit_now(req);
        } else {
            inline.engine.submit(req);
        }
        inline.path = ctx.path.clone();
        inline.offset = ctx.offset;
        inline.after = after_head(&ctx.text, ctx.offset);
        inline.manual = manual;
        inline.started = Instant::now();
        Ok(())
    }

    /// Typing in Insert mode: queue a debounced suggestion (auto mode only).
    pub fn on_edit(&mut self, ctx: &EditorContext) {
        self.suggestion = None;
        if !self.inline_enabled() || ctx.path.is_none() {
            return;
        }
        // Only where an insertion is natural: the end of the line, or before
        // closing brackets/quotes.
        let rest = ctx
            .text
            .get(ctx.offset..)
            .and_then(|r| r.split('\n').next())
            .unwrap_or("");
        if !rest
            .chars()
            .all(|c| c.is_whitespace() || ")]}>;,\"'`".contains(c))
        {
            if let Some(e) = &self.engine {
                e.engine.cancel();
            }
            return;
        }
        if let Err(e) = self.queue(ctx, false) {
            self.inline_mode = Some("manual");
            self.attention = Some(format!("Automatic AI suggestions off: {e}"));
        }
    }

    /// Ask for a suggestion at the cursor now (Alt+\). Returns a status message.
    pub fn suggest_now(&mut self, ctx: &EditorContext) -> String {
        if self.inline_mode() == "off" {
            return "Inline AI suggestions are off (/inline manual in the AI panel)".into();
        }
        self.suggestion = None;
        self.inline_paused = None;
        match self.queue(ctx, true) {
            Ok(()) => format!(
                "AI suggestion requested from {} (Esc cancels)",
                self.engine.as_ref().map_or("", |e| e.label.as_str())
            ),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Drop the suggestion and any request (cursor moved, Esc, mode change).
    pub fn dismiss(&mut self) {
        self.suggestion = None;
        if let Some(e) = &self.engine {
            e.engine.cancel();
        }
    }

    /// Stop a running request without dropping a shown suggestion (the
    /// editor lost focus).
    pub fn pause_requests(&mut self) {
        if let Some(e) = &self.engine {
            if e.busy() {
                e.engine.cancel();
            }
        }
    }

    /// Spinner text while a suggestion is being computed.
    pub fn inline_status(&self) -> Option<String> {
        let e = self.engine.as_ref().filter(|e| e.busy())?;
        Some(format!(
            "AI suggestion {} {:.1}s (Esc cancels)",
            super::spinner(e.started.elapsed()),
            e.started.elapsed().as_secs_f32()
        ))
    }

    /// The user typed text at a suggestion's position: when it is the start
    /// of the suggestion, keep the rest (no new request). Returns whether
    /// the suggestion was kept or fully typed.
    pub fn type_through(&mut self, path: Option<&Path>, text: &str, cursor: usize) -> bool {
        let Some(s) = &mut self.suggestion else {
            return false;
        };
        if s.path.as_deref() != path || cursor <= s.offset {
            return false;
        }
        let Some(typed) = text.get(s.offset..cursor) else {
            return false;
        };
        let after_now = text.get(cursor..).unwrap_or("");
        if !s.text.starts_with(typed) || !after_now.starts_with(s.after.as_str()) {
            return false;
        }
        let rest = s.text[typed.len()..].to_string();
        if rest.is_empty() {
            self.suggestion = None;
        } else {
            s.offset = cursor;
            s.text = rest;
        }
        true
    }

    pub(super) fn tick_inline(&mut self) -> bool {
        let Some(e) = &mut self.engine else {
            return false;
        };
        let busy = e.busy();
        let Some((_, result)) = e.engine.poll() else {
            // The status-bar spinner advances while a request runs.
            return busy;
        };
        let (path, offset, after, manual) = (e.path.clone(), e.offset, e.after.clone(), e.manual);
        match result {
            Ok(c) if c.insertion.trim().is_empty() => {
                e.errors = 0;
                if manual {
                    self.attention = Some("No AI suggestion here".into());
                }
            }
            Ok(c) => {
                e.errors = 0;
                let text = complete::reindent(
                    &complete::limit_lines(&c.insertion, self.cfg.completion.max_lines),
                    self.inline_tabs,
                    self.inline_width,
                );
                let lines = text.lines().count();
                self.suggestion = Some(Suggestion {
                    path,
                    offset,
                    text,
                    after,
                });
                if manual || lines > 1 {
                    self.attention = Some(format!(
                        "AI suggestion{}: Tab accepts · Esc dismisses",
                        if lines > 1 {
                            format!(" ({lines} lines)")
                        } else {
                            String::new()
                        }
                    ));
                }
            }
            Err(Error::Cancelled) => {}
            Err(err) => {
                e.errors += 1;
                let errors = e.errors;
                let pause = match &err {
                    Error::Auth(_) | Error::Config(_) | Error::Offline => {
                        Some(Duration::from_secs(3600))
                    }
                    Error::RateLimited(secs, _) => {
                        Some(Duration::from_secs(secs.unwrap_or(60).max(10)))
                    }
                    _ if errors >= 3 => Some(Duration::from_secs(60)),
                    _ => None,
                };
                match pause {
                    Some(p) if !manual => {
                        self.inline_paused = Some(Instant::now() + p);
                        self.attention = Some(format!(
                            "Automatic AI suggestions paused: {err} (Alt+\\ still asks)"
                        ));
                    }
                    _ => {
                        if manual || errors == 1 {
                            self.attention = Some(format!("AI suggestion failed: {err}"));
                        }
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedide_workspace::config::AiConfig;

    fn ctx(text: &str, offset: usize) -> EditorContext {
        EditorContext {
            path: Some(PathBuf::from("/w/a.c")),
            root: PathBuf::from("/w"),
            text: text.into(),
            offset,
            line: 1,
            column: offset + 1,
            language: "c".into(),
            indent_width: 4,
            ..Default::default()
        }
    }

    #[test]
    fn typing_the_start_of_a_suggestion_keeps_the_rest() {
        let mut a = Assistant::new(AiConfig::default());
        let path = Some(PathBuf::from("/w/a.c"));
        a.suggestion = Some(Suggestion {
            path: path.clone(),
            offset: 4,
            text: "return a + b;".into(),
            after: "\n}".into(),
        });
        // "ret" typed
        assert!(a.type_through(path.as_deref(), "    ret\n}", 7));
        let s = a.suggestion.as_ref().unwrap();
        assert_eq!((s.offset, s.text.as_str()), (7, "urn a + b;"));
        // Something else typed: not kept
        assert!(!a.type_through(path.as_deref(), "    retx\n}", 8));
        // An auto-inserted closer changed the text after the cursor: not kept
        a.suggestion = Some(Suggestion {
            path: path.clone(),
            offset: 0,
            text: "(a, b)".into(),
            after: "\n".into(),
        });
        assert!(!a.type_through(path.as_deref(), "()\n", 1));
        // Typing all of it consumes the suggestion
        a.suggestion = Some(Suggestion {
            path: path.clone(),
            offset: 0,
            text: "x;".into(),
            after: String::new(),
        });
        assert!(a.type_through(path.as_deref(), "x;", 2));
        assert!(a.suggestion.is_none());
    }

    #[test]
    fn modes_gate_requests_and_the_request_carries_context() {
        let mut a = Assistant::new(AiConfig::default());
        assert_eq!(a.inline_mode(), "manual");
        assert!(!a.inline_enabled(), "manual mode never asks while typing");
        a.inline_command("auto");
        assert!(a.inline_enabled());
        a.inline_paused = Some(Instant::now() + Duration::from_secs(60));
        assert!(!a.inline_enabled(), "paused after an error");
        a.inline_command("off");
        assert!(a.suggest_now(&ctx("int x", 5)).contains("off"));
        let text = "int helper(int x);\n\nint main(void) {\n    \n}\n";
        let at = text.find("    \n").unwrap() + 4;
        let mut c = ctx(text, at);
        c.project = Some("demo (C)".into());
        a.cfg.completion.prefix_chars = 200;
        let req = a.completion_request(&c);
        assert_eq!(req.project.as_deref(), Some("demo (C)"));
        assert_eq!(req.prefix_chars, 200);
        assert!(
            req.symbols.is_empty(),
            "declarations inside the sent window are not repeated"
        );
    }
}
