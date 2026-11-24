//! `embed ai …` end to end, against a local mock HTTP server and a fake
//! subscription CLI. No real provider is contacted; nothing can be billed.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

const KEY: &str = "mock-key-0123456789-SECRET";

type Log = Arc<Mutex<Vec<(String, BTreeMap<String, String>, Value)>>>;

fn serve(bodies: Vec<String>) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let l2 = log.clone();
    thread::spawn(move || {
        for body in bodies {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut first = String::new();
            r.read_line(&mut first).unwrap();
            let mut hs = BTreeMap::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap();
                hs.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
            let n: usize = hs
                .get("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut b = vec![0; n];
            r.read_exact(&mut b).unwrap();
            l2.lock().unwrap().push((
                first.split_whitespace().nth(1).unwrap_or("").to_string(),
                hs,
                serde_json::from_slice(&b).unwrap_or(Value::Null),
            ));
            let ct = if body.starts_with("data:") {
                "text/event-stream"
            } else {
                "application/json"
            };
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
        }
    });
    (base, log)
}

struct T {
    dir: PathBuf,
}
impl T {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("embedide ai {} {name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        Self { dir }
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_embed"));
        c.args(args)
            .current_dir(&self.dir)
            .env("EMBEDIDE_CONFIG_DIR", self.dir.join("cfg"));
        for k in [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "MOONSHOT_API_KEY",
            "XAI_API_KEY",
            "EMBEDIDE_AI_OFFLINE",
        ] {
            c.env_remove(k);
        }
        c
    }
    fn run(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &str,
    ) -> (Value, i32, String, String) {
        let mut c = self.cmd(args);
        c.envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let v = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
        (
            v,
            out.status.code().unwrap_or(-1),
            stdout,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn set(&self, key: &str, value: &str) {
        let (v, code, _, e) = self.run(&["--json", "config", "set", key, value], &[], "");
        assert_eq!(code, 0, "{v} {e}");
    }
}
impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn providers_lists_four_families_and_never_prints_keys() {
    let t = T::new("prov");
    let (v, code, out, _) = t.run(
        &["--json", "ai", "providers"],
        &[("OPENAI_API_KEY", KEY)],
        "",
    );
    assert_eq!(code, 0);
    let ids: Vec<_> = v["data"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ids,
        ["anthropic", "openai", "moonshot", "xai", "antigravity"]
    );
    let openai = &v["data"]["providers"][1];
    assert_eq!(openai["key"], "environment");
    assert_eq!(openai["auth_modes"]["subscription"], "codex");
    assert_eq!(
        v["data"]["providers"][0]["auth_modes"]["subscription"],
        "claude-code"
    );
    assert_eq!(
        v["data"]["providers"][4]["auth_modes"]["subscription"],
        "antigravity"
    );
    assert!(v["data"]["providers"][2]["auth_modes"]["subscription"]["not-integrated"].is_string());
    assert!(!out.contains(KEY));
    let (_, _, human, _) = t.run(&["ai", "providers"], &[("OPENAI_API_KEY", KEY)], "");
    assert!(human.contains("set (OPENAI_API_KEY)") && !human.contains(KEY));
}

#[test]
fn chat_dry_run_shows_context_and_never_connects() {
    let t = T::new("dry");
    std::fs::write(t.dir.join("main.c"), "int main(void) { return 0; }\n").unwrap();
    std::fs::write(t.dir.join(".env"), "PASSWORD=hunter2\n").unwrap();
    // base URL that nothing listens on: a connection attempt would fail the command
    t.set("ai.providers.anthropic.base_url", "http://127.0.0.1:9");
    let (v, code, _, e) = t.run(
        &[
            "--json",
            "ai",
            "chat",
            "explain",
            "--provider",
            "anthropic",
            "--model",
            "claude-x",
            "--file",
            "main.c",
            "--file",
            ".env",
            "--dry-run",
        ],
        &[],
        "",
    );
    assert_eq!(code, 0, "{v} {e}");
    assert_eq!(v["data"]["dry_run"], true);
    assert_eq!(v["data"]["context"]["files"][0]["path"], "main.c");
    assert!(v["data"]["context"]["refused"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("excluded"));
    let body = v["data"]["request"]["messages"][0]["content"]
        .as_str()
        .unwrap();
    assert!(body.contains("int main") && !body.contains("hunter2"));
}

#[test]
fn chat_streams_through_the_configured_provider() {
    let t = T::new("chat");
    let sse = [
        json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Use "}}),
        json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "volatile."}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 3}}),
    ]
    .iter()
    .map(|e| format!("data: {e}\n\n"))
    .collect::<String>();
    let (base, log) = serve(vec![sse]);
    t.set("ai.provider", "anthropic");
    t.set("ai.providers.anthropic.base_url", &base);
    t.set("ai.providers.anthropic.model", "claude-mock");
    std::fs::write(t.dir.join("reg.c"), "*(int*)0x40020000 = 1;\n").unwrap();
    let (_, code, out, err) = t.run(
        &["ai", "chat", "Is this right?", "--file", "reg.c"],
        &[("ANTHROPIC_API_KEY", KEY)],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Use volatile."), "{out}");
    assert!(
        err.contains("→ Anthropic API, model claude-mock: sending") && err.contains("reg.c"),
        "{err}"
    );
    let (path, headers, body) = log.lock().unwrap()[0].clone();
    assert_eq!(path, "/v1/messages");
    assert_eq!(headers["x-api-key"], KEY);
    assert!(body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("0x40020000"));
    assert!(!out.contains(KEY) && !err.contains(KEY));
}

