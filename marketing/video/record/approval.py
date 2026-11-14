#!/usr/bin/env python3
"""Skills, policies and a real permission prompt: Claude Code is asked to run `rm -rf build`; the user answers `n`."""
from common import *   # noqa
import subprocess, time

ESC, ENTER = b"\x1b[27u", b"\r"


def main():
    root = make_project("ringbuf4", broken=False)
    cfg = fresh_cfg("approval")
    subprocess.run([str(BINARY), "build", str(root)], check=True, capture_output=True, env=env(cfg))
    rec = T.Recorder([BINARY], root, env(cfg), 31, 118)
    try:
        rec.until("EmbedIDE")
        rec.pump(1.5)
        rec.keys(b"\x1b2", pause=1.0)
        rec.type("/skills", pause=0.06)
        rec.keys(ENTER, pause=1.5)
        rec.pump(3.5)
        rec.mark("skills")
        rec.type("/policy", pause=0.06)
        rec.keys(ENTER, pause=1.5)
        rec.pump(3.5)
        rec.mark("policy")
        rec.type("/provider claude-code", pause=0.06)
        rec.keys(ENTER, pause=0.5)
        rec.until("logged in", 40)
        rec.pump(1.0)
        rec.type("Delete the build directory with rm -rf build, then rebuild the project with `embed build .` "
                 "and tell me whether it succeeded.", pause=0.03)
        rec.keys(ENTER, pause=0.3)
        rec.mark("task")
        deadline = time.monotonic() + 240
        while "Approval needed" not in rec.text():
            if time.monotonic() > deadline:
                raise RuntimeError("no approval request appeared:\n" + rec.text())
            rec.pump(0.3)
        rec.pump(4.0)                       # let the viewer read the request
        rec.mark("approval")
        rec.keys("n", pause=0.5)
        rec.mark("denied")
        handled = 1
        deadline = time.monotonic() + 240
        while not any(k in rec.text() for k in ("■ done", "□ finished", "□ stopped", "■ stopped")):
            if time.monotonic() > deadline:
                raise RuntimeError("did not finish:\n" + rec.text())
            seen = rec.text().count("Approval needed:")
            if seen > handled:
                handled = seen
                rec.pump(3.0)
                rec.keys("n", pause=0.5)
            rec.pump(0.4)
        rec.pump(3.5)
        rec.mark("done")
        print([(round(t, 1), n) for t, n in rec.events])
        print(rec.text())
        print("build dir still exists:", (root / "build").exists())
    finally:
        rec.save(OUT / "approval.pkl")
        rec.close()


if __name__ == "__main__":
    main()
