//! Claude Code / Codex CLI backends against a mock CLI (nothing is contacted).
#![cfg(unix)]
use embedide_ai::{
    backend,
    cli::{self, AuthState, CliKind},
    AuthMode, Cancel, Error, Message, ProviderId, Request,
};
use embedide_workspace::config::{AiConfig, CliConfig, ProviderConfig};
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Mock {
    dir: PathBuf,
    program: PathBuf,
}
impl Mock {
    /// A `claude`/`codex` executable that runs the Python mock in `mode`.
    fn new(kind: &str, mode: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "embed cli mock ж {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_cli.py");
        let program = dir.join(if kind == "codex" { "codex" } else { "claude" });
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nexec python3 '{}' {kind} {mode} '{}' \"$@\"\n",
                script.display(),
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, program }
    }
    fn json(&self, name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.dir.join(name)).unwrap()).unwrap()
    }
    fn config(&self, provider: &str, cli: CliConfig) -> AiConfig {
        let mut cfg = AiConfig {
            cli,
            ..AiConfig::default()
        };
        cfg.providers.insert(
            provider.into(),
            ProviderConfig {
                cli: Some(self.program.clone()),
                ..Default::default()
            },
        );
        cfg
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn request(text: &str) -> Request {
    Request {
        model: "default".into(),
        system: Some("system".into()),
        messages: vec![Message::user(text)],
        max_tokens: 100,
        temperature: None,
        stream: true,
    }
}

fn send(
    mock: &Mock,
    provider: ProviderId,
    cli: CliConfig,
    text: &str,
) -> (embedide_ai::Result<embedide_ai::Response>, String) {
    let cfg = mock.config(provider.id(), cli);
    let backend = backend(provider, AuthMode::Subscription, &cfg).unwrap();
    let mut streamed = String::new();
    let result = backend.send(&request(text), &Cancel::default(), &mut |d| {
        streamed.push_str(d)
    });
    (result, streamed)
}

#[test]
fn claude_code_streams_structured_output_without_tools_or_keys() {
    let mock = Mock::new("claude", "ok");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-should-not-be-passed");
    std::env::set_var("CLAUDE_CODE_SESSION_ID", "parent-session");
    let (result, streamed) = send(&mock, ProviderId::Anthropic, CliConfig::default(), "MARK-1");
    let response = result.unwrap();
    assert_eq!(
        response.text, "Привет red 猫 answer for MARK-1",
        "ANSI removed, Unicode kept"
    );
    assert_eq!(streamed, response.text, "streamed in pieces");
    assert_eq!(
        (response.usage.input_tokens, response.usage.output_tokens),
        (Some(11), Some(7))
    );
    assert_eq!(response.model.as_deref(), Some("mock-model"));
    let args: Vec<String> = serde_json::from_value(mock.json("args")).unwrap();
    assert_eq!(
        args,
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            "",
            "--no-session-persistence"
        ]
    );
    let env = mock.json("env");
    assert!(
        env.get("ANTHROPIC_API_KEY").is_none(),
        "no silent API-key billing"
    );
    assert!(
        env.get("CLAUDE_CODE_SESSION_ID").is_none(),
        "not bound to a parent session"
    );
    let cwd = std::fs::read_to_string(mock.dir.join("cwd")).unwrap();
    assert!(
        cwd.ends_with("\n0") && !cwd.starts_with(&mock.dir.display().to_string()),
        "empty private dir: {cwd}"
    );
    assert!(std::fs::read_to_string(mock.dir.join("stdin"))
        .unwrap()
        .contains("system\n\nUser: MARK-1"));
    // An explicit opt-in passes the key through.
    let mock2 = Mock::new("claude", "ok");
    let cli = CliConfig {
        allow_api_key: true,
        ..CliConfig::default()
    };
    send(&mock2, ProviderId::Anthropic, cli, "MARK-2")
        .0
        .unwrap();
    assert!(mock2.json("env").get("ANTHROPIC_API_KEY").is_some());
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("CLAUDE_CODE_SESSION_ID");
}

#[test]
fn codex_uses_json_events_a_read_only_sandbox_and_an_empty_directory() {
    let mock = Mock::new("codex", "ok");
    let (result, streamed) = send(&mock, ProviderId::Openai, CliConfig::default(), "MARK-C");
    let response = result.unwrap();
    assert_eq!(response.text, "codex answer for MARK-C ж");
    assert_eq!(streamed, response.text);
    assert_eq!(response.usage.output_tokens, Some(3));
    let args: Vec<String> = serde_json::from_value(mock.json("args")).unwrap();
    assert_eq!(
        &args[..5],
        [
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--json"
        ]
    );
    assert!(args.contains(&"--ephemeral".to_string()) && args.last().unwrap() == "-");
    let work = &args[args.iter().position(|a| a == "-C").unwrap() + 1];
    assert!(
        !Path::new(work).exists(),
        "the private directory is removed afterwards"
    );
    let prompt = std::fs::read_to_string(mock.dir.join("stdin")).unwrap();
    assert!(prompt.contains("Do not run commands and do not modify files"));
}

