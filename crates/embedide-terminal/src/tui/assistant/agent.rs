//! Agent mode of the AI panel: tasks run by Claude Code / Codex CLI in the
//! project (see `embedide_ai::agent`). The panel shows each step — commands
//! with their results, files changed, refusals — asks for approvals inline
//! (y / a / n), and keeps the CLI session so follow-up tasks share context.
use super::{Assistant, EditorContext, Kind};
use embedide_ai::{
    agent::{
        self as rt,
        snapshot::{Change, ChangeKind, Snapshot},
        Approval, ApprovalKind, Decision, Event, Host, Outcome, Policy, Session, Task,
    },
    cli::{self, CliKind},
    skills, Cancel,
};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

/// Messages from the agent worker.
pub(super) enum Msg {
    Event(Event),
    Approval(Approval),
    Done(Box<Session>, Result<Finished, String>),
}

/// What a finished task produced.
pub(super) struct Finished {
    outcomes: Vec<Outcome>,
    check: Option<rt::CheckResult>,
}

/// A running agent task.
pub(super) struct AgentRun {
    rx: mpsc::Receiver<Msg>,
    cancel: Cancel,
    decide: mpsc::Sender<Decision>,
    handle: Option<thread::JoinHandle<()>>,
    /// When the task started.
    pub(super) started: Instant,
    pending: Option<Approval>,
    steps: usize,
    /// Transcript entry the current answer text streams into.
    pub(super) text_slot: Option<usize>,
    /// Transcript entry of the command that is running.
    pub(super) command_slot: Option<usize>,
    root: PathBuf,
    stopping: bool,
}

/// What the last task changed.
pub(super) struct Review {
    snapshot: Arc<Snapshot>,
    changes: Vec<Change>,
}

/// Bridges the worker thread to the panel.
struct ChannelHost {
    tx: mpsc::Sender<Msg>,
    decisions: mpsc::Receiver<Decision>,
}

impl Host for ChannelHost {
    fn event(&mut self, event: Event) {
        let _ = self.tx.send(Msg::Event(event));
    }
    fn approve(&mut self, request: &Approval, cancel: &Cancel) -> Decision {
        if self.tx.send(Msg::Approval(request.clone())).is_err() {
            return Decision::Cancel;
        }
        loop {
            match self.decisions.recv_timeout(Duration::from_millis(100)) {
                Ok(d) => return d,
                Err(mpsc::RecvTimeoutError::Timeout) if !cancel.is_cancelled() => {}
                _ => return Decision::Cancel,
            }
        }
    }
}

/// `/usr/bin/bash -lc 'make'` (how Codex reports commands) → `make`.
fn display_command(command: &str) -> String {
    for prefix in [
        "/usr/bin/bash -lc ",
        "/bin/bash -lc ",
        "bash -lc ",
        "/bin/sh -c ",
        "sh -c ",
    ] {
        if let Some(rest) = command.strip_prefix(prefix) {
            let rest = rest.trim();
            if rest.len() >= 2
                && ((rest.starts_with('\'') && rest.ends_with('\''))
                    || (rest.starts_with('"') && rest.ends_with('"')))
            {
                return rest[1..rest.len() - 1].to_string();
            }
            return rest.to_string();
        }
    }
    command.to_string()
}

fn relative(root: &Path, path: &str) -> String {
    let p = Path::new(path);
    let canonical = root.canonicalize().ok();
    p.strip_prefix(root)
        .ok()
        .or_else(|| canonical.as_deref().and_then(|c| p.strip_prefix(c).ok()))
        .map(|r| r.display().to_string())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| path.to_string())
}

/// The command `/fix` runs when none is given.
fn default_check(root: &Path, configured: Option<&str>) -> Result<Vec<String>, String> {
    if let Some(c) = configured {
        return rt::check_argv(c);
    }
    if let Ok(exe) = std::env::current_exe() {
        if exe
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("embed"))
        {
            return Ok(vec![exe.display().to_string(), "build".into(), ".".into()]);
        }
    }
    for (file, argv) in [
        ("Makefile", &["make"][..]),
        ("makefile", &["make"][..]),
        ("Cargo.toml", &["cargo", "build"][..]),
        ("build.zig", &["zig", "build"][..]),
    ] {
        if root.join(file).is_file() {
            return Ok(argv.iter().map(|s| s.to_string()).collect());
        }
    }
    Err("No build command found: /fix <command>, or set ai.agent.fix_command".into())
}

