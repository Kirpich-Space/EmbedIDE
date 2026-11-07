//! Wire-protocol tests against a local mock HTTP server. No real provider is
//! contacted and no request can be billed: every URL is 127.0.0.1.
use embedide_ai::{api_backend, chat, complete, edit, Cancel, Error, ProviderId, Secret};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{mpsc, Arc, Mutex},
    thread,
};

/// (status, headers, body)
type Canned = (u16, Vec<(&'static str, String)>, String);

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

/// Serve `responses` in order, one per connection; returns base URL and the request log.
fn serve(responses: Vec<Canned>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::new()));
    let l2 = log.clone();
    let (ready, wait) = mpsc::channel();
    thread::spawn(move || {
        ready.send(()).unwrap();
        for (status, headers, body) in responses {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut r = BufReader::new(sock.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
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
            let len: usize = hs
                .get("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut buf = vec![0; len];
            r.read_exact(&mut buf).unwrap();
            l2.lock().unwrap().push(Seen {
                method,
                path,
                headers: hs,
                body: serde_json::from_slice(&buf).unwrap_or(Value::Null),
            });
            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                429 => "Too Many Requests",
                _ => "Error",
            };
            let mut resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                resp.push_str(&format!("{k}: {v}\r\n"));
            }
            resp.push_str("\r\n");
            resp.push_str(&body);
            sock.write_all(resp.as_bytes()).unwrap();
        }
    });
    wait.recv().unwrap();
    (base, log)
}

fn json_ok(v: Value) -> Canned {
    (
        200,
        vec![("Content-Type", "application/json".into())],
        v.to_string(),
    )
}

fn sse(events: &[Value], done: bool) -> Canned {
    let mut body: String = events
        .iter()
        .map(|e| format!("event: x\ndata: {e}\n\n"))
        .collect();
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    (
        200,
        vec![("Content-Type", "text/event-stream".into())],
        body,
    )
}

const KEY: &str = "test-key-1234567890-SECRET";

#[test]
fn anthropic_messages_request_and_non_streaming_reply() {
    let (base, log) = serve(vec![json_ok(json!({
        "model": "claude-test", "stop_reason": "end_turn",
        "content": [{"type": "text", "text": "Use a volatile "}, {"type": "text", "text": "pointer."}],
        "usage": {"input_tokens": 12, "output_tokens": 5}
    }))]);
    let b = api_backend(ProviderId::Anthropic, &base, Secret::new(KEY)).unwrap();
    let req = chat::request(
        "claude-test",
        &[],
        "How do I write a register?",
        None,
        256,
        false,
    );
    let r = chat::send(b.as_ref(), &req, &Cancel::default(), &mut |_| {}).unwrap();
    assert_eq!(r.text, "Use a volatile pointer.");
    assert_eq!(
        (r.usage.input_tokens, r.usage.output_tokens),
        (Some(12), Some(5))
    );
    let seen = &log.lock().unwrap()[0];
    assert_eq!(
        (seen.method.as_str(), seen.path.as_str()),
        ("POST", "/v1/messages")
    );
    assert_eq!(seen.headers["x-api-key"], KEY);
    assert_eq!(seen.headers["anthropic-version"], "2023-06-01");
    assert!(!seen.headers.contains_key("authorization"));
    assert_eq!(seen.body["model"], "claude-test");
    assert_eq!(seen.body["max_tokens"], 256);
    assert!(seen.body["system"].as_str().unwrap().contains("EmbedIDE"));
    assert_eq!(seen.body["messages"][0]["role"], "user");
}

#[test]
fn anthropic_streaming_deltas() {
    let (base, _) = serve(vec![sse(
        &[
            json!({"type": "message_start", "message": {"model": "claude-test", "usage": {"input_tokens": 3}}}),
            json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hel"}}),
            json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "lo"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
            json!({"type": "message_stop"}),
        ],
        false,
    )]);
    let b = api_backend(ProviderId::Anthropic, &base, Secret::new(KEY)).unwrap();
    let mut deltas = Vec::new();
    let r = b
        .send(
            &chat::request("m", &[], "hi", None, 16, true),
            &Cancel::default(),
            &mut |d| deltas.push(d.to_string()),
        )
        .unwrap();
    assert_eq!(deltas, ["Hel", "lo"]);
    assert_eq!(r.text, "Hello");
    assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(
        (r.usage.input_tokens, r.usage.output_tokens),
        (Some(3), Some(2))
    );
}