#[test]
fn older_clis_without_structured_output_fall_back_to_text() {
    let mock = Mock::new("old-claude", "ok");
    let (result, streamed) = send(&mock, ProviderId::Anthropic, CliConfig::default(), "OLD");
    assert_eq!(result.unwrap().text, "plain answer for OLD");
    assert_eq!(streamed, "plain answer for OLD");
    let args: Vec<String> = serde_json::from_value(mock.json("args")).unwrap();
    assert_eq!(args, ["-p", "--output-format", "text"]);
}

#[test]
fn login_rate_limit_and_crash_errors_are_classified_with_hints() {
    for (kind, provider) in [
        ("claude", ProviderId::Anthropic),
        ("codex", ProviderId::Openai),
    ] {
        let mock = Mock::new(kind, "auth");
        match send(&mock, provider, CliConfig::default(), "x").0 {
            Err(Error::Auth(m)) => assert!(m.contains("log"), "{m}"),
            other => panic!("{kind}: {other:?}"),
        }
        let mock = Mock::new(kind, "limit");
        assert!(
            matches!(
                send(&mock, provider, CliConfig::default(), "x").0,
                Err(Error::RateLimited(..))
            ),
            "{kind}"
        );
        let mock = Mock::new(kind, "crash");
        match send(&mock, provider, CliConfig::default(), "x").0 {
            Err(Error::Provider(3, m)) => assert!(m.contains("mock crashed"), "{m}"),
            other => panic!("{kind}: {other:?}"),
        }
    }
}

#[test]
fn a_cli_that_exits_without_reading_its_prompt_is_a_clean_error() {
    let mock = Mock::new("claude", "noread");
    let big = "y".repeat(4 * 1024 * 1024); // more than a pipe buffer: broken pipe
    let (result, _) = send(&mock, ProviderId::Anthropic, CliConfig::default(), &big);
    assert!(matches!(result, Err(Error::Provider(..))), "{result:?}");
}

