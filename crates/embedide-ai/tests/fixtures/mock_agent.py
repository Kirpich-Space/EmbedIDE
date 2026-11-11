#!/usr/bin/env python3
"""Mock Claude Code / Codex CLI in agent mode for EmbedIDE tests (contacts nobody).

mock_agent.py KIND SCENARIO LOGDIR [cli args...]
KIND: claude | codex
SCENARIO:
  edit     write add.c, run a build command, then try `rm -rf build`
  fix      write the file `fixed` (makes check.py succeed)
  apikey   claude: report an API key source in system/init
  hang     start a grandchild (`sleep 3001`, pid in LOGDIR/grandchild) and wait
  plan     codex: read-only sandbox -> answer with a plan; otherwise carry it out
Each run appends its arguments to LOGDIR/runs (one JSON list per line) and
the permission decisions it received to LOGDIR/decisions.
"""
import json, os, subprocess, sys, time

kind, scenario, logdir, args = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def log(name, line):
    with open(os.path.join(logdir, name), "a") as f:
        f.write(line + "\n")


if args == ["--version"]:
    print("2.1.322 (Claude Code)" if kind == "claude" else "codex-cli 0.163.0")
    sys.exit(0)
if args in (["--help"], ["exec", "--help"]):
    if kind == "claude":
        print('  -p, --print\n  --output-format stream-json\n  --input-format <format> text|stream-json\n'
              '  --include-partial-messages\n  --permission-mode <mode> (choices: "acceptEdits", "manual", "plan")\n'
              '  --tools <tools...>\n  --resume\n  --session-id <uuid>\n')
    else:
        print("  -s, --sandbox <SANDBOX_MODE>\n  --json\n  --ephemeral\n  resume\n")
    sys.exit(0)

log("runs", json.dumps(args))
with open(os.path.join(logdir, "env"), "w") as f:
    json.dump(dict(os.environ), f)
cwd = os.getcwd()


def hang():
    p = subprocess.Popen(["sleep", "3001"])
    with open(os.path.join(logdir, "grandchild"), "w") as f:
        f.write(str(p.pid))
    time.sleep(300)


# ---------------------------------------------------------------- claude
if kind == "claude":
    def read_msg():
        line = sys.stdin.readline()
        if not line:
            sys.exit(0)
        return json.loads(line)

    init = read_msg()
    assert init["type"] == "control_request" and init["request"]["subtype"] == "initialize", init
    emit({"type": "control_response", "response": {"subtype": "success", "request_id": init["request_id"],
                                                    "response": {"commands": []}}})
    user = read_msg()
    log("prompts", json.dumps(user["message"]["content"]))
    session = "none"
    for flag in ("--session-id", "--resume"):
        if flag in args:
            session = args[args.index(flag) + 1]
    emit({"type": "system", "subtype": "init", "session_id": session, "model": "mock-opus",
          "apiKeySource": "ANTHROPIC_API_KEY" if scenario == "apikey" else "none", "cwd": cwd})
    if scenario == "apikey":
        sys.stdin.read()
        sys.exit(0)
    if scenario == "hang":
        hang()
    n = [0]

    def tool(name, inp, run):
        n[0] += 1
        tid = "toolu_%d" % n[0]
        emit({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": tid, "name": name, "input": inp}]}})
        rid = "req_%d" % n[0]
        emit({"type": "control_request", "request_id": rid,
              "request": {"subtype": "can_use_tool", "tool_name": name, "input": inp, "tool_use_id": tid}})
        while True:
            msg = read_msg()
            if msg.get("type") == "control_response" and msg["response"].get("request_id") == rid:
                break
        behavior = msg["response"]["response"]["behavior"]
        log("decisions", "%s %s" % (name, behavior))
        if behavior == "allow":
            content, err = run(msg["response"]["response"]["updatedInput"])
        else:
            content, err = msg["response"]["response"].get("message", "denied"), True
        emit({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": tid,
                                                        "content": content, "is_error": err}]}})
        return behavior

    def write(inp):
        with open(inp["file_path"], "w") as f:
            f.write(inp["content"])
        return "File created successfully at: " + inp["file_path"], False

    if scenario == "fix":
        tool("Write", {"file_path": os.path.join(cwd, "fixed"), "content": "ok\n"}, write)
        summary = "fixed the build"
    else:
        tool("Write", {"file_path": os.path.join(cwd, "add.c"),
                       "content": "int add(int a, int b) { return a + b; }\n"}, write)
        tool("Read", {"file_path": "/etc/hostname"}, lambda i: ("host", False))
        tool("Bash", {"command": "gcc -o app main.c add.c && ./app"}, lambda i: ("5", False))
        tool("Bash", {"command": "make broken"}, lambda i: ("Exit code 2\nmake: *** No rule to make target 'broken'", True))
        tool("Bash", {"command": "rm -rf build"}, lambda i: ("", False))
        summary = "Created add.c; built and ran app (5)."
    for part in ["Created ", "add.c; ", "built."]:
        emit({"type": "stream_event", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": part}}})
    emit({"type": "result", "subtype": "success", "is_error": False, "result": summary, "session_id": session,
          "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 20}})
    # Like the real CLI: stays until stdin closes.
    sys.stdin.read()
    sys.exit(0)

# ---------------------------------------------------------------- codex
prompt = sys.stdin.read()
log("prompts", json.dumps(prompt))
resumed = args[:2] == ["exec", "resume"]
thread = args[2] if resumed else "thread-1"
emit({"type": "thread.started", "thread_id": thread})
emit({"type": "turn.started"})
if scenario == "hang":
    hang()
read_only = ("read-only" in args) or ('sandbox_mode="read-only"' in args)
if scenario == "plan" and read_only:
    emit({"type": "item.completed", "item": {"id": "item_0", "type": "agent_message",
                                             "text": "1. Create add.c\n2. Run gcc -o app main.c add.c"}})
    emit({"type": "turn.completed", "usage": {"input_tokens": 50, "output_tokens": 9}})
    sys.exit(0)
emit({"type": "item.completed", "item": {"id": "item_1", "type": "reasoning", "text": "**Planning**"}})
emit({"type": "item.completed", "item": {"id": "item_2", "type": "todo_list",
                                         "items": [{"text": "write add.c", "completed": True},
                                                   {"text": "build", "completed": False}]}})
cmd = "/usr/bin/bash -lc 'gcc -o app main.c add.c && ./app'"
emit({"type": "item.started", "item": {"id": "item_3", "type": "command_execution", "command": cmd,
                                       "aggregated_output": "", "exit_code": None, "status": "in_progress"}})
if read_only:
    emit({"type": "item.completed", "item": {"id": "item_4", "type": "file_change", "status": "failed",
                                             "changes": []}})
else:
    target = os.path.join(cwd, "fixed" if scenario == "fix" else "add.c")
    with open(target, "w") as f:
        f.write("int add(int a, int b) { return a + b; }\n")
    emit({"type": "item.completed", "item": {"id": "item_4", "type": "file_change", "status": "completed",
                                             "changes": [{"path": target, "kind": "add"}]}})
emit({"type": "item.completed", "item": {"id": "item_3", "type": "command_execution", "command": cmd,
                                         "aggregated_output": "5\n", "exit_code": 0, "status": "completed"}})
emit({"type": "item.completed", "item": {"id": "item_5", "type": "agent_message", "text": "Done: add.c created, app prints 5."}})
emit({"type": "turn.completed", "usage": {"input_tokens": 70, "cached_input_tokens": 40, "output_tokens": 12}})
