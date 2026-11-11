//! Agent runtime against mock Claude Code / Codex CLIs (nothing is contacted).
#![cfg(unix)]
use embedide_ai::{
    agent::{self, Approval, ApprovalKind, Decision, Event, Host, Policy, Session, Task},
    cli::CliKind,
    Cancel, Error,
};
use embedide_workspace::config::AiConfig;
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A mock CLI plus a disposable workspace with `main.c`.
struct Fixture {
    dir: PathBuf,
    program: PathBuf,
    ws: PathBuf,
}

impl Fixture {
    fn new(kind: &str, scenario: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "embed agent mock ж {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let ws = dir.join("work space");
        std::fs::create_dir_all(ws.join("build")).unwrap();
        std::fs::write(
            ws.join("main.c"),
            "int add(int a, int b);\nint main(void) { return add(2, 3); }\n",
        )
        .unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_agent.py");
        let program = dir.join(kind);
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nexec python3 '{}' {kind} {scenario} '{}' \"$@\"\n",
                script.display(),
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, program, ws }
    }
    fn session(&self, kind: CliKind, policy: Policy) -> Session {
        Session::new(
            kind,
            self.program.clone(),
            &self.ws,
            policy,
            &AiConfig::default(),
            None,
        )
    }
    fn runs(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.dir.join("runs"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    fn decisions(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("decisions"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Records events; answers approvals from a script (then denies).
#[derive(Default)]
struct Recorder {
    events: Vec<Event>,
    asked: Vec<Approval>,
    answers: Vec<Decision>,
}

impl Host for Recorder {
    fn event(&mut self, event: Event) {
        self.events.push(event);
    }
    fn approve(&mut self, request: &Approval, _: &Cancel) -> Decision {
        self.asked.push(request.clone());
        if self.answers.is_empty() {
            Decision::Deny("no".into())
        } else {
            self.answers.remove(0)
        }
    }
}

fn task(prompt: &str) -> Task {
    Task {
        prompt: prompt.into(),
        skills: Vec::new(),
    }
}

#[test]
fn claude_agent_edits_builds_and_asks_before_destructive_and_outside_actions() {
    let f = Fixture::new("claude", "edit");
    let mut s = f.session(CliKind::ClaudeCode, Policy::WorkspaceWrite);
    let mut host = Recorder::default();
    let out = s
        .run(&task("implement add"), &mut host, &Cancel::default())
        .unwrap();
    assert!(out.ok, "{:?}", out.error);
    assert_eq!(out.summary, "Created add.c; built and ran app (5).");
    // Write inside the workspace and the build ran without asking; reading
    // /etc/hostname and `rm -rf build` asked (and were denied by the host).
    let asked: Vec<&str> = host.asked.iter().map(|a| a.summary.as_str()).collect();
    assert_eq!(asked, ["Read /etc/hostname", "$ rm -rf build"], "{asked:?}");
    assert_eq!(host.asked[1].kind, ApprovalKind::Command);
    assert!(host.asked[1].reason.contains("deletes"));
    assert_eq!(
        f.decisions(),
        [
            "Write allow",
            "Read deny",
            "Bash allow",
            "Bash allow",
            "Bash deny"
        ]
    );
    assert_eq!(out.approvals, 2);
    assert_eq!(out.denied, 2);
    assert_eq!(out.commands, 3);
    assert_eq!(out.failed_commands, 1);
    assert!(host.events.contains(&Event::CommandDone {
        id: "toolu_4".into(),
        command: "make broken".into(),
        exit: Some(2),
        output: "make: *** No rule to make target 'broken'".into(),
    }));
    assert!(host
        .events
        .iter()
        .any(|e| matches!(e, Event::FileChanged { path, .. } if path.ends_with("add.c"))));
    assert!(host.events.contains(&Event::Text("built.".into())));
    assert_eq!(out.usage.input_tokens, Some(100));
    // The snapshot sees the new file and can revert it.
    let names: Vec<String> = out
        .changes
        .iter()
        .map(|c| c.path.display().to_string())
        .collect();
    assert_eq!(names, ["add.c"]);
    let snap = out.snapshot.clone().unwrap();
    assert!(snap.diff(&out.changes[0]).contains("+int add"));
    snap.revert(&out.changes[0]).unwrap();
    assert!(!f.ws.join("add.c").exists());

    // The host protocol, a fresh session id, the workspace as cwd
    let runs = f.runs();
    let a = &runs[0];
    for flag in [
        "--input-format",
        "--permission-prompt-tool",
        "--permission-mode",
        "--session-id",
    ] {
        assert!(a.iter().any(|x| x == flag), "{flag} missing: {a:?}");
    }
    assert_eq!(
        a[a.iter().position(|x| x == "--permission-mode").unwrap() + 1],
        "manual"
    );
    assert!(!a
        .iter()
        .any(|x| x == "--dangerously-skip-permissions" || x == "bypassPermissions"));
    let id = a[a.iter().position(|x| x == "--session-id").unwrap() + 1].clone();
    assert_eq!(s.id(), Some(id.as_str()));
    let env: Value =
        serde_json::from_str(&std::fs::read_to_string(f.dir.join("env")).unwrap()).unwrap();
    assert!(env.get("ANTHROPIC_API_KEY").is_none());
    assert!(env.get("CLAUDECODE").is_none());

    // The next turn continues the same session.
    let mut host = Recorder {
        answers: vec![Decision::Allow, Decision::AllowSimilar],
        ..Default::default()
    };
    let out = s
        .run(&task("again"), &mut host, &Cancel::default())
        .unwrap();
    assert!(out.ok);
    let b = &f.runs()[1];
    assert_eq!(b[b.iter().position(|x| x == "--resume").unwrap() + 1], id);
    assert_eq!(
        f.decisions()[9],
        "Bash allow",
        "the user allowed rm this time"
    );
}

#[test]
fn claude_approval_required_asks_for_every_change_and_command() {
    let f = Fixture::new("claude", "edit");
    let mut s = f.session(CliKind::ClaudeCode, Policy::ApprovalRequired);
    let mut host = Recorder {
        answers: vec![Decision::Allow, Decision::Deny(String::new())],
        ..Default::default()
    };
    let out = s.run(&task("x"), &mut host, &Cancel::default()).unwrap();
    assert!(out.ok);
    assert_eq!(host.asked.len(), 5, "{:?}", host.asked);
    assert_eq!(host.asked[0].kind, ApprovalKind::Edit);
    assert!(
        f.ws.join("add.c").exists(),
        "the first answer allowed the write"
    );
}

#[test]
fn claude_read_only_limits_tools_and_refuses_changes_without_asking() {
    let f = Fixture::new("claude", "edit");
    let mut s = f.session(CliKind::ClaudeCode, Policy::ReadOnly);
    let mut host = Recorder::default();
    let out = s.run(&task("look"), &mut host, &Cancel::default()).unwrap();
    assert!(out.ok);
    let a = &f.runs()[0];
    assert_eq!(
        a[a.iter().position(|x| x == "--tools").unwrap() + 1],
        "Read,Glob,Grep"
    );
    assert!(!f.ws.join("add.c").exists());
    assert!(out.changes.is_empty());
    // Only reading outside the workspace is a question; changes are refused.
    assert_eq!(host.asked.len(), 1);
    assert!(f.decisions().iter().all(|d| d.ends_with("deny")));
}

#[test]
fn claude_using_an_api_key_is_stopped_before_anything_is_sent() {
    let f = Fixture::new("claude", "apikey");
    let mut s = f.session(CliKind::ClaudeCode, Policy::WorkspaceWrite);
    let started = Instant::now();
    let out = s
        .run(&task("x"), &mut Recorder::default(), &Cancel::default())
        .unwrap();
    assert!(!out.ok);
    match out.error {
        Some(Error::Config(m)) => assert!(m.contains("API key") && m.contains("billed"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(f.decisions().is_empty(), "no tool ran");
}

fn assert_gone(f: &Fixture) {
    let pid: i32 = std::fs::read_to_string(f.dir.join("grandchild"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // kill(pid, 0) fails once the process is gone (and reaped by init)
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.split_whitespace().nth(2) == Some("Z"))
            .unwrap_or(false);
        if !alive || zombie {
            return;
        }
        assert!(Instant::now() < deadline, "grandchild {pid} still running");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn cancel_stops_the_cli_and_its_children() {
    for kind in ["claude", "codex"] {
        let f = Fixture::new(kind, "hang");
        let cli = if kind == "claude" {
            CliKind::ClaudeCode
        } else {
            CliKind::Codex
        };
        let mut s = f.session(cli, Policy::WorkspaceWrite);
        let cancel = Cancel::default();
        let c2 = cancel.clone();
        let grandchild = f.dir.join("grandchild");
        let t = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !grandchild.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            c2.cancel();
        });
        let started = Instant::now();
        let out = s
            .run(&task("x"), &mut Recorder::default(), &cancel)
            .unwrap();
        t.join().unwrap();
        assert_eq!(out.error, Some(Error::Cancelled), "{kind}");
        assert!(started.elapsed() < Duration::from_secs(25), "{kind}");
        assert_gone(&f);
    }
}

#[test]
fn codex_agent_uses_the_workspace_sandbox_and_resumes_the_thread() {
    let f = Fixture::new("codex", "edit");
    let mut s = f.session(CliKind::Codex, Policy::WorkspaceWrite);
    let mut host = Recorder::default();
    let out = s
        .run(&task("implement add"), &mut host, &Cancel::default())
        .unwrap();
    assert!(out.ok, "{:?}", out.error);
    assert_eq!(out.summary, "Done: add.c created, app prints 5.");
    assert_eq!(out.commands, 1);
    assert!(host.asked.is_empty(), "codex exec has no approval channel");
    assert!(host.events.contains(&Event::Thinking));
    assert!(host
        .events
        .iter()
        .any(|e| matches!(e, Event::Note(n) if n.contains("[x] write add.c"))));
    assert!(host.events.iter().any(|e| matches!(e,
        Event::CommandDone { exit: Some(0), output, .. } if output == "5\n")));
    assert_eq!(out.changes.len(), 1);
    let a = &f.runs()[0];
    let joined = a.join(" ");
    assert!(
        joined.starts_with("exec --sandbox workspace-write"),
        "{joined}"
    );
    assert!(joined.contains(&format!("-C {}", f.ws.canonicalize().unwrap().display())));
    assert!(joined.contains("approval_policy=\"never\""));
    assert!(!joined.contains("dangerously"));
    assert_eq!(s.id(), Some("thread-1"));
    let env: Value =
        serde_json::from_str(&std::fs::read_to_string(f.dir.join("env")).unwrap()).unwrap();
    assert!(env.get("OPENAI_API_KEY").is_none() && env.get("CODEX_API_KEY").is_none());

    s.run(
        &task("now tests"),
        &mut Recorder::default(),
        &Cancel::default(),
    )
    .unwrap();
    let b = f.runs()[1].join(" ");
    assert!(
        b.starts_with("exec resume thread-1 -c sandbox_mode=\"workspace-write\""),
        "{b}"
    );
}

#[test]
fn codex_approval_required_plans_read_only_then_waits_for_approval() {
    // Denied: nothing changes.
    let f = Fixture::new("codex", "plan");
    let mut s = f.session(CliKind::Codex, Policy::ApprovalRequired);
    let mut host = Recorder::default();
    let out = s
        .run(&task("implement add"), &mut host, &Cancel::default())
        .unwrap();
    assert_eq!(host.asked.len(), 1);
    assert_eq!(host.asked[0].kind, ApprovalKind::Plan);
    assert!(host.asked[0].summary.contains("1. Create add.c"));
    assert!(out.changes.is_empty());
    assert_eq!(f.runs().len(), 1);
    assert!(f.runs()[0].join(" ").contains("--sandbox read-only"));
    // Approved: the same thread carries it out with workspace-write.
    let f = Fixture::new("codex", "plan");
    let mut s = f.session(CliKind::Codex, Policy::ApprovalRequired);
    let mut host = Recorder {
        answers: vec![Decision::Allow],
        ..Default::default()
    };
    let out = s
        .run(&task("implement add"), &mut host, &Cancel::default())
        .unwrap();
    assert!(out.ok, "{:?}", out.error);
    assert_eq!(out.changes.len(), 1);
    let runs = f.runs();
    assert_eq!(runs.len(), 2);
    assert!(runs[1]
        .join(" ")
        .starts_with("exec resume thread-1 -c sandbox_mode=\"workspace-write\""));
    let prompts = std::fs::read_to_string(f.dir.join("prompts")).unwrap();
    assert!(prompts.contains("PLAN ONLY"));
    assert!(prompts.contains("approved your plan"));
}

#[test]
fn read_only_codex_runs_in_the_read_only_sandbox() {
    let f = Fixture::new("codex", "edit");
    let mut s = f.session(CliKind::Codex, Policy::ReadOnly);
    let out = s
        .run(&task("look"), &mut Recorder::default(), &Cancel::default())
        .unwrap();
    assert!(out.ok);
    assert!(f.runs()[0].join(" ").contains("--sandbox read-only"));
    assert!(out.changes.is_empty());
}

#[test]
fn fix_loop_builds_sends_errors_and_rebuilds_until_it_passes() {
    for (kind, cli) in [("claude", CliKind::ClaudeCode), ("codex", CliKind::Codex)] {
        let f = Fixture::new(kind, "fix");
        std::fs::write(
            f.ws.join("check.py"),
            "import os, sys\nif not os.path.exists('fixed'):\n    print('main.c:3:5: error: expected ;')\n    sys.exit(1)\nprint('ok')\n",
        )
        .unwrap();
        let mut s = f.session(cli, Policy::WorkspaceWrite);
        let mut host = Recorder::default();
        let argv = agent::check_argv("python3 check.py").unwrap();
        let (check, outcomes) =
            agent::fix(&mut s, &argv, &task(""), 3, &mut host, &Cancel::default()).unwrap();
        assert!(check.ok, "{kind}: {check:?}");
        assert_eq!(outcomes.len(), 1, "{kind}");
        let checks: Vec<bool> = host
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Check { ok, .. } => Some(*ok),
                _ => None,
            })
            .collect();
        assert_eq!(checks, [false, true], "{kind}");
        let prompts = std::fs::read_to_string(f.dir.join("prompts")).unwrap();
        assert!(
            prompts.contains("expected ;"),
            "{kind}: the errors reach the agent"
        );
    }
}
