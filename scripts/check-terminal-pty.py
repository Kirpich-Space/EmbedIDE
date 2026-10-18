#!/usr/bin/env python3
"""Unix PTY smoke tests for the actual terminal frontend; no human input needed.

The frontend deliberately uses cooked mode. These tests verify actual TTY input,
resize tolerance and unchanged termios on exit, errors and SIGTERM. They do not
claim to validate a full-screen/raw TUI. Uses disposable files and a private PTY.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import signal
import statistics
import struct
import subprocess
import tempfile
import termios
import time
import unittest

BINARY = None


class Terminal:
    def __init__(self, path, *, line=True, cli=False, workspace=False):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.output = bytearray()
        self.offset = 0
        args = [str(BINARY)] + (["open"] if workspace else ["edit"] if cli else []) + (["--line"] if line else []) + ["--", str(path)]
        environment = os.environ.copy()
        if not line:
            environment["TERM"] = "xterm-256color"
        self.process = subprocess.Popen(args,
                                        stdin=self.slave, stdout=self.slave, stderr=self.slave, env=environment,
                                        start_new_session=True)

    def pump(self, timeout):
        if select.select([self.master], [], [], timeout)[0]:
            self.output.extend(os.read(self.master, 65536))
            if len(self.output) > 1024 * 1024:
                raise AssertionError("unexpected terminal output flood")

    def until(self, marker):
        deadline = time.monotonic() + 10
        while marker not in self.output[self.offset:]:
            if self.process.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"missing {marker!r}: {bytes(self.output)!r}")
            self.pump(0.1)
        self.offset = self.output.index(marker, self.offset) + len(marker)

    def send(self, keys):
        os.write(self.master, keys)

    def finish(self):
        deadline = time.monotonic() + 10
        while self.process.poll() is None:
            if time.monotonic() >= deadline:
                raise AssertionError(f"terminal did not exit: {bytes(self.output)!r}")
            self.pump(0.1)
        while select.select([self.master], [], [], 0)[0]:
            self.pump(0)
        if termios.tcgetattr(self.slave) != self.before:
            raise AssertionError("frontend changed terminal attributes")
        return self.process.returncode

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        # Restore only our private fixture PTY, also on an assertion failure.
        termios.tcsetattr(self.slave, termios.TCSANOW, self.before)
        os.close(self.master)
        os.close(self.slave)


class TerminalContracts(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="embedide-pty-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "source 猫.c"
        self.path.write_text("old", encoding="utf-8")

    def terminal(self):
        terminal = Terminal(self.path)
        self.addCleanup(terminal.close)
        terminal.until(b"keys> ")
        return terminal

    def test_actual_escape_save_quit_and_termios(self):
        terminal = self.terminal()
        terminal.send("ciwновое\x1b\n:wq\n".encode())
        self.assertEqual(terminal.finish(), 0)
        self.assertEqual(self.path.read_text(encoding="utf-8"), "новое")

    def test_dirty_quit_recovery_and_resize(self):
        terminal = self.terminal()
        terminal.send(b"A!<Esc>\n:q\n")
        terminal.until(b"unsaved changes")
        self.assertIsNone(terminal.process.poll())
        fcntl.ioctl(terminal.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 45, 0, 0))
        terminal.send(b"<Esc>:q!\n")
        self.assertEqual(terminal.finish(), 0)
        self.assertEqual(self.path.read_text(), "old")

    def test_eof_reports_unsaved_changes_without_terminal_damage(self):
        terminal = self.terminal()
        terminal.send(b"A!<Esc>\n")
        terminal.until(b"keys> ")
        terminal.send(b"\x04")
        self.assertEqual(terminal.finish(), 1)
        self.assertIn(b"input ended with unsaved changes", terminal.output)
        self.assertEqual(self.path.read_text(), "old")

    def test_sigterm_keeps_terminal_modes_and_original_file(self):
        terminal = self.terminal()
        terminal.send(b"A!<Esc>\n")
        terminal.until(b"keys> ")
        terminal.process.send_signal(signal.SIGTERM)
        self.assertEqual(terminal.finish(), -signal.SIGTERM)
        self.assertEqual(self.path.read_text(), "old")


def measure(ready=b"keys> ", quit_keys=b":q\n", line=True):
    """Warm-file launch-to-prompt timing and direct-child Linux idle/RSS stats."""
    measurements = []
    with tempfile.TemporaryDirectory(prefix="embedide-terminal-bench-") as directory:
        path = Path(directory) / "source.c"
        for size in (0, 1024 * 1024, 10 * 1024 * 1024):
            # Allocate/write outside the timed region. Includes actual open/read/index/render.
            source = (b"0123456789abcdef\n" * (size // 17) + b"x" * (size % 17))
            path.write_bytes(source)
            samples = []
            for _ in range(5):
                start = time.perf_counter_ns()
                terminal = Terminal(path, line=line)
                try:
                    terminal.until(ready)
                    samples.append((time.perf_counter_ns() - start) / 1_000_000)
                    terminal.send(quit_keys)
                    if terminal.finish() != 0:
                        raise AssertionError(bytes(terminal.output))
                finally:
                    terminal.close()
            measurements.append({"bytes": size, "startup_ms_min": min(samples),
                                 "startup_ms_median": statistics.median(samples), "startup_ms_max": max(samples)})
        path.write_bytes(b"")
        terminal = Terminal(path, line=line)
        try:
            terminal.until(ready)
            proc = Path("/proc") / str(terminal.process.pid)

            def cpu_ticks():
                fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
                return int(fields[11]) + int(fields[12])

            before = cpu_ticks()
            time.sleep(0.5)
            idle_ticks = cpu_ticks() - before
            rss = next(int(line.split()[1]) for line in (proc / "status").read_text().splitlines() if line.startswith("VmRSS:"))
            terminal.send(quit_keys)
            if terminal.finish() != 0:
                raise AssertionError(bytes(terminal.output))
        finally:
            terminal.close()
    print(json.dumps({"binary": str(BINARY), "sha256": hashlib.sha256(BINARY.read_bytes()).hexdigest(),
                      "samples_per_size": 5, "measurements": measurements, "idle_window_ms": 500,
                      "idle_cpu_ticks": idle_ticks, "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
                      "empty_document_rss_kib": rss}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--measure", action="store_true", help="also measure warm startup and Linux /proc idle CPU/RSS")
    args, remaining = parser.parse_known_args()
    BINARY = args.binary.resolve(strict=True)
    result = unittest.main(argv=[__file__, *remaining], exit=False).result
    if not result.wasSuccessful():
        raise SystemExit(1)
    if args.measure:
        measure()
