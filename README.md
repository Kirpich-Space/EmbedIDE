# EmbedIDE

**Engineering IDE for embedded firmware, drivers, kernels, systems and scientific code**

Built by [Kirpich Space](https://github.com/Kirpich-Space). Languages: C, C++, Rust, Zig, Assembly, Fortran.

> **Direction (2026-11):** EmbedIDE is now **Rust-first**. The core, build engine, toolchain
> management and AI layer are Rust crates, used today by the **`embed`** terminal IDE and later by a
> native **Qt 6** application (the name `embedide` is reserved for it). The Electron desktop app
> described further below is **deprecated**: it is kept for reference and is no longer extended.

## `embed` — the terminal IDE (Rust)

`embed` opens a full-screen, keyboard-driven IDE in your terminal:

```
┌ EmbedIDE 0.1.0 | build status | project | target | profile ──────────────────────────────┐
│ EXPLORER (C-A-1) │ editor tabs, Vim modes, syntax highlighting           │ AI (C-A-2)    │
│                  │                                                        │ chat, context │
│                  │                                                        │ edits, review │
├ TERMINAL (C-A-3) — your shell in a real PTY │ BUILD OUTPUT │ PROBLEMS │ … (F4) ──────────┤
└ NORMAL | src/main.c | c | gcc | Ln 12, Col 5 | message ─────────────────────────────────┘
```

```sh
cargo install --path crates/embedide-cli --locked --root ~/.local   # → ~/.local/bin/embed
command -v embed && embed --version                                  # → embed 0.1.0
cd my-project && embed                     # or: embed path/to/project, embed src/main.c
```

If your shell says `Unknown command: embed`, `~/.local/bin` is not on `PATH` (fish:
`fish_add_path ~/.local/bin`; bash/zsh: add `export PATH="$HOME/.local/bin:$PATH"` to the rc file).
A plain `cargo install` puts the binary in `~/.cargo/bin`, which is often not on `PATH` either.
**New here? Read [Getting started](docs/cli/getting-started.md)** — install, a first project, keys,
the embedded workflow and known limitations of 0.1.0.

Panels: **Ctrl+Alt+1** explorer, **Ctrl+Alt+2** AI assistant, **Ctrl+Alt+3** terminal (press again to
return to the editor); hold the digit and press Delete to hide a panel. These chords need a terminal with
the kitty keyboard protocol; elsewhere **Alt+1/2/3** switch panels and `:hide_*` commands (or your own
bindings) hide them — see [what terminals can report](docs/cli/tui.md#what-terminals-can-report). F5 builds, F3 shows problems (Enter jumps to the source line), F8 cancels, Ctrl+K opens the
command palette, `:help` lists every key. In the integrated terminal Shift+PageUp/PageDown/Home/End
scroll through history; `:mouse` turns on mouse support. Enter indents by language, and a completion
popup appears while you type (rust-analyzer, clangd and zls when installed, otherwise words of the
file). See [the TUI guide](docs/cli/tui.md).

You choose the compilers. EmbedIDE detects what is installed — GCC, Clang, `zig cc` / `zig c++`,
rustc/Cargo, Zig, GNU as, NASM, gfortran, Flang, LFortran and triple-prefixed cross GCCs — and builds
with exactly what you select in `embedide.toml` (per project and profile) or in your user
`config.toml`. It never installs or downloads toolchains, never flashes hardware and never starts a
debugger by itself. AI is optional: without a provider the IDE works fully offline.

The same services are scriptable through subcommands:

```sh
embed doctor                                     # OS, config, toolchains per language
embed toolchain list                             # versions, paths, host/cross
embed project init hello --language c --toolchain c=clang
embed build hello                                # native build with clang
embed build hello --toolchain c=zig-cc           # same project, zig cc
embed build hello --profile release
embed clean hello
embed terminal hello                             # your shell, project env + toolchain dirs on PATH

embed project init fw --template c-cortex-m --board stm32f407vg
embed build fw                                   # arm-none-eabi-gcc → fw/build/debug/fw.elf

embed ai providers                               # Anthropic, OpenAI, Moonshot (Kimi), xAI (Grok)
embed ai chat "Why does this hard-fault?" --file src/main.c --provider anthropic --model <id> --dry-run
```

- [Getting started: install, first project, embedded workflow](docs/cli/getting-started.md)
- [TUI guide: panels, keys, configuration](docs/cli/tui.md)
- [Command reference](docs/cli/commands.md)
- [`embedide.toml` reference](docs/cli/embedide-toml.md)
- [AI subsystem](docs/cli/ai.md)
- [Verified and unverified functionality](docs/cli/status.md)

No Electron, Node.js or graphical session is needed. The binary links only against the C runtime.

## Deprecated: Electron desktop application

The sections below describe the previous Electron + React app (`electron/`, `src/`).

## Features

- Multi-language editor — Rust, C, C++, Assembly, Zig (CodeMirror 6)
- **67 target MCUs** — STM32, NXP, Nordic, Infineon, Renesas, Microchip (flight / industrial class)
- Project kinds: firmware, device drivers, OS / kernel (all supported languages)
- Build & flash via OpenOCD (board-aware profiles)
- Serial monitor
- Memory analyzer (flash/RAM from board profile)
- Themes (dark & light engineering palettes)
- UI languages: English, Russian, Chinese, Japanese, German, French
- File search across the project
- Optional AI assistants — **off by default**; Ollama, OpenAI, Claude (Anthropic), OpenRouter, Groq, DeepSeek, Qwen, Kimi, and other providers
- Works fully offline for edit / build / flash / serial (toolchains installed on the host)

## Supported boards (v1.1)

| Family | Examples |
|--------|----------|
| STM32F0 / F1 / F3 | F030, F072, F103, F107, F303, F334 |
| STM32F4 | F401…F479 (F407VG default) |
| STM32F7 / H5 / H7 | F722…F769, H563/H573, H723…H7B3 |
| STM32G0 / G4 | G071, G0B1, G431, G474, G491 |
| STM32L4 / L5 / U5 | L432…L4R5, L552, U575, U5A5 |
| STM32WB / WL / C0 | WB55, WLE5, WL55, C031 |
| NXP | LPC55S69, LPC54628, i.MX RT1062 / RT1176 |
| Nordic | nRF52832, nRF52840, nRF5340, nRF9160 |
| Infineon | XMC4700, PSoC 6 CY8C624 |
| Renesas | RA6M5, RA8D1 |
| Microchip | ATSAME54P20, ATSAME70Q21 |

## Requirements

The native Rust migration runs alongside the existing desktop application.
See the [architecture](docs/native/architecture.md),
[migration plan](docs/native/migration.md) and
[verification report](docs/native/session-report.md) for its current scope.
The workspace includes headless project/build services, a CLI, and one shared
Vim editor engine. `embedide edit FILE` now opens a full-screen terminal editor
with immediate input, scrolling, Unicode display, search, undo/redo and staged
file saves. Explicit `--line` and `--batch` modes remain available.
The complete workspace builds on Void Linux musl: 132 Rust tests, 28 Unix PTY
tests, rustfmt and Clippy passed. Registry dependencies are now resolved.
Native GUI, LSP and Rust hardware adapters are not available yet. See the
[full-screen editor guide and verification report](docs/native/fullscreen-editor.md)
for platform status, Unicode limitations and file-safety details.

Rust workspace checks (Rust 1.85+; cached dependencies needed offline):

```bash
cargo check --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
./target/release/embedide edit example.rs
```

Press `i`, type text, press Esc, then type `:wq` and press Enter to save and quit.
Standalone: `./target/release/embedide-terminal example.rs`.
For automation, use `embedide --json edit --batch FILE`; interactive editing
requires terminal stdin/stdout and an ANSI-compatible terminal.

With the Unicode dependency cached, retain the line-oriented fallback:

```bash
python3 scripts/check-editor-offline.py --terminal --release --build-terminal target/editor-offline/embedide-terminal
target/editor-offline/embedide-terminal --line example.rs
```

This checker requires Python 3.11+ and validates only editor/line frontend
sources with the TUI feature disabled, not the complete workspace.

The native project workspace is available separately from the desktop UI:

```bash
cargo build --release --workspace --locked
./target/release/embedide .
./target/release/embedide open examples/native/firmware-logic
```

It provides a lazy explorer, independent documents, command palette, background
Cargo/Make/CMake build plans, diagnostics, bounded process console, configured
probe-rs flash confirmation, serial monitoring, and an initial GDB/MI/OpenOCD service.
Hardware workflows have not been verified on a physical board. PlatformIO/Arduino
are tool inventory entries only. See [workspace guide](docs/native/workspace.md),
[hardware capabilities and limitations](docs/native/hardware.md), and
[validation and benchmarks](docs/native/workspace-validation.md).

The requirements below apply to the existing Electron desktop application.

- Node.js 18+
- npm
- Platform: Linux (primary), Windows, macOS

### Host toolchains (for build/flash)

- `arm-none-eabi-gcc` — C/C++/ASM
- `rustc` + `cargo` (+ rustup targets) — Rust
- `zig` — Zig firmware / drivers / OS
- `openocd` — flash / debug probe
- `make`

## Quick Start

```bash
git clone https://github.com/Kirpich-Space/EmbedIDE.git
cd EmbedIDE
npm install
npm run dev          # development
npm run launch       # production build + run
```

## Build Distribution

```bash
npm run dist:linux   # AppImage + deb
npm run dist:win     # NSIS
npm run dist:mac     # DMG
```

Artifacts are written to `release/`.

## AI (optional)

Settings → AI:

- **Disabled** (default) — no panel, no network calls
- **Local** — OpenAI-compatible endpoint, default `http://127.0.0.1:11434/v1` (Ollama)
- **Cloud** — HTTPS endpoint + API key

Core IDE features never require AI or internet.

## Project layout

```
EmbedIDE/
├── electron/          # Main process (boards, project, toolchain, serial)
├── src/               # React UI
├── public/fonts/      # Offline UI/editor fonts
├── build/icons        # App icons
└── package.json
```

## License

MIT — see [LICENSE](LICENSE).
