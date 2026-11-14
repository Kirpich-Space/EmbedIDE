# Contributor outreach

## Invitation text (README, Discussions, forum "call for participation")

> **EmbedIDE needs your terminal, your board and your edge cases.** It's an alpha Rust terminal IDE for
> embedded and systems work (GPL-3.0). You do not need to write Rust to help: running it in a terminal
> emulator we haven't tried, on FreeBSD, or with your toolchain and telling us what breaks is valuable. If
> you do write Rust, the editor engine, build engine, ELF/HEX tools and agent runtime are separate crates
> with tests; start with `CONTRIBUTING.md` and `docs/cli/architecture.md`. No contribution is too small —
> typos in the docs count.

## How to help (ordered by effort)

1. **Try it and report** — `embed --version`, `embed doctor`, terminal, steps. Template:
   https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose
2. **Terminal / key compatibility reports** — which terminal emulators and multiplexers work (kitty,
   foot, WezTerm, Alacritty, GNOME Terminal, Konsole, xterm, tmux, over SSH). Alt+digit chords and OSC 52
   copying are the usual suspects.
3. **FreeBSD** — build, run `embed doctor`, try the integrated terminal and serial listing.
4. **Hardware** — flash/debug/serial paths are implemented but unverified: boards, probes (ST-Link,
   CMSIS-DAP, J-Link), reports of what the commands did.
5. **Docs and tutorials** — a walkthrough for a board you own; fix anything unclear.
6. **Code** — see candidate tasks below.

## Candidate "good first issue" / "help wanted" items

Create these as GitHub issues when you are ready (labels: `good first issue`, `help wanted`):

- Add a tutorial for a specific board (STM32 Nucleo, RP2040, nRF52, ESP32-C3 with a RISC-V GCC).
- Add `embed doctor --json` fields for terminal capabilities (kitty keyboard, colour depth).
- FreeBSD: verify serial port enumeration and write a short "FreeBSD notes" section from real output.
- Assembly: add a lexer/dialect fixture for another assembler syntax; extend mnemonic completion tables.
- Syntax highlighting: add a language or improve one (Fortran, shell, TOML edge cases) with tests.
- A PTY test for a terminal quirk you hit (see `scripts/check-embed-pty.py`).
- Package: AUR `PKGBUILD`, FreeBSD port skeleton, Nix flake (kept in their own directories/repos).
- Theme: contrast review of the Bone (light) theme in real terminals.

## First-response checklist for maintainers

- Thank the reporter; ask for `embed --version`, `embed doctor`, terminal, and a minimal repro if missing.
- Label (`bug`, `terminal-compat`, `freebsd`, `hardware`, `docs`, `agent`), reproduce or ask, and say what
  you will and won't do. Answer within a few days even if it's "not now".
- Turn every confirmed bug into a regression test (PTY suite where it is a key sequence).
- Credit reporters in the CHANGELOG.

## Pull request flow

Fork → branch → `cargo fmt`, `cargo clippy -D warnings`, `cargo test` → PR using the checklist in
`CONTRIBUTING.md`. GPL-3.0-only; contributions are accepted under the same license.
