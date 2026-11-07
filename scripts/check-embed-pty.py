#!/usr/bin/env python3
"""End-to-end PTY tests for the `embed` terminal IDE.

Drives the real binary through a private pseudo-terminal with keyboard input
only: explorer / AI / terminal panels (Alt+1/2/3), Vim editing and saving,
builds with a configured compiler, diagnostics jump, cancellation, the
interactive shell and the AI panel against a local mock HTTP server. No real
AI provider is contacted, nothing is flashed, no GUI or Node.js is used.

    python3 scripts/check-embed-pty.py --binary target/debug/embed
"""
import argparse
import fcntl
import http.server
import importlib.util
import json
import os
from pathlib import Path
import pty
import re
import shutil
import struct
import subprocess
import tempfile
import termios
import threading
import time
import unittest

spec = importlib.util.spec_from_file_location("line_pty", Path(__file__).with_name("check-terminal-pty.py"))
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)

ALT_1, ALT_2, ALT_3 = b"\x1b1", b"\x1b2", b"\x1b3"
F3, F5, F8 = b"\x1bOR", b"\x1b[15~", b"\x1b[19~"
CTRL_P, CTRL_Q = b"\x10", b"\x11"


class Ide(driver.Terminal):
    """`embed` started with no arguments in `cwd`, like a user would."""

    def __init__(self, cwd, env, size=(40, 120)):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", size[0], size[1], 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.output = bytearray()
        self.offset = 0
        self.process = subprocess.Popen([str(driver.BINARY)], cwd=cwd, env=env,
                                        stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                        start_new_session=True)

    def until(self, marker, timeout=15):
        deadline = time.monotonic() + timeout
        while marker not in self.output[self.offset:]:
            if self.process.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"missing {marker!r}; screen:\n{self.screen()}")
            self.pump(0.05)
        self.offset = self.output.index(marker, self.offset) + len(marker)

    def screen(self):
        """Approximate visible screen (row-addressed output of the IDE renderer)."""
        rows, row, col = {}, 1, 1
        text = bytes(self.output).decode("utf-8", "replace")
        for token in re.finditer(r"\x1b\[([0-9;? ]*)([A-Za-z@])|\x1b.|([^\x1b]+)", text, re.S):
            if token.group(3):
                line = rows.get(row, "").ljust(col - 1)
                chunk = token.group(3).replace("\r", "").replace("\n", "")
                rows[row] = line[:col - 1] + chunk + line[col - 1 + len(chunk):]
                col += len(chunk)
            elif token.group(2) == "H":
                parts = (token.group(1) or "1;1").split(";")
                row, col = int(parts[0] or 1), int(parts[1] or 1) if len(parts) > 1 else 1
            elif token.group(2) == "K" and token.group(1) == "2":
                rows[row] = ""
            elif token.group(2) == "J" and token.group(1) == "2":
                rows = {}
        return "\n".join(rows.get(i, "") for i in range(1, max(rows or [1]) + 1))

    def keys(self, *chunks, pause=0.03):
        """Send keys one chunk at a time, as typed (escape sequences stay whole)."""
        for chunk in chunks:
            self.send(chunk)
            time.sleep(pause)
            self.pump(0)


