# EmbedIDE

**Engineering IDE for embedded firmware, drivers, kernels, systems and scientific code**

Built by [Kirpich Space](https://github.com/Kirpich-Space). Languages: C, C++, Rust, Zig, Assembly, Fortran.

> EmbedIDE is a pure **Rust** project: the core, build engine, toolchain management and AI layer are
> crates, used by the **`embed`** terminal IDE. The earlier Electron/React desktop application has been
> removed (it remains in the git history before the commit "Remove the legacy Electron application").

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

Panels: **Alt+1** explorer, **Alt+2** AI assistant, **Alt+3** terminal (press again to return to the
editor; Ctrl+Alt+1/2/3 work too). Holding Alt and the digit and pressing Delete hides a panel in terminals
with the kitty keyboard protocol; elsewhere use `:hide_*` commands or your own bindings — see
[what terminals can report](docs/cli/tui.md#what-terminals-can-report). F5 builds, F3 shows problems (Enter jumps to the source line), F8 cancels, Ctrl+K opens the
command palette, `:help` lists every key. In the integrated terminal Shift+PageUp/PageDown/Home/End
scroll through history; the mouse works (click to focus, wheel, drag in the AI panel to select and copy its text; `:mouse` toggles it). Enter indents by language, and a completion
popup appears while you type (rust-analyzer, clangd and zls when installed, otherwise words of the
file). See [the TUI guide](docs/cli/tui.md).

**Development version 0.2.0-dev** adds:

- **Claude Code and Codex CLI as AI backends** — `/provider claude-code` or `/provider codex-cli` in the
  AI panel uses your installed, already logged-in official CLI (your subscription, no API key, no second
  login), streams the answer, and gives the model no file access (Claude Code) or a read-only sandbox
  (Codex). See [AI](docs/cli/ai.md#claude-code-and-codex-cli-backends-claude-code-codex-cli).
- **Agent mode** — `/agent <task>` lets Claude Code or Codex CLI work in the project: read, search,
  create, edit and delete files, run builds and tests and fix the errors, under a policy you choose
  (`read-only`, `workspace-write`, `approval-required`). Every command, file change and refusal is
  shown; destructive, privileged, network, hardware and out-of-workspace actions ask first (`y` / `a`
  / `n`); `/review` and `/revert` work on what really changed; `/cancel` stops every process. See
  [agent mode](docs/cli/agents.md).
- **Inline AI suggestions** — `Alt+\` shows a (multi-line) suggestion as ghost text, Tab accepts,
  Esc dismisses; optional automatic mode; any backend, including `codex-cli` without an API key. See
  [inline suggestions](docs/cli/ai.md#inline-suggestions).
- **Skills** — agents get EmbedIDE's built-in working instructions and the skills that fit the
  project (C/C++, firmware, board bring-up, Rust, Zig, assembly) automatically; your own `SKILL.md`
  files per project (`.embedide/skills`) or globally (`~/.config/embedide/skills`) replace or extend
  them (`/skill <name>`). See [skills](docs/cli/skills.md).
- **Assembly** — NASM, GNU as AT&T/Intel, ARM, AArch64 and RISC-V dialects detected per file; branch
  highlighting, labels (`:label`), completion of labels/instructions/registers, assembler errors in
  PROBLEMS; [examples/asm](examples/asm).
- **Binary files** — HEX viewer with search and a confirmed, undoable edit mode; ELF inspector (headers,
  segments, sections, symbols); disassembly via objdump/llvm-objdump. See
  [binary files](docs/cli/tui.md#binary-files-hex-elf-and-disassembly).

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
embed ai agent --provider codex-cli "make the unit tests pass"   # agent task in the project
embed ai skills                                  # project, extra and global skills
```

- [Getting started: install, first project, embedded workflow](docs/cli/getting-started.md)
- [TUI guide: panels, keys, configuration](docs/cli/tui.md)
- [Command reference](docs/cli/commands.md)
- [`embedide.toml` reference](docs/cli/embedide-toml.md)
- [AI subsystem](docs/cli/ai.md) · [agent mode](docs/cli/agents.md) · [skills](docs/cli/skills.md)
- [Verified and unverified functionality](docs/cli/status.md)

No Node.js or graphical session is needed. The binary links only against the C runtime.

## Requirements and build

- Rust 1.85+ (`cargo build --release --locked`; offline builds need the cached registry dependencies)
- Linux, macOS or Windows for the CLI; the terminal IDE and its integrated terminal are verified on Linux
- Host toolchains of your choice for building projects: GCC/Clang, `arm-none-eabi-gcc`, `rustc`/`cargo`,
  `zig`, NASM/GNU as, `make`/CMake; `openocd`/`probe-rs` for hardware (never run automatically)

```bash
cargo build --release --locked
cargo install --path crates/embedide-cli --locked --root ~/.local   # installs `embed`
embed doctor
```

Checks used in development:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check-embed-pty.py --binary target/release/embed
```

## Project layout

```
EmbedIDE/
├── crates/            # embedide-project, -core, -editor, -terminal (TUI), -toolchains, -workspace,
│                      # -build, -ai, -cli (binary `embed`)
├── docs/              # cli/ (user guides), native/ (design notes)
├── examples/          # asm, native project examples
└── scripts/           # PTY and CLI verification scripts
```

## License

MIT — see [LICENSE](LICENSE).
