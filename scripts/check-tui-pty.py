#!/usr/bin/env python3
"""Actual full-screen PTY workflows, without Enter for editing keys.

Use --cli to exercise `embedide edit` instead of the standalone frontend.
The fixture terminal checks exact termios restoration and cleanup sequences.
"""
import argparse
import fcntl
import importlib.util
from pathlib import Path
import signal
import struct
import tempfile
import termios
import unittest

spec = importlib.util.spec_from_file_location("line_pty", Path(__file__).with_name("check-terminal-pty.py"))
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
CLI = False


class FullscreenContracts(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="embedide-tui-pty-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "source 猫.c"
        self.path.write_text("old", encoding="utf-8")

    def terminal(self):
        terminal = driver.Terminal(self.path, line=False, cli=CLI)
        self.addCleanup(terminal.close)
        terminal.until(b"Normal ")
        attributes = termios.tcgetattr(terminal.slave)
        self.assertFalse(attributes[3] & termios.ICANON)
        self.assertFalse(attributes[3] & termios.ECHO)
        self.assertIn(b"\x1b[?1049h", terminal.output)
        return terminal

    def escape(self, terminal):
        terminal.send(b"\x1b")
        terminal.until(b"Normal ")

    def finish(self, terminal, expected=0):
        self.assertEqual(terminal.finish(), expected, bytes(terminal.output))
        self.assertIn(b"\x1b[?25h", terminal.output)
        self.assertIn(b"\x1b[?1049l", terminal.output)
        self.assertIn(b"\x1b[?2004l", terminal.output)

    def test_immediate_keys_unicode_save_and_reopen(self):
        terminal = self.terminal()
        terminal.send(b"ciw")
        terminal.until(b"Insert ")
        terminal.send("новое猫".encode())
        terminal.until("новое猫".encode())
        self.assertEqual(self.path.read_text(), "old")
        self.escape(terminal)
        terminal.send(b":wq\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "новое猫")
        # A second actual process reads the saved file.
        terminal = self.terminal()
        self.assertIn("новое猫".encode(), terminal.output)
        terminal.send(b":q\r")
        self.finish(terminal)

    def test_registers_delete_paste_undo_redo_and_search(self):
        self.path.write_text("one\ntwo\nthree")
        terminal = self.terminal()
        terminal.send(b"jyypdd")
        terminal.send(b"u\x12/two\rciw")
        terminal.until(b"Insert ")
        terminal.send(b"new")
        self.escape(terminal)
        terminal.send(b":wq\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "one\nnew\nthree")

    def test_dirty_quit_is_recoverable_and_force_quit_is_explicit(self):
        terminal = self.terminal()
        terminal.send(b"iX")
        terminal.until(b"Insert ")
        self.escape(terminal)
        terminal.send(b":q\r")
        terminal.until(b"unsaved changes")
        self.assertIsNone(terminal.process.poll())
        self.escape(terminal)
        terminal.send(b":q!\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "old")

    def test_external_edit_and_failed_writequit_keep_disk_and_session(self):
        terminal = self.terminal()
        terminal.send(b"A!")
        terminal.until(b"Insert ")
        self.escape(terminal)
        self.path.write_text("external")
        terminal.send(b":wq\r")
        terminal.until(b"changed externally")
        self.assertIsNone(terminal.process.poll())
        self.escape(terminal)
        terminal.send(b":q!\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "external")

    def test_readonly_save_error_remains_recoverable(self):
        self.path.chmod(0o444)
        terminal = self.terminal()
        terminal.send(b"A!")
        terminal.until(b"Insert ")
        self.escape(terminal)
        terminal.send(b":wq\r")
        terminal.until(b"read-only")
        self.assertIsNone(terminal.process.poll())
        self.escape(terminal)
        terminal.send(b":q!\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "old")

    def test_ctrl_c_cancels_insert_without_quitting_or_losing_data(self):
        terminal = self.terminal()
        terminal.send(b"iHello\x03")
        terminal.until(b"Interrupted;")
        self.assertIsNone(terminal.process.poll())
        terminal.send(b":q!\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "old")

    def test_resize_scroll_and_crlf_content(self):
        source = ("αβ猫" * 30 + "\r\n") * 100
        self.path.write_bytes(source.encode())
        terminal = self.terminal()
        terminal.send(b"i")
        terminal.until(b"Insert ")
        self.escape(terminal)
        fcntl.ioctl(terminal.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 9, 32, 0, 0))
        terminal.process.send_signal(signal.SIGWINCH)
        terminal.until(b"\x1b[8;1H")
        terminal.send(b"99j40l:q\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_bytes(), source.encode())

    def test_bracketed_paste_never_executes_normal_commands(self):
        self.path.write_text("")
        terminal = self.terminal()
        terminal.send(b"\x1b[200~:q!dd\x1b[201~i")
        terminal.until(b"Insert ")
        text = ":q!\n猫"
        terminal.send(b"\x1b[200~" + text.encode() + b"\x1b[201~")
        terminal.until(b":q!")
        self.escape(terminal)
        terminal.send(b":wq\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), text)

    def test_unknown_key_does_not_corrupt_document(self):
        terminal = self.terminal()
        terminal.send(b"\x1b[24~")
        terminal.until(b"Unsupported key")
        terminal.send(b":q\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "old")

    def test_invalid_ex_command_is_recoverable_and_never_runs_a_shell(self):
        terminal = self.terminal()
        marker = Path(self.directory.name) / "marker"
        terminal.send(f":!touch {marker}\r".encode())
        terminal.until(b"Error:")
        self.assertIsNone(terminal.process.poll())
        self.assertFalse(marker.exists())
        self.escape(terminal)
        terminal.send(b":q\r")
        self.finish(terminal)
        self.assertEqual(self.path.read_text(), "old")

    def test_open_failure_never_enters_raw_or_alternate_screen(self):
        terminal = driver.Terminal(Path(self.directory.name) / "absent" / "file.c", line=False, cli=CLI)
        self.addCleanup(terminal.close)
        self.assertEqual(terminal.finish(), 1)
        self.assertNotIn(b"\x1b[?1049h", terminal.output)

    def test_external_termination_signals_restore_terminal(self):
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            with self.subTest(signal=sig):
                terminal = self.terminal()
                terminal.send(b"iUnsaved")
                terminal.until(b"Insert ")
                terminal.process.send_signal(sig)
                self.finish(terminal, 1)
                self.assertEqual(self.path.read_text(), "old")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--cli", action="store_true")
    parser.add_argument("--measure", action="store_true")
    args, remaining = parser.parse_known_args()
    driver.BINARY = args.binary.resolve(strict=True)
    CLI = args.cli
    result = unittest.main(argv=[__file__, *remaining], exit=False).result
    if not result.wasSuccessful():
        raise SystemExit(1)
    if args.measure:
        if CLI:
            parser.error("measure the standalone binary without --cli")
        driver.measure(ready=b"Normal ", quit_keys=b":q\r", line=False)
