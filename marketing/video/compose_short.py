#!/usr/bin/env python3
"""Video 3 — Short promotional video (1080x1920, ~45 s) from a real narrow-terminal recording."""
from compose_common import *   # noqa
from lib import term as T

V = F.VERTICAL
rec = T.load(OUT / "short_narrow.pkl")
ev = {n: t for t, n in rec["events"]}
film = F.Film(V)


def clip(segs, narr, labels=()):
    return F.Clip(rec, segs, title="ringbuf — embed", narration=narr, labels=labels)


film.add(F.Card(logo_on=True, headline="A native Rust IDE for embedded work", sub="In your terminal.",
                narration="A native Rust IDE for embedded development, in your terminal.", min_dur=4.0), 0.4)
film.add(clip([(1.5, ev["error"] + 2.5, 1.5)], "Build with GCC, and the error is one keypress away.",
              labels=[(0.5, 3.5, "F5 · build")]), 0.3)
film.add(clip([(ev["error"] + 2.5, ev["built"] + 1.0, 1.7)], "Fix it, and build again."), 0.3)
film.add(clip([(ev["built"] + 1.0, ev["agent-start"] + 0.5, 2.4)],
              "Then hand a task to an AI agent. EmbedIDE drives the Claude Code or Codex CLI you already use.",
              labels=[(0.3, 3.5, "Alt+2 · AI panel")]), 0.3)
film.add(clip([(ev["agent-start"] + 0.5, ev["agent-done"] + 1.0, 6.5)],
              "It edits files, builds, and runs your program, and every step is visible."), 0.3)
film.add(clip([(ev["agent-done"] + 1.0, ev["agent-done"] + 3.0, 1)],
              "Review every change, or revert it."), 0.4)
film.add(F.Card(logo_on=True, headline="Open source. Alpha.", sub="Try it, and tell us what breaks.",
                code=["github.com/Kirpich-Space/EmbedIDE"],
                narration="EmbedIDE is open source and in alpha. Try it, and tell us what breaks.", min_dur=6.0,
                note="Linux · FreeBSD (untested) · GUI planned"), 0.0)

TITLE = "EmbedIDE in 45 seconds: a native Rust terminal IDE with AI agents #shorts"
DESC = """EmbedIDE (`embed`) is a native Rust IDE for embedded and systems development that runs in your terminal. Real recording of version 0.1.0-alpha.1: build with GCC, jump to the error, fix it, then let an AI agent (Claude Code / Codex CLI, your own login) add a function, build and run it.

Official Kirpich Space / EmbedIDE video. Alpha: Linux verified, FreeBSD compiles but not yet run, no Windows/macOS, native GUI planned.

https://github.com/Kirpich-Space/EmbedIDE  (GPL-3.0)
Install: curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh

#shorts #rust #embedded #opensource #terminal"""
TAGS = ["EmbedIDE", "Rust", "embedded", "terminal IDE", "TUI", "AI coding agent", "open source", "shorts"]

if __name__ == "__main__":
    final = build(film, "video3_short")
    c = F.Clip(rec, [(ev["error"] + 1.5, ev["error"] + 2.5, 1)], title="ringbuf — embed")
    c.frame(0.3, V).save(OUT / "video3_thumbnail.png")
    metadata("video3_short", TITLE, DESC, TAGS, film)
    print("done", final)
