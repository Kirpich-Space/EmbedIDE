//! Subscription mode through the providers' official CLIs only.
//!
//! - Anthropic: Claude Code in print mode, `claude -p --output-format text`.
//! - OpenAI: Codex CLI, `codex exec --sandbox read-only … -`.
//!
//! The prompt goes on stdin; the CLI runs in an empty temporary directory so
//! it cannot read or change the user's project on its own; authentication is
//! entirely the CLI's (its own login). Usage is not reported by these CLIs.
use crate::{
    providers::Subscription, Backend, Cancel, Error, ModelInfo, ProviderId, Request, Response,
    Result, Role, Usage,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct CliBackend {
    kind: Subscription,
    program: PathBuf,
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| {
            d.join(if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            })
        })
        .find(|p| p.is_file())
}

pub(crate) fn backend(p: ProviderId, configured: Option<PathBuf>) -> Result<Box<dyn Backend>> {
    let kind = p.info().subscription;
    let name = match kind {
        Subscription::ClaudeCode => "claude",
        Subscription::Codex => "codex",
        Subscription::NotIntegrated(why) => {
            return Err(Error::Config(format!(
                "Subscription mode is not available for {}: {why}",
                p.info().name
            )))
        }
    };
    let program = configured.or_else(|| on_path(name)).ok_or_else(|| {
        Error::Config(format!(
            "Subscription mode for {} uses the official `{name}` CLI, which was not found. Install it and log in with it, or set ai.providers.{}.cli",
            p.info().name,
            p.id()
        ))
    })?;
    Ok(Box::new(CliBackend { kind, program }))
}

fn prompt_text(req: &Request) -> String {
    let mut s = String::new();
    if let Some(sys) = &req.system {
        s.push_str(sys);
        s.push_str("\n\n");
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

impl Backend for CliBackend {
    fn label(&self) -> String {
        format!(
            "{} (subscription via {})",
            match self.kind {
                Subscription::ClaudeCode => "Anthropic",
                _ => "OpenAI",
            },
            self.program.display()
        )
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
        if req.model.starts_with('-') {
            return Err(Error::Config("Model id cannot start with '-'".into()));
        }
        let work = std::env::temp_dir().join(format!(
            "embedide-ai-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&work).map_err(|e| Error::Config(e.to_string()))?;
        let last = work.join("last-message.txt");
        let mut cmd = Command::new(&self.program);
        match self.kind {
            Subscription::ClaudeCode => {
                cmd.args(["-p", "--output-format", "text"]);
                if !req.model.is_empty() && req.model != "default" {
                    cmd.args(["--model", &req.model]);
                }
            }
            _ => {
                cmd.args(["exec", "--skip-git-repo-check", "--sandbox", "read-only"]);
                if !req.model.is_empty() && req.model != "default" {
                    cmd.args(["-m", &req.model]);
                }
                cmd.arg("--output-last-message").arg(&last).arg("-");
            }
        }
        let mut child = cmd
            .current_dir(&work)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Config(format!("Cannot run {}: {e}", self.program.display())))?;
        let prompt = prompt_text(req);
        let mut stdin = child.stdin.take().expect("piped");
        let writer = thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
        let mut out = child.stdout.take().expect("piped");
        let mut err = child.stderr.take().expect("piped");
        let ro = thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            s
        });
        let re = thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        let start = Instant::now();
        let status = loop {
            if cancel.is_cancelled() || start.elapsed() > Duration::from_secs(600) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&work);
                return Err(if cancel.is_cancelled() {
                    Error::Cancelled
                } else {
                    Error::Network("CLI did not answer within 10 minutes".into())
                });
            }
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(Error::Network(e.to_string())),
            }
        };
        let _ = writer.join();
        let stdout = ro.join().unwrap_or_default();
        let stderr = re.join().unwrap_or_default();
        let text = std::fs::read_to_string(&last)
            .ok()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(stdout);
        let _ = std::fs::remove_dir_all(&work);
        if !status.success() {
            let msg: String = stderr.trim().chars().take(500).collect();
            return Err(Error::Provider(
                status.code().unwrap_or(1) as u16,
                if msg.is_empty() {
                    "CLI failed".into()
                } else {
                    msg
                },
            ));
        }
        let text = text.trim().to_string();
        on_delta(&text);
        Ok(Response {
            text,
            usage: Usage::default(),
            stop_reason: None,
            model: None,
        })
    }

    fn models(&self) -> Result<Vec<ModelInfo>> {
        Err(Error::Config("Model listing is not available in subscription mode; pass a model the CLI accepts with --model".into()))
    }
}
