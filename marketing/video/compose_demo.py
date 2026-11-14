#!/usr/bin/env python3
"""Video 2 — Technical demonstration (one real development session plus real permission/binary footage)."""
from compose_common import *   # noqa
from lib import term as T

L = F.LANDSCAPE
rec = T.load(OUT / "demo_wide.pkl")
ev = {n: t for t, n in rec["events"]}
ap = T.load(OUT / "approval.pkl")
av = {n: t for t, n in ap["events"]}
binr = T.load(OUT / "overview_binary.pkl")
bv = {n: t for t, n in binr["events"]}
film = F.Film(L)


def clip(r, segs, narr, chapter=None, labels=(), tail=0.0, title="ringbuf — embed"):
    c = F.Clip(r, segs, title=title, narration=narr, labels=labels, freeze_tail=tail)
    c.chapter = chapter
    return c


t0 = F.Card(logo_on=True, headline="Technical demonstration", sub="One real development session, recorded from the program",
            narration="This is a technical demonstration of EmbedIDE, version 0.1.0-alpha.1. Everything in the recorded "
                      "sections is the real program, not a mockup. Waiting time is sped up, and it is always labelled. "
                      "The project is a small C ring buffer, and we will edit it, build it, and then let an AI agent "
                      "extend it.", min_dur=9.0)
t0.chapter = "Introduction"
film.add(t0, 0.6)

film.add(clip(rec, [(1.0, ev["opened"] + 0.5, 1)],
              "We start EmbedIDE inside the project directory. The explorer is on the left, the editor in the "
              "middle, and the AI panel on the right. I expand the source folder and open ring.c. Nothing has been "
              "sent anywhere, and nothing will be until I press Enter in the AI panel.", "Open a project",
              labels=[(1.0, 4.5, "Explorer · Enter opens a file")]), 0.4)

film.add(clip(rec, [(ev["opened"] + 0.5, ev["jumped"] + 1.6, 1)],
              "Press F5 to build with the project's compiler, here GCC. The build fails: a missing semicolon. The "
              "problems list shows the error and a warning with file, line and column, parsed from the compiler "
              "output. Enter jumps straight to the line in the editor.", "Build, error, jump",
              labels=[(1.0, 4.0, "F5 · build with GCC"), (4.5, 8.5, "Problems · Enter jumps to the line")]), 0.4)

film.add(clip(rec, [(ev["jumped"] + 1.6, ev["build-ok"] + 0.6, 1)],
              "I append the semicolon, press Escape, and save with colon w. Builds use the files on disk, so saving "
              "comes first. F5 again, and this time the build succeeds.", "Fix and rebuild"), 0.4)

film.add(clip(rec, [(ev["build-ok"] + 0.6, ev["ran"] + 3.0, 1)],
              "Alt+3 opens the integrated terminal: a real shell in a pseudo-terminal, started in the project "
              "directory. The program runs and prints the values popped from the ring buffer. Alt+3 again returns "
              "to the editor.", "Integrated terminal",
              labels=[(0.5, 4.0, "Alt+3 · integrated terminal (real PTY)")]), 0.4)

film.add(clip(ap, [(av["skills"] - 3.5, av["policy"] + 3.5, 1)],
              "Now the AI side. In the AI panel, slash skills lists what agents get: EmbedIDE's own working "
              "instructions and the skills that match this project, chosen automatically. Slash policy shows the "
              "three permission policies: read-only, workspace-write, and approval-required.", "Skills and policies",
              title="AI panel — embed", labels=[(0.5, 6.0, "/skills"), (7.0, 13.0, "/policy")]), 0.4)

film.add(clip(rec, [(ev["ran"] + 3.0, ev["agent-start"] + 0.5, 1.25)],
              "I select the Codex CLI backend. EmbedIDE needs no API key: it runs the Codex CLI you are already "
              "logged in with. In agent mode, plain text is a task. I ask for a peek function, a call from main, "
              "a build, and a run.", "AI agent: the task",
              labels=[(1.0, 5.0, "Alt+2 · AI panel · /provider codex-cli")]), 0.4)

film.add(clip(rec, [(ev["agent-start"] + 0.5, ev["agent-done"] + 1.0, 3.0)],
              "The agent reads the files, edits them, builds and runs the program. Every command with its output "
              "and every file change is shown as it happens. Codex works in its own sandbox. This part is sped up.",
              "The agent works"), 0.4)

film.add(clip(rec, [(ev["agent-done"] + 1.0, ev["rebuilt"] + 0.2, 1)],
              "When it finishes, EmbedIDE shows a summary: files changed on disk, commands run, and time. Slash "
              "review shows the diff, and slash revert would undo the whole task. Files open in the editor are "
              "reloaded automatically.", "Review and revert", labels=[(0.5, 4.0, "/review · /revert")]), 0.4)

