# Social posts

Each post is written for its platform. Post from the owner's own accounts, by hand, once. Images: the
real screenshots in `docs/img/` (alt text provided). No engagement pods, no purchased followers, no
automation.

---

## Mastodon (≈ 500 chars) — suggested hashtags: #Rust #RustLang #EmbeddedSystems #FreeBSD #TUI #OpenSource

> I've been building EmbedIDE: a terminal IDE in Rust for firmware/systems work. Vim-style editor, PTY
> terminal, builds with jump-to-source errors, and an ELF/HEX/disassembly view for the binary you just
> built. It can also run your Claude Code / Codex CLI as agents inside the project, with a permission
> policy and revertible diffs.
>
> Alpha, GPL-3.0, Linux (FreeBSD cross-compiles, untested). Looking for testers with unusual terminals and
> boards. https://github.com/Kirpich-Space/EmbedIDE
>
> #Rust #EmbeddedSystems #FreeBSD #TUI #OpenSource

Alt text for `docs/img/editor-build.png`: "Terminal IDE showing a C file with syntax highlighting, an
explorer on the left, an AI panel on the right and compiler output at the bottom."

Also post in the FreeBSD community on Mastodon with the FreeBSD-testers paragraph from `reddit-posts.md`.

---

## X / Twitter (single post + 3-post thread, ≤ 280 chars each)

1. `Built a terminal IDE in Rust for embedded/systems work: Vim-style editor, PTY terminal, builds with jump-to-source errors, ELF/HEX/disassembly view. Alpha, GPL-3.0. https://github.com/Kirpich-Space/EmbedIDE` *(attach editor-build.png)*
2. `The build engine makes a reviewable plan first (no shell), then runs it with process-group cancel. You pick the compiler: GCC, Clang, zig cc, arm-none-eabi-gcc, NASM. It never installs or flashes anything.` *(attach disassembly.png)*
3. `It can drive the Claude Code / Codex CLI you already use as agents in your project: policy per tool call, diff review, /revert, no API keys. EmbedIDE strips API-key env vars so subscription use can't silently become billed.` *(attach agent-codex.png)*
4. `Honest status: Linux verified, FreeBSD compiled but unrun, no Windows/macOS, hardware flashing untested, GUI only planned. Testers with odd terminals/boards very welcome → issues with embed doctor output.`

---

## LinkedIn (professional tone, ≈ 150 words)

> After a lot of firmware and systems work, I wanted the whole loop — edit, cross-build, read the
> diagnostics, inspect the produced ELF — in one place that lives in the terminal and doesn't need a browser
> engine. So I built EmbedIDE: a native Rust terminal IDE (Vim-style editor, integrated terminal, build
> output with jump-to-source, HEX/ELF/disassembly view). It also integrates the Claude Code and Codex CLIs
> as project agents under an explicit permission policy, with reviewable and revertible changes.
>
> It is an alpha under GPL-3.0, verified on Linux (FreeBSD builds are untested), and I'm looking for
> embedded and systems engineers to try it and tell me what breaks.
>
> https://github.com/Kirpich-Space/EmbedIDE
>
> #Rust #EmbeddedSystems #OpenSource #DeveloperTools

---

## Short promotional announcement (website / newsletter / Discord, ≈ 60 words)

EmbedIDE 0.1.0-alpha.1 is out: a native Rust IDE for embedded and systems development, in your terminal.
Vim-style editor, builds with jump-to-source diagnostics, ELF/HEX/disassembly view, and optional Claude
Code / Codex CLI agents with a permission policy. Linux (verified) and FreeBSD (compiled). GPL-3.0.
Testers welcome: https://github.com/Kirpich-Space/EmbedIDE

---

## Development-focused announcement (changelog-style, for Rust/embedded forums)

**EmbedIDE 0.1.0-alpha.1** — what's in it: nine-crate Rust workspace; editor engine without I/O; build
engine with reviewable plans; ELF32/64 parser, paged HEX editor, objdump view; LSP completion (rust-analyzer,
clangd, zls) with a word fallback; assembly dialects (NASM, GNU as AT&T/Intel, ARM, AArch64, RISC-V);
agent runtime for Claude Code (stream-json host protocol) and Codex (`exec --json`), with policy, snapshots
and skills. What's missing: hardware verification, FreeBSD runtime verification, DWARF mapping, GUI.
Changelog: https://github.com/Kirpich-Space/EmbedIDE/blob/main/CHANGELOG.md

## Community contribution invitation

See `contributor-outreach.md`.

## Where else to share a link (no post needed)

- This Week in Rust: submit to https://this-week-in-rust.org via a pull request to the "Call for
  participation"/project sections *after* you have a first-time-contributor-friendly issue list.
- Rust Users Forum "announcements" category: https://users.rust-lang.org/c/announcements
