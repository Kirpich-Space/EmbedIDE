#!/usr/bin/env python3
"""Extra real recordings for the overview: install.sh, `/providers` + a Claude Code agent task, binary view."""
from common import *   # noqa
import subprocess, time

ESC, ENTER = b"\x1b[27u", b"\r"


def install():
    DEMO.mkdir(exist_ok=True)
    shutil.rmtree(DEMO / "prefix", ignore_errors=True)
    rec = T.Recorder(["/bin/sh", "-i"], DEMO, env(fresh_cfg("install"), {"PREFIX": str(DEMO / "prefix")}), 31, 118)
    try:
        rec.pump(1.0)
        rec.type("curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh", pause=0.04)
        rec.keys(ENTER, pause=0.3)
        rec.until("Next:", 90)
        rec.pump(1.5)
        rec.type("~/.local/bin/embed --version" if False else str(DEMO / "prefix/bin/embed") + " --version", pause=0.04)
        rec.keys(ENTER, pause=1.0)
        rec.pump(1.0)
        rec.type(str(DEMO / "prefix/bin/embed") + " doctor | head -12", pause=0.04)
        rec.keys(ENTER, pause=1.5)
        rec.pump(3.0)
        print(rec.text())
    finally:
        rec.save(OUT / "overview_install.pkl")
        rec.close()


def claude():
    root = make_project("ringbuf2", broken=False)
    cfg = fresh_cfg("claude")
    rec = T.Recorder([BINARY], root, env(cfg), 31, 118)
    try:
        rec.until("EmbedIDE")
        rec.pump(1.5)
        rec.keys(b"\x1b2", pause=1.0)
        rec.type("/providers", pause=0.06)
        rec.keys(ENTER, pause=0.5)
        t_end = time.monotonic() + 60
        while time.monotonic() < t_end and not ("codex-cli" in rec.text() and "claude-code" in rec.text() and rec.text().count("logged in") >= 2):
            rec.pump(0.3)
        rec.pump(3.0)
        rec.mark("providers")
        rec.type("/provider claude-code", pause=0.06)
        rec.keys(ENTER, pause=0.5)
        rec.until("logged in", 40)
        rec.pump(1.5)
        rec.mark("claude")
        task = ("Add bool ring_is_full(const ring_t *r) to ring.c and ring.h, and print \"full: yes\" or \"full: no\" "
                "from main.c after the push loop. Build with `embed build .` and run ./build/debug/ringbuf.")
        rec.type(task, pause=0.03)
        rec.keys(ENTER, pause=0.3)
        rec.mark("agent-start")
        handled = 0
        deadline = time.monotonic() + 420
        while not any(k in rec.text() for k in ("■ done", "□ finished", "□ stopped", "■ stopped")):
            if time.monotonic() > deadline:
                raise RuntimeError("claude agent did not finish\n" + rec.text())
            seen = rec.text().count("Approval needed:")
            if seen > handled:
                handled = seen
                rec.pump(3.0)
                rec.keys("y", pause=0.5)
            rec.pump(0.5)
        rec.pump(3.0)
        rec.mark("agent-done")
        print(rec.text())
    finally:
        rec.save(OUT / "overview_claude.pkl")
        rec.close()


def binary():
    root = make_project("ringbuf3", broken=False)
    cfg = fresh_cfg("binary")
    subprocess.run([str(BINARY), "build", str(root)], check=True, capture_output=True, env=env(cfg))
    rec = T.Recorder([BINARY], root, env(cfg), 31, 118)
    try:
        rec.until("EmbedIDE")
        rec.pump(1.0)
        rec.keys(ESC, pause=0.4)
        for hide in (":hide_explorer", ":hide_assistant"):
            rec.type(hide, pause=0.03)
            rec.keys(ENTER, pause=0.3)
        rec.type(":e build/debug/ringbuf", pause=0.05)
        rec.keys(ENTER, pause=0.5)
        rec.until("OFFSET", 20)
        rec.pump(3.0)
        rec.mark("hex")
        rec.keys(b"\t", pause=2.0)       # ELF inspector
        rec.mark("elf")
        rec.keys("4", pause=2.0)         # symbols
        rec.type("/ring_pop", pause=0.06)
        rec.keys(ENTER, pause=2.0)
        rec.mark("symbols")
        rec.keys(ENTER, pause=2.5)       # open in disassembly
        rec.pump(4.0)
        rec.mark("disasm")
        print(rec.text())
    finally:
        rec.save(OUT / "overview_binary.pkl")
        rec.close()


if __name__ == "__main__":
    for step in (install, claude, binary):
        print("=====", step.__name__)
        step()