film.add(clip(rec, [(ev["rebuilt"] + 0.2, ev["ran-again"] + 3.2, 1)],
              "Back in the editor, the build is already up to date, because the agent built it. Running the program "
              "shows the new function at work: it prints the peeked value first.", "Run the result"), 0.6)

film.add(clip(ap, [(av["task"] - 1.0, av["done"] + 2.0, 1)],
              "Permissions are the other half. Here I ask Claude Code to delete the build directory. Deleting data "
              "is not something the workspace-write policy allows silently, so EmbedIDE stops and asks. The request "
              "shows the exact command and why it needs approval. I press n. The agent is told it was refused, and "
              "the directory is still there.", "Permission prompts", title="AI panel — embed",
              labels=[(5.0, 11.0, "Approval needed · y allow · a allow similar · n deny")]), 0.5)

film.add(clip(binr, [(bv["hex"] - 1.0, bv["disasm"] + 3.0, 1.3)],
              "And what you build can be inspected without leaving the IDE. This is the compiled program in the "
              "hex view, then the ELF inspector with its symbols, then the disassembly of ring_pop, produced by "
              "objdump. The file is only read; editing bytes is an explicit mode with a confirmed, atomic save.",
              "Binary view", title="build/debug/ringbuf — embed"), 0.6)

summary = F.Card(headline="What you just saw", bullets=[
    "Native Rust, one binary, no Electron",
    "GCC diagnostics with jump to source",
    "Integrated terminal (real PTY)",
    "Codex CLI agent: edits, builds, review and revert",
    "Permission prompts for risky actions",
    "ELF, HEX and disassembly view"],
    narration="That was one real session: editing, compiler diagnostics, an integrated terminal, an AI agent that "
              "edited files and built the project with a diff you can review and revert, a permission prompt for a "
              "risky action, and a view of the compiled program. EmbedIDE is an alpha. Linux is verified, FreeBSD "
              "builds but has not been run, and the native GUI is only planned.", min_dur=13.0)
summary.chapter = "Summary"
film.add(summary, 0.6)

end = F.Card(logo_on=True, headline="Try it. Tell us what breaks.", code=[
    "$ curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh",
    "github.com/Kirpich-Space/EmbedIDE"],
    sub="Official Kirpich Space production · GPL-3.0 · Linux · FreeBSD (compiled, not yet run)",
    narration="Try it, and tell us what breaks. The repository link and the install command are in the description.",
    min_dur=8.0, note="Alpha software. The native GUI is planned, not available.")
end.chapter = "Install"
film.add(end, 0.0)

TITLE = "EmbedIDE technical demo: edit, build, AI agent and permissions in a Rust terminal IDE (alpha)"
DESC = """A real development session in EmbedIDE (`embed`), the native Rust terminal IDE for embedded and systems work. Every recorded section is the actual program, version 0.1.0-alpha.1; waiting time is sped up and labelled. Nothing is mocked.

What happens: open a C project, build with GCC, read the diagnostics and jump to the error, fix it, use the integrated terminal, then let the Codex CLI agent add a function, build and run it — with a summary, a reviewable diff and revert. Then a Claude Code request to delete a directory is stopped by a permission prompt, and the compiled program is inspected in the ELF/HEX/disassembly view.

Official Kirpich Space / EmbedIDE video. Status: alpha. Linux verified, FreeBSD compiles but not yet run, Windows/macOS not supported, native GUI planned (not available).

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64): curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Bugs and feedback: https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

The AI agents use the Claude Code / Codex CLI you are logged in with; EmbedIDE needs no API key.
Voice: Piper TTS, LibriTTS-R voice (CC BY 4.0). Music: original, synthesised for this video."""
TAGS = ["EmbedIDE", "Rust", "terminal IDE", "TUI", "embedded", "C programming", "GCC", "AI coding agent",
        "Claude Code", "Codex CLI", "ELF", "disassembly", "open source", "Linux", "FreeBSD"]

if __name__ == "__main__":
    final = build(film, "video2_technical_demo")
    c = F.Clip(rec, [(ev["jumped"], ev["jumped"] + 1, 1)], title="ringbuf — embed")
    thumbnail(OUT / "video2_thumbnail.png", c.frame(0.5, L), ["Real C workflow", "with an AI agent"],
              kicker="EmbedIDE · technical demo")
    metadata("video2_technical_demo", TITLE, DESC, TAGS, film)
    print("done", final)
