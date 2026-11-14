#!/usr/bin/env python3
"""Video 1 — Official EmbedIDE overview. Real footage + clearly labelled animation and instruction cards."""
from compose_common import *   # noqa
from lib import term as T

L = F.LANDSCAPE
demo = T.load(OUT / "demo_wide.pkl")
dv = {n: t for t, n in demo["events"]}
inst = T.load(OUT / "overview_install.pkl")
cl = T.load(OUT / "overview_claude.pkl")
cv = {n: t for t, n in cl["events"]}
binr = T.load(OUT / "overview_binary.pkl")
bv = {n: t for t, n in binr["events"]}
film = F.Film(L)


def clip(rec, segs, narr, chapter=None, title="embed", labels=(), tail=0.0):
    c = F.Clip(rec, segs, title=title, narration=narr, labels=labels, freeze_tail=tail)
    c.chapter = chapter
    return c


a = F.Card(logo_on=True, headline="A native Rust IDE for embedded and systems development",
           sub="Terminal-first. Your compilers. Your AI agents. Your rules.",
           narration="EmbedIDE is a native Rust IDE for embedded and systems development. It lives in your terminal.",
           min_dur=7.0)
a.chapter = "What is EmbedIDE"
film.add(a, 0.6)

film.add(F.Card(headline="One binary. No Electron. No browser.", bullets=[
    "Vim-style editor, explorer, integrated terminal",
    "Builds with jump-to-source diagnostics",
    "ELF, HEX and disassembly view for what you build",
    "Optional AI agents through Claude Code and Codex CLI"],
    narration="It combines a Vim-style editor, an explorer, an integrated terminal, builds with jump-to-source "
              "diagnostics, a view for the binaries you produce, and optional AI agents. One Rust binary, with no "
              "Electron and no browser.", min_dur=9.0), 0.6)

d = F.Diagram(narration="Under the hood it is a Cargo workspace of nine crates. The editor engine has no I/O and no "
                        "terminal types, so a native GUI could reuse it later. The build engine makes a reviewable "
                        "plan and then runs it, without a shell. There is no unsafe code, processes are launched "
                        "with argument vectors, and files are written atomically. The GUI itself is planned, not "
                        "started: this alpha is a terminal application.", min_dur=14.0)
d.chapter = "Architecture"
film.add(d, 0.6)

film.add(clip(demo, [(1.0, dv["opened"] + 0.5, 1.4), (dv["opened"] + 0.5, dv["jumped"] + 1.6, 1)],
              "This is the real program. Open a file from the explorer, press F5, and GCC's error appears in the "
              "problems list with file, line and column. Enter jumps to it.", "Editor and builds",
              title="ringbuf — embed",
              labels=[(0.5, 3.0, "Explorer"), (4.0, 7.0, "F5 · build"), (7.5, 10.5, "Problems · Enter jumps")]), 0.5)

film.add(clip(demo, [(dv["jumped"] + 1.6, dv["ran"] + 3.0, 1.3)],
              "Fix, build again, and use the integrated terminal: a real shell in a pseudo-terminal, one key away.",
              "Integrated terminal", title="ringbuf — embed"), 0.5)

film.add(clip(binr, [(bv["hex"] - 1.0, bv["disasm"] + 3.0, 1.6)],
              "Open the program you just built. A hex viewer and editor, an ELF inspector with sections and "
              "symbols, and a disassembly view that runs objdump. The file is only read, unless you explicitly edit "
              "bytes and confirm the save.", "Binary view", title="build/debug/ringbuf — embed"), 0.5)

film.add(clip(cl, [(cv["providers"] - 2.0, cv["claude"] + 3.0, 1)],
              "AI integration uses the Claude Code and Codex command-line tools you already have installed and "
              "logged in. EmbedIDE needs no API key, never reads their credentials, and removes API key "
              "variables from their environment, so a subscription request can never silently become billed "
              "API usage.", "Claude and Codex", title="ringbuf — embed",
              labels=[(2.0, 6.5, "/providers · installed CLIs and login state")]), 0.5)

film.add(clip(cl, [(cv["claude"] + 3.0, cv["agent-start"] + 0.5, 1.6), (cv["agent-start"] + 0.5, cv["agent-done"] + 1.5, 2.2)],
              "In agent mode, a plain message is a task. Here Claude Code adds a function, edits three files, builds "
              "with the project's compiler and runs the program. EmbedIDE answers each permission request from a "
              "policy: edits and builds inside the project run, while deleting files, network access, hardware "
              "tools and anything outside the project ask first.", "AI agent edits files", title="ringbuf — embed"), 0.5)

