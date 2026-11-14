#!/usr/bin/env python3
"""Technical demonstration: one real session on a real project (1920x1080 layout, 118x31 terminal).

Edit -> build error -> jump -> fix -> build -> run in the integrated terminal -> real Codex CLI agent adds
a function and builds it -> /review -> rebuild -> run. Nothing is faked: the recording is the program's
own output with real timestamps. Needs Codex CLI logged in.
"""
from common import *   # noqa

import time

ESC = b"\x1b[27u"
F3, F5 = b"\x1bOR", b"\x1b[15~"
DOWN, ENTER = b"\x1b[B", b"\r"


def main():
    root = make_project()
    cfg = fresh_cfg("wide")
    rec = T.Recorder([BINARY], root, env(cfg), 31, 118)
    try:
        rec.until("EmbedIDE")
        rec.pump(2.0)
        rec.mark("start")
        # Explorer: expand src, open ring.c
        rec.keys(ENTER, pause=0.9)       # `src` is selected: expand it
        rec.keys(DOWN, pause=0.5)
        rec.keys(DOWN, pause=0.5)
        rec.keys(ENTER, pause=1.0)
        rec.until("ring_init")
        rec.pump(1.5)
        rec.mark("opened")
        # Build: expect an error
        rec.keys(F5, pause=0.5)
        rec.until("Finished:", 60)
        rec.pump(2.5)
        rec.mark("build-error")
        rec.keys(F3, pause=1.5)          # problems list
        rec.mark("problems")
        rec.keys(ENTER, pause=1.5)       # jump to the error
        rec.mark("jumped")
        rec.pump(1.5)
        # Fix: append the missing semicolon at the end of the line, save
        rec.keys("A", pause=0.5)
        rec.type(";", pause=0.2)
        rec.keys(ESC, pause=0.5)
        rec.type(":w", pause=0.15)
        rec.keys(ENTER, pause=1.0)
        rec.mark("fixed")
        rec.keys(F5, pause=0.5)
        rec.until("Finished: exit=Some(0)", 60)
        rec.pump(2.5)
        rec.mark("build-ok")
        # Integrated terminal: run the program
        rec.keys(b"\x1b3", pause=1.2)
        rec.type("./build/debug/ringbuf", pause=0.07)
        rec.keys(ENTER, pause=1.2)
        rec.until("popped 50")
        rec.pump(2.0)
        rec.mark("ran")
        rec.keys(b"\x1b3", pause=1.0)    # back to the editor
        # AI panel and the real agent
        rec.keys(b"\x1b2", pause=1.0)
        rec.type("/provider codex-cli", pause=0.06)
        rec.keys(ENTER, pause=0.5)
        rec.until("logged in", 40)
        rec.pump(1.5)
        rec.mark("provider")
        task = ("Add ring_peek(const ring_t *r, int *out) to ring.c and ring.h: return the oldest element "
                "without removing it. Call it once from main.c before the pop loop and print the result. "
                "Build with `embed build .` and run ./build/debug/ringbuf.")
        rec.type(task, pause=0.028)
        rec.keys(ENTER, pause=0.3)
        rec.mark("agent-start")
        handled = 0
        deadline = time.monotonic() + 420
        while "done" not in rec.text() and "finished with problems" not in rec.text() and "stopped" not in rec.text():
            if time.monotonic() > deadline:
                raise RuntimeError("agent did not finish:\n" + rec.text())
            seen = rec.text().count("Approval needed:")
            if seen > handled:
                handled = seen
                rec.pump(3.0)
                rec.keys("y", pause=0.5)
            rec.pump(0.5)
        rec.pump(2.5)
        rec.mark("agent-done")
        rec.type("/review", pause=0.08)
        rec.keys(ENTER, pause=0.5)
        rec.pump(4.0)
        rec.mark("review")
        rec.keys(ESC, pause=0.6)         # back to the editor (buffer reloaded)
        rec.pump(2.0)
        rec.keys(F5, pause=0.5)
        t_end = time.monotonic() + 60
        while "Up to date" not in rec.text() and "Finished:" not in rec.text():   # the agent already built it
            assert time.monotonic() < t_end, rec.text()
            rec.pump(0.2)
        rec.pump(2.5)
        rec.mark("rebuilt")
        rec.keys(b"\x1b3", pause=1.0)
        rec.type("./build/debug/ringbuf", pause=0.07)
        rec.keys(ENTER, pause=1.2)
        rec.until("popped 50")
        rec.pump(3.0)
        rec.mark("ran-again")
        print("events:", [(round(t, 1), n) for t, n in rec.events])
        print("last screen:\n" + rec.text())
    finally:
        rec.save(OUT / "demo_wide.pkl")
        rec.close()
        print("recorded", len(rec.frames), "screen states in", round(rec.now(), 1), "s")


if __name__ == "__main__":
    main()
