#!/usr/bin/env python3
"""Mock Claude Code / Codex CLI for EmbedIDE tests (never contacts anyone).

mock_cli.py KIND MODE LOGDIR [cli args...]
KIND: claude | codex | old-claude (no structured output in --help)
MODE: ok | slow | auth | limit | crash | hang | huge | noread | loggedout | child
Writes LOGDIR/args, LOGDIR/env, LOGDIR/cwd, LOGDIR/stdin for inspection.
"""
import json, os, subprocess, sys, time

kind, mode, logdir, args = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
out = sys.stdout


def emit(obj):
    out.write(json.dumps(obj) + "\n")
    out.flush()


if args == ["--version"]:
    if "claude" in kind:
        print("2.1.322 (Claude Code)")
    elif kind == "agy":
        print("1.5.14")
    else:
        print("codex-cli 0.163.0")
    sys.exit(0)
if args in (["--help"], ["exec", "--help"]):
    if kind == "claude":
        print("  -p, --print\n  --output-format stream-json\n  --verbose\n  --include-partial-messages\n"
              "  --tools <tools...>\n  --no-session-persistence\n")
    elif kind == "old-claude":
        print("  -p, --print  Print response and exit\n  --output-format text|json\n")
    elif kind == "agy":
        print("  --print\n  --output-format (text, json, stream-json)\n  --mode (accept-edits, auto, plan)\n"
              "  --sandbox\n  --disable-slash-commands\n  --conversation\n")
    else:
        print("  -s, --sandbox <SANDBOX_MODE>\n  --json\n  --ephemeral\n")
    sys.exit(0)
if args == ["models"]:
    if mode == "loggedout":
        print("Error: not authenticated. Please sign in.", file=sys.stderr)
        sys.exit(1)
    print("\r⠋ Fetching available models...\rgemini-4-neon-high   Gemini 4 Neon (High)")
    print("claude-sonnet-5-5-medium  Claude Sonnet 5.5 (Medium)")
    sys.exit(0)
if args[:2] == ["auth", "status"]:
    logged = mode != "loggedout"
    print(json.dumps({"loggedIn": logged, "authMethod": "claude.ai", "subscriptionType": "max",
                      "email": "person@example.com", "orgName": "Secret Org"}))
    sys.exit(0 if logged else 1)
if args[:2] == ["login", "status"]:
    if mode == "loggedout":
        print("Not logged in", file=sys.stderr)
        sys.exit(1)
    print("Logged in using ChatGPT")
    sys.exit(0)

with open(os.path.join(logdir, "args"), "w") as f:
    json.dump(args, f)
with open(os.path.join(logdir, "env"), "w") as f:
    json.dump(dict(os.environ), f)
with open(os.path.join(logdir, "cwd"), "w") as f:
    f.write(os.getcwd() + "\n" + str(len(os.listdir("."))))
if mode == "noread":
    sys.exit(0)
prompt = sys.stdin.read()
if kind == "agy" and not prompt:
    for a in args:
        if a.startswith("--print="):
            prompt = a.removeprefix("--print=")
            break
with open(os.path.join(logdir, "stdin"), "w") as f:
    f.write(prompt)
marker = (prompt.strip().splitlines()[-1] if prompt.strip() else "").removeprefix("User: ")

if mode == "child":
    subprocess.Popen(["sleep", "3001"])  # grandchild in the same process group
    time.sleep(300)
if mode == "hang":
    time.sleep(300)
if mode == "crash":
    print("panic: mock crashed", file=sys.stderr)
    sys.exit(3)
if "Instruction:" in prompt and "```" in prompt:
    body = prompt.split("```\n", 1)[1].split("\n```", 1)[0]
    reply = "```\n// reviewed by mock\n" + body + "\n```"
    if kind == "codex":
        emit({"type": "item.completed", "item": {"type": "agent_message", "text": reply}})
        emit({"type": "turn.completed", "usage": {}})
    else:
        emit({"type": "result", "subtype": "success", "is_error": False, "result": reply, "usage": {}})
    sys.exit(0)
if kind == "old-claude":
    print("plain answer for " + marker)
    sys.exit(0)
if kind == "claude":
    emit({"type": "system", "subtype": "init", "apiKeySource": "none"})
    if mode == "auth":
        emit({"type": "result", "subtype": "success", "is_error": True,
              "result": "Invalid API key · Please run /login", "usage": {}})
        sys.exit(1)
    if mode == "limit":
        emit({"type": "result", "subtype": "success", "is_error": True,
              "result": "Claude AI usage limit reached|1760000000", "usage": {}})
        sys.exit(1)
    if mode == "huge":
        chunk = "x" * 60000
        for _ in range(400):
            emit({"type": "stream_event", "event": {"type": "content_block_delta",
                  "delta": {"type": "text_delta", "text": chunk}}})
        sys.exit(0)
    parts = ["Привет ", "\x1b[31mred\x1b[0m ", "猫 ", "answer for " + marker]
    for p in parts:
        emit({"type": "stream_event", "event": {"type": "content_block_delta",
              "delta": {"type": "text_delta", "text": p}}})
        time.sleep(0.6 if mode == "slow" else 0.02)
    emit({"type": "assistant", "message": {"model": "mock-model", "content": [{"type": "text", "text": "".join(parts)}]}})
    emit({"type": "result", "subtype": "success", "is_error": False, "result": "".join(parts),
          "usage": {"input_tokens": 11, "output_tokens": 7}})
    sys.exit(0)
if kind == "agy":
    emit({"event": "init", "conversation_id": "conv-1", "init": {"model": "gemini-4-neon-high"}})
    if mode == "auth":
        emit({"event": "result", "result": {"status": "ERROR", "error": "401 Unauthorized: please sign in", "usage": {}}})
        sys.exit(1)
    if mode == "limit":
        emit({"event": "result", "result": {"status": "ERROR", "error": "quota exceeded (429)", "usage": {}}})
        sys.exit(1)
    reply = "agy answer for " + marker
    emit({"event": "step_update", "step_update": {"step_index": 1, "state": "DONE", "step_type": "agent_response", "text_delta": reply}})
    emit({"event": "result", "result": {"conversation_id": "conv-1", "status": "SUCCESS", "response": reply, "usage": {"input_tokens": 19, "output_tokens": 6}}})
    sys.exit(0)
# codex
emit({"type": "thread.started", "thread_id": "t"})
emit({"type": "turn.started"})
if mode == "auth":
    emit({"type": "error", "message": "unexpected status 401 Unauthorized: token expired"})
    emit({"type": "turn.failed", "error": {"message": "401 Unauthorized"}})
    sys.exit(1)
if mode == "limit":
    emit({"type": "turn.failed", "error": {"message": "You've hit your usage limit. Try again later."}})
    sys.exit(1)
print("2026-11-11 ERROR codex_models_manager: failed to refresh available models", file=sys.stderr)
emit({"type": "item.completed", "item": {"type": "reasoning", "text": "thinking"}})
emit({"type": "item.completed", "item": {"type": "agent_message", "text": "codex answer for " + marker + " ж"}})
emit({"type": "turn.completed", "usage": {"input_tokens": 5, "output_tokens": 3}})