film.add(F.Card(headline="You choose the toolchain", bullets=[
    "GCC, Clang, zig cc, arm-none-eabi-gcc, NASM",
    "Make, CMake, Cargo, Zig, or EmbedIDE's native build",
    "Never installs or flashes anything by itself",
    "GDB, OpenOCD and serial: explicit commands, not yet verified on hardware"],
    narration="You choose the compilers: GCC, Clang, zig cc, a cross GCC for Cortex-M, NASM. EmbedIDE detects what is "
              "installed and never installs or flashes anything on its own. Debugging with GDB and OpenOCD, and a "
              "serial monitor, exist as explicit commands, but they are not yet verified on real hardware.",
    min_dur=12.0), 0.6)

st = F.Card(headline="Honest status: an alpha", bullets=[
    "Linux: verified",
    "FreeBSD: compiles, not yet run",
    "Windows and macOS: not supported",
    "Native GUI: planned, not started"],
    narration="This is an alpha. Linux is the verified platform. FreeBSD compiles but has not been run yet. Windows and "
              "macOS are not supported, and the native GUI is planned, not started.", min_dur=9.0)
st.chapter = "Status"
film.add(st, 0.6)

film.add(clip(inst, [(0.5, 24.0, 1.3)],
              "To try it, one command downloads a static build into your home directory and verifies its checksum. "
              "Then embed doctor shows what EmbedIDE found on your machine.", "Install", title="/bin/sh",
              labels=[(1.0, 6.0, "install.sh · checksum verified")]), 0.5)

src = F.Card(headline="Or build from source", code=[
    "$ git clone https://github.com/Kirpich-Space/EmbedIDE",
    "$ cd EmbedIDE && cargo build --release --locked",
    "$ ./target/release/embed doctor"],
    sub="Rust 1.85 or newer · instructions, not a recording",
    narration="To build from source you need Rust one point eighty-five or newer. Clone the repository, run cargo "
              "build with the release flag, and start embed.", min_dur=9.0)
src.chapter = "Build from source"
film.add(src, 0.6)

end = F.Card(logo_on=True, headline="Try it. Tell us what breaks.", sub="Official Kirpich Space production · GPL-3.0",
             code=["github.com/Kirpich-Space/EmbedIDE"],
             narration="EmbedIDE is open source under the GPL. Try it, and tell us what breaks. The link is in the "
                       "description.", min_dur=7.0, note="Alpha 0.1.0 · Linux · FreeBSD (compiled, not yet run)")
film.add(end, 0.0)

TITLE = "EmbedIDE — a native Rust IDE for embedded and systems development (alpha overview)"
DESC = """EmbedIDE (`embed`) is a native Rust IDE for embedded and systems development that runs in your terminal: a Vim-style editor, explorer, integrated terminal, builds with jump-to-source diagnostics, an ELF/HEX/disassembly view for the programs you build, and optional AI agents that work in your project through the Claude Code or Codex CLI you already use — with a permission policy you control and diffs you can review and revert.

Official Kirpich Space / EmbedIDE video. Recorded sections are the real program (0.1.0-alpha.1); the architecture slide is an animation and says so. Waiting time is sped up and labelled.

Status: alpha. Linux verified, FreeBSD compiles but has not been run, Windows/macOS not supported. The native GUI is planned, not started.

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64, static build, checksum-verified):
curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Found a bug? https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

Voice: Piper TTS, LibriTTS-R voice (CC BY 4.0). Music: original, synthesised for this video."""
TAGS = ["EmbedIDE", "Rust", "embedded", "IDE", "terminal", "TUI", "firmware", "systems programming", "open source",
        "Claude Code", "Codex CLI", "FreeBSD", "Linux"]

if __name__ == "__main__":
    final = build(film, "video1_overview")
    c = F.Clip(demo, [(dv["jumped"], dv["jumped"] + 1, 1)], title="ringbuf — embed")
    thumbnail(OUT / "video1_thumbnail.png", c.frame(0.5, L), ["A native Rust IDE", "for embedded work"],
              kicker="EmbedIDE · alpha overview")
    metadata("video1_overview", TITLE, DESC, TAGS, film)
    print("done", final)