#[test]
fn complete_prints_the_insertion() {
    let t = T::new("complete");
    let (base, log) = serve(vec![json!({"choices": [{"message": {"content": "return a * b;"}, "finish_reason": "stop"}], "usage": {}}).to_string()]);
    t.set("ai.providers.xai.base_url", &base);
    std::fs::write(t.dir.join("m.c"), "int mul(int a, int b) {\n    \n}\n").unwrap();
    let (v, code, _, e) = t.run(
        &[
            "--json",
            "ai",
            "complete",
            "--file",
            "m.c",
            "--line",
            "2",
            "--column",
            "5",
            "--provider",
            "xai",
            "--model",
            "grok-mock",
        ],
        &[("XAI_API_KEY", KEY)],
        "",
    );
    assert_eq!(code, 0, "{e}");
    assert_eq!(v["data"]["completion"]["insertion"], "return a * b;");
    assert_eq!(log.lock().unwrap()[0].0, "/chat/completions");
    assert_eq!(log.lock().unwrap()[0].2["model"], "grok-mock");
}

#[test]
fn edit_shows_a_diff_and_writes_only_with_apply() {
    let t = T::new("edit");
    let reply = json!({"content": [{"type": "text", "text": "int x = 2;\n"}], "stop_reason": "end_turn", "usage": {}}).to_string();
    let (base, _) = serve(vec![reply.clone(), reply]);
    t.set("ai.providers.anthropic.base_url", &base);
    std::fs::write(t.dir.join("x.c"), "int x = 1;\n").unwrap();
    let args = [
        "ai",
        "edit",
        "x.c",
        "--instruction",
        "make it 2",
        "--provider",
        "anthropic",
        "--model",
        "m",
    ];
    let (_, code, out, e) = t.run(&args, &[("ANTHROPIC_API_KEY", KEY)], "");
    assert_eq!(code, 0, "{e}");
    assert!(out.contains("-int x = 1;\n+int x = 2;"), "{out}");
    assert_eq!(
        std::fs::read_to_string(t.dir.join("x.c")).unwrap(),
        "int x = 1;\n",
        "no write without --apply"
    );
    let mut with_apply = args.to_vec();
    with_apply.extend(["--apply", "--yes"]);
    let (_, code, _, e) = t.run(&with_apply, &[("ANTHROPIC_API_KEY", KEY)], "");
    assert_eq!(code, 0, "{e}");
    assert_eq!(
        std::fs::read_to_string(t.dir.join("x.c")).unwrap(),
        "int x = 2;\n"
    );
}

