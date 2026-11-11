#!/usr/bin/env python3
"""End-to-end PTY tests for the `embed` terminal IDE.

Drives the real binary through a private pseudo-terminal with keyboard input
only. The driver plays the outer terminal: in "kitty" mode it answers the
kitty keyboard protocol query and sends exactly the bytes kitty sends
(Ctrl+Alt+1 = CSI 49;7u, release = CSI 49;7:3u, Ctrl+Alt+Delete = CSI 3;7~,
Esc = CSI 27u, Ctrl+C = CSI 99;5u); in "legacy" mode it answers only Primary
Device Attributes and sends classic xterm bytes.

Covered: explorer / AI / terminal panels (Ctrl+Alt+1/2/3) and the held-digit
+Delete hide chords, the interactive shell (sh, bash and fish when installed)
with real commands, Ctrl+C, session preservation, PTY resize and cleanup,
`embed <file|dir>` launches, syntax colours, Vim editing and saving, builds
with a configured compiler, diagnostics jump, cancellation, and the AI panel
against a local mock HTTP server. No real AI provider is contacted, nothing is
flashed, no GUI or Node.js is used.

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
import signal
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

F3, F5, F8 = b"\x1bOR", b"\x1b[15~", b"\x1b[19~"
KEYBOARD_QUERY = b"\x1b[?u"          # crossterm: kitty flags query (followed by DA1)
KITTY_PUSH, KITTY_POP = b"\x1b[>3u", b"\x1b[<1u"  # disambiguate | report event types


def chord(digit, kind=1):
    """kitty: Ctrl+Alt+<digit> press (1), repeat (2) or release (3)."""
    return f"\x1b[{ord(digit)};7{'' if kind == 1 else ':' + str(kind)}u".encode()


CTRL_ALT_DELETE = b"\x1b[3;7~"
CTRL_ALT_DELETE_RELEASE = b"\x1b[3;7:3~"


class Ide(driver.Terminal):
    """`embed [args]` started in `cwd`, like a user would, behind a terminal
    that speaks the kitty keyboard protocol (or only legacy xterm input)."""

    def __init__(self, cwd, env, size=(40, 120), args=(), keyboard="kitty"):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", size[0], size[1], 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.output = bytearray()
        self.offset = 0
        self.keyboard = keyboard
        self.answered = 0
        self.process = subprocess.Popen([str(driver.BINARY), *map(str, args)], cwd=cwd, env=env,
                                        stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                        start_new_session=True)

    def pump(self, timeout):
        super().pump(timeout)
        # Answer the keyboard capability query exactly once, as the terminal would.
        while self.output.count(KEYBOARD_QUERY) > self.answered:
            self.answered += 1
            reply = b"\x1b[?0u" if self.keyboard == "kitty" else b""
            self.send(reply + b"\x1b[?62;22c")

    # Key encodings of the emulated terminal.
    @property
    def esc(self):
        return b"\x1b[27u" if self.keyboard == "kitty" else b"\x1b"

    def ctrl(self, letter):
        if self.keyboard == "kitty":
            return f"\x1b[{ord(letter)};5u".encode()
        return bytes([ord(letter) - ord("a") + 1])

    def panel(self, digit):
        """Tap Ctrl+Alt+<digit>: press then release."""
        self.keys(chord(digit), chord(digit, 3))

    def hide(self, digit):
        """Hold Ctrl+Alt+<digit>, press Delete, release both."""
        self.keys(chord(digit), chord(digit, 2), CTRL_ALT_DELETE, CTRL_ALT_DELETE_RELEASE, chord(digit, 3))

    def type(self, text):
        for ch in text:
            self.keys(ch.encode())

    def resize(self, rows, cols):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.killpg(self.process.pid, signal.SIGWINCH)

    def latest(self, pattern, timeout=15):
        """Wait for a regex in new output and return its match."""
        deadline = time.monotonic() + timeout
        regex = re.compile(pattern)
        while True:
            match = regex.search(bytes(self.output), self.offset)
            if match:
                self.offset = match.end()
                return match
            if self.process.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"missing {pattern!r}; screen:\n{self.screen()}")
            self.pump(0.05)

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

    def ide(self, size=(40, 120), args=(), keyboard="kitty", cwd=None):
        ide = Ide(cwd or self.root, self.env, size, args, keyboard)
        self.addCleanup(ide.close)
        ide.until(b"EmbedIDE 0.")
        if keyboard == "kitty":
            self.assertIn(KITTY_PUSH, ide.output, "kitty keyboard flags were not requested")
        else:
            self.assertNotIn(KITTY_PUSH, ide.output)
        return ide

    def open_file(self, ide, name):
        ide.keys(ide.esc)  # the IDE starts in the explorer; Esc focuses the editor
        ide.type(f":e {name}")
        ide.keys(b"\r")
        ide.until(b"NORMAL | " + name.encode())

    def quit(self, ide):
        ide.keys(ide.ctrl("q"))
        self.assertEqual(ide.finish(), 0, bytes(ide.output[-2000:]))
        for sequence in (b"\x1b[?25h", b"\x1b[?1049l"):
            self.assertIn(sequence, ide.output)
        if ide.keyboard == "kitty":
            self.assertIn(KITTY_POP, ide.output, "kitty keyboard flags were not restored")

    def test_no_arguments_open_the_ide_and_vim_editing_saves_a_real_file(self):
        ide = self.ide()
        self.open_file(ide, "notes.txt")
        ide.until(b"Ln 1, Col 1")
        ide.keys(b"j", b"l")
        ide.until(b"Ln 2, Col 2")
        ide.keys(b"i")
        ide.until(b"INSERT | notes.txt")
        ide.keys("猫".encode(), ide.esc)
        ide.until(b"NORMAL | notes.txt [+]")
        ide.keys(b":", b"w", b"\r")
        ide.until(b"Saved")
        self.assertEqual((self.root / "notes.txt").read_text(), "one\nt猫wo\nthree\n")
        ide.keys(b":", b"q", b"\r")  # last document: :q leaves the IDE
        self.assertEqual(ide.finish(), 0)

    def test_ctrl_alt_keys_switch_explorer_assistant_and_terminal(self):
        ide = self.ide()
        ide.panel("2")
        ide.until(b"ASSISTANT \xc2\xb7 NORMAL")
        ide.panel("2")  # again: back to the editor
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        ide.panel("1")
        ide.until(b"> EXPLORER")
        ide.until(b"EXPLORER \xc2\xb7 NORMAL")
        # Explorer: src/ is first; expand it and open main.c with j/Enter.
        ide.keys(b"\r", b"j", b"\r")
        ide.until(b"NORMAL | src/main.c")
        ide.until(b"| c | no embedide.toml")
        ide.panel("3")
        ide.until(b"> TERMINAL")
        ide.keys(b"echo marker-$((6*7)) && printf shell > made-by-shell.txt\r")
        ide.until(b"marker-42")
        self.assertEqual((self.root / "made-by-shell.txt").read_text(), "shell")
        # Ctrl+C and Esc belong to the shell, not to the IDE.
        ide.keys(b"sleep 30\r", ide.ctrl("c"))
        ide.keys(b"echo still-$((1+1))\r")
        ide.until(b"still-2")
        ide.keys(b"cat -v\r", ide.esc, b"\r", ide.ctrl("d"))  # Esc reaches the program, Ctrl+D ends it
        ide.until(b"^[")
        ide.keys(b"echo after-$((2*2))cat\r")
        ide.until(b"after-4cat")
        ide.panel("3")
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
        ide.panel("2")
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
        ide.panel("2")
        ide.until(b"NORMAL | src/main.c [+]")
        ide.keys(b":", b"w", b"\r")
        ide.until(b"Saved")
        self.assertEqual((self.root / "src" / "main.c").read_text(),
                         "// reviewed\nint main(void) {\n    return 0;\n}\n")
        # Inline completion: explicit request, ghost text, Tab accepts.
        ide.keys(b"G", b"o")
        ide.until(b"INSERT | src/main.c")
        ide.keys(ide.esc)
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
        ide.keys(b"o", ide.esc, b":", b"w", b"\r")
        ide.until(b"Saved")
        before = (self.root / "src" / "main.c").read_text()
        for ch in ":complete":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Tab accepts, Esc rejects")
        ide.keys(ide.esc)
        ide.until(b"Suggestion rejected")
        ide.keys(b":", b"w", b"\r")
        self.quit(ide)
        self.assertEqual((self.root / "src" / "main.c").read_text(), before)

    def test_mouse_is_opt_in_and_clicks_open_files(self):
        (self.root / "clicked.c").write_text("int clicked;\n")
        ide = self.ide()
        ide.keys(ide.esc)
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        self.assertNotIn(b"\x1b[?1000h", ide.output, "mouse must be off by default")
        ide.type(":mouse\r")
        ide.until(b"\x1b[?1000h")
        ide.until(b"Mouse on")
        # SGR mouse, 1-based: row 4 is the first explorer entry (src/), row 5
        # clicked.c. A click selects; a second click opens.
        ide.keys(b"\x1b[<0;3;5M", b"\x1b[<0;3;5m")
        ide.until(b"> EXPLORER")
        ide.keys(b"\x1b[<0;3;5M", b"\x1b[<0;3;5m")
        ide.until(b"NORMAL | clicked.c")
        # Wheel over the editor and a click in it keep working with the keyboard.
        ide.keys(b"\x1b[<65;40;6M", b"\x1b[<0;40;3M")
        ide.type("ix")
        ide.keys(ide.esc)
        ide.until(b"clicked.c [+]")
        ide.type(":w\r")
        ide.until(b"Saved")
        self.quit(ide)
        self.assertEqual((self.root / "clicked.c").read_text(), "int clickedx;\n")
        self.assertIn(b"\x1b[?1000l", ide.output, "mouse capture was not released")

    def mock_cli(self, kind, mode):
        """An executable `claude`/`codex` running the repository's mock CLI."""
        script = Path(__file__).resolve().parent.parent / "crates/embedide-ai/tests/fixtures/mock_cli.py"
        logdir = self.config / f"mock-{kind}-{mode}"
        logdir.mkdir(exist_ok=True)
        program = logdir / ("codex" if kind == "codex" else "claude")
        program.write_text(f"#!/bin/sh\nexec python3 '{script}' {kind} {mode} '{logdir}' \"$@\"\n")
        program.chmod(0o755)
        return program, logdir

    def say(self, ide, text):
        for ch in text:
            ide.keys(ch.encode())
        ide.keys(b"\r")

    def until_panel(self, ide, text, timeout=20):
        """Wait for `text` in the AI column, ignoring where the panel wraps it."""
        deadline = time.monotonic() + timeout
        while True:
            column = "".join(line.split("|")[-1] for line in ide.screen().splitlines() if "|" in line)
            if text in column:
                return
            if ide.process.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"missing {text!r} in the AI panel:\n{column}")
            ide.pump(0.1)

    def wait_file(self, ide, path):
        deadline = time.monotonic() + 15
        while not path.exists():
            self.assertLess(time.monotonic(), deadline, f"{path} never appeared")
            ide.pump(0.1)

    def test_claude_code_and_codex_cli_backends_with_mock_clis(self):
        claude, claude_log = self.mock_cli("claude", "ok")
        codex, _ = self.mock_cli("codex", "ok")
        hang, hang_log = self.mock_cli("codex", "child")
        (self.config / "config.toml").write_text(
            f'[ai]\nprovider = "claude-code"\n[ai.providers.claude-code]\ncli = "{claude}"\n'
            f'[ai.providers.codex-cli]\ncli = "{codex}"\n')
        self.env["ANTHROPIC_API_KEY"] = "sk-ant-must-not-reach-the-cli"
        ide = self.ide()
        self.open_file(ide, "src/main.c")
        ide.panel("2")
        ide.until(b"claude-code")
        self.say(ide, "/status")
        self.until_panel(ide, "logged in via claude.ai")
        self.say(ide, "/file")
        self.until_panel(ide, "Attached src/main.c")
        self.say(ide, "Explain please")
        self.until_panel(ide, "answer for Explain please")
        env = json.loads((claude_log / "env").read_text())
        self.assertNotIn("ANTHROPIC_API_KEY", env, "no silent API-key billing")
        self.assertIn("int main(void)", (claude_log / "stdin").read_text())
        # Edit: diff preview, nothing written, /apply into the buffer only.
        self.say(ide, "/edit add a review line")
        self.until_panel(ide, "+// reviewed by mock")
        self.say(ide, "/reject")
        self.until_panel(ide, "Proposal rejected")
        self.say(ide, "/edit add a review line")
        self.until_panel(ide, "+// reviewed by mock")
        self.say(ide, "/apply")
        ide.until(b"AI edit applied")
        self.assertEqual((self.root / "src" / "main.c").read_text(), "int main(void) {\n    return 0;\n}\n",
                         "nothing is written before :w")
        # Codex CLI (the AI panel still has the focus).
        self.say(ide, "/provider codex-cli")
        self.until_panel(ide, "logged in via ChatGPT")
        self.say(ide, "hello codex")
        self.until_panel(ide, "codex answer for hello codex")
        # A request that never ends: /cancel stops the CLI and its children.
        (self.config / "config.toml").write_text(
            f'[ai]\nprovider = "codex-cli"\n[ai.providers.codex-cli]\ncli = "{hang}"\n')
        ide.keys(ide.esc)
        self.say(ide, ":reload")
        ide.panel("2")
        self.say(ide, "never answers")
        self.wait_file(ide, hang_log / "stdin")
        self.say(ide, "/cancel")
        self.until_panel(ide, "Cancelled")
        time.sleep(0.8)
        self.assertEqual(subprocess.run(["pgrep", "-f", str(hang_log)], capture_output=True).stdout, b"",
                         "the cancelled CLI is still running")
        # Quit while a request runs: the CLI stops with the IDE.
        (hang_log / "stdin").unlink()
        self.say(ide, "never answers either")
        self.wait_file(ide, hang_log / "stdin")
        ide.keys(ide.esc)
        ide.keys(ide.ctrl("q"))
        ide.until(b"Discard ALL unsaved")
        ide.keys(b"y")
        self.assertEqual(ide.finish(), 0)
        time.sleep(0.5)
        self.assertEqual(subprocess.run(["pgrep", "-f", str(hang_log)], capture_output=True).stdout, b"")
        self.assertEqual(subprocess.run(["pgrep", "-f", "^sleep 3001$"], capture_output=True).stdout, b"")

    def test_ide_works_offline_without_ai_configuration(self):
        self.env["EMBEDIDE_AI_OFFLINE"] = "1"
        ide = self.ide()
        ide.panel("2")
        ide.until(b"no provider")
        for ch in "hello":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"No provider")
        ide.keys(ide.esc)
        self.open_file(ide, "notes.txt")
        ide.keys(b"d", b"d", b":", b"w", b"\r")
        ide.until(b"Saved")
        self.quit(ide)
        self.assertEqual((self.root / "notes.txt").read_text(), "two\nthree\n")

    def test_narrow_terminal_keeps_panels_reachable(self):
        ide = self.ide(size=(24, 80))
        ide.panel("2")  # AI panel overlays the editor below 100 columns
        ide.until(b"> AI \xc2\xb7 no provider")
        ide.panel("3")
        ide.until(b"> TERMINAL")
        ide.keys(b"echo narrow-$((2+3))\r")
        ide.until(b"narrow-5")
        ide.panel("1")
        ide.until(b"> EXPLORER")
        self.quit(ide)

    def test_command_mode_runs_ide_commands(self):
        ide = self.ide()
        ide.keys(ide.esc)
        for ch in ":help":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Ctrl+Alt+1 explorer | Ctrl+Alt+2 AI assistant | Ctrl+Alt+3 terminal")
        ide.keys(ide.esc)
        for ch in ":e notes.txt":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"NORMAL | notes.txt")
        for ch in ":nonsense":
            ide.keys(ch.encode())
        ide.keys(b"\r")
        ide.until(b"Unknown command :nonsense")
        self.quit(ide)


    # --- Integrated terminal: the real shell inside the real IDE -------------

    def shell_session(self, shell):
        """Steps 1-12 of the alpha terminal contract for one shell."""
        path = shutil.which(shell)
        if not path:
            self.skipTest(f"{shell} not installed")
        self.env["SHELL"] = path
        if shell == "fish":
            # Isolated fish config/history; fish still runs its startup queries.
            self.env["XDG_CONFIG_HOME"] = str(self.config / "xdg-config")
            self.env["XDG_DATA_HOME"] = str(self.config / "xdg-data")
        assign = "set x 42" if shell == "fish" else "x=42"
        marker = f"{shell}-{os.getpid()}"
        ide = self.ide()
        ide.keys(ide.esc)
        # 2-3. open the terminal panel and wait for the interactive prompt
        ide.panel("3")
        ide.until(b"> TERMINAL | " + path.encode())
        # 4-5. a unique command whose output differs from its echo
        ide.type("printf 'EMBED_%s_OK\\n' TERMINAL\r")
        ide.until(b"EMBED_TERMINAL_OK", timeout=20)
        pid_var = "$fish_pid" if shell == "fish" else "$$"
        ide.type(f"printf 'PID_%s_PID\\n' {pid_var}\r")
        pid = ide.latest(rb"PID_(\d+)_PID").group(1)
        ide.type(f"{assign}\r")
        ide.type("echo 'to stderr' 1>&2; printf 'ERR_%s\\n' SEEN\r")
        ide.until(b"ERR_SEEN")
        self.assertIn(b"to stderr", ide.output)
        # 6-7. switch to other panels and come back to the same session
        ide.panel("1")
        ide.until(b"> EXPLORER")
        ide.panel("2")
        ide.until(b"ASSISTANT \xc2\xb7 NORMAL")
        ide.panel("3")
        ide.until(b"> TERMINAL")
        # 8-9. the same shell: same PID and the variable set before switching
        ide.type(f"printf 'PID_%s_PID X_%s_X\\n' {pid_var} $x\r")
        again = ide.latest(rb"PID_(\d+)_PID X_(\w*)_X")
        self.assertEqual(again.group(1), pid, "a new shell was started")
        self.assertEqual(again.group(2), b"42", "shell state was lost")
        # 10. Ctrl+C (as the outer terminal encodes it) interrupts the foreground job
        ide.type("sleep 30\r")
        time.sleep(0.5)
        started = time.monotonic()
        ide.keys(ide.ctrl("c"))
        ide.type("printf 'AFTER_%s\\n' INT\r")
        ide.until(b"AFTER_INT", timeout=10)
        self.assertLess(time.monotonic() - started, 8, "Ctrl+C did not interrupt sleep")
        # 11. resizing the IDE resizes the shell's PTY (panel rows x width)
        ide.type("stty size | sed 's/^/SIZE_/'\r")
        ide.latest(rb"SIZE_17 119")
        ide.resize(30, 100)
        time.sleep(0.3)
        ide.type("stty size | sed 's/^/SIZE_/'\r")
        ide.latest(rb"SIZE_12 99")
        # 12. leave a foreground and a background job running, then quit
        ide.type(f"sleep 4242{len(marker)} &\r")
        ide.type(f"sleep 4343{len(marker)}\r")
        time.sleep(0.5)
        ide.panel("3")  # back to the editor; the shell keeps running
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        self.quit(ide)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            alive = subprocess.run(["pgrep", "-f", f"sleep 4[23]4[23]{len(marker)}"], capture_output=True).stdout
            if not alive:
                break
            time.sleep(0.1)
        self.assertEqual(alive, b"", f"{shell}: child processes survived the IDE")
        self.assertEqual(subprocess.run(["ps", "-p", pid.decode()], capture_output=True).returncode, 1,
                         "the shell survived the IDE")

    def test_terminal_session_with_sh(self):
        self.shell_session("sh")

    def test_terminal_session_with_bash(self):
        self.shell_session("bash")

    def test_terminal_session_with_fish(self):
        # fish 4 blocks on terminal queries (DA1, cursor position) before every
        # prompt; this is the configuration the integrated terminal failed with.
        self.shell_session("fish")

    def test_terminal_keeps_shell_colours_and_inverse_video(self):
        self.env["SHELL"] = "/bin/sh"
        ide = self.ide()
        ide.keys(ide.esc)
        ide.panel("3")
        ide.until(b"> TERMINAL | /bin/sh")
        # The echoed command line has no SGR; only the output is red/inverse.
        ide.type("printf '\\033[31mRED_%s\\033[0m \\033[7mINV_%s\\033[0m\\n' OK OK\r")
        ide.latest(rb"\x1b\[38;5;1m(?:\x1b\[[0-9;]*m)*RED_OK")
        ide.latest(rb"\x1b\[7m(?:\x1b\[[0-9;]*m)*INV_OK")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.panel("3")
        self.quit(ide)

    def test_terminal_scrollback_with_shift_page_keys(self):
        self.env["SHELL"] = "/bin/sh"
        ide = self.ide()
        ide.keys(ide.esc)
        ide.panel("3")
        ide.until(b"> TERMINAL | /bin/sh")
        ide.type("seq -f 'ROW_%03g' 1 120; echo SEQ_DONE\r")
        ide.until(b"SEQ_DONE")
        ide.pump(0.3)
        ide.keys(b"\x1b[5;2~")                      # Shift+PageUp
        ide.latest(rb"HISTORY \d+/\d+ lines up")
        ide.keys(b"\x1b[1;2H")                      # Shift+Home: oldest line
        ide.until(b"ROW_001")
        ide.keys(b"\x1b[6;2~", b"\x1b[1;2B")         # Shift+PageDown, Shift+Down
        ide.latest(rb"HISTORY \d+/\d+ lines up")
        ide.keys(b"\x1b[1;2F")                      # Shift+End: live screen again
        ide.until(b"/bin/sh | keys go to the shell")
        # Typing while scrolled back returns to the live screen and reaches the shell.
        ide.keys(b"\x1b[5;2~")
        ide.latest(rb"HISTORY")
        ide.type("printf 'BACK_%s\\n' LIVE\r")
        ide.until(b"BACK_LIVE")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.panel("3")
        self.quit(ide)

    def test_full_screen_programs_ctrl_d_and_shell_restart(self):
        pager = "less" if shutil.which("less") else "vim" if shutil.which("vim") else None
        if not pager:
            self.skipTest("neither less nor vim installed")
        self.env["SHELL"] = "/bin/sh"
        self.env["LESS"] = ""
        (self.root / "pages.txt").write_text("".join(f"PAGE_{i:03}\n" for i in range(1, 501)))
        ide = self.ide()
        ide.keys(ide.esc)
        ide.panel("3")
        ide.until(b"> TERMINAL | /bin/sh")
        # Many lines of history first, so Shift+PageUp would scroll if it were taken.
        ide.type("seq -f 'HIST_%03g' 1 100\r")
        ide.until(b"HIST_100")
        ide.type("less pages.txt\r" if pager == "less" else "vim -u NONE -N -n pages.txt\r")
        ide.until(b"PAGE_001")
        # The program owns the alternate screen: Shift+PageUp is its key, not history.
        ide.keys(b"\x1b[5;2~")
        ide.pump(0.3)
        self.assertNotIn(b"HISTORY", ide.output[ide.offset:])
        ide.type("G")
        ide.until(b"PAGE_500")
        ide.type("q" if pager == "less" else ":q!\r")
        ide.pump(1.0)  # let the program restore the screen before the shell reads input
        ide.type("printf 'LESS_%s\\n' QUIT\r")
        ide.until(b"LESS_QUIT")
        # Ctrl+D ends the shell; Enter starts a new one in the same panel.
        ide.keys(ide.ctrl("d"))
        ide.until(b"shell ended (exit 0); Enter restarts")
        ide.keys(b"\r")
        ide.type("printf 'NEW_%s\\n' SHELL\r")
        ide.until(b"NEW_SHELL")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.panel("3")
        self.quit(ide)

    def test_flooding_program_keeps_ide_responsive_and_memory_bounded(self):
        self.env["SHELL"] = "/bin/sh"
        ide = self.ide()
        ide.keys(ide.esc)
        ide.panel("3")
        ide.until(b"> TERMINAL | /bin/sh")
        ide.type("yes EMBED_FLOOD_LINE\r")
        ide.until(b"EMBED_FLOOD_LINE")
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            ide.pump(0.1)
            del ide.output[:-200000]                  # the driver keeps only the tail
            ide.offset = 0
        rss = int(re.search(rb"VmRSS:\s+(\d+)", Path(f"/proc/{ide.process.pid}/status").read_bytes()).group(1))
        self.assertLess(rss, 200 * 1024, f"RSS {rss} KiB while flooding")
        started = time.monotonic()
        ide.keys(ide.ctrl("c"))
        ide.type("printf 'FLOOD_%s\\n' STOPPED\r")
        ide.until(b"FLOOD_STOPPED", timeout=15)
        self.assertLess(time.monotonic() - started, 10, "Ctrl+C was not delivered promptly")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.panel("3")
        self.quit(ide)

    def test_release_smoke_workflow_c_project(self):
        """v0.1.0 acceptance path: create, edit, save, build in the terminal and
        with F5, read errors, scroll history, fix, rebuild, run, quit."""
        if not (shutil.which("gcc") and shutil.which("make")):
            self.skipTest("gcc and make are required")
        project = self.root / "blink"
        project.mkdir()
        (project / "Makefile").write_text("hello: main.c\n\tgcc -Wall -o hello main.c\n")
        self.env["SHELL"] = "/bin/sh"
        ide = self.ide(cwd=project)
        ide.until(b"> EXPLORER")
        # 1-3. create a source file from the explorer
        ide.type("n")
        ide.until(b"Create file")
        ide.type("main.c\r")
        ide.until(b"NORMAL | main.c")
        # 4-5. edit (with a deliberate error) and save
        ide.type('i#include <stdio.h>\rint main(void) { puts("SMOKE_OK"); return 0 }')
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        # 6-8. integrated terminal: build, read the compiler error
        ide.panel("3")
        ide.until(b"> TERMINAL")
        ide.type("seq -f 'FILL_%03g' 1 60; make\r")
        ide.until(b"hello] Error 1")              # gcc colours "error:", make does not
        # 9. scroll through history to the start of the output and back
        ide.keys(b"\x1b[5;2~", b"\x1b[1;2H")         # Shift+PageUp, Shift+Home
        ide.until(b"FILL_001")
        ide.keys(b"\x1b[1;2F")
        ide.until(b"keys go to the shell")
        # 10. back to the editor, fix line 2, save, build with F5 in the IDE
        ide.panel("3")
        ide.until(b"\x1b[49mNORMAL | main.c")
        ide.type(":2\r$i;")
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        ide.keys(F5)
        ide.until(b"Finished: exit=Some(0)")
        # run the program in the same shell session
        ide.panel("3")
        ide.until(b"> TERMINAL")
        ide.type("./hello; echo RC_$?\r")
        ide.until(b"SMOKE_OK")
        ide.until(b"RC_0")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.panel("3")
        # 11-12. quit; quit() checks exit 0, screen/cursor restore, kitty pop,
        # and finish() checks the terminal attributes are unchanged.
        self.quit(ide)
        self.assertIn('puts("SMOKE_OK"); return 0 ;}', (project / "main.c").read_text())

    def test_release_smoke_workflow_rust_project(self):
        if not shutil.which("cargo"):
            self.skipTest("cargo not installed")
        project = self.root / "rusty"
        (project / "src").mkdir(parents=True)
        (project / "Cargo.toml").write_text('[package]\nname = "rusty"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n')
        (project / "src" / "main.rs").write_text('fn main() {\n    let x: u32 = "no";\n    println!("{x}");\n}\n')
        ide = self.ide(cwd=project)
        self.open_file(ide, "src/main.rs")
        ide.until(b"| rust |")
        ide.keys(F5)
        ide.until(b"Failed / cancelled", timeout=120)
        ide.keys(F3)
        ide.until(b"PROBLEMS")
        ide.keys(b"\r")
        ide.until(b"Ln 2, Col")          # rustc: mismatched types on line 2
        ide.keys(ide.esc)
        ide.type(":2\rdd")
        ide.type('Olet x: u32 = 7;')
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        ide.keys(F5)
        ide.until(b"Finished: exit=Some(0)", timeout=120)
        self.quit(ide)

    # --- Smart indentation and completion -----------------------------------

    def test_auto_indent_enter_closers_tab_and_shift_tab(self):
        (self.root / "src" / "lib.rs").write_text("")
        ide = self.ide(args=["src/lib.rs"])
        ide.until(b"NORMAL | src/lib.rs")
        ide.type("i")
        for line in ["fn main() {", "if ok {", "run();"]:
            ide.type(line)
            ide.keys(b"\r")
        ide.type("}")
        ide.keys(b"\r")
        ide.type("}")
        ide.keys(b"\x1b[Z")          # Shift+Tab on a line without indentation: no change
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        self.assertEqual((self.root / "src" / "lib.rs").read_text(),
                         "fn main() {\n    if ok {\n        run();\n    }\n}")
        # Normal mode: Tab / Shift+Tab shift the line; u undoes.
        ide.type("gg")
        ide.keys(b"\t")
        ide.type(":w\r")
        ide.until(b"Saved")
        self.assertTrue((self.root / "src" / "lib.rs").read_text().startswith("    fn main() {"))
        ide.type("u:w\r")
        ide.until(b"Saved")
        self.assertTrue((self.root / "src" / "lib.rs").read_text().startswith("fn main() {"))
        self.quit(ide)

    def lsp_completion(self, server, files, path, line, typed, expect_label, expect_text):
        if not shutil.which(server):
            self.skipTest(f"{server} not installed")
        for name, text in files.items():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        ide = self.ide(args=[path])
        ide.until(b"NORMAL |")
        ide.type(f"{line}GA")
        ide.keys(b"\r")
        for ch in typed:
            ide.type(ch)
            ide.pump(0.1)
        ide.until(f"{server} · ".encode(), timeout=60)
        self.assertIn(expect_label.encode(), ide.output[ide.offset - 4000:])
        ide.keys(b"\x1b[B")           # Down selects the first item
        ide.keys(b"\t")               # Tab accepts it
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        self.assertIn(expect_text, (self.root / path).read_text())
        self.quit(ide)

    def test_rust_analyzer_completion(self):
        self.lsp_completion(
            "rust-analyzer",
            {"ra/Cargo.toml": '[package]\nname = "ra"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n',
             "ra/src/main.rs": "fn main() {\n    let values: Vec<u32> = Vec::new();\n}\n"},
            "ra/src/main.rs", 2, "values.le", "len", "values.len")

    def test_clangd_completion(self):
        self.lsp_completion(
            "clangd",
            {"cc/main.c": "#include <string.h>\nint main(void) {\n    return 0;\n}\n"},
            "cc/main.c", 2, "strle", "strlen", "strlen")

    def test_zls_completion(self):
        self.lsp_completion(
            "zls",
            {"zg/main.zig": 'const std = @import("std");\npub fn main() void {\n}\n'},
            "zg/main.zig", 2, "std.me", "mem", "std.mem")

    # --- Panel chords ---------------------------------------------------------

    def test_held_digit_delete_hides_panels_and_reopening_restores_state(self):
        ide = self.ide()
        ide.keys(ide.esc)
        ide.panel("2")
        ide.until(b"ASSISTANT \xc2\xb7 NORMAL")
        ide.type("unsent draft")
        ide.hide("2")
        ide.until(b"AI panel hidden, chat kept")
        ide.panel("2")
        ide.until(b"> unsent draft")
        ide.keys(ide.esc)
        ide.panel("3")
        ide.until(b"> TERMINAL")
        ide.type("printf 'KEEP_%s\\n' ME\r")
        ide.until(b"KEEP_ME")
        ide.type("printf 'PID_%s_PID\\n' $$\r")
        pid = ide.latest(rb"PID_(\d+)_PID").group(1)
        ide.hide("3")
        ide.until(b"Terminal hidden, shell running")
        ide.panel("3")
        ide.until(b"KEEP_ME")  # previous output is restored with the session
        ide.type("printf 'PID_%s_PID\\n' $$\r")
        self.assertEqual(ide.latest(rb"PID_(\d+)_PID").group(1), pid)
        ide.panel("3")
        ide.hide("1")
        ide.until(b"Explorer hidden")
        # Separate presses (released digit, then Ctrl+Alt+Delete) never hide.
        ide.panel("1")
        ide.until(b"> EXPLORER")
        ide.keys(CTRL_ALT_DELETE, CTRL_ALT_DELETE_RELEASE)
        time.sleep(0.3)
        self.assertNotIn(b"CONFIRM", ide.output[ide.offset:], "Ctrl+Alt+Delete opened the delete prompt")
        ide.keys(ide.esc)
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        ide.pump(0.3)
        self.assertIn("notes.txt", ide.screen(), "the explorer was hidden by separate presses")
        self.quit(ide)

    def test_legacy_terminal_switches_panels_with_alt_digits(self):
        ide = self.ide(keyboard="legacy")
        ide.keys(ide.esc)
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        # A legacy terminal sends ESC 1 for both Alt+1 and Ctrl+Alt+1: that is
        # the fallback for the chord it cannot report.
        ide.keys(b"\x1b1")
        ide.until(b"> EXPLORER")
        ide.keys(b"\x1b1")
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        ide.keys(b"\x1b3")
        ide.until(b"> TERMINAL")
        ide.until(b"Alt+3 back to editor")
        ide.type("printf 'LEGACY_%s\\n' OK\r")
        ide.until(b"LEGACY_OK")
        ide.keys(b"\x1b3")
        ide.until(b"\x1b[49mNORMAL | [untitled]")
        # The command fallback hides panels everywhere; the shell keeps running.
        ide.type(":hide_terminal\r")
        ide.until(b"Terminal hidden, shell running (Alt+3")
        ide.keys(b"\x1b3")
        ide.until(b"> TERMINAL")
        ide.keys(b"exit\r")
        ide.until(b"shell ended")
        ide.keys(ide.esc)
        ide.keys(ide.ctrl("q"))
        self.assertEqual(ide.finish(), 0)

    # --- Editor state across panels, save and reopen ----------------------------

    def test_unsaved_edits_survive_panel_switching_then_save_and_reopen(self):
        (self.root / "main.c").write_text("int main(void) {\n    return 0;\n}\n")
        ide = self.ide(args=["main.c"])
        ide.until(b"NORMAL | main.c | c |")
        ide.keys(b"O")
        ide.type("// edited")
        ide.keys(ide.esc)
        ide.until(b"NORMAL | main.c [+]")
        for digit, marker in (("1", b"> EXPLORER"), ("2", b"ASSISTANT \xc2\xb7"), ("3", b"> TERMINAL")):
            ide.panel(digit)
            ide.until(marker)
        ide.type("printf 'RAN_%s\\n' CMD\r")
        ide.until(b"RAN_CMD")
        for digit in "123":
            ide.hide(digit)
        # Hiding panels never closes documents or drops unsaved changes.
        ide.latest(rb"NORMAL \| main\.c \[\+\][^\x1b]*Terminal hidden")
        self.assertEqual((self.root / "main.c").read_text(), "int main(void) {\n    return 0;\n}\n")
        # Quitting with unsaved changes asks first; cancelling keeps the edit.
        ide.keys(ide.ctrl("q"))
        ide.until(b"Discard ALL unsaved documents")
        ide.keys(b"n")
        ide.until(b"NORMAL | main.c [+]")
        ide.type(":w\r")
        ide.until(b"Saved")
        self.quit(ide)
        self.assertEqual((self.root / "main.c").read_text(),
                         "// edited\nint main(void) {\n    return 0;\n}\n")
        ide = self.ide(args=["main.c"])
        ide.until(b"// edited")
        ide.until(b"NORMAL | main.c | c |")
        self.quit(ide)

    # --- embed <path> -------------------------------------------------------------

    def test_positional_files_directories_and_new_files(self):
        names = ["file with spaces.cpp", "файл ядра.c"]
        for name in names:
            (self.root / name).write_text("int x;\n")
        for name in names:
            ide = self.ide(args=[name])
            ide.until(f"NORMAL | {name}".encode())
            self.quit(ide)
            self.assertEqual((self.root / name).read_text(), "int x;\n", "opening modified a file")
        outside = Path(self.directory.name) / "elsewhere" / "kernel"
        outside.mkdir(parents=True)
        (outside / "main.c").write_text("int k;\n")
        ide = self.ide(args=[outside / "main.c"])  # absolute path outside cwd
        ide.until(b"| kernel |")  # workspace = the file's directory
        ide.until(b"NORMAL | main.c")
        self.quit(ide)
        project = self.root / "my-project"
        project.mkdir()
        (project / "only-here.txt").write_text("x\n")
        ide = self.ide(args=["./my-project/"])
        ide.until(b"| my-project |")
        ide.until(b"only-here.txt")
        self.quit(ide)
        # A missing file in an existing directory becomes a new buffer, written on :w.
        ide = self.ide(args=["new.rs"])
        ide.until(b"New file")
        self.assertFalse((self.root / "new.rs").exists())
        ide.keys(b"i")
        ide.type("fn main() {}")
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        self.quit(ide)
        self.assertEqual((self.root / "new.rs").read_text(), "fn main() {}")
        # A missing parent directory is reported before the TUI starts.
        failed = subprocess.run([str(driver.BINARY), "no/such/dir/x.c"], cwd=self.root, env=self.env,
                                capture_output=True, text=True)
        self.assertEqual(failed.returncode, 1)
        self.assertIn("parent directory", failed.stderr)

    def test_launch_sh_is_highlighted_not_plain(self):
        (self.root / "launch.sh").write_text(
            '#!/usr/bin/env bash\n# build\nset -e\nDIR="${1:-build}"\n'
            'if [[ -d "$DIR" ]]; then\n    echo "in $DIR" >&2\nfi\nexit 0\n')
        self.env["COLORTERM"] = "truecolor"
        ide = self.ide(args=["launch.sh"])
        ide.until(b"NORMAL | launch.sh | shell |")
        colours = set(re.findall(rb"\x1b\[38;2;(\d+;\d+;\d+)m", bytes(ide.output)))
        self.assertGreaterEqual(len(colours), 6, colours)
        # Editing and saving still work with highlighting on.
        ide.keys(b"G", b"o")
        ide.type("echo done")
        ide.keys(ide.esc)
        ide.type(":w\r")
        ide.until(b"Saved")
        self.quit(ide)
        saved = (self.root / "launch.sh").read_text()
        self.assertIn("exit 0\n", saved)
        self.assertIn("echo done", saved)

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    args, remaining = parser.parse_known_args()
    driver.BINARY = args.binary.resolve(strict=True)
    result = unittest.main(argv=[__file__, *remaining], exit=False, verbosity=2).result
    raise SystemExit(0 if result.wasSuccessful() else 1)
