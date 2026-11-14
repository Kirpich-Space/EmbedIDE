# Product description and brand text

Source of truth for every announcement. Wording rule: say what exists today; label everything else
"planned". Visual identity is the approved one in `Desige/` (Carbon / Bone / rust accent) — never
re-draw or recolour the logo.

## Positioning (one line)

**EmbedIDE — A Native Rust IDE for Embedded and Systems Development.**

## Official short description (GitHub "About", ≤ 350 chars)

EmbedIDE — terminal IDE (Rust) for embedded firmware, systems code and assembly, with Claude Code / Codex CLI agents. Linux and FreeBSD.

## Official product description (≈ 90 words)

EmbedIDE (`embed`) is a native Rust IDE for embedded and systems developers that lives in your terminal.
It combines a Vim-style editor, a project explorer, a real PTY terminal, builds with jump-to-source
diagnostics, and an ELF / HEX / disassembly view for the programs you produce. You pick the compilers —
GCC, Clang, `zig cc`, `arm-none-eabi-gcc`, NASM — and EmbedIDE never installs or flashes anything by
itself. Optional AI agents work inside your project through the Claude Code or Codex CLI you already use,
under a permission policy you control, with a diff you can review and revert. GPL-3.0, Linux and FreeBSD.

## Technical description (for engineers)

- **Architecture:** Cargo workspace of nine crates. The editor (piece table, bounded undo, Vim input
  parser, syntax lexers, indentation) has no I/O and no terminal types; frontends are thin adapters.
  Build engine = reviewable plan → run, no shell, process-group cancellation, depfile-incremental native
  builds, or the project's own Make / CMake / Cargo / Zig.
- **Terminal:** crossterm + vt100 + portable-pty; kitty keyboard protocol when available (legacy
  fallback); diffed screen writes; 256-colour and truecolour themes with WCAG-checked contrast.
- **Binaries:** paged HEX viewer/editor with atomic, confirmed saves; bounds-checked ELF32/64 parser;
  objdump / llvm-objdump disassembly (never executes the binary).
- **AI:** provider-independent layer. API providers (Anthropic, OpenAI, Moonshot, xAI) *or* the installed
  Claude Code / Codex CLI with their own login. Agents: Claude Code over its stream-json host protocol
  (every tool call checked against EmbedIDE's policy), Codex through `exec --json` in its sandbox;
  workspace snapshot for `/review` and `/revert`; API-key variables removed from the CLI environment so a
  subscription request never silently becomes billed API use; credentials are never read from CLI config.
- **Safety:** no `unsafe`, processes launched with argument vectors, atomic file writes that refuse to
  overwrite files changed on disk, hardware actions only on explicit commands.
- **Verification:** 361 Rust tests plus pseudo-terminal suites that drive the real binary; mock Claude/Codex
  CLIs for the agent protocol; real-tool checks for gcc, clang, zig, nasm, rust-analyzer, clangd, zls.
  Details and gaps: `docs/cli/status.md`.

## What it is not (say this early)

- Not a GUI yet — the native Rust GUI is planned, not started.
- Not a debugger front end that starts hardware for you; flashing/debugging need explicit commands and
  have not been verified on a physical board.
- Linux is the verified platform; FreeBSD is compiled and linted, not yet run; Windows and macOS are not supported.
- An alpha: expect rough edges, especially terminal-specific key handling.

## Taglines (use sparingly, one per post)

- A native Rust IDE for embedded and systems development.
- Terminal-first. Your compilers. Your CLI agents. Your rules.
- From `main.c` to the disassembly, without leaving the terminal.

## GitHub social preview concept (1280 × 640 PNG, upload under Settings → Social preview)

Background Carbon `#0B0B0C`. Left third: the approved **wordmark on dark**
(`Desige/embedide-wordmark-on-dark.svg`) with the tagline "A native Rust IDE for embedded and systems
development" in bone `#F4F1EC` (two lines, mono font). Right two thirds: the real screenshot
`docs/img/editor-build.png` cropped to the editor + build output, with a 1 px `#2A2A2F` border and a
slight shadow. Bottom-left chip row in `#1C1C20` with bone text: `Rust` · `TUI` · `Linux` · `FreeBSD` ·
`GPL-3.0`. Rust accent `#C8562A` used once (underline under "embedded"). No stock art, no invented
mascot, no claims beyond the chip row. (A browser upload is required; the GitHub API cannot set it.)

## Demo video

No public demo video URL was available when this file was written. Put it here once it exists and
link it from the README and every post that allows a link:

`DEMO_VIDEO_URL = <to be filled in by the owner>`

A 60–90 s recording works best: `embed` → open `main.c` → F5 build → error → jump → fix → build →
`:hex` / ELF symbols / disassembly → `/provider codex-cli` → one agent task with a visible approval.
