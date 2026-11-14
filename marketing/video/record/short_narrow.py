#!/usr/bin/env python3
"""Vertical (Shorts) footage: the same program in a narrow 72x46 terminal; real build error -> fix -> Codex agent."""
from common import *   # noqa
import time

ESC, ENTER, F3, F5 = b"\x1b[27u", b"\r", b"\x1bOR", b"\x1b[15~"


def main():
    root = make_project("ringbuf-short", broken=True)
    cfg = fresh_cfg("short")
    rec = T.Recorder([BINARY], root, env(cfg), 46, 72)
    try:
        rec.until("EmbedIDE")
        rec.pump(1.5)
        rec.keys(ESC, pause=0.4)
        rec.type(":e src/ring.c", pause=0.06)
        rec.keys(ENTER, pause=1.0)
        rec.until("ring_init")
        rec.pump(1.2)
        rec.mark("opened")
        rec.keys(F5, pause=0.5)
        rec.until("Finished:", 60)
        rec.pump(2.0)
        rec.mark("error")
        rec.keys(F3, pause=1.2)
        rec.keys(ENTER, pause=1.5)
        rec.keys("A", pause=0.4)
        rec.type(";", pause=0.2)
        rec.keys(ESC, pause=0.4)
        rec.type(":w", pause=0.15)
        rec.keys(ENTER, pause=0.8)
        rec.keys(F5, pause=0.5)
        rec.until("Finished: exit=Some(0)", 60)
        rec.pump(2.0)
        rec.mark("built")
        rec.keys(b"\x1b2", pause=1.0)
        rec.type("/provider codex-cli", pause=0.05)
        rec.keys(ENTER, pause=0.5)
        rec.until("logged in", 40)
        rec.pump(1.0)
        rec.type("Add ring_peek(const ring_t *r, int *out) returning the oldest item without removing it; "
                 "print it from main.c; build with `embed build .` and run ./build/debug/ringbuf.", pause=0.02)
        rec.keys(ENTER, pause=0.3)
        rec.mark("agent-start")
        handled = 0
        deadline = time.monotonic() + 420
        while not any(k in rec.text() for k in ("■ done", "□ finished", "□ stopped", "■ stopped")):
            if time.monotonic() > deadline:
                raise RuntimeError("agent did not finish\n" + rec.text())
            seen = rec.text().count("Approval needed:")
            if seen > handled:
                handled = seen
                rec.pump(3.0)
                rec.keys("y", pause=0.5)
            rec.pump(0.5)
        rec.pump(3.0)
        rec.mark("agent-done")
        print([(round(t, 1), n) for t, n in rec.events])
        print(rec.text())
    finally:
        rec.save(OUT / "short_narrow.pkl")
        rec.close()


if __name__ == "__main__":
    main()
