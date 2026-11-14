# Reddit posts

One post per subreddit, never the same text, never cross-posted at once. **Read each subreddit's current
rules and sidebar on the day**; wording below is a draft that already follows the usual self-promotion
norms (be the author, say it is yours, be specific, disclose limits, no vote requests). Space posts at
least a few days apart and reply to every comment.

Order: r/rust → r/embedded → r/commandline → r/FreeBSD → r/opensource. Optional last: r/ClaudeAI / r/ChatGPTCoding.

---

## r/rust — https://www.reddit.com/r/rust/submit

Check: the "project" / "Show & Tell" flair and the AI-assisted-content rules in the sidebar. This repo
discloses AI co-authorship up front — keep that line.

**Title:** `EmbedIDE: a terminal IDE for embedded/systems work, written in Rust (alpha, GPL-3.0)`

**Body:**

I've been building `embed`, a keyboard-driven terminal IDE in Rust, aimed at firmware and systems
programming. Nine crates in a workspace; the interesting engineering bits for this sub:

- the editor is a piece-table + bounded undo + Vim input parser with **no I/O and no terminal types**, so a
  future native GUI can reuse it unchanged;
- TUI on crossterm + vt100 + portable-pty (integrated terminal), diffed screen writes, kitty keyboard
  protocol with a legacy fallback;
- build engine = a reviewable plan, then run: no shell, process-group cancellation, depfile-incremental
  native builds; or your Make/CMake/Cargo/Zig project untouched;
- bounds-checked ELF32/64 parser + paged HEX editor + objdump view for the binaries you build;
- `unsafe_code = "forbid"` workspace-wide; 361 tests plus PTY suites that drive the real binary.

It can also run Claude Code / Codex CLI as agents inside the project (permission policy, diff review,
revert), using their own login rather than API keys. Parts of the code were written with AI assistants
(commit trailers say so); everything goes through the same tests.

Honest status: alpha. Linux verified; FreeBSD compiles/lints but is unrun; no Windows/macOS. GUI is
planned, not started. I'd love bug reports from terminals other than mine.

Repo: https://github.com/Kirpich-Space/EmbedIDE

---

## r/embedded — https://www.reddit.com/r/embedded/submit

Check the rules about self-promotion and "projects" (the sub is picky about low-effort promotion).
Lead with the embedded workflow, not with AI.

**Title:** `I built a terminal IDE around the arm-none-eabi workflow: build → ELF symbols → disassembly (alpha, looking for testers)`

**Body:**

For firmware work I kept juggling an editor, a Makefile, `arm-none-eabi-objdump`/`readelf` and a hex
editor. `embed` puts them in one terminal app: edit, F5 build with the compiler you choose, errors jump to
the line, then open the resulting `.elf` in an ELF inspector (sections, symbols, vector table) and a
disassembly view, or patch bytes in a HEX view with an atomic, confirmed save.

What exists: a `c-cortex-m` template for 12 boards (M0–M85) that builds with `arm-none-eabi-gcc`, GNU
as/NASM/AArch64/RISC-V assembly highlighting and label navigation, per-profile toolchain selection.

What does **not** exist or is untested: flashing/debugging/serial are implemented as explicit commands
(probe-rs/OpenOCD/GDB-MI, serial monitor) but **I have not verified them on real hardware** — that is where
I most need testers with boards. Nothing flashes or resets anything automatically. Linux only for now.

GPL-3.0, Rust, one binary: https://github.com/Kirpich-Space/EmbedIDE — tutorials in `docs/cli/tutorials.md`.
Which boards/probes would you want covered first?

---

## r/commandline — https://www.reddit.com/r/commandline/submit

**Title:** `embed – a keyboard-first terminal IDE (Vim-style editor, PTY terminal, build output, ELF/HEX view), alpha`

**Body:** Short: screenshot (docs/img/editor-build.png) + 4 bullets (modal editor, integrated PTY terminal,
builds with jump-to-source errors, HEX/ELF/disassembly view) + install one-liner + limits (Linux/FreeBSD
only, alpha, terminal key quirks) + link. Ask: "which terminal/multiplexer breaks for you?"

---

## r/FreeBSD — https://www.reddit.com/r/freebsd/submit

Check the rules about software announcements. Lead with the ask.

**Title:** `Looking for FreeBSD testers: a Rust terminal IDE that cross-compiles for FreeBSD but has never been run there`

**Body:**

I develop EmbedIDE (`embed`), a terminal IDE in Rust (GPL-3.0). It is verified on Linux; for FreeBSD I have
only cross-compiled and linted (`x86_64-unknown-freebsd`), so I know nothing about how it behaves on a
real system. If you're willing to try: `pkg install rust gmake git`, then
`cargo install --path crates/embedide-cli --locked --root ~/.local`. FreeBSD-specific work already done:
GNU make is used as `gmake` (BSD make can't read GNU Makefiles), `pkg install` hints for missing
toolchains. Things I'd like checked: the integrated PTY terminal, serial port listing (`/dev/cuaU*`),
`embed doctor` output, colours in your terminal/console. Please open an issue with `embed doctor`:
https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

---

## r/opensource — https://www.reddit.com/r/opensource/submit

**Title:** `EmbedIDE – GPL-3.0 terminal IDE in Rust for embedded/systems work; first alpha, looking for contributors and testers`

**Body:** What it is (2 sentences), why GPL (improvements stay open), where help is wanted (terminal
testing, FreeBSD, tutorials per board, assembly dialects), how to start (CONTRIBUTING.md, issue templates),
honest status. Link only the repo.

---

## Optional: r/ClaudeAI, r/ChatGPTCoding

Only if the rules allow project posts. Angle: "agent permission layer for Claude Code/Codex in a terminal
IDE — policy per tool call, diff review, revert, no API keys." Disclose you are the author; do not imply
affiliation with Anthropic or OpenAI.
