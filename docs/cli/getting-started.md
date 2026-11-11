# Getting started with `embed` 0.1.0

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
command -v embed && embed --version      # → embed 0.1.0
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
4. **Ctrl+Alt+3** (or **Alt+3**, see below) opens your shell in the project. Run `make`, `./hello`,
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
| Ctrl+Alt+1 / 2 / 3 (Alt+1 / 2 / 3 without the kitty keyboard protocol) | explorer / AI / terminal; again returns to the editor |
| F1 or `:help` | every key in the IDE |
| Ctrl+K | command palette (all commands, files, targets, profiles, tasks) |
| Ctrl+P | find a file |
| Ctrl+S, `:w` | save; `:w other.c` saves as a new file (never overwrites) |
| Ctrl+W / F10 | close document / next document |
| F2 / F4 / F6 | toggle explorer / cycle bottom panels / cycle focus |
| Shift+PageUp, Shift+PageDown, Shift+Home, Shift+End | terminal history |
| `:mouse` | turn mouse support on/off (off by default) |
| Tab / Shift+Tab | indent / dedent (Insert: the current line; Normal/Visual: line or selection) |
| typing in Insert mode | completion popup: Down/Ctrl+N select, Tab/Enter accept, Esc close; Ctrl+Space asks explicitly |

Which keys a terminal can deliver is explained in
[the TUI guide](tui.md#what-terminals-can-report). In short: Ctrl+Alt+digit and "hold a digit, press
Delete" (hide a panel) exist only with the kitty keyboard protocol; everything in the table above
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

## 6. Configuration

| File | Scope |
|---|---|
| `embedide.toml` | project: language, build system, toolchains, flags, profiles ([reference](embedide-toml.md)) |
| `embedide.json` → `native` | IDE per project: `theme` (mono/dark/light), `tab_width`, `indent`, `completion`, `keys`, `mouse`, `targets`, `serial`, `debug`, `tasks`, `tools`, `environment` |
| `embedide.local.json` | personal overrides of `native`, not meant for version control |
| `~/.config/embedide/config.toml` | per user: preferred compilers, AI provider |

All files are optional. Errors in them never stop the IDE: they are shown at startup and in
`:settings`, and defaults are used. `:edit_project_config`, `:edit_user_config` open them; `:reload`
applies changes.

## 7. Known limitations of 0.1.0

* Verified on Linux only; Windows/macOS are untested.
* One shell session in the terminal panel; 5000 lines of history.
* Mouse support is basic (focus, cursor placement, explorer rows, wheel) and opt-in; no drag selection.
* Real hardware (probes, boards, USB serial adapters) was not used in testing.
* Fortran builds were not run (no Fortran compiler on the test machine).
* Builds use the files on disk: save before F5.
* Completion: no snippet placeholders, automatic imports, signature help or LSP diagnostics yet.

More in [status](status.md).
