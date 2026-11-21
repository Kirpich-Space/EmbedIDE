//! `embed ai …` — network access happens only in models/chat/complete/edit,
//! and only after printing what will be sent. --dry-run never connects.
use crate::{fail, CliResult, Out};
use clap::{Args, Subcommand};
use embedide_ai::{
    agent,
    auth::{self, KeySource},
    chat, complete, context, edit,
    providers::Subscription,
    skills, AuthMode, Cancel, Message, ProviderId, Secret,
};
use embedide_workspace::{Manifest, UserConfig};
use serde_json::{json, Value};
use std::{
    io::{BufRead, IsTerminal, Read, Write},
    path::{Path, PathBuf},
};

/// Provider/model/auth selection shared by AI commands.
#[derive(Args, Clone)]
pub struct Pick {
    /// anthropic, openai, moonshot, xai, or claude-code / codex-cli (the
    /// installed official CLI with its own login; default: ai.provider).
    #[arg(long)]
    provider: Option<String>,
    /// Model id exactly as the provider names it (see `embed ai models`).
    #[arg(long)]
    model: Option<String>,
    /// api-key (default) or subscription (official Claude Code / Codex CLI).
    #[arg(long)]
    auth: Option<AuthMode>,
    /// Output token cap for this request (default ai.max_tokens).
    #[arg(long)]
    max_tokens: Option<u32>,
    /// Print exactly what would be sent and exit; no network, no key needed.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
pub enum AiCmd {
    /// Providers, authentication modes, key status and configured models (no network).
    Providers,
    /// List the models a provider offers (one network request with your key).
    Models {
        #[arg(long)]
        provider: Option<ProviderId>,
    },
    /// Store an API key read from stdin in the private credentials file (0600, outside projects).
    Login { provider: ProviderId },
    /// Remove a stored API key.
    Logout { provider: ProviderId },
    /// Chat about code, diagnostics or architecture (interactive when no message is given).
    Chat {
        /// The message; omit for an interactive session (or pipe it on stdin).
        message: Option<String>,
        /// Attach a file as context (repeatable; [ai] exclude and secret patterns apply).
        #[arg(long = "file")]
        files: Vec<PathBuf>,
        /// Print the full reply at the end instead of streaming.
        #[arg(long)]
        no_stream: bool,
        #[command(flatten)]
        pick: Pick,
    },
    /// Code completion at a file position; prints the text to insert.
    Complete {
        #[arg(long)]
        file: PathBuf,
        /// 1-based line.
        #[arg(long)]
        line: usize,
        /// 1-based column.
        #[arg(long)]
        column: usize,
        /// Related files for context (repeatable).
        #[arg(long)]
        related: Vec<PathBuf>,
        /// Language id (default: from the file extension).
        #[arg(long)]
        language: Option<String>,
        #[command(flatten)]
        pick: Pick,
    },
    /// Run an agent task with Claude Code or Codex CLI in the project: it may
    /// read, search, create, edit and delete files and run builds and tests
    /// under the chosen policy. Approvals are asked on the terminal; without
    /// a terminal every request that needs approval is refused.
    Agent {
        /// What to do.
        task: String,
        /// claude-code or codex-cli (default: ai.provider).
        #[arg(long)]
        provider: Option<String>,
        /// Model for the CLI (default: the CLI's own default).
        #[arg(long)]
        model: Option<String>,
        /// read-only, workspace-write or approval-required (default: ai.agent.policy).
        #[arg(long)]
        policy: Option<String>,
        /// Workspace (default: the project containing the current directory, else the current directory).
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Activate a skill for this task (repeatable; see `embed ai skills`).
        #[arg(long = "skill")]
        skills: Vec<String>,
        /// Build/test command for a fix loop: run it, give failures to the agent, repeat
        /// (up to ai.agent.max_fix_iterations rounds). No shell is used.
        #[arg(long)]
        fix: Option<String>,
        /// Continue an earlier CLI session id.
        #[arg(long)]
        resume: Option<String>,
        /// Do not add EmbedIDE's built-in instructions and the skills detected
        /// for the project (ai.skills.auto).
        #[arg(long)]
        no_auto_skills: bool,
    },
    /// List skills (project .embedide/skills, ai.skills.dirs, global) or show one.
    Skills {
        /// Show this skill's instructions.
        name: Option<String>,
    },
    /// Propose an edit of one file as a unified diff. Nothing is written without --apply.
    Edit {
        file: PathBuf,
        /// What to change.
        #[arg(long)]
        instruction: String,
        /// Write the proposal (asks for confirmation unless --yes).
        #[arg(long)]
        apply: bool,
        /// Confirm --apply non-interactively.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        pick: Pick,
    },
}

fn user() -> CliResult<UserConfig> {
    UserConfig::load().map_err(|e| fail(e.0))
}

struct Chosen {
    provider: ProviderId,
    auth: AuthMode,
    model: String,
    max_tokens: u32,
}

/// A provider id, or `claude-code` / `codex-cli`, which select the provider
/// whose official CLI it is together with subscription authentication.
fn backend_id(s: &str) -> CliResult<(ProviderId, Option<AuthMode>)> {
    match embedide_ai::cli::CliKind::parse(s) {
        Some(kind) => Ok((kind.provider(), Some(AuthMode::Subscription))),
        None => Ok((s.parse::<ProviderId>().map_err(fail)?, None)),
    }
}

fn choose(cfg: &UserConfig, task: &str, pick: &Pick) -> CliResult<Chosen> {
    let spec = cfg.ai.models.get(task).cloned();
    let (spec_provider, spec_model) = match spec.as_deref().and_then(|s| s.split_once(':')) {
        Some((p, m)) => (
            Some(p.parse::<ProviderId>().map_err(fail)?),
            Some(m.to_string()),
        ),
        None => (None, spec.clone()),
    };
    let picked = pick.provider.as_deref().map(backend_id).transpose()?;
    let configured = cfg.ai.provider.as_deref().map(backend_id).transpose()?;
    let (provider, implied_auth) = picked
        .or(spec_provider.map(|p| (p, None)))
        .or(configured)
        .ok_or_else(|| fail("Choose a provider with --provider, or `embed config set ai.provider anthropic|openai|moonshot|xai|claude-code|codex-cli`".into()))?;
    let pc = cfg
        .ai
        .providers
        .get(provider.id())
        .cloned()
        .unwrap_or_default();
    let auth = match pick.auth.or(implied_auth) {
        Some(a) => a,
        None => pc
            .auth
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(fail)?
            .unwrap_or(AuthMode::ApiKey),
    };
    let model = pick.model.clone().or(spec_model).or(pc.model.clone());
    let model = match (model, auth) {
        (Some(m), _) => m,
        (None, AuthMode::Subscription) => "default".into(),
        (None, AuthMode::ApiKey) => {
            return Err(fail(format!(
                "No model selected for {task} with {p}. EmbedIDE does not guess model names: run `embed ai models --provider {p}`, then pass --model or `embed config set ai.providers.{p}.model <id>`",
                p = provider.id()
            )))
        }
    };
    if model.starts_with('-') || model.chars().any(char::is_whitespace) {
        return Err(fail("Invalid model id".into()));
    }
    let max_tokens = pick.max_tokens.unwrap_or(cfg.ai.max_tokens);
    if max_tokens == 0 || max_tokens > 64_000 {
        return Err(fail("--max-tokens must be 1..64000".into()));
    }
    Ok(Chosen {
        provider,
        auth,
        model,
        max_tokens,
    })
}

fn project_excludes(start: &Path) -> (Option<PathBuf>, Vec<String>) {
    let mut dir = start.canonicalize().ok();
    while let Some(d) = dir {
        if let Ok(Some(m)) = Manifest::load(&d) {
            return (Some(d), m.ai.exclude);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    (None, Vec::new())
}

fn gather(files: &[PathBuf], cfg: &UserConfig) -> CliResult<context::Context> {
    let anchor = files
        .first()
        .and_then(|f| f.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let (root, exclude) = project_excludes(if anchor.as_os_str().is_empty() {
        Path::new(".")
    } else {
        &anchor
    });
    // Paths are shown relative to the project (or the current directory) so a
    // provider never learns the user's home layout
    let root = root.or_else(|| std::env::current_dir().ok());
    context::gather(root.as_deref(), files, &exclude, cfg.ai.max_context_bytes)
        .map_err(|e| fail(e.to_string()))
}

fn announce(out: &Out, c: &Chosen, bytes: usize, ctx: Option<&context::Context>) {
    let what = ctx
        .map(|c| {
            c.files
                .iter()
                .map(|f| {
                    format!(
                        "{}{}",
                        f.path,
                        if f.truncated { " (truncated)" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "no files".into());
    out.err(&format!(
        "→ {} {}, model {}: sending {bytes} bytes ({what}); max {} output tokens",
        c.provider.info().name,
        match c.auth {
            AuthMode::ApiKey => "API",
            AuthMode::Subscription => "subscription CLI",
        },
        c.model,
        c.max_tokens
    ));
    for r in ctx.map(|c| c.refused.as_slice()).unwrap_or_default() {
        out.err(&format!("  not sent: {} ({})", r.path, r.reason));
    }
}

fn request_bytes(r: &embedide_ai::Request) -> usize {
    r.system.as_ref().map_or(0, |s| s.len())
        + r.messages.iter().map(|m| m.content.len()).sum::<usize>()
}

fn dry(out: &Out, c: &Chosen, req: &embedide_ai::Request, ctx: Option<&context::Context>) -> Value {
    out.line(&format!(
        "Dry run — nothing sent. Provider {} ({:?}), model {}",
        c.provider.info().name,
        c.auth,
        c.model
    ));
    if let Some(ctx) = ctx {
        for f in &ctx.files {
            out.line(&format!(
                "  attach {} ({} bytes{})",
                f.path,
                f.bytes,
                if f.truncated { ", truncated" } else { "" }
            ));
        }
        for r in &ctx.refused {
            out.line(&format!("  refuse {} ({})", r.path, r.reason));
        }
    }
    out.line("--- system ---");
    out.line(req.system.as_deref().unwrap_or(""));
    for m in &req.messages {
        out.line(&format!("--- {:?} ---", m.role));
        out.line(&m.content);
    }
    json!({"dry_run": true, "provider": c.provider, "auth": c.auth, "model": c.model, "request": req, "context": ctx, "bytes": request_bytes(req)})
}

fn language_of(p: &Path) -> String {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
        "rs" => "rust",
        "zig" => "zig",
        "s" | "S" | "asm" => "asm",
        "f" | "f90" | "f95" | "f03" | "f08" | "F90" => "fortran",
        other => other,
    }
    .to_string()
}

pub fn run(cmd: AiCmd, out: &Out) -> CliResult<(Value, u8)> {
    let cfg = user()?;
    let cancel = Cancel::default();
    let c2 = cancel.clone();
    let _ = ctrlc::set_handler(move || c2.cancel());
    let to_err = |e: embedide_ai::Error| fail(e.to_string());
    match cmd {
        AiCmd::Providers => {
            let mut rows = Vec::new();
            out.line(&format!(
                "{:<10} {:<12} {:<20} {:<16} {}",
                "PROVIDER", "MODELS", "API KEY", "SUBSCRIPTION", "DEFAULT MODEL"
            ));
            for p in ProviderId::ALL {
                let i = p.info();
                let src = auth::key_source(p);
                let pc = cfg.ai.providers.get(i.id).cloned().unwrap_or_default();
                let sub = match i.subscription {
                    Subscription::ClaudeCode => "claude-code".to_string(),
                    Subscription::Codex => "codex".to_string(),
                    Subscription::NotIntegrated(_) => "not integrated".to_string(),
                };
                let key = match src {
                    KeySource::Environment => format!("set ({})", i.key_env),
                    KeySource::CredentialsFile => "stored".into(),
                    KeySource::Missing => format!("missing ({})", i.key_env),
                };
                out.line(&format!(
                    "{:<10} {:<12} {:<20} {:<16} {}",
                    i.id,
                    i.models_family,
                    key,
                    sub,
                    pc.model.as_deref().unwrap_or("—")
                ));
                rows.push(json!({"id": i.id, "name": i.name, "models": i.models_family, "base_url": pc.base_url.clone().unwrap_or(i.base_url.into()),
                    "protocol": i.protocol, "key_env": i.key_env, "key": src, "auth_modes": {"api-key": true, "subscription": i.subscription},
                    "configured_auth": pc.auth, "model": pc.model}));
            }
            out.line("Network is used only by `ai models`, `ai chat`, `ai complete` and `ai edit`. Subscription modes use the provider's official CLI and its own login.");
            Ok((
                json!({"providers": rows, "default_provider": cfg.ai.provider, "models": cfg.ai.models}),
                0,
            ))
        }
        AiCmd::Models { provider } => {
            let p = provider
                .or(cfg
                    .ai
                    .provider
                    .as_deref()
                    .and_then(|s| backend_id(s).ok())
                    .map(|(p, _)| p))
                .ok_or_else(|| fail("--provider required".into()))?;
            out.err(&format!(
                "→ {}: listing models (one request)",
                p.info().name
            ));
            let b = embedide_ai::backend(p, AuthMode::ApiKey, &cfg.ai).map_err(to_err)?;
            let list = b.models().map_err(to_err)?;
            for m in &list {
                out.line(&format!(
                    "{}{}",
                    m.id,
                    m.name
                        .as_deref()
                        .map(|n| format!("  ({n})"))
                        .unwrap_or_default()
                ));
            }
            Ok((json!({"provider": p, "models": list}), 0))
        }
        AiCmd::Login { provider } => {
            if std::io::stdin().is_terminal() {
                eprintln!("Paste the {} API key and press Enter (it will be visible; to avoid that, pipe it: `printf %s \"$KEY\" | embed ai login {}`):", provider.info().name, provider.id());
            }
            let mut line = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut line)
                .map_err(|e| fail(e.to_string()))?;
            let path = auth::store_key(provider, &Secret::new(line.trim())).map_err(to_err)?;
            out.line(&format!(
                "Stored the {} key in {} (0600). It is never written to projects.",
                provider.info().name,
                path.display()
            ));
            Ok((
                json!({"provider": provider, "stored": true, "file": path}),
                0,
            ))
        }
        AiCmd::Logout { provider } => {
            let removed = auth::remove_key(provider).map_err(to_err)?;
            out.line(if removed {
                "Key removed"
            } else {
                "No stored key"
            });
            Ok((json!({"provider": provider, "removed": removed}), 0))
        }
        AiCmd::Chat {
            message,
            files,
            no_stream,
            pick,
        } => {
            let c = choose(&cfg, "chat", &pick)?;
            let ctx = if files.is_empty() {
                None
            } else {
                Some(gather(&files, &cfg)?)
            };
            let interactive = message.is_none() && std::io::stdin().is_terminal();
            let first = match message {
                Some(m) => Some(m),
                None if !interactive => {
                    let mut s = String::new();
                    std::io::stdin()
                        .read_to_string(&mut s)
                        .map_err(|e| fail(e.to_string()))?;
                    Some(s)
                }
                None => None,
            };
            if pick.dry_run {
                let req = chat::request(
                    &c.model,
                    &[],
                    first.as_deref().unwrap_or("<your message>"),
                    ctx.as_ref(),
                    c.max_tokens,
                    !no_stream,
                );
                return Ok((dry(out, &c, &req, ctx.as_ref()), 0));
            }
            let backend = embedide_ai::backend(c.provider, c.auth, &cfg.ai).map_err(to_err)?;
            let mut history: Vec<Message> = Vec::new();
            let mut last = None;
            let mut pending = first;
            let stdin = std::io::stdin();
            loop {
                let text = match pending.take() {
                    Some(t) => t,
                    None if interactive => {
                        eprint!("you> ");
                        let _ = std::io::stderr().flush();
                        let mut l = String::new();
                        if stdin
                            .lock()
                            .read_line(&mut l)
                            .map_err(|e| fail(e.to_string()))?
                            == 0
                            || l.trim() == "/exit"
                        {
                            break;
                        }
                        l
                    }
                    None => break,
                };
                if text.trim().is_empty() {
                    continue;
                }
                let attach = if history.is_empty() {
                    ctx.as_ref()
                } else {
                    None
                };
                let req = chat::request(
                    &c.model,
                    &history,
                    text.trim(),
                    attach,
                    c.max_tokens,
                    !no_stream,
                );
                announce(out, &c, request_bytes(&req), attach);
                let streaming = !no_stream && !out.json;
                let resp = chat::send(backend.as_ref(), &req, &cancel, &mut |d| {
                    if streaming {
                        print!("{d}");
                        let _ = std::io::stdout().flush();
                    }
                })
                .map_err(to_err)?;
                if streaming {
                    println!();
                } else {
                    out.line(&resp.text);
                }
                if let (Some(i), Some(o)) = (resp.usage.input_tokens, resp.usage.output_tokens) {
                    out.err(&format!("  tokens: {i} in, {o} out"));
                }
                history.push(Message::user(
                    req.messages
                        .last()
                        .map(|m| m.content.clone())
                        .unwrap_or_default(),
                ));
                history.push(Message::assistant(resp.text.clone()));
                last = Some(resp);
                if !interactive {
                    break;
                }
            }
            Ok((
                json!({"provider": c.provider, "auth": c.auth, "model": c.model, "response": last}),
                0,
            ))
        }
        AiCmd::Complete {
            file,
            line,
            column,
            related,
            language,
            pick,
        } => {
            let c = choose(&cfg, "complete", &pick)?;
            let own = gather(std::slice::from_ref(&file), &cfg)?;
            if own.files.is_empty() {
                let why = own
                    .refused
                    .first()
                    .map(|r| r.reason.clone())
                    .unwrap_or_default();
                return Err(fail(format!("{} cannot be sent: {why}", file.display())));
            }
            let text = std::fs::read_to_string(&file)
                .map_err(|e| fail(format!("Cannot read {}: {e}", file.display())))?;
            let rel_ctx = gather(&related, &cfg)?;
            let rel: Vec<(String, String)> = rel_ctx
                .files
                .iter()
                .zip(
                    related
                        .iter()
                        .filter(|p| rel_ctx.files.iter().any(|f| p.ends_with(&f.path))),
                )
                .filter_map(|(f, p)| std::fs::read_to_string(p).ok().map(|t| (f.path.clone(), t)))
                .collect();
            let req = complete::CompletionRequest {
                language: language.unwrap_or_else(|| language_of(&file)),
                path: Some(own.files[0].path.clone()),
                text,
                line,
                column,
                related: rel,
                max_tokens: c.max_tokens,
                ..Default::default()
            };
            if pick.dry_run {
                let r = complete::request(&req, &c.model).map_err(to_err)?;
                return Ok((dry(out, &c, &r, Some(&rel_ctx)), 0));
            }
            let backend = embedide_ai::backend(c.provider, c.auth, &cfg.ai).map_err(to_err)?;
            let r = complete::request(&req, &c.model).map_err(to_err)?;
            announce(out, &c, request_bytes(&r), Some(&own));
            let comp =
                complete::complete(backend.as_ref(), &req, &c.model, &cancel).map_err(to_err)?;
            out.line(&comp.insertion);
            Ok((
                json!({"provider": c.provider, "model": c.model, "completion": comp}),
                0,
            ))
        }
        AiCmd::Agent {
            task,
            provider,
            model,
            policy,
            workspace,
            skills,
            fix,
            resume,
            no_auto_skills,
        } => agent_task(
            &cfg,
            out,
            &cancel,
            AgentArgs {
                task,
                provider,
                model,
                policy,
                workspace,
                skills,
                fix,
                resume,
                no_auto_skills,
            },
        ),
        AiCmd::Skills { name } => skills_cmd(&cfg, out, name),
        AiCmd::Edit {
            file,
            instruction,
            apply,
            yes,
            pick,
        } => {
            let c = choose(&cfg, "edit", &pick)?;
            let ctx = gather(std::slice::from_ref(&file), &cfg)?;
            if ctx.files.is_empty() {
                return Err(fail(format!(
                    "{} cannot be sent: {}",
                    file.display(),
                    ctx.refused.first().map(|r| r.reason.as_str()).unwrap_or("")
                )));
            }
            if ctx.files[0].truncated {
                return Err(fail(format!(
                    "{} is larger than ai.max_context_bytes; AI edit needs the whole file",
                    file.display()
                )));
            }
            let content = std::fs::read_to_string(&file).map_err(|e| fail(e.to_string()))?;
            let shown = ctx.files[0].path.clone();
            if pick.dry_run {
                let r = edit::request(&shown, &content, &instruction, &c.model, c.max_tokens);
                return Ok((dry(out, &c, &r, Some(&ctx)), 0));
            }
            let backend = embedide_ai::backend(c.provider, c.auth, &cfg.ai).map_err(to_err)?;
            let r = edit::request(&shown, &content, &instruction, &c.model, c.max_tokens);
            announce(out, &c, request_bytes(&r), Some(&ctx));
            let p = edit::propose(
                backend.as_ref(),
                &shown,
                &content,
                &instruction,
                &c.model,
                c.max_tokens,
                &cancel,
            )
            .map_err(to_err)?;
            if p.diff.is_empty() {
                out.line("No changes proposed.");
                return Ok((json!({"proposal": p, "applied": false}), 0));
            }
            out.line(&p.diff);
            let mut applied = false;
            if apply {
                let confirmed = yes || {
                    if !std::io::stdin().is_terminal() {
                        return Err(fail(
                            "--apply needs confirmation: run in a terminal or add --yes".into(),
                        ));
                    }
                    eprint!("Apply this change to {}? [y/N] ", file.display());
                    let _ = std::io::stderr().flush();
                    let mut a = String::new();
                    std::io::stdin()
                        .lock()
                        .read_line(&mut a)
                        .map_err(|e| fail(e.to_string()))?;
                    a.trim().eq_ignore_ascii_case("y")
                };
                if confirmed {
                    edit::apply(&file, &p).map_err(to_err)?;
                    applied = true;
                    out.line(&format!("Applied to {}", file.display()));
                } else {
                    out.line("Not applied.");
                }
            } else {
                out.line("(not written; use --apply to write it)");
            }
            Ok((json!({"proposal": p, "applied": applied}), 0))
        }
    }
}

struct AgentArgs {
    task: String,
    provider: Option<String>,
    model: Option<String>,
    policy: Option<String>,
    workspace: Option<PathBuf>,
    skills: Vec<String>,
    fix: Option<String>,
    resume: Option<String>,
    no_auto_skills: bool,
}

fn project_root() -> Option<PathBuf> {
    project_excludes(Path::new(".")).0
}

/// Terminal host: progress on stderr, approvals asked on the terminal.
struct TermHost {
    interactive: bool,
    text_open: bool,
    /// Last whole message printed (the final summary is not repeated).
    last: String,
}

impl TermHost {
    fn close_text(&mut self) {
        if self.text_open {
            eprintln!();
            self.text_open = false;
        }
    }
}

impl agent::Host for TermHost {
    fn event(&mut self, event: agent::Event) {
        use agent::Event as E;
        if !matches!(event, E::Text(_)) {
            self.close_text();
        }
        match event {
            E::Started { session, model } => eprintln!(
                "· started{}{}",
                model.map(|m| format!(" · {m}")).unwrap_or_default(),
                session
                    .map(|s| format!(" · session {s}"))
                    .unwrap_or_default()
            ),
            E::Text(t) => {
                eprint!("{t}");
                if !self.text_open {
                    self.last.clear();
                }
                self.last.push_str(&t);
                self.text_open = true;
            }
            E::Message(t) => {
                eprintln!("{t}");
                self.last = t;
            }
            E::Thinking => {}
            E::Tool { name, detail } => eprintln!("· {name} {detail}"),
            E::Command { command, .. } => eprintln!("$ {command}"),
            E::CommandDone { exit, output, .. } => {
                let last: Vec<&str> = output.lines().rev().take(6).collect();
                for l in last.iter().rev() {
                    eprintln!("  │ {l}");
                }
                eprintln!(
                    "  └ {}",
                    match exit {
                        Some(0) => "ok".to_string(),
                        Some(c) => format!("exit {c}"),
                        None => "not run".to_string(),
                    }
                );
            }
            E::FileChanged { path, kind } => eprintln!("✎ {path} ({kind})"),
            E::Denied { what, reason } => eprintln!("⊘ {what}: {reason}"),
            E::Note(n) => eprintln!("· {n}"),
            E::Check {
                command,
                ok,
                output,
            } => {
                for l in output
                    .lines()
                    .rev()
                    .take(8)
                    .collect::<Vec<_>>()
                    .iter()
                    .rev()
                {
                    eprintln!("  │ {l}");
                }
                eprintln!("{} check: {command}", if ok { "✓" } else { "✗" });
            }
        }
    }
    fn approve(&mut self, req: &agent::Approval, cancel: &Cancel) -> agent::Decision {
        self.close_text();
        eprintln!("⚠ Approval needed: {}\n  why: {}", req.summary, req.reason);
        if !self.interactive {
            eprintln!("  refused (no terminal to ask on)");
            return agent::Decision::Deny("No terminal to ask the user; refused".into());
        }
        if cancel.is_cancelled() {
            return agent::Decision::Cancel;
        }
        eprint!("  [y] allow once  [a] allow similar  [n] deny  [s] stop > ");
        let _ = std::io::stderr().flush();
        let mut a = String::new();
        if std::io::stdin().lock().read_line(&mut a).is_err() {
            return agent::Decision::Cancel;
        }
        match a.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => agent::Decision::Allow,
            "a" | "always" => agent::Decision::AllowSimilar,
            "s" | "stop" => agent::Decision::Cancel,
            "" | "n" | "no" => agent::Decision::Deny(String::new()),
            other => agent::Decision::Deny(other.to_string()),
        }
    }
}

fn agent_task(
    cfg: &UserConfig,
    out: &Out,
    cancel: &Cancel,
    a: AgentArgs,
) -> CliResult<(Value, u8)> {
    let id = a
        .provider
        .clone()
        .or(cfg.ai.provider.clone())
        .ok_or_else(|| fail("Choose --provider claude-code or --provider codex-cli".into()))?;
    let kind = embedide_ai::cli::CliKind::parse(&id).ok_or_else(|| {
        fail(format!(
            "Agent mode runs through an official CLI: --provider claude-code or codex-cli (not '{id}')"
        ))
    })?;
    let policy: agent::Policy = a
        .policy
        .as_deref()
        .unwrap_or(&cfg.ai.agent.policy)
        .parse()
        .map_err(fail)?;
    let workspace = match a.workspace {
        Some(w) => w,
        None => project_root()
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| fail("No workspace".into()))?,
    };
    if !workspace.is_dir() {
        return Err(fail(format!("{} is not a directory", workspace.display())));
    }
    let configured = cfg
        .ai
        .providers
        .get(kind.id())
        .or_else(|| cfg.ai.providers.get(kind.provider().id()))
        .and_then(|c| c.cli.clone());
    let program =
        embedide_ai::cli::locate(kind, configured.as_deref()).map_err(|e| fail(e.to_string()))?;
    let model = a.model.or_else(|| {
        cfg.ai
            .providers
            .get(kind.id())
            .and_then(|c| c.model.clone())
    });
    let catalog = skills::discover(Some(&workspace), &cfg.ai.skills);
    let mut chosen = Vec::new();
    for name in &a.skills {
        let info = catalog.get(name).map_err(fail)?;
        chosen.push(skills::load(info, cfg.ai.skills.max_bytes).map_err(fail)?);
    }
    let mut skill_cfg = cfg.ai.skills.clone();
    skill_cfg.auto &= !a.no_auto_skills;
    let (active, problems) = skills::for_task(&workspace, &skill_cfg, &chosen, &[]);
    for p in problems {
        out.err(&format!("✗ {p}"));
    }
    let mut session = agent::Session::new(kind, program, &workspace, policy, &cfg.ai, model);
    if let Some(id) = &a.resume {
        session.resume(id).map_err(|e| fail(e.to_string()))?;
    }
    out.err(&format!(
        "Agent: {} · policy {policy} · workspace {}{}",
        kind.name(),
        session.workspace().display(),
        if active.is_empty() {
            String::new()
        } else {
            format!(
                " · skills {}",
                active
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ));
    let mut host = TermHost {
        interactive: std::io::stdin().is_terminal(),
        text_open: false,
        last: String::new(),
    };
    let task = agent::Task {
        prompt: a.task,
        skills: active,
    };
    let (outcomes, check) = match &a.fix {
        Some(command) => {
            let argv = agent::check_argv(command).map_err(fail)?;
            let (check, outcomes) = agent::fix(
                &mut session,
                &argv,
                &task,
                cfg.ai.agent.max_fix_iterations,
                &mut host,
                cancel,
            )
            .map_err(|e| fail(e.to_string()))?;
            (outcomes, Some(check))
        }
        None => (
            vec![session
                .run(&task, &mut host, cancel)
                .map_err(|e| fail(e.to_string()))?],
            None,
        ),
    };
    host.close_text();
    let mut ok = check.as_ref().is_none_or(|c| c.ok);
    let mut files = Vec::new();
    for o in &outcomes {
        ok &= o.ok;
        if let Some(e) = &o.error {
            out.err(&format!("✗ {e}"));
        }
        for c in &o.changes {
            files.push(json!({"path": c.path, "change": c.kind.label()}));
        }
    }
    if let Some(last) = outcomes.last() {
        // Shown once: on a terminal the streamed answer already was the summary.
        let shown = std::io::stdout().is_terminal()
            && std::io::stderr().is_terminal()
            && last.summary.trim() == host.last.trim();
        if !last.summary.is_empty() && !shown {
            out.line(&last.summary);
        }
    }
    for f in &files {
        out.line(&format!(
            "{} {}",
            f["change"].as_str().unwrap_or(""),
            f["path"].as_str().unwrap_or("")
        ));
    }
    let data = json!({
        "provider": kind.id(),
        "policy": policy.id(),
        "workspace": session.workspace(),
        "session": session.id(),
        "ok": ok,
        "summary": outcomes.last().map(|o| o.summary.clone()).unwrap_or_default(),
        "changes": files,
        "commands": outcomes.iter().map(|o| o.commands).sum::<usize>(),
        "failed_commands": outcomes.iter().map(|o| o.failed_commands).sum::<usize>(),
        "denied": outcomes.iter().map(|o| o.denied).sum::<usize>(),
        "check": check.as_ref().map(|c| json!({"ok": c.ok, "code": c.code})),
        "errors": outcomes.iter().filter_map(|o| o.error.as_ref().map(|e| e.to_string())).collect::<Vec<_>>(),
    });
    if let Some(id) = session.id() {
        out.err(&format!("Session {id} (continue with --resume {id})"));
    }
    Ok((data, if ok { 0 } else { 1 }))
}

fn skills_cmd(cfg: &UserConfig, out: &Out, name: Option<String>) -> CliResult<(Value, u8)> {
    let project = project_root().or_else(|| std::env::current_dir().ok());
    let cat = skills::discover(project.as_deref(), &cfg.ai.skills);
    if let Some(name) = name {
        let info = cat.get(&name).map_err(fail)?;
        let text = skills::load(info, cfg.ai.skills.max_bytes).map_err(fail)?;
        out.line(&format!(
            "{} ({})\n{}\n\n{}",
            info.name, text.source, info.description, text.body
        ));
        return Ok((
            json!({"name": info.name, "source": text.source, "body": text.body}),
            0,
        ));
    }
    if !cfg.ai.skills.enabled {
        out.line("Skills are disabled (ai.skills.enabled = false)");
    }
    let auto: Vec<&str> = match (&project, cfg.ai.skills.auto) {
        (Some(p), true) => skills::detect(p)
            .into_iter()
            .filter(|n| !cfg.ai.skills.exclude.iter().any(|e| e == n))
            .collect(),
        _ => Vec::new(),
    };
    for s in &cat.skills {
        let mark = if auto.contains(&s.name.as_str()) {
            "auto"
        } else {
            ""
        };
        out.line(&format!(
            "{:<20} {:<8} {:<4} {}",
            s.name, s.scope, mark, s.description
        ));
        for w in &s.warnings {
            out.line(&format!("{:<20} note: {w}", ""));
        }
    }
    for s in &cat.shadowed {
        out.line(&format!(
            "{:<20} {:<8} (shadowed by a skill with the same name) {}",
            s.name,
            s.scope,
            s.dir.display()
        ));
    }
    for b in &cat.broken {
        out.line(&format!("broken: {}: {}", b.path.display(), b.error));
    }
    if cat.skills.is_empty() {
        out.line("No skills found. Searched:");
    } else {
        out.line("Searched:");
    }
    for (scope, d) in &cat.searched {
        out.line(&format!("  {scope:<8} {}", d.display()));
    }
    let list: Vec<Value> = cat
        .skills
        .iter()
        .map(|s| json!({"name": s.name, "description": s.description, "scope": s.scope.to_string(), "dir": s.dir, "warnings": s.warnings, "auto": auto.contains(&s.name.as_str())}))
        .collect();
    let broken: Vec<Value> = cat
        .broken
        .iter()
        .map(|b| json!({"path": b.path, "error": b.error}))
        .collect();
    Ok((
        json!({"skills": list, "broken": broken, "searched": cat.searched.iter().map(|(s, d)| json!({"scope": s.to_string(), "dir": d})).collect::<Vec<_>>()}),
        0,
    ))
}
