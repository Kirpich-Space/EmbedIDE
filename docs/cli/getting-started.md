# Getting started with `embed` 0.1.0-alpha.1

`embed` is the terminal edition of EmbedIDE: a keyboard-first IDE (editor with Vim modes, explorer,
integrated terminal, builds with diagnostics, serial monitor, optional AI) in one binary. It runs in
any terminal; no GUI, Electron or Node.js is needed.

Verified on Linux x86_64 (see [status](status.md)). Windows and macOS builds are not verified yet.

## 1. Install

Requirements: Rust 1.85 or newer (`rustup` or your distribution's package), a C toolchain only for
the projects you build (EmbedIDE never installs compilers).

From the repository root:

```sh
cargo install --path crates/embedide-cli --locked --root ~/.local   # installs ~/.local/bin/embed
```

or, equivalently:

```sh
cargo build --release --locked -p embedide-cli
install -Dm755 target/release/embed ~/.local/bin/embed
```

Check that your shell finds it:

```sh
command -v embed && embed --version      # → embed 0.1.0-alpha.1
```

`fish: Unknown command: embed` (or bash `command not found`) means the install directory is not on
your `PATH`. A plain `cargo install --path crates/embedide-cli` installs into `~/.cargo/bin`, which many
setups do not put on `PATH`. Either install with `--root ~/.local` as above, or add the directory:

| Shell | Add `~/.local/bin` permanently |
|---|---|
| fish | `fish_add_path ~/.local/bin` (run once) |
| bash | `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc`, then open a new shell |
| zsh | `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc`, then open a new shell |

`cargo run -p embedide-cli -- …` runs the IDE from the source tree but does not install anything.

Uninstall: `cargo uninstall --root ~/.local embedide-cli` (or delete `~/.local/bin/embed`).

## 2. Launch

| Command | Opens |
|---|---|
| `embed` | the IDE in the current directory |
| `embed path/to/project` | the IDE in that directory |
| `embed src/main.c` | that file, in the nearest directory with `embedide.toml` (or the current directory) |
| `embed new.c` | a new, unsaved buffer bound to `new.c`; the file is created on the first `:w` |
| `embed --help`, `embed --version` | help / version |

A project manifest is never required: any directory opens. Nothing is built or executed when a
directory is opened. Subcommand names win over file names (`embed ./build` opens a file named
`build`). Without an interactive terminal, `embed` prints a short error instead of starting.

## 3. A first C project (GCC + Make)

```sh
mkdir -p ~/src/blink && cd ~/src/blink
printf 'hello: main.c\n\tgcc -Wall -o hello main.c\n' > Makefile
embed
```

1. The IDE starts in the **explorer**. Press `n`, type `main.c`, Enter: the file is created and opened.
2. Press `i` (Insert mode) and type a program; `Esc` returns to Normal mode; `:w` Enter saves.
   The tab bar shows `main.c*` and the status bar `[+]` while there are unsaved changes.
3. **F5** builds (here: `make`). Output appears in BUILD OUTPUT; the header shows Running / Ready /
   Failed. **F8** cancels. Errors go to **F3** PROBLEMS: `j`/`k` select, Enter jumps to the line.
4. **Alt+3** opens your shell in the project. Run `make`, `./hello`,
   `git status` — anything. **Shift+PageUp** scrolls back through the output, **Shift+End** returns.
5. Press the same panel key again to return to the editor. `:2` jumps to line 2, `/text` searches,
   `n`/`N` repeat the search.
6. **Ctrl+Q** quits. Unsaved documents ask for confirmation (`y`), the shell and its jobs are stopped,
   and the terminal is restored.

A Rust project works the same way: `cargo new app && cd app && embed`, then F5 runs `cargo build`.
CMake (`CMakeLists.txt`), Zig (`build.zig`) and Make projects are detected; for full control over
compilers and flags, create an `embedide.toml` (`embed project init`, see the
[reference](embedide-toml.md)).

## 4. Keys you will use first

| Key | Action |
|---|---|
| Alt+1 / 2 / 3 (Ctrl+Alt+1 / 2 / 3 work too) | explorer / AI / terminal; again returns to the editor |
| F1 or `:help` | every key in the IDE |
| Ctrl+K | command palette (all commands, files, targets, profiles, tasks) |
| Ctrl+P | find a file |
| Ctrl+S, `:w` | save; `:w other.c` saves as a new file (never overwrites) |
| Ctrl+W / F10 | close document / next document |
| F2 / F4 / F6 | toggle explorer / cycle bottom panels / cycle focus |
| Shift+PageUp, Shift+PageDown, Shift+Home, Shift+End | terminal history |
| `:mouse` | turn mouse support on/off (on by default) |
| Tab / Shift+Tab | indent / dedent (Insert: the current line; Normal/Visual: line or selection) |
| typing in Insert mode | completion popup: Down/Ctrl+N select, Tab/Enter accept, Esc close; Ctrl+Space asks explicitly |

Which keys a terminal can deliver is explained in
[the TUI guide](tui.md#what-terminals-can-report). In short: "hold Alt+digit, press Delete" (hide a
panel) needs the kitty keyboard protocol; everything in the table above
except those works in any xterm-compatible terminal, tmux and over SSH.

Enter indents after `{` in Rust/C/C++/Zig and a typed `}` aligns itself. For semantic completion install
the language servers you need (rust-analyzer, clangd, zls); without them the popup offers words from
the current file. Details: [indentation](tui.md#indentation) and [completion](tui.md#code-completion).

## 5. Embedded workflow

```sh
embed doctor                     # OS, configuration, which compilers/debuggers are installed
embed toolchain list             # compilers with versions, host vs. cross
embed project templates
embed project init fw --template c-cortex-m --board stm32f407vg
embed build fw                   # → fw/build/debug/fw.elf with arm-none-eabi-gcc
```

Inside the IDE (Ctrl+K, type the command name):

* **Serial**: `Discover Serial Ports`, `Select Serial Port / Baud` (e.g. `/dev/ttyACM0 115200`),
  `Connect Serial Monitor`, `Send Serial Text`, `Disconnect Serial Monitor`. A missing device or
  missing permissions are reported with the next step (for permissions: the device's group from
  `ls -l`, often `dialout` or `uucp`). Device removal is reported; reconnect explicitly.
* **Flash** (`Flash Firmware`): uses `probe-rs` with the chip, probe and artifact of the selected
  target in `embedide.json` → `native.targets`. The exact command is shown and must be confirmed with
  `y`. Nothing is ever flashed or erased automatically.
* **Debug** (`Start Debugging`): GDB/MI with an optional OpenOCD from `native.debug`
  (`gdb`, `openocd`, `configs`, `remote`, `elf`), after a confirmation that shows the command.
  Continue / Interrupt / Step / Next / Breakpoint / Backtrace / Registers are palette commands; this is
  a command-level debugger, not a full graphical frontend.
* **Tasks**: `native.tasks` in `embedide.json` defines named commands (`{"flash-dry-run":
  {"program": "make", "args": ["flash-dry-run"]}}`) run with `task: <name>` from Ctrl+K. Programs get
  literal arguments; there is no implicit shell.

Hardware paths were tested with virtual serial ports and fake tools only; see [status](status.md).

## 6. AI with your Claude Code or Codex subscription

If `claude` (Claude Code) or `codex` (Codex CLI) is installed and logged in, nothing else is needed:

1. Alt+2 opens the AI panel (the right column; the input line is at its bottom).
2. `/provider claude-code` (or `/provider codex-cli`) — the panel checks the CLI and shows e.g.
   `Claude Code 2.1.322 … ready, logged in via claude.ai (pro)`.
3. `/file` attaches the current file; type a question and press Enter. `/cancel` stops a request.
4. `/edit <instruction>` shows a diff; `/apply` puts it into the buffer (undo with `u`; `:w` saves).
5. With `claude-code` or `codex-cli`, plain text is a task: the CLI works in the project itself — edit files, build, run tests, fix
   errors — under the `workspace-write` policy: risky actions ask first (`y` / `a` / `n`), `/review`
   shows what changed, `/revert all` undoes it. See [agent mode](agents.md).
6. In Insert mode `Alt+\` asks for an inline suggestion; Tab accepts it, Esc dismisses it.

Requests count against that CLI's plan and limits, not an API key. Not logged in? Run `claude` or
`codex login` in a normal terminal once. To make it the default: `ai.provider = "claude-code"` in
`~/.config/embedide/config.toml`. Details: [AI](ai.md).

## 7. Assembly and binaries

* `embed hello.asm` / `embed start.S`: the dialect (NASM, GNU as AT&T/Intel, ARM, AArch64, RISC-V) is
  detected; `:label name` jumps to a label; F5 runs your Makefile; errors appear in F3 PROBLEMS.
  Try `examples/asm`.
* `embed ./prog` (any binary): HEX view, `e` ELF headers/sections/symbols, `d` disassembly, `m` main,
  `g` go to offset/address, `/` search. Editing starts only with `i` and writes only after `w` + `y`.

## 8. Configuration

| File | Scope |
|---|---|
| `embedide.toml` | project: language, build system, toolchains, flags, profiles ([reference](embedide-toml.md)) |
| `embedide.json` → `native` | IDE per project: `theme` (mono/dark/light), `tab_width`, `indent`, `completion`, `keys`, `mouse`, `targets`, `serial`, `debug`, `tasks`, `tools`, `environment` |
| `embedide.local.json` | personal overrides of `native`, not meant for version control |
| `~/.config/embedide/config.toml` | per user: preferred compilers, AI provider |

All files are optional. Errors in them never stop the IDE: they are shown at startup and in
`:settings`, and defaults are used. `:edit_project_config`, `:edit_user_config` open them; `:reload`
applies changes.

## 9. Known limitations

* Verified on Linux only; Windows/macOS are untested.
* One shell session in the terminal panel; 5000 lines of history.
* Mouse support (on by default): focus, cursor placement, explorer rows, wheel, and drag selection with copy in the AI panel; no drag selection in the editor.
* Real hardware (probes, boards, USB serial adapters) was not used in testing.
* Fortran builds were not run (no Fortran compiler on the test machine).
* Builds use the files on disk: save before F5.
* Completion: no snippet placeholders, automatic imports, signature help or LSP diagnostics yet.

More in [status](status.md).
