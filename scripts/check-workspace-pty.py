#!/usr/bin/env python3
"""Project workspace PTY tests. All hardware commands use disposable mocks only."""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import signal
import shutil
import struct
import tempfile
import termios
import time
import unittest

spec = importlib.util.spec_from_file_location("line_pty", Path(__file__).with_name("check-terminal-pty.py"))
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
MEASURE = False


class WorkspaceContracts(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="embedide-workspace-猫 ")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "main.c").write_text("one\ntwo\nthree", encoding="utf-8")
        (self.root / "other.c").write_text("second", encoding="utf-8")
        (self.root / "Makefile").write_text("all:\n\t@echo real make\n")
        compiler = self.root / "fake compiler"
        compiler.write_text("#!/bin/sh\nprintf 'phase-one\\n'\nsleep 0.4\nprintf 'main.c:2:1: error: fixture compiler failure\\n'\nexit 2\n")
        compiler.chmod(0o700)
        (self.root / "embedide.json").write_text(json.dumps({"version": 1, "native": {"tools": {"make": str(compiler)}}}))

    def terminal(self):
        terminal = driver.Terminal(self.root, line=False, workspace=True)
        self.addCleanup(terminal.close)
        terminal.until(b"EmbedIDE 2.0")
        return terminal

    def command(self, terminal, query):
        terminal.send(b"\x0b")
        terminal.until(b"COMMANDS >")
        terminal.send(query.encode() + b"\r")

    def open_file(self, terminal, name):
        terminal.send(b"\x10")
        terminal.until(b"COMMANDS > file:")
        terminal.send(name.encode() + b"\r")
        terminal.until(("Normal " + name).encode())

    def finish(self, terminal):
        terminal.send(b"\x11")
        self.assertEqual(terminal.finish(), 0, bytes(terminal.output))
        for sequence in (b"\x1b[?25h", b"\x1b[?1049l", b"\x1b[?2004l"):
            self.assertIn(sequence, terminal.output)

    def test_explorer_palette_multiple_documents_and_save(self):
        terminal = self.terminal()
        self.open_file(terminal, "main.c")
        terminal.send("i猫".encode() + b"\x1b\x13")
        terminal.until(b"Saved")
        self.open_file(terminal, "other.c")
        terminal.send(b"\x1b[21~")  # F10 switches to previous document.
        terminal.until(b">main.c")
        self.finish(terminal)
        self.assertEqual((self.root / "main.c").read_text(), "猫one\ntwo\nthree")

    def test_background_build_preserves_immediate_editing_and_problem_jump(self):
        terminal = self.terminal()
        self.open_file(terminal, "main.c")
        terminal.send(b"\x1b[15~")  # F5
        terminal.until(b"phase-one")
        edit_started = time.perf_counter_ns()
        terminal.send(b"iLIVE\x1b\x13")
        terminal.until(b"Saved")
        if MEASURE:
            print(json.dumps({"edit_and_save_during_build_ms": (time.perf_counter_ns() - edit_started) / 1e6}))
        terminal.until(b"Finished: exit=Some(2)")
        self.command(terminal, "Show Problems")
        terminal.until(b"fixture compiler failure")
        terminal.send(b"\r")
        terminal.until(b"2:1")
        self.finish(terminal)
        self.assertEqual((self.root / "main.c").read_text(), "LIVEone\ntwo\nthree")

    def test_resize_hidden_panels_and_dirty_workspace_quit_protection(self):
        terminal = self.terminal()
        self.open_file(terminal, "main.c")
        terminal.send(b"iUNSAVED\x1b")
        terminal.until(b"Normal ")
        for rows, columns in [(7, 25), (2, 2), (28, 110)]:
            fcntl.ioctl(terminal.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
            terminal.process.send_signal(signal.SIGWINCH)
            terminal.pump(0.3)
            self.assertIsNone(terminal.process.poll())
        self.command(terminal, "Keyboard Help")
        terminal.until(b"HELP")
        terminal.send(b"\x11")
        terminal.until(b"Discard ALL")
        terminal.send(b"\x1b")
        terminal.until(b"Cancelled")
        terminal.send(b"\x11y")
        self.assertEqual(terminal.finish(), 0)
        self.assertEqual((self.root / "main.c").read_text(), "one\ntwo\nthree")

    def test_cancellation_and_signal_cleanup_terminate_build_descendants(self):
        compiler = self.root / "fake compiler"
        compiler.write_text("#!/bin/sh\nsleep 30 &\nprintf '%s' $! > child.pid\nprintf 'waiting-for-cancel\\n'\nwait\n")
        terminal = self.terminal()
        terminal.send(b"\x1b[15~")
        terminal.until(b"waiting-for-cancel")
        terminal.send(b"\x1b[19~")  # F8
        terminal.until(b"Finished: exit=None")
        child = int((self.root / "child.pid").read_text())
        proc = Path(f"/proc/{child}/stat")
        if proc.exists():
            self.assertEqual(proc.read_text().split()[2], "Z", "descendant is still running")
        self.finish(terminal)

    @unittest.skipUnless(shutil.which("cargo"), "Cargo is not installed")
    def test_real_cargo_build_and_structured_diagnostic_navigation(self):
        (self.root / "Cargo.toml").write_text('[package]\nname="pty_firmware_logic"\nversion="0.1.0"\nedition="2021"\n[workspace]\n')
        (self.root / "src").mkdir()
        source = self.root / "src/lib.rs"
        source.write_text("#![no_std]\npub fn tick(n: u32) -> u32 { n.wrapping_add(1) }\n")
        (self.root / "embedide.json").write_text(json.dumps({"version": 1, "buildSystem": "cargo"}))
        terminal = self.terminal()
        terminal.send(b"\x1b[15~")
        terminal.until(b"Finished: exit=Some(0)")
        source.write_text('pub fn tick() -> u32 { "type mismatch" }\n')
        terminal.send(b"\x1b[18~")  # F7 Cargo check.
        terminal.until(b"Finished: exit=Some(101)")
        self.command(terminal, "Show Problems")
        terminal.until(b"mismatched types")
        terminal.send(b"\r")
        terminal.until(b"Normal lib.rs")
        self.finish(terminal)

    def test_flashing_without_target_reports_configuration_error(self):
        terminal = self.terminal()
        self.command(terminal, "Flash Firmware")
        terminal.until(b"Configure and select a target")
        self.finish(terminal)

    def test_signal_restores_terminal_with_active_build(self):
        compiler = self.root / "fake compiler"
        compiler.write_text("#!/bin/sh\nprintf 'long-build\\n'\nsleep 30\n")
        terminal = self.terminal()
        terminal.send(b"\x1b[15~")
        terminal.until(b"long-build")
        terminal.process.send_signal(signal.SIGTERM)
        self.assertEqual(terminal.finish(), 1)
        self.assertIn(b"\x1b[?1049l", terminal.output)


def measure(binary):
    with tempfile.TemporaryDirectory(prefix="embedide-workspace-bench-") as temporary:
        root = Path(temporary)
        for index in range(100):
            folder = root / f"dir{index:03d}"
            folder.mkdir()
            for file in range(100):
                (folder / f"file{file:03d}.c").touch()
        samples = []
        for _ in range(10):
            start = time.perf_counter_ns()
            terminal = driver.Terminal(root, line=False, workspace=True)
            try:
                terminal.until(b"EmbedIDE 2.0")
                samples.append((time.perf_counter_ns() - start) / 1e6)
                terminal.send(b"\x11")
                assert terminal.finish() == 0
            finally:
                terminal.close()
        terminal = driver.Terminal(root, line=False, workspace=True)
        try:
            terminal.until(b"EmbedIDE 2.0")
            proc = Path(f"/proc/{terminal.process.pid}")
            before = proc.joinpath("stat").read_text().split()
            time.sleep(0.5)
            after = proc.joinpath("stat").read_text().split()
            rss = next(line for line in proc.joinpath("status").read_text().splitlines() if line.startswith("VmRSS:"))
            resize_started = time.perf_counter_ns()
            fcntl.ioctl(terminal.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
            terminal.process.send_signal(signal.SIGWINCH)
            terminal.until(b"\x1b[2J")
            resize_ms = (time.perf_counter_ns() - resize_started) / 1e6
            print(json.dumps({"resize_to_redraw_ms": resize_ms, "workspace_files": 10000, "root_entries": 100, "startup_ms": sorted(samples), "rss": rss, "idle_ticks_500ms": sum(map(int, after[13:15])) - sum(map(int, before[13:15])), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK")}, ensure_ascii=False))
            terminal.send(b"\x11")
            assert terminal.finish() == 0
        finally:
            terminal.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--measure", action="store_true")
    args, remaining = parser.parse_known_args()
    driver.BINARY = args.binary.resolve(strict=True)
    MEASURE = args.measure
    result = unittest.main(argv=[__file__, *remaining], exit=False).result
    if not result.wasSuccessful():
        raise SystemExit(1)
    if args.measure:
        measure(driver.BINARY)
