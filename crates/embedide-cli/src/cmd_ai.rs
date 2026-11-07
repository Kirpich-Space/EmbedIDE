//! `embed ai …` — network access happens only in models/chat/complete/edit,
//! and only after printing what will be sent. --dry-run never connects.
use crate::{fail, CliResult, Out};
use clap::{Args, Subcommand};
use embedide_ai::{
    auth::{self, KeySource},
    chat, complete, context, edit,
    providers::Subscription,
    AuthMode, Cancel, Message, ProviderId, Secret,
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
    /// anthropic, openai, moonshot or xai (default: ai.provider or the task's model setting).
    #[arg(long)]
    provider: Option<ProviderId>,
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

fn choose(cfg: &UserConfig, task: &str, pick: &Pick) -> CliResult<Chosen> {
    let spec = cfg.ai.models.get(task).cloned();
    let (spec_provider, spec_model) = match spec.as_deref().and_then(|s| s.split_once(':')) {
        Some((p, m)) => (
            Some(p.parse::<ProviderId>().map_err(fail)?),
            Some(m.to_string()),
        ),
        None => (None, spec.clone()),
    };
    let provider = pick
        .provider
        .or(spec_provider)
        .or(cfg.ai.provider.as_deref().map(str::parse).transpose().map_err(fail)?)
        .ok_or_else(|| fail("Choose a provider with --provider, or `embed config set ai.provider anthropic|openai|moonshot|xai`".into()))?;
    let pc = cfg
        .ai
        .providers
        .get(provider.id())
        .cloned()
        .unwrap_or_default();
    let auth = match pick.auth {
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
                .or(cfg.ai.provider.as_deref().and_then(|s| s.parse().ok()))
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
                eprintln!("Paste the {} API key and press Enter (it will be visible; to avoid that, pipe it: `printf %s \"$KEY\" | embedide ai login {}`):", provider.info().name, provider.id());
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