class MockProvider(http.server.BaseHTTPRequestHandler):
    """OpenAI-style Chat Completions; replies depend on the system prompt."""
    requests = []

    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        MockProvider.requests.append({"auth": self.headers.get("Authorization"), "body": body})
        system = next((m["content"] for m in body["messages"] if m["role"] == "system"), "")
        if system.startswith("You are a code completion engine"):
            text = "return 42;"
        elif system.startswith("You edit one source file"):
            text = "// reviewed\nint main(void) {\n    return 0;\n}\n"
        else:
            text = "Mock says: the file defines main."
        if body.get("stream"):
            events = [{"choices": [{"delta": {"content": text[:5]}}]},
                      {"choices": [{"delta": {"content": text[5:]}, "finish_reason": "stop"}]},
                      {"choices": [], "usage": {"prompt_tokens": 11, "completion_tokens": 7}}]
            payload = "".join(f"data: {json.dumps(e)}\n\n" for e in events) + "data: [DONE]\n\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
        else:
            payload = json.dumps({"choices": [{"message": {"content": text}, "finish_reason": "stop"}],
                                  "usage": {"prompt_tokens": 11, "completion_tokens": 7}})
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
        data = payload.encode()
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class EmbedContracts(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="embed pty 猫 ")
        self.addCleanup(self.directory.cleanup)
        base = Path(self.directory.name)
        self.root = base / "project"
        self.root.mkdir()
        self.config = base / "config"
        self.config.mkdir()
        self.bin = base / "bin"
        self.bin.mkdir()
        # Any attempt to use Node.js would leave this marker behind.
        self.node_marker = base / "node-was-run"
        node = self.bin / "node"
        node.write_text(f"#!/bin/sh\ntouch '{self.node_marker}'\nexit 1\n")
        node.chmod(0o700)
        (self.root / "src").mkdir()
        (self.root / "src" / "main.c").write_text("int main(void) {\n    return 0;\n}\n")
        (self.root / "notes.txt").write_text("one\ntwo\nthree\n")
        self.env = {k: v for k, v in os.environ.items()
                    if k not in ("DISPLAY", "WAYLAND_DISPLAY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY",
                                 "MOONSHOT_API_KEY", "XAI_API_KEY", "EMBEDIDE_AI_OFFLINE", "CARGO_TARGET_DIR")}
        self.env.update({"TERM": "xterm-256color", "SHELL": "/bin/sh", "EMBEDIDE_CONFIG_DIR": str(self.config),
                         "PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}"})

    def tearDown(self):
        self.assertFalse(self.node_marker.exists(), "embed executed node")

    def manifest(self, extra=""):
        (self.root / "embedide.toml").write_text(
            'schema = 1\n[project]\nname = "demo"\nlanguage = "c"\n'
            '[build]\nsystem = "native"\nsources = ["src"]\n'
            '[toolchain]\nc = "gcc"\n[profiles.debug]\nflags = ["-O0", "-g"]\n'
            f'[profiles.release]\nflags = ["-O2"]\n{extra}')

    def ide(self, size=(40, 120)):
        ide = Ide(self.root, self.env, size)
        self.addCleanup(ide.close)
        ide.until(b"EmbedIDE 2.0")
        return ide

    def open_file(self, ide, name):
        ide.keys(b"\x1b")  # the IDE starts in the explorer; Esc focuses the editor
        for ch in f":e {name}":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"NORMAL | " + name.encode())

    def quit(self, ide):
        ide.keys(CTRL_Q)
        self.assertEqual(ide.finish(), 0, bytes(ide.output[-2000:]))
        for sequence in (b"\x1b[?25h", b"\x1b[?1049l"):
            self.assertIn(sequence, ide.output)

    def test_no_arguments_open_the_ide_and_vim_editing_saves_a_real_file(self):
        ide = self.ide()
        self.open_file(ide, "notes.txt")
        ide.until(b"Ln 1, Col 1")
        ide.keys(b"j", b"l")
        ide.until(b"Ln 2, Col 2")
        ide.keys(b"i")
        ide.until(b"INSERT | notes.txt")
        ide.keys("猫".encode(), b"\x1b")
        ide.until(b"NORMAL | notes.txt [+]")
        ide.keys(b":", b"w", b"\r")
        ide.until(b"Saved")
        self.assertEqual((self.root / "notes.txt").read_text(), "one\nt猫wo\nthree\n")
        ide.keys(b":", b"q", b"\r")  # last document: :q leaves the IDE
        self.assertEqual(ide.finish(), 0)

    def test_alt_keys_switch_explorer_assistant_and_terminal(self):
        ide = self.ide()
        ide.keys(ALT_2)
        ide.until(b"ASSISTANT \xc2\xb7 NORMAL")
        ide.keys(ALT_2)  # again: back to the editor
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        ide.keys(ALT_1)
        ide.until(b"> EXPLORER")
        ide.until(b"EXPLORER \xc2\xb7 NORMAL")
        # Explorer: src/ is first; expand it and open main.c with j/Enter.
        ide.keys(b"\r", b"j", b"\r")
        ide.until(b"NORMAL | src/main.c")
        ide.until(b"| c | no embedide.toml")
        ide.keys(ALT_3)
        ide.until(b"> TERMINAL")
        ide.keys(b"echo marker-$((6*7)) && printf shell > made-by-shell.txt\r")
        ide.until(b"marker-42")
        self.assertEqual((self.root / "made-by-shell.txt").read_text(), "shell")
        # Ctrl+C and Esc belong to the shell, not to the IDE.
        ide.keys(b"sleep 30\r", b"\x03")
        ide.keys(b"echo still-$((1+1))\r")
        ide.until(b"still-2")
        ide.keys(b"cat -v\r", b"\x1b", b"\r", b"\x04")  # Esc reaches the program, Ctrl+D ends it
        ide.until(b"^[")
        ide.keys(b"echo after-$((2*2))cat\r")
        ide.until(b"after-4cat")
        ide.keys(ALT_3)
        ide.until(b"NORMAL | src/main.c")
        self.quit(ide)

    def test_build_with_configured_gcc_jumps_to_diagnostic(self):
        if not shutil.which("gcc"):
            self.skipTest("gcc not installed")
        self.manifest()
        (self.root / "src" / "main.c").write_text("int main(void) {\n    int x = 1\n    return x;\n}\n")
        ide = self.ide()
        self.open_file(ide, "src/main.c")
        ide.until(b"| c | gcc |")
        ide.keys(F5)
        ide.until(b"[c] gcc")
        ide.until(b"Failed / cancelled", timeout=60)
        ide.keys(F3)
        ide.until(b"PROBLEMS")
        ide.keys(b"\r")
        ide.until(b"Ln 3, Col 5")  # gcc: expected ';' before 'return'
        self.quit(ide)

    def test_successful_build_and_safe_cancellation(self):
        if not shutil.which("gcc") or not shutil.which("make"):
            self.skipTest("gcc/make not installed")
        self.manifest()
        ide = self.ide()
        ide.keys(F5)
        ide.until(b"Succeeded", timeout=60)
        self.assertTrue((self.root / "build" / "debug" / "demo").is_file())
        ide.keys(F5)
        ide.until(b"Up to date")
        self.quit(ide)
        # A long make build is cancelled with F8 without leaving processes behind.
        (self.root / "embedide.toml").write_text(
            'schema = 1\n[project]\nname = "slow"\nlanguage = "c"\n[build]\nsystem = "make"\n'
            '[profiles.debug]\n')
        (self.root / "Makefile").write_text("all:\n\tsleep 30\n")
        ide = self.ide()
        ide.keys(F5)
        ide.until(b"sleep 30", timeout=30)
        started = time.monotonic()
        ide.keys(F8)
        ide.until(b"Failed / cancelled", timeout=15)
        self.assertLess(time.monotonic() - started, 10)
        self.quit(ide)

    def test_assistant_chat_edit_review_and_inline_completion_with_mock_provider(self):
        MockProvider.requests = []
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MockProvider)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        (self.config / "config.toml").write_text(
            '[ai]\nprovider = "openai"\n[ai.providers.openai]\n'
            f'base_url = "http://127.0.0.1:{server.server_port}/v1"\nmodel = "mock-model"\n')
        self.env["OPENAI_API_KEY"] = "sk-test-only"
        ide = self.ide()
        self.open_file(ide, "src/main.c")
        ide.keys(ALT_2)
        ide.until(b"openai/mock-model")
        self.assertEqual(MockProvider.requests, [], "nothing may be sent before Enter")
        for ch in "/file":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Attached src/main.c")
        for ch in "What is this?":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Mock says: the file defines main.")
        chat = MockProvider.requests[-1]
        self.assertEqual(chat["auth"], "Bearer sk-test-only")
        self.assertEqual(chat["body"]["model"], "mock-model")
        self.assertIn("int main(void)", chat["body"]["messages"][-1]["content"])
        # Edit proposal → reviewed diff → apply to the buffer → :w
        for ch in "/edit add a review comment":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"+// reviewed")
        ide.until(b"/apply puts it")
        self.assertEqual((self.root / "src" / "main.c").read_text(), "int main(void) {\n    return 0;\n}\n")
        for ch in "/apply":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"AI edit applied")
        ide.keys(ALT_2)
        ide.until(b"NORMAL | src/main.c [+]")
        ide.keys(b":", b"w", b"\r")
        ide.until(b"Saved")
        self.assertEqual((self.root / "src" / "main.c").read_text(),
                         "// reviewed\nint main(void) {\n    return 0;\n}\n")
        # Inline completion: explicit request, ghost text, Tab accepts.
        ide.keys(b"G", b"o")
        ide.until(b"INSERT | src/main.c")
        ide.keys(b"\x1b")
        for ch in ":complete":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"return 42;")  # ghost text at the cursor, not yet in the buffer
        ide.until(b"Tab accepts, Esc rejects")
        self.assertNotIn("return 42;", (self.root / "src" / "main.c").read_text())
        ide.keys(b"\t")
        ide.until(b"Suggestion inserted")
        ide.keys(b":", b"w", b"\r")
        ide.until(b"Saved")
        self.assertIn("return 42;", (self.root / "src" / "main.c").read_text())
        self.assertEqual(len(MockProvider.requests), 3)
        # Reject path: a new suggestion dismissed with Esc changes nothing.
        ide.keys(b"o", b"\x1b", b":", b"w", b"\r")
        ide.until(b"Saved")
        before = (self.root / "src" / "main.c").read_text()
        for ch in ":complete":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Tab accepts, Esc rejects")
        ide.keys(b"\x1b")
        ide.until(b"Suggestion rejected")
        ide.keys(b":", b"w", b"\r")
        self.quit(ide)
        self.assertEqual((self.root / "src" / "main.c").read_text(), before)

    def test_ide_works_offline_without_ai_configuration(self):
        self.env["EMBEDIDE_AI_OFFLINE"] = "1"
        ide = self.ide()
        ide.keys(ALT_2)
        ide.until(b"no provider")
        for ch in "hello":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"No provider")
        ide.keys(b"\x1b")
        self.open_file(ide, "notes.txt")
        ide.keys(b"d", b"d", b":", b"w", b"\r")
        ide.until(b"Saved")
        self.quit(ide)
        self.assertEqual((self.root / "notes.txt").read_text(), "two\nthree\n")

    def test_narrow_terminal_keeps_panels_reachable(self):
        ide = self.ide(size=(24, 80))
        ide.keys(ALT_2)  # AI panel overlays the editor below 100 columns
        ide.until(b"> AI \xc2\xb7 no provider")
        ide.keys(ALT_3)
        ide.until(b"> TERMINAL")
        ide.keys(b"echo narrow-$((2+3))\r")
        ide.until(b"narrow-5")
        ide.keys(ALT_1)
        ide.until(b"> EXPLORER")
        self.quit(ide)

    def test_command_mode_runs_ide_commands(self):
        ide = self.ide()
        ide.keys(b"\x1b")
        for ch in ":help":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Alt+1 explorer | Alt+2 AI assistant | Alt+3 terminal")
        ide.keys(b"\x1b")
        for ch in ":e notes.txt":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"NORMAL | notes.txt")
        for ch in ":nonsense":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Unknown command :nonsense")
        self.quit(ide)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    args, remaining = parser.parse_known_args()
    driver.BINARY = args.binary.resolve(strict=True)
    result = unittest.main(argv=[__file__, *remaining], exit=False, verbosity=2).result
    raise SystemExit(0 if result.wasSuccessful() else 1)