#[test]
fn chat_completions_providers_share_protocol_with_their_token_parameter() {
    for (p, cap) in [
        (ProviderId::Openai, "max_completion_tokens"),
        (ProviderId::Moonshot, "max_tokens"),
        (ProviderId::Xai, "max_tokens"),
    ] {
        let (base, log) = serve(vec![sse(
            &[
                json!({"model": "m1", "choices": [{"delta": {"role": "assistant"}}]}),
                json!({"choices": [{"delta": {"content": "int "}}]}),
                json!({"choices": [{"delta": {"content": "x;"}, "finish_reason": "stop"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 9, "completion_tokens": 2}}),
            ],
            true,
        )]);
        let b = api_backend(p, &base, Secret::new(KEY)).unwrap();
        let mut got = String::new();
        let r = b
            .send(
                &chat::request("m1", &[], "q", None, 99, true),
                &Cancel::default(),
                &mut |d| got.push_str(d),
            )
            .unwrap();
        assert_eq!(got, "int x;", "{p}");
        assert_eq!(r.usage.output_tokens, Some(2), "{p}");
        let seen = &log.lock().unwrap()[0];
        assert_eq!(seen.path, "/chat/completions", "{p}");
        assert_eq!(
            seen.headers["authorization"],
            format!("Bearer {KEY}"),
            "{p}"
        );
        assert_eq!(seen.body[cap], 99, "{p} uses {cap}");
        assert_eq!(seen.body["messages"][0]["role"], "system", "{p}");
        assert_eq!(seen.body["stream_options"]["include_usage"], true, "{p}");
    }
}

#[test]
fn model_listing_per_protocol() {
    let (base, log) = serve(vec![json_ok(
        json!({"data": [{"id": "b-model", "display_name": "B"}, {"id": "a-model"}]}),
    )]);
    let ids: Vec<_> = api_backend(ProviderId::Anthropic, &base, Secret::new(KEY))
        .unwrap()
        .models()
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, ["a-model", "b-model"]);
    assert_eq!(log.lock().unwrap()[0].path, "/v1/models");
    let (base, log) = serve(vec![json_ok(
        json!({"object": "list", "data": [{"id": "grok-x"}]}),
    )]);
    let m = api_backend(ProviderId::Xai, &base, Secret::new(KEY))
        .unwrap()
        .models()
        .unwrap();
    assert_eq!(m[0].id, "grok-x");
    assert_eq!(log.lock().unwrap()[0].path, "/models");
}

#[test]
fn auth_and_rate_limit_errors_are_structured_and_redacted() {
    let (base, _) = serve(vec![
        (
            401,
            vec![],
            json!({"error": {"message": format!("invalid x-api-key {KEY}")}}).to_string(),
        ),
        (
            429,
            vec![("Retry-After", "7".into())],
            json!({"error": {"message": "slow down"}}).to_string(),
        ),
        (500, vec![], "upstream exploded".into()),
    ]);
    let b = api_backend(ProviderId::Anthropic, &base, Secret::new(KEY)).unwrap();
    let req = chat::request("m", &[], "q", None, 8, false);
    let e = b.send(&req, &Cancel::default(), &mut |_| {}).unwrap_err();
    assert!(
        matches!(&e, Error::Auth(m) if m.contains("[REDACTED]") && !m.contains(KEY)),
        "{e:?}"
    );
    assert!(!e.to_string().contains(KEY));
    assert_eq!(
        b.send(&req, &Cancel::default(), &mut |_| {}).unwrap_err(),
        Error::RateLimited(Some(7), "slow down".into())
    );
    assert!(matches!(
        b.send(&req, &Cancel::default(), &mut |_| {}).unwrap_err(),
        Error::Provider(500, _)
    ));
}

#[test]
fn non_https_endpoints_are_refused_and_cancel_is_honoured_before_sending() {
    assert!(matches!(
        api_backend(
            ProviderId::Openai,
            "http://example.com/v1",
            Secret::new(KEY)
        ),
        Err(Error::Config(_))
    ));
    let (base, log) = serve(vec![json_ok(json!({}))]);
    let b = api_backend(ProviderId::Openai, &base, Secret::new(KEY)).unwrap();
    let c = Cancel::default();
    c.cancel();
    assert_eq!(
        b.send(
            &chat::request("m", &[], "q", None, 8, false),
            &c,
            &mut |_| {}
        )
        .unwrap_err(),
        Error::Cancelled
    );
    assert!(log.lock().unwrap().is_empty(), "nothing was sent");
}

#[test]
fn completion_pipeline_through_http() {
    let (base, log) = serve(vec![json_ok(json!({
        "choices": [{"message": {"content": "```c\n    return a + b;\n```"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 40, "completion_tokens": 6}
    }))]);
    let b = api_backend(ProviderId::Moonshot, &base, Secret::new(KEY)).unwrap();
    let req = complete::CompletionRequest {
        language: "c".into(),
        path: Some("math.c".into()),
        text: "int add(int a, int b) {\n    \n}\n".into(),
        line: 2,
        column: 5,
        related: vec![("math.h".into(), "int add(int a, int b);".into())],
        max_tokens: 64,
    };
    let c = complete::complete(b.as_ref(), &req, "kimi-test", &Cancel::default()).unwrap();
    assert_eq!(c.insertion, "return a + b;");
    let prompt = log.lock().unwrap()[0].body["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(prompt.contains("int add(int a, int b) {\n    <|cursor|>\n}"));
    assert!(prompt.contains("Related file math.h"));
    assert_eq!(log.lock().unwrap()[0].body["temperature"], 0.0);
}

#[test]
fn edit_proposal_is_a_diff_not_a_write() {
    let (base, _) = serve(vec![json_ok(json!({
        "content": [{"type": "text", "text": "int main(void) {\n    return 1;\n}\n"}], "stop_reason": "end_turn", "usage": {}
    }))]);
    let b = api_backend(ProviderId::Anthropic, &base, Secret::new(KEY)).unwrap();
    let original = "int main(void) {\n    return 0;\n}\n";
    let p = edit::propose(
        b.as_ref(),
        "main.c",
        original,
        "return 1",
        "m",
        512,
        &Cancel::default(),
    )
    .unwrap();
    assert!(
        p.diff.contains("-    return 0;\n+    return 1;\n"),
        "{}",
        p.diff
    );
}