impl Assistant {
    /// Whether the agent waits for the user's approval.
    pub fn approval_pending(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.pending.is_some())
    }

    /// Whether an agent task is running.
    pub fn agent_running(&self) -> bool {
        self.run.is_some()
    }

    /// Files the agent changed since the last call (absolute paths), for
    /// reloading editor buffers.
    pub fn take_changed_files(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.changed)
    }

    /// A one-time message for the status bar (approval requests while the
    /// panel may be hidden).
    pub fn take_attention(&mut self) -> Option<String> {
        self.attention.take()
    }

    /// Answer the pending approval.
    pub fn answer(&mut self, decision: Decision) {
        let Some(run) = &mut self.run else { return };
        let Some(req) = run.pending.take() else {
            return;
        };
        let _ = run.decide.send(decision.clone());
        let what = req.summary.lines().next().unwrap_or("").to_string();
        match decision {
            Decision::Allow => self.note(Kind::Info, format!("Allowed: {what}")),
            Decision::AllowSimilar => self.note(
                Kind::Info,
                format!(
                    "Allowed (and similar {} requests for this session): {what}",
                    req.tool
                ),
            ),
            Decision::Deny(_) => self.note(Kind::Info, format!("Denied: {what}")),
            Decision::Cancel => self.note(Kind::Info, "Stopping the agent…"),
        }
    }

    pub(super) fn agent_activity(&self) -> Option<String> {
        let run = self.run.as_ref()?;
        let secs = run.started.elapsed().as_secs();
        if run.pending.is_some() {
            return Some(format!("⚠ approval needed · {secs}s"));
        }
        let spin = super::spinner(run.started.elapsed());
        let what = if run.stopping { "stopping" } else { "agent" };
        Some(format!(
            "{spin} {what} {secs}s · step {} · {}",
            run.steps, self.policy
        ))
    }

    pub(super) fn cancel_agent(&mut self) {
        if let Some(run) = &mut self.run {
            if !run.stopping {
                run.stopping = true;
                run.cancel.cancel();
                run.pending = None;
                self.note(
                    Kind::Info,
                    "Stopping the agent and every command it started…",
                );
            }
        }
    }

    pub(super) fn shutdown_agent(&mut self) {
        if let Some(mut run) = self.run.take() {
            run.cancel.cancel();
            if let Some(handle) = run.handle.take() {
                let deadline = Instant::now() + Duration::from_secs(3);
                while !handle.is_finished() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                if handle.is_finished() {
                    let _ = handle.join();
                }
            }
        }
    }

    /// Agent commands; returns whether `cmd` was one.
    pub(super) fn agent_command(
        &mut self,
        cmd: &str,
        arg: &str,
        line: &str,
        ctx: &EditorContext,
    ) -> bool {
        if self.approval_pending() {
            let decision = match cmd {
                "y" | "yes" | "/approve" | "/allow" => Some(Decision::Allow),
                "a" | "always" | "/allow-similar" => Some(Decision::AllowSimilar),
                "n" | "no" | "/deny" => Some(Decision::Deny(arg.to_string())),
                _ => None,
            };
            if let Some(d) = decision {
                self.answer(d);
                return true;
            }
        }
        if self.run.is_some() && !line.starts_with('/') {
            self.note(
                Kind::Error,
                if self.approval_pending() {
                    "The agent waits for your answer: y allow · a allow similar · n deny · /cancel"
                } else {
                    "The agent is working; wait for it or /cancel"
                },
            );
            return true;
        }
        match cmd {
            "/agent" if arg.is_empty() => self.agent_status(),
            "/agent" => self.start_agent(ctx, arg.to_string(), None),
            "/mode" => match arg {
                "agent" => {
                    self.agent_mode = true;
                    self.note(Kind::Info, "Agent mode: plain text starts an agent task in the project (/mode chat to chat)");
                }
                "chat" => {
                    self.agent_mode = false;
                    self.note(Kind::Info, "Chat mode: plain text is a chat message (/agent <what to do> still runs the agent)");
                }
                "" => self.note(
                    Kind::Info,
                    format!(
                        "Mode: {}",
                        match (self.agent_mode, self.agent_active()) {
                            (true, true) => "agent",
                            (true, false) => "agent (this backend chats: agent tasks need /provider claude-code or codex-cli)",
                            _ => "chat",
                        }
                    ),
                ),
                other => self.note(
                    Kind::Error,
                    format!("Unknown mode {other}: /mode agent or /mode chat"),
                ),
            },
            "/policy" if arg.is_empty() => {
                let lines: Vec<String> = Policy::ALL
                    .iter()
                    .map(|p| {
                        format!(
                            "{} {:<17} {}",
                            if *p == self.policy { "●" } else { "○" },
                            p.id(),
                            p.describe()
                        )
                    })
                    .collect();
                for l in lines {
                    self.note(Kind::Info, l);
                }
            }
            "/policy" => match arg.parse::<Policy>() {
                Ok(p) => {
                    self.policy = p;
                    if let Some(s) = &mut self.session {
                        s.set_policy(p);
                    }
                    self.note(Kind::Info, format!("Policy {p}: {}", p.describe()));
                    if p == Policy::ApprovalRequired && self.cli_backend() == Some(CliKind::Codex) {
                        self.note(Kind::Info, "Codex CLI has no per-command approval in exec mode: it plans read-only first and carries out the plan after you approve it");
                    }
                }
                Err(e) => self.note(Kind::Error, e),
            },
            "/new" => {
                self.session = None;
                self.review = None;
                self.note(
                    Kind::Info,
                    "New agent session: the next task starts without earlier context",
                );
            }
            "/changes" => self.list_changes(),
            "/review" => self.review_changes(arg),
            "/revert" => self.revert_changes(arg),
            "/fix" => {
                let argv = if arg.is_empty() {
                    default_check(&ctx.root, self.cfg.agent.fix_command.as_deref())
                } else {
                    rt::check_argv(arg)
                };
                match argv {
                    Ok(argv) => self.start_agent(ctx, String::new(), Some(argv)),
                    Err(e) => self.note(Kind::Error, e),
                }
            }
            "/skills" => self.list_skills(&ctx.root),
            "/skill" => self.skill_command(arg, &ctx.root),
            _ => return false,
        }
        true
    }

    fn agent_status(&mut self) {
        let who = self
            .cli_backend()
            .map(|k| k.name().to_string())
            .unwrap_or_else(|| "no CLI backend (/provider claude-code or codex-cli)".into());
        let session = match &self.session {
            Some(s) => format!(
                "session {} · {} turn(s) · workspace {}",
                s.id().unwrap_or("not started"),
                s.turns,
                s.workspace().display()
            ),
            None => "no session yet".into(),
        };
        let skills = if self.skills.is_empty() {
            "none".to_string()
        } else {
            self.skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.note(
            Kind::Info,
            format!(
                "Agent: {who} · policy {} · mode {} · {session} · skills: {skills}",
                self.policy,
                if self.agent_mode { "agent" } else { "chat" }
            ),
        );
        self.note(
            Kind::Info,
            "Run a task: /agent <what to do>  (or /mode agent, then type tasks)",
        );
    }

    /// Start an agent turn (`fix`: run that check in a fix loop).
    pub(super) fn start_agent(
        &mut self,
        ctx: &EditorContext,
        prompt: String,
        fix: Option<Vec<String>>,
    ) {
        if self.run.is_some() {
            return self.note(Kind::Error, "An agent task is already running (/cancel)");
        }
        // `/agent task` copied from a usage line is not a task.
        let placeholder = prompt.trim().trim_matches(['<', '>']).to_lowercase();
        if fix.is_none()
            && ["task", "what to do", "задача", "задание", "..."].contains(&placeholder.as_str())
        {
            return self.note(
                Kind::Error,
                "Describe the task after /agent, for example: /agent fix the warnings in main.c and run it",
            );
        }
        let Some(kind) = self.cli_backend() else {
            return self.note(
                Kind::Error,
                "Agent mode runs through an official CLI: /provider claude-code or /provider codex-cli",
            );
        };
        let root = ctx.root.clone();
        let reuse = self.session.take().filter(|s| {
            s.kind() == kind && root.canonicalize().ok().as_deref() == Some(s.workspace())
        });
        let mut session = match reuse {
            Some(s) => s,
            None => {
                let configured = self
                    .cfg
                    .providers
                    .get(kind.id())
                    .or_else(|| self.cfg.providers.get(kind.provider().id()))
                    .and_then(|c| c.cli.clone());
                let program = match cli::locate(kind, configured.as_deref()) {
                    Ok(p) => p,
                    Err(e) => return self.note(Kind::Error, e.to_string()),
                };
                Session::new(
                    kind,
                    program,
                    &root,
                    self.policy,
                    &self.cfg,
                    self.model.clone(),
                )
            }
        };
        session.set_policy(self.policy);
        let mut text = prompt.clone();
        if !text.is_empty() {
            if let Some(path) = &ctx.path {
                text.push_str(&format!(
                    "\n\n(The user's editor shows {} at line {}.)",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    ctx.line
                ));
            }
            let attach: String = self.attachments.iter().map(|(_, t)| t.as_str()).collect();
            if !attach.is_empty() {
                text = format!("{attach}\n{text}");
                self.attachments.clear();
            }
        }
        let (skills, problems) =
            skills::for_task(&root, &self.cfg.skills, &self.skills, &self.skills_off);
        for p in problems {
            self.note(Kind::Error, p);
        }
        let already = session.skills_sent();
        let new: Vec<String> = skills
            .iter()
            .filter(|s| !already.contains(&s.name))
            .map(|s| {
                if s.trusted {
                    format!("{} (built-in)", s.name)
                } else {
                    s.name.clone()
                }
            })
            .collect();
        let task = Task {
            prompt: text,
            skills,
        };
        self.note(Kind::Header, "You");
        match &fix {
            Some(argv) => self.note(Kind::User, format!("/fix {}", argv.join(" "))),
            None => self.note(Kind::User, prompt),
        }
        self.note(
            Kind::Header,
            format!("Agent · {} · {}", kind.name(), self.policy),
        );
        if session.turns > 0 {
            self.note(
                Kind::Meta,
                format!(
                    "continuing session {} (turn {})",
                    session.id().unwrap_or("?"),
                    session.turns + 1
                ),
            );
        }
        if !new.is_empty() {
            self.note(Kind::Meta, format!("skills: {}", new.join(", ")));
        }
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let (dtx, drx) = mpsc::channel();
        let rounds = self.cfg.agent.max_fix_iterations;
        let handle = thread::spawn(move || {
            let mut host = ChannelHost {
                tx: tx.clone(),
                decisions: drx,
            };
            let result = match fix {
                Some(argv) => rt::fix(&mut session, &argv, &task, rounds, &mut host, &c2).map(
                    |(check, outcomes)| Finished {
                        outcomes,
                        check: Some(check),
                    },
                ),
                None => session.run(&task, &mut host, &c2).map(|o| Finished {
                    outcomes: vec![o],
                    check: None,
                }),
            }
            .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Done(Box::new(session), result));
        });
        self.run = Some(AgentRun {
            rx,
            cancel,
            decide: dtx,
            handle: Some(handle),
            started: Instant::now(),
            pending: None,
            steps: 0,
            text_slot: None,
            command_slot: None,
            root,
            stopping: false,
        });
    }

    /// Process worker messages; returns whether the panel changed.
    pub(super) fn tick_agent(&mut self) -> bool {
        let Some(run) = &self.run else { return false };
        let frame = run.started.elapsed().as_millis() / 100;
        let mut changed = frame != self.anim;
        self.anim = frame;
        let mut msgs = Vec::new();
        let mut gone = false;
        loop {
            match run.rx.try_recv() {
                Ok(m) => {
                    msgs.push(m);
                    if msgs.len() >= 512 {
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        for m in msgs {
            changed = true;
            match m {
                Msg::Event(e) => self.agent_event(e),
                Msg::Approval(a) => self.agent_approval(a),
                Msg::Done(session, result) => self.agent_done(*session, result),
            }
        }
        if gone && self.run.is_some() {
            self.run = None;
            self.note(Kind::Error, "The agent worker stopped unexpectedly");
            changed = true;
        }
        changed
    }

    fn close_text(&mut self) {
        if let Some(run) = &mut self.run {
            run.text_slot = None;
        }
    }

    fn step(&mut self, kind: Kind, text: String) {
        self.close_text();
        self.note(kind, text);
        if let Some(run) = &mut self.run {
            if kind == Kind::Step {
                run.steps += 1;
            }
        }
    }

    fn agent_event(&mut self, e: Event) {
        let root = self
            .run
            .as_ref()
            .map(|r| r.root.clone())
            .unwrap_or_default();
        match e {
            Event::Started { session, model } => {
                let mut parts = Vec::new();
                if let Some(m) = model {
                    parts.push(m);
                }
                if let Some(s) = session {
                    parts.push(format!("session {}", s.chars().take(8).collect::<String>()));
                }
                if !parts.is_empty() {
                    self.note(Kind::Meta, parts.join(" · "));
                }
            }
            Event::Text(t) => {
                let slot = self.run.as_ref().and_then(|r| r.text_slot);
                match slot.and_then(|i| self.transcript.get_mut(i)) {
                    Some((Kind::Assistant, text)) => text.push_str(&t),
                    _ => {
                        if t.trim().is_empty() {
                            return;
                        }
                        self.note(Kind::Assistant, t.trim_start().to_string());
                        let i = self.transcript.len() - 1;
                        if let Some(run) = &mut self.run {
                            run.text_slot = Some(i);
                        }
                    }
                }
            }
            Event::Message(t) => {
                self.close_text();
                if !t.trim().is_empty() {
                    self.note(Kind::Assistant, t);
                }
            }
            Event::Thinking => {}
            Event::Tool { name, detail } => {
                let detail = relative(&root, &detail);
                self.step(
                    Kind::Step,
                    format!("{name} {detail}").trim_end().to_string(),
                );
            }
            Event::Command { command, .. } => {
                self.step(Kind::Step, format!("$ {}", display_command(&command)));
                let i = self.transcript.len() - 1;
                if let Some(run) = &mut self.run {
                    run.command_slot = Some(i);
                }
            }
            Event::CommandDone { exit, output, .. } => {
                if let Some(run) = &mut self.run {
                    run.command_slot = None;
                }
                if exit.is_some() {
                    let lines: Vec<&str> =
                        output.lines().filter(|l| !l.trim().is_empty()).collect();
                    let keep = if exit == Some(0) { 3 } else { 8 };
                    if !lines.is_empty() {
                        let skip = lines.len().saturating_sub(keep);
                        let mut text = String::new();
                        if skip > 0 {
                            text.push_str(&format!("… {skip} more lines\n"));
                        }
                        text.push_str(&lines[skip..].join("\n"));
                        self.note(Kind::Output, text);
                    }
                }
                match exit {
                    Some(0) => self.note(Kind::StepOk, "ok"),
                    Some(c) => self.note(Kind::StepFail, format!("exit {c}")),
                    None => self.note(Kind::StepFail, "not run"),
                }
            }
            Event::FileChanged { path, kind } => {
                self.step(
                    Kind::Changed,
                    format!("{} ({kind})", relative(&root, &path)),
                );
            }
            Event::Denied { what, reason } => {
                self.step(Kind::Denied, format!("{what}: {reason}"));
            }
            Event::Note(n) => self.step(Kind::Info, n),
            Event::Check {
                command,
                ok,
                output,
            } => {
                self.step(Kind::Step, format!("$ {command}   (check)"));
                let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
                let skip = lines.len().saturating_sub(if ok { 2 } else { 8 });
                if !lines.is_empty() {
                    self.note(Kind::Output, lines[skip..].join("\n"));
                }
                if ok {
                    self.note(Kind::StepOk, "check passes");
                } else {
                    self.note(Kind::StepFail, "check fails");
                }
            }
        }
    }

    fn agent_approval(&mut self, a: Approval) {
        self.close_text();
        let text = match a.kind {
            ApprovalKind::Plan => format!(
                "Approve this plan? {}\n{}\ny carry it out · n reject · /cancel stop",
                a.reason, a.summary
            ),
            _ => format!(
                "Approval needed: {}\nwhy: {}\ny allow once · a allow similar {} requests · n deny · /cancel stop",
                a.summary, a.reason, a.tool
            ),
        };
        self.note(Kind::Approval, text);
        let short: String = a
            .summary
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect();
        self.attention = Some(format!(
            "AI agent needs approval: {short} — answer y / a / n in the AI panel"
        ));
        if let Some(run) = &mut self.run {
            run.pending = Some(a);
        }
    }

    fn agent_done(&mut self, session: Session, result: Result<Finished, String>) {
        let run = self.run.take();
        if let Some(mut run) = run {
            if let Some(h) = run.handle.take() {
                let _ = h.join();
            }
        }
        let started_root = session.workspace().to_path_buf();
        self.session = Some(session);
        let fin = match result {
            Ok(f) => f,
            Err(e) => return self.note(Kind::Error, e),
        };
        let Some(last) = fin.outcomes.last() else {
            if let Some(check) = &fin.check {
                if check.ok {
                    self.note(Kind::StepOk, "The check already passes; nothing to fix");
                }
            }
            return;
        };
        let mut cancelled = false;
        for o in &fin.outcomes {
            match &o.error {
                Some(embedide_ai::Error::Cancelled) => cancelled = true,
                Some(e) => self.note(Kind::Error, e.to_string()),
                None => {}
            }
        }
        if cancelled {
            self.note(Kind::Info, "Agent stopped; its processes were ended");
        }
        // The final answer, unless it was just streamed
        let summary = last.summary.trim().to_string();
        let shown = self
            .transcript
            .iter()
            .rev()
            .find(|(k, _)| *k == Kind::Assistant)
            .is_some_and(|(_, t)| t.trim() == summary);
        if !summary.is_empty() && !shown {
            self.note(Kind::Assistant, summary);
        }
        // Changes relative to the snapshot before the first turn
        let review = fin
            .outcomes
            .first()
            .and_then(|o| o.snapshot.clone())
            .map(|snapshot| Review {
                changes: snapshot.changes(),
                snapshot,
            });
        let ok = fin.outcomes.iter().all(|o| o.ok) && fin.check.as_ref().is_none_or(|c| c.ok);
        let commands: usize = fin.outcomes.iter().map(|o| o.commands).sum();
        let failed: usize = fin.outcomes.iter().map(|o| o.failed_commands).sum();
        let denied: usize = fin.outcomes.iter().map(|o| o.denied).sum();
        let approvals: usize = fin.outcomes.iter().map(|o| o.approvals).sum();
        let secs: f32 = fin.outcomes.iter().map(|o| o.elapsed.as_secs_f32()).sum();
        let tokens = fin.outcomes.iter().fold((0u64, 0u64, false), |acc, o| {
            match (o.usage.input_tokens, o.usage.output_tokens) {
                (Some(i), Some(out)) => (acc.0 + i, acc.1 + out, true),
                _ => acc,
            }
        });
        let changed = review.as_ref().map_or(0, |r| r.changes.len());
        let mut meta = format!(
            "{} {} · {changed} file(s) changed · {commands} command(s)",
            if ok { "■" } else { "□" },
            if cancelled {
                "stopped"
            } else if ok {
                "done"
            } else {
                "finished with problems"
            },
        );
        if failed > 0 {
            meta.push_str(&format!(" ({failed} failed)"));
        }
        if approvals > 0 {
            meta.push_str(&format!(" · {approvals} approval(s)"));
        }
        if denied > 0 {
            meta.push_str(&format!(" · {denied} refused"));
        }
        meta.push_str(&format!(" · {secs:.1} s"));
        if tokens.2 {
            meta.push_str(&format!(" · tokens {} in, {} out", tokens.0, tokens.1));
        }
        if fin.outcomes.len() > 1 {
            meta.push_str(&format!(" · {} rounds", fin.outcomes.len()));
        }
        self.note(Kind::Meta, meta);
        if let Some(check) = &fin.check {
            if check.ok {
                self.note(Kind::StepOk, "check passes");
            } else {
                self.note(Kind::StepFail, "check still fails (see the output above)");
            }
        }
        if let Some(r) = &review {
            for c in r.changes.iter().take(20) {
                self.note(
                    Kind::Changed,
                    format!("{} ({})", c.path.display(), c.kind.label()),
                );
                self.changed.push(started_root.join(&c.path));
            }
            if r.changes.len() > 20 {
                self.note(
                    Kind::Changed,
                    format!("… {} more (/changes)", r.changes.len() - 20),
                );
                for c in &r.changes[20..] {
                    self.changed.push(started_root.join(&c.path));
                }
            }
            if !r.changes.is_empty() {
                self.note(
                    Kind::Info,
                    "/review shows the diffs · /revert <file>|all undoes · open buffers without unsaved edits were reloaded",
                );
            }
        }
        self.attention = Some(format!(
            "AI agent {}",
            if cancelled {
                "stopped"
            } else if ok {
                "finished"
            } else {
                "finished with problems"
            }
        ));
        self.review = review;
    }

    fn list_changes(&mut self) {
        let lines: Vec<String> = match &self.review {
            Some(r) if !r.changes.is_empty() => r
                .changes
                .iter()
                .map(|c| {
                    format!(
                        "{} ({}){}",
                        c.path.display(),
                        c.kind.label(),
                        if c.revertible {
                            ""
                        } else {
                            " — not revertible"
                        }
                    )
                })
                .collect(),
            _ => return self.note(Kind::Info, "No agent changes to review"),
        };
        for l in lines {
            self.note(Kind::Changed, l);
        }
    }

    fn review_changes(&mut self, arg: &str) {
        let Some(r) = &self.review else {
            return self.note(Kind::Info, "No agent changes to review");
        };
        let selected: Vec<&Change> = r
            .changes
            .iter()
            .filter(|c| arg.is_empty() || c.path == Path::new(arg))
            .collect();
        if selected.is_empty() {
            return self.note(
                Kind::Error,
                format!("No change to {arg} (/changes lists them)"),
            );
        }
        let mut diffs = Vec::new();
        let mut lines = 0;
        for c in selected {
            let d = r.snapshot.diff(c);
            lines += d.lines().count();
            diffs.push(d);
            if lines > 3000 {
                diffs.push("… (more diffs: /review <file>)".into());
                break;
            }
        }
        for d in diffs {
            self.note(Kind::Diff, d);
        }
    }

    fn revert_changes(&mut self, arg: &str) {
        if self.run.is_some() {
            return self.note(
                Kind::Error,
                "Wait for the agent to finish (or /cancel) before reverting",
            );
        }
        let Some(r) = &mut self.review else {
            return self.note(Kind::Info, "No agent changes to revert");
        };
        if arg.is_empty() {
            return self.note(Kind::Error, "/revert <file> or /revert all");
        }
        let root = r.snapshot.root().to_path_buf();
        let (targets, keep): (Vec<Change>, Vec<Change>) = r
            .changes
            .drain(..)
            .partition(|c| arg == "all" || c.path == Path::new(arg));
        r.changes = keep;
        if targets.is_empty() {
            return self.note(
                Kind::Error,
                format!("No change to {arg} (/changes lists them)"),
            );
        }
        let mut notes = Vec::new();
        for c in targets {
            match r.snapshot.revert(&c) {
                Ok(()) => {
                    notes.push((
                        Kind::Info,
                        format!(
                            "Reverted {} ({})",
                            c.path.display(),
                            match c.kind {
                                ChangeKind::Added => "removed",
                                _ => "restored",
                            }
                        ),
                    ));
                    self.changed.push(root.join(&c.path));
                }
                Err(e) => {
                    notes.push((Kind::Error, e));
                    r.changes.push(c);
                }
            }
        }
        r.changes.sort_by(|a, b| a.path.cmp(&b.path));
        for (k, n) in notes {
            self.note(k, n);
        }
    }

    fn list_skills(&mut self, root: &Path) {
        let cat = skills::discover(Some(root), &self.cfg.skills);
        let auto: Vec<&str> = if self.cfg.skills.auto {
            skills::detect(root)
                .into_iter()
                .filter(|n| {
                    !self.cfg.skills.exclude.iter().any(|e| e == n)
                        && !self.skills_off.iter().any(|o| o == n)
                })
                .collect()
        } else {
            Vec::new()
        };
        if !self.cfg.skills.enabled {
            return self.note(
                Kind::Info,
                "Skills are disabled (ai.skills.enabled = false)",
            );
        }
        let mut lines = Vec::new();
        for s in &cat.skills {
            let automatic = auto.contains(&s.name.as_str());
            let active = automatic || self.skills.iter().any(|a| a.name == s.name);
            lines.push((
                Kind::Info,
                format!(
                    "{} {} ({}{}) — {}",
                    if active { "●" } else { "○" },
                    s.name,
                    s.scope,
                    if automatic { ", auto" } else { "" },
                    s.description
                ),
            ));
            for w in &s.warnings {
                lines.push((Kind::Meta, format!("    {}: {w}", s.name)));
            }
        }
        for s in &cat.shadowed {
            lines.push((
                Kind::Meta,
                format!(
                    "  {} ({}) is shadowed by a skill with the same name",
                    s.name, s.scope
                ),
            ));
        }
        for b in &cat.broken {
            lines.push((Kind::Error, format!("{}: {}", b.path.display(), b.error)));
        }
        if cat.skills.is_empty() {
            lines.push((
                Kind::Info,
                "No skills found. Create <dir>/<name>/SKILL.md in:".into(),
            ));
        } else {
            lines.push((
                Kind::Info,
                "● = given to agent tasks (auto = chosen for this project). /skill <name> adds one, /skill off <name> removes one. Searched, first wins:".into(),
            ));
        }
        for (scope, d) in &cat.searched {
            lines.push((Kind::Meta, format!("  {scope:<8} {}", d.display())));
        }
        for (k, l) in lines {
            self.note(k, l);
        }
    }

    fn skill_command(&mut self, arg: &str, root: &Path) {
        let (sub, name) = arg
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((arg, ""));
        match (sub, name) {
            ("", _) => self.note(Kind::Error, "/skill <name> activates a skill (/skills lists them); /skill off <name>; /skill clear"),
            ("clear", _) => {
                self.skills.clear();
                self.note(Kind::Info, "No skills active");
            }
            ("off", n) if !n.is_empty() => {
                let before = self.skills.len();
                self.skills.retain(|s| s.name != n);
                let automatic = self.cfg.skills.auto && skills::detect(root).contains(&n);
                if automatic && !self.skills_off.iter().any(|o| o == n) {
                    self.skills_off.push(n.to_string());
                    self.note(Kind::Info, format!("Skill {n} off: no longer given automatically in this IDE session (ai.skills.exclude makes it permanent; a running CLI session keeps what it already received, /new starts without it)"));
                } else if self.skills.len() == before {
                    self.note(Kind::Error, format!("Skill {n} is not active"));
                } else {
                    self.note(Kind::Info, format!("Skill {n} off"));
                }
            }
            (n, _) => {
                if let Some(i) = self.skills_off.iter().position(|o| o == n) {
                    self.skills_off.remove(i);
                    return self.note(Kind::Info, format!("Skill {n} is given automatically again"));
                }
                if self.skills.iter().any(|s| s.name == n) {
                    return self.note(Kind::Info, format!("Skill {n} is already active"));
                }
                let cat = skills::discover(Some(root), &self.cfg.skills);
                match cat.get(n).and_then(|info| {
                    skills::load(info, self.cfg.skills.max_bytes)
                        .map(|t| (t, info.warnings.clone()))
                }) {
                    Ok((text, warnings)) => {
                        let note = if text.trusted {
                            format!("Skill {n} active for agent tasks ({}, {} bytes)", text.source, text.body.len())
                        } else {
                            format!(
                                "Skill {n} active for agent tasks ({}, {} bytes). It is guidance only: it cannot change the policy or permissions.",
                                text.source,
                                text.body.len()
                            )
                        };
                        self.note(Kind::Info, note);
                        for w in warnings {
                            self.note(Kind::Meta, format!("  {w}"));
                        }
                        self.skills.push(text);
                    }
                    Err(e) => self.note(Kind::Error, e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_shell_wrappers_are_hidden_in_the_transcript() {
        assert_eq!(
            display_command("/usr/bin/bash -lc 'gcc -o app main.c && ./app'"),
            "gcc -o app main.c && ./app"
        );
        assert_eq!(display_command("make -j4"), "make -j4");
        assert_eq!(relative(Path::new("/w"), "/w/src/a.c"), "src/a.c");
        assert_eq!(relative(Path::new("/w"), "/etc/hosts"), "/etc/hosts");
    }
}