#[test]
fn offline_mode_missing_model_and_missing_key_are_explained() {
    let t = T::new("guards");
    let (base, log) = serve(vec!["{}".into()]);
    t.set("ai.providers.openai.base_url", &base);
    let (v, code, _, _) = t.run(
        &[
            "--json",
            "ai",
            "chat",
            "hi",
            "--provider",
            "openai",
            "--model",
            "m",
        ],
        &[("OPENAI_API_KEY", KEY), ("EMBEDIDE_AI_OFFLINE", "1")],
        "",
    );
    assert_eq!(code, 1);
    assert!(v["error"]["message"].as_str().unwrap().contains("disabled"));
    let (v, _, _, _) = t.run(
        &["--json", "ai", "chat", "hi", "--provider", "openai"],
        &[("OPENAI_API_KEY", KEY)],
        "",
    );
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("does not guess model names"));
    let (v, _, _, _) = t.run(
        &[
            "--json",
            "ai",
            "chat",
            "hi",
            "--provider",
            "moonshot",
            "--model",
            "k",
        ],
        &[],
        "",
    );
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("MOONSHOT_API_KEY"));
    let (v, _, _, _) = t.run(
        &[
            "--json",
            "ai",
            "chat",
            "hi",
            "--provider",
            "xai",
            "--auth",
            "subscription",
        ],
        &[],
        "",
    );
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not available for xAI"));
    assert!(
        log.lock().unwrap().is_empty(),
        "nothing reached the network"
    );
}

#[test]
fn login_stores_a_private_key_and_logout_removes_it() {
    let t = T::new("login");
    let (_, code, out, err) = t.run(&["ai", "login", "moonshot"], &[], &format!("{KEY}\n"));
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains(KEY) && !err.contains(KEY));
    let file = t.dir.join("cfg/credentials.toml");
    assert!(std::fs::read_to_string(&file).unwrap().contains(KEY));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let (v, _, _, _) = t.run(&["--json", "ai", "providers"], &[], "");
    assert_eq!(v["data"]["providers"][2]["key"], "credentials-file");
    let (_, code, _, _) = t.run(&["ai", "logout", "moonshot"], &[], "");
    assert_eq!(code, 0);
    assert!(!std::fs::read_to_string(&file).unwrap().contains(KEY));
}

#[cfg(unix)]
#[test]
fn subscription_mode_runs_the_official_cli_in_an_empty_directory() {
    use std::os::unix::fs::PermissionsExt;
    let t = T::new("sub");
    let fake = t.dir.join("fake claude");
    let log = t.dir.join("claude-log.txt");
    std::fs::write(&fake, format!(
        "#!/bin/sh\nif [ \"$1\" = --help ]; then echo '  -p, --print  Print response and exit'; exit 0; fi\n{{ echo \"args:$*\"; echo \"cwd:$(pwd)\"; echo \"files:$(ls -A | wc -l)\"; cat; }} > '{}'\necho 'answer from the subscription CLI'\n",
        log.display()
    )).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    t.set("ai.providers.anthropic.cli", fake.to_str().unwrap());
    t.set("ai.providers.anthropic.auth", "subscription");
    std::fs::write(t.dir.join("src.c"), "int v;\n").unwrap();
    let (_, code, out, err) = t.run(
        &[
            "ai",
            "chat",
            "review",
            "--provider",
            "anthropic",
            "--file",
            "src.c",
        ],
        &[],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("answer from the subscription CLI"));
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(seen.contains("args:-p --output-format text"), "{seen}");
    // `wc -l` pads its answer on the BSDs ("files:       0"), so compare the number, not the text.
    assert!(
        seen.lines()
            .any(|l| l.strip_prefix("files:").is_some_and(|n| n.trim() == "0")),
        "the CLI starts in an empty directory: {seen}"
    );
    assert!(!seen.contains(&format!("cwd:{}", t.dir.display())));
    assert!(
        seen.contains("int v;") && seen.contains("review"),
        "prompt on stdin"
    );
    let _ = Path::new(&fake);
}
