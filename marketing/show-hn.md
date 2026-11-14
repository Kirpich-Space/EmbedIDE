# Show HN

Destination: https://news.ycombinator.com/submit — read https://news.ycombinator.com/showhn.html first.
Post as a **Show HN** from your own account, with the GitHub repo as the URL. Do not ask anyone to
upvote (HN detects voting rings and penalises them). Be around for the first 2–3 hours to answer.

## Title (≤ 80 chars)

`Show HN: EmbedIDE – a terminal IDE in Rust for embedded work, with CLI agents`

Alternatives: `Show HN: A Rust TUI IDE with ELF/disassembly view and Claude Code/Codex agents` ·
`Show HN: EmbedIDE, a Vim-style terminal IDE for firmware and systems code`

## URL

https://github.com/Kirpich-Space/EmbedIDE

## First comment (post immediately after submitting)

Hi HN, I'm the author. EmbedIDE (`embed`) is a terminal IDE I wanted for firmware and systems work: a
Vim-style editor, explorer, a real PTY terminal, builds with jump-to-source diagnostics, and a view for
the binaries you produce (HEX editor, ELF inspector, objdump disassembly) so "build → look at what the
compiler emitted" doesn't need three tools.

What it does today (alpha, 0.1.0-alpha.1):

- You choose the toolchain (GCC, Clang, `zig cc`, `arm-none-eabi-gcc`, NASM…); it detects and never
  installs or flashes anything on its own. Native builds of C, C++, Rust (Cargo), GNU assembly; or your
  Make/CMake/Cargo/Zig project as is.
- Optional AI: instead of API keys it can drive the Claude Code or Codex CLI you already have logged in.
  In agent mode they read/edit files and run builds in your project under a policy (read-only,
  workspace-write, approval-required). Anything destructive, networked, outside the project or touching
  hardware asks first; you get a diff (`/review`) and `/revert`. EmbedIDE removes API-key variables from
  the CLI's environment so a subscription request never silently turns into billed API usage.
- One Rust binary, no Electron, no telemetry. GPL-3.0.

Honest state: Linux is the only platform I've verified (361 Rust tests + pseudo-terminal suites that
drive the real binary; I haven't hand-tested every terminal emulator). FreeBSD cross-compiles and lints
but I haven't run it there. Windows/macOS aren't supported. Flashing/debug/serial code exists but is
untested on real hardware. A native GUI is planned, not started. Parts of the code were written with AI
assistants (visible in the commit trailers) and go through the same tests.

Quick try (Linux x86_64, static build, no root):
`curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh`

I'd especially like to hear: which terminals/keys break for you, what's missing for your board or
toolchain, and whether the agent permission model matches what you'd trust. Bugs: GitHub issues with
`embed --version` and `embed doctor`.

## Likely questions — prepared answers

- *Why not Neovim/Helix + plugins?* Those are great. EmbedIDE bundles build + diagnostics + binary view +
  agent permissions as one tested whole and keeps the editor engine reusable for a future GUI. If your
  Neovim setup already does everything you need, you may not need this.
- *Why GPL?* The author wants improvements to stay open. Programs you build or edit are unaffected.
- *Is my code sent anywhere?* Only through the AI backend you select, only when you press Enter. With
  `EMBEDIDE_AI_OFFLINE=1` every AI request is refused. No telemetry.
- *Does it run on macOS/Windows?* No, not supported. Linux and FreeBSD only.
- *Is the agent safe?* It runs with your user's permissions. Claude Code is gated per tool call by
  EmbedIDE's rules; Codex runs in its own OS sandbox. For untrusted projects use `read-only` or
  `approval-required`. Rules are heuristics that ask when unsure (see docs/cli/agents.md).
- *How was it tested?* See docs/cli/status.md (verified with real tools vs mocks vs not verified).
