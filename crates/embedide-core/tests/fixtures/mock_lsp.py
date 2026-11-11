#!/usr/bin/env python3
"""Scriptable mock language server for EmbedIDE's LSP client tests.

Usage: mock_lsp.py MODE LOG
  normal   answer completion with items built from the request position
  reverse  hold the first completion and answer it after the second (stale)
  crash    exit right after `initialized`
  utf16    like normal, but do not accept utf-8 positions
  garbage  answer initialize, then write an invalid frame
Every received message is appended to LOG as one JSON line.
"""
import json
import sys

mode, log_path = sys.argv[1], sys.argv[2]
stdin, stdout = sys.stdin.buffer, sys.stdout.buffer


def log(message):
    with open(log_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(message) + "\n")


def read():
    length = None
    while True:
        line = stdin.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        name, _, value = line.partition(b":")
        if name.lower() == b"content-length":
            length = int(value)
    return json.loads(stdin.read(length))


def write(message):
    body = json.dumps(message).encode()
    stdout.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    stdout.flush()


def items(request):
    pos = request["params"]["position"]
    start = {"line": pos["line"], "character": max(0, pos["character"] - 2)}
    return {
        "isIncomplete": False,
        "items": [
            {"label": "println!", "kind": 3, "detail": "macro", "sortText": "1",
             "textEdit": {"range": {"start": start, "end": pos}, "newText": "println!($0)"},
             "insertTextFormat": 2},
            {"label": "print_ж", "kind": 6, "detail": "u32 at %d:%d" % (pos["line"], pos["character"]),
             "sortText": "2"},
            {"no_label": True},
        ],
    }


held = None
while True:
    message = read()
    if message is None:
        break
    log(message)
    method = message.get("method")
    if method == "initialize":
        encodings = message["params"]["capabilities"]["general"]["positionEncodings"]
        encoding = "utf-16" if mode == "utf16" else encodings[0]
        write({"jsonrpc": "2.0", "id": message["id"], "result": {"capabilities": {
            "positionEncoding": encoding,
            "completionProvider": {"triggerCharacters": [".", ":"]}}}})
        # A server request and a notification the client must cope with.
        write({"jsonrpc": "2.0", "id": "cfg-1", "method": "workspace/configuration",
               "params": {"items": [{"section": "x"}, {"section": "y"}]}})
        write({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
               "params": {"uri": "file:///x", "diagnostics": []}})
    elif method == "initialized":
        if mode == "crash":
            sys.exit(3)
        if mode == "garbage":
            stdout.write(b"Content-Length: 5\r\n\r\n{nope")
            stdout.flush()
    elif method == "textDocument/completion":
        if mode == "reverse" and held is None:
            held = message
            continue
        write({"jsonrpc": "2.0", "id": message["id"], "result": items(message)})
        if held is not None:
            write({"jsonrpc": "2.0", "id": held["id"], "result": items(held)})
            held = None
    elif method == "shutdown":
        write({"jsonrpc": "2.0", "id": message["id"], "result": None})
    elif method == "exit":
        break