#[test]
fn oversized_output_is_stopped_at_the_configured_limit() {
    let mock = Mock::new("claude", "huge");
    let cli = CliConfig {
        max_output_bytes: 1024 * 1024,
        ..CliConfig::default()
    };
    let started = Instant::now();
    match send(&mock, ProviderId::Anthropic, cli, "x").0 {
        Err(Error::Provider(_, m)) => assert!(m.contains("max_output_bytes"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(20));
}

fn alive(dir: &Path) -> bool {
    // Anchor the end: mock `…-1` must not match `…-10` of a concurrent test.
    let out = std::process::Command::new("pgrep")
        .args(["-f", &format!("{}( |/|$)", dir.display())])
        .output()
        .unwrap();
    !out.stdout.is_empty()
}

#[test]
fn cancel_and_timeout_stop_the_whole_process_group() {
    let mock = Mock::new("claude", "child");
    let cfg = mock.config("anthropic", CliConfig::default());
    let backend = backend(ProviderId::Anthropic, AuthMode::Subscription, &cfg).unwrap();
    let cancel = Cancel::default();
    let c2 = cancel.clone();
    let worker = std::thread::spawn(move || backend.send(&request("x"), &c2, &mut |_| {}));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !mock.dir.join("stdin").exists() {
        assert!(Instant::now() < deadline, "mock never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    let grandchild = || {
        !std::process::Command::new("pgrep")
            .args(["-f", "^sleep 3001$"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    };
    assert!(grandchild(), "the mock started its grandchild");
    cancel.cancel();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!alive(&mock.dir), "the CLI survived cancellation");
    assert!(
        !grandchild(),
        "a process the CLI started survived cancellation"
    );
    let mock = Mock::new("codex", "hang");
    let cli = CliConfig {
        timeout_secs: 1,
        ..CliConfig::default()
    };
    match send(&mock, ProviderId::Openai, cli, "x").0 {
        Err(Error::Network(m)) => assert!(m.contains("timeout_secs"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(!alive(&mock.dir));
}

#[test]
fn concurrent_requests_are_isolated() {
    let mocks: Vec<Mock> = (0..4).map(|_| Mock::new("claude", "ok")).collect();
    let answers: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = mocks
            .iter()
            .enumerate()
            .map(|(i, m)| {
                s.spawn(move || {
                    send(
                        m,
                        ProviderId::Anthropic,
                        CliConfig::default(),
                        &format!("REQ-{i}"),
                    )
                    .0
                    .unwrap()
                    .text
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, answer) in answers.iter().enumerate() {
        assert!(answer.ends_with(&format!("answer for REQ-{i}")), "{answer}");
    }
}

#[test]
fn discovery_versions_and_login_state_never_show_personal_data() {
    let mock = Mock::new("claude", "ok");
    let info = cli::inspect(CliKind::ClaudeCode, &mock.program, &CliConfig::default());
    assert_eq!(info.version.as_deref(), Some("2.1.322"));
    assert!(info.caps.structured && info.caps.no_tools && info.supported());
    assert_eq!(
        info.auth,
        AuthState::LoggedIn {
            method: "claude.ai".into(),
            plan: Some("max".into())
        }
    );
    let summary = info.summary();
    assert!(
        summary.contains("ready, logged in via claude.ai (max)"),
        "{summary}"
    );
    assert!(!summary.contains("example.com") && !summary.contains("Secret Org"));
    let mock = Mock::new("codex", "loggedout");
    let info = cli::inspect(CliKind::Codex, &mock.program, &CliConfig::default());
    assert_eq!(info.version.as_deref(), Some("0.163.0"));
    assert_eq!(info.auth, AuthState::LoggedOut);
    assert!(info.summary().contains("codex login"));
    let mock = Mock::new("codex", "ok");
    let info = cli::inspect(CliKind::Codex, &mock.program, &CliConfig::default());
    assert_eq!(
        info.auth,
        AuthState::LoggedIn {
            method: "ChatGPT".into(),
            plan: None
        }
    );
    // Missing executables name the setting.
    let missing = cli::locate(CliKind::Codex, Some(Path::new("/nonexistent/codex")))
        .unwrap_err()
        .to_string();
    assert!(missing.contains("does not exist"), "{missing}");
    assert_eq!(cli::parse_version("codex-cli 0.163.0"), Some((0, 163, 0)));
    assert_eq!(
        cli::parse_version("2.1.322 (Claude Code)"),
        Some((2, 1, 322))
    );
    assert_eq!(cli::parse_version("no version"), None);
    assert_eq!(
        cli::parse_auth(
            CliKind::Codex,
            true,
            "Logged in using an API key - sk-proj-abcdef1234567890"
        ),
        AuthState::LoggedIn {
            method: "API key".into(),
            plan: None
        }
    );
    assert_eq!(
        cli::redact("token sk-ant-api03-SECRET here"),
        "token [redacted] here"
    );
    assert_eq!(cli::sanitize("a\x1b]0;title\x07b\x1b[2Jc\x07"), "abc");
}

#[test]
fn antigravity_cli_streams_lists_models_and_checks_auth() {
    let mock = Mock::new("agy", "ok");
    let info = cli::inspect(CliKind::Antigravity, &mock.program, &CliConfig::default());
    assert_eq!(info.version.as_deref(), Some("1.5.14"));
    assert!(info.caps.headless && info.caps.structured && info.supported());
    assert_eq!(
        info.auth,
        AuthState::LoggedIn {
            method: "Antigravity CLI".into(),
            plan: None
        }
    );

    let logged_out = Mock::new("agy", "loggedout");
    let info_out = cli::inspect(
        CliKind::Antigravity,
        &logged_out.program,
        &CliConfig::default(),
    );
    assert_eq!(info_out.auth, AuthState::LoggedOut);

    let (res, streamed) = send(
        &mock,
        ProviderId::Antigravity,
        CliConfig::default(),
        "AGY-1",
    );
    let resp = res.unwrap();
    assert_eq!(resp.text, "agy answer for AGY-1");
    assert_eq!(streamed, "agy answer for AGY-1");
    assert_eq!(resp.model.as_deref(), Some("gemini-4-neon-high"));
    assert_eq!(resp.usage.input_tokens, Some(19));
    assert_eq!(resp.usage.output_tokens, Some(6));

    // Model discovery through `agy models`.
    let mut cfg = AiConfig::default();
    cfg.providers.insert(
        "antigravity".into(),
        ProviderConfig {
            cli: Some(mock.program.clone()),
            ..Default::default()
        },
    );
    let b = backend(ProviderId::Antigravity, AuthMode::Subscription, &cfg).unwrap();
    let models = b.models().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "gemini-4-neon-high");
    assert_eq!(models[1].id, "claude-sonnet-5-5-medium");

    // ApiKey mode is refused with a helpful hint to use subscription/`agy`.
    let err = backend(ProviderId::Antigravity, AuthMode::ApiKey, &cfg)
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("agy"), "{err}");
}
