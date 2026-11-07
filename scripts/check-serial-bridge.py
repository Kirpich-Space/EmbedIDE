#!/usr/bin/env python3
"""Contract checks for `embedide serial-bridge` against a real OS serial device.

Unix only: a pseudo-terminal pair stands in for a USB-UART. The bridge opens
the pty slave through the same serialport code as a hardware port; the test
plays the device on the master side. No hardware is touched.

    python3 scripts/check-serial-bridge.py --binary target/debug/embedide
"""
import argparse
import json
import os
import select
import subprocess
import sys
import time
import tty
import unittest

BINARY = None


class Bridge:
    def __init__(self):
        self.p = subprocess.Popen([BINARY, "serial-bridge"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.buf = b""

    def send(self, obj):
        self.p.stdin.write((json.dumps(obj) + "\n").encode())
        self.p.stdin.flush()

    def event(self, timeout=5.0, want=None):
        deadline = time.time() + timeout
        while True:
            while b"\n" in self.buf:
                line, self.buf = self.buf.split(b"\n", 1)
                ev = json.loads(line)
                if want is None or ev.get("event") in want:
                    return ev
            left = deadline - time.time()
            if left <= 0:
                raise AssertionError(f"no event {want} within {timeout}s")
            r, _, _ = select.select([self.p.stdout], [], [], left)
            if r:
                chunk = os.read(self.p.stdout.fileno(), 65536)
                if not chunk:
                    raise AssertionError("bridge exited")
                self.buf += chunk

    def collect(self, n, timeout=5.0):
        data = b""
        deadline = time.time() + timeout
        while len(data) < n:
            ev = self.event(max(0.05, deadline - time.time()), want={"data", "disconnected", "error"})
            if ev["event"] != "data":
                raise AssertionError(f"unexpected {ev}")
            data += bytes.fromhex(ev["hex"])
        return data

    def close(self):
        try:
            self.p.stdin.close()
        except Exception:
            pass
        try:
            self.p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.p.kill()
            raise
        finally:
            for f in (self.p.stdout, self.p.stderr):
                try:
                    f.close()
                except Exception:
                    pass


def pty_pair():
    master, slave = os.openpty()
    tty.setraw(master)
    name = os.ttyname(slave)
    return master, slave, name


def read_master(fd, n, timeout=5.0):
    out = b""
    deadline = time.time() + timeout
    while len(out) < n and time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            out += os.read(fd, 4096)
    return out


class SerialBridgeContracts(unittest.TestCase):
    def test_list_reports_ports_event(self):
        b = Bridge()
        try:
            b.send({"op": "list"})
            ev = b.event(want={"ports", "error"})
            self.assertEqual(ev["event"], "ports")
            self.assertIsInstance(ev["ports"], list)
        finally:
            b.close()

    def test_binary_both_directions_and_split_utf8(self):
        master, slave, name = pty_pair()
        b = Bridge()
        try:
            b.send({"op": "open", "config": {"port": name, "baud": 115200}})
            self.assertEqual(b.event(want={"opened", "error"})["event"], "opened")
            outbound = bytes(range(256))
            b.send({"op": "write", "hex": outbound.hex()})
            self.assertEqual(read_master(master, len(outbound)), outbound)
            # device sends invalid UTF-8, NUL, CR, and a 4-byte emoji split across writes
            emoji = "🛰".encode()
            inbound = b"\x00\xff\xfe\r\nOK " + emoji[:2]
            os.write(master, inbound)
            time.sleep(0.05)
            os.write(master, emoji[2:] + "\n".encode())
            got = b.collect(len(inbound) + 3)
            self.assertEqual(got, inbound + emoji[2:] + b"\n")
            self.assertEqual(got.split(b" ", 1)[1].decode(), "🛰\n")
        finally:
            b.close()
            os.close(master)
            os.close(slave)

    def test_open_errors_and_write_without_port(self):
        b = Bridge()
        try:
            b.send({"op": "write", "hex": "00"})
            ev = b.event(want={"error"})
            self.assertIn("not open", ev["message"])
            b.send({"op": "open", "config": {"port": "/dev/embedide-no-such-port", "baud": 115200}})
            ev = b.event(want={"error", "opened"})
            self.assertEqual((ev["event"], ev["op"]), ("error", "open"))
            self.assertIn("embedide-no-such-port", ev["message"])
            b.send({"op": "open", "config": {"port": "/dev/x", "baud": 115200, "parity": "mark"}})
            self.assertEqual(b.event(want={"error"})["op"], "open")
            b.send({"op": "write", "hex": "abc"})
            self.assertEqual(b.event(want={"error"})["op"], "write")
            b.p.stdin.write(b"not json\n")
            b.p.stdin.flush()
            self.assertIn("Invalid JSON", b.event(want={"error"})["message"])
        finally:
            b.close()

    def test_port_disappearance_is_reported_and_reopen_works(self):
        master, slave, name = pty_pair()
        b = Bridge()
        try:
            b.send({"op": "open", "config": {"port": name, "baud": 9600}})
            self.assertEqual(b.event(want={"opened", "error"})["event"], "opened")
            os.close(slave)  # only the bridge holds the slave now
            os.close(master)  # device vanishes (like unplugging a USB-UART)
            ev = b.event(timeout=5, want={"disconnected", "error"})
            self.assertEqual(ev["event"], "disconnected")
            # a new device appears: the same bridge can open it
            m2, s2, n2 = pty_pair()
            try:
                b.send({"op": "open", "config": {"port": n2, "baud": 9600}})
                self.assertEqual(b.event(want={"opened", "error"})["event"], "opened")
                os.write(m2, b"back\n")
                self.assertEqual(b.collect(5), b"back\n")
            finally:
                os.close(m2)
                os.close(s2)
        finally:
            b.close()

    def test_stdin_eof_closes_port_and_exits(self):
        master, slave, name = pty_pair()
        b = Bridge()
        try:
            b.send({"op": "open", "config": {"port": name, "baud": 115200}})
            self.assertEqual(b.event(want={"opened", "error"})["event"], "opened")
            t0 = time.time()
            b.close()
            self.assertEqual(b.p.returncode, 0)
            self.assertLess(time.time() - t0, 3)
        finally:
            os.close(master)
            os.close(slave)

    def test_close_request_reports_closed(self):
        master, slave, name = pty_pair()
        b = Bridge()
        try:
            b.send({"op": "open", "config": {"port": name, "baud": 115200}})
            b.event(want={"opened"})
            b.send({"op": "close"})
            self.assertEqual(b.event(want={"closed"})["event"], "closed")
        finally:
            b.close()
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    args, rest = ap.parse_known_args()
    BINARY = os.path.abspath(args.binary)
    if os.name != "posix":
        print("SKIP: pseudo-terminals are POSIX-only")
        sys.exit(0)
    unittest.main(argv=[sys.argv[0], "-v", *rest])
