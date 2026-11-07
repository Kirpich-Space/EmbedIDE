#!/usr/bin/env python3
"""Plays a serial device for tests (POSIX). A pseudo-terminal stands in for a
USB-UART; a symlink gives it a stable name so "unplug" and "replug" can be
simulated. Line commands on stdin, replies on stdout:

  new <link>   create a pty, point <link> at its slave  -> "ready <slave>"
  send <hex>   device transmits bytes                    -> "sent"
  read         bytes received from the host so far       -> "got <hex>"
  vanish       close the pty and remove <link>           -> "gone"
  quit
"""
import os
import select
import sys
import tty

master = slave = None
link = None
received = b""


def drain():
    global received
    if master is None:
        return
    while True:
        r, _, _ = select.select([master], [], [], 0)
        if not r:
            return
        try:
            data = os.read(master, 4096)
        except OSError:
            return
        if not data:
            return
        received += data


def vanish():
    global master, slave
    for fd in (master, slave):
        if fd is not None:
            try:
                os.close(fd)
            except OSError:
                pass
    master = slave = None
    if link and os.path.lexists(link):
        os.unlink(link)


for line in sys.stdin:
    parts = line.strip().split(" ", 1)
    cmd = parts[0]
    arg = parts[1] if len(parts) > 1 else ""
    if cmd == "new":
        vanish()
        link = arg
        master, slave = os.openpty()
        tty.setraw(master)
        name = os.ttyname(slave)
        if os.path.lexists(link):
            os.unlink(link)
        os.symlink(name, link)
        print("ready", name, flush=True)
    elif cmd == "send":
        os.write(master, bytes.fromhex(arg))
        print("sent", flush=True)
    elif cmd == "read":
        drain()
        print("got", received.hex(), flush=True)
    elif cmd == "vanish":
        drain()
        vanish()
        print("gone", flush=True)
    elif cmd == "quit":
        break
vanish()
