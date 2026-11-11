# CLI status — verified vs. unverified (2026-11-11, `embed` 0.1.0 release candidate)

Environment of record: Linux x86_64 (Artix), rustc 1.99.1. Evidence: `cargo test --workspace`,
`crates/embedide-cli/tests/workflows.rs` (real compilers), `crates/embedide-cli/tests/ai.rs` and
`crates/embedide-ai/tests/mock_providers.rs` (mock servers), `scripts/check-*.py` (PTY/serial),
`scripts/check-embed-pty.py` (the `embed` IDE driven through a real pseudo-terminal).

## Verified with real tools

| Area | Tools actually used |
|---|---|
| Toolchain detection, version and target probing, host/cross distinction, custom paths with spaces | gcc 16.2.1, g++, clang/clang++ 23.1.3, zig 0.17.0 (`zig cc`/`zig c++` → Clang 22.1.8), rustc/cargo 1.99.1, GNU as 2.47, NASM 3.02, arm-none-eabi-gcc 14.2.1 (xPack, in a directory whose path has spaces) |
| C project built separately with gcc, clang and `zig cc` | same |
| C++, GNU assembly (via gcc), NASM, Zig, Rust (Cargo, offline) | same |
| Profiles, per-profile toolchain override, incremental rebuild incl. header change via depfile | gcc |
| Compiler errors: non-zero exit, parsed diagnostics, no crash | gcc |
| Adopting an existing Make project without touching it; `CC=` handed to Make | make, clang |
| Firmware template `c-cortex-m` on 12 boards (M0, M0+, M3, M4, M7, M33, M85): ARM ELF, vectors in flash, SP in RAM, Thumb reset vector, no warnings | arm-none-eabi-gcc 14.2.1 |
| `clean` deletes only marked output; refuses unmarked directories | — |
| Runs without Electron, Node.js or a graphical session (`env -i`, no DISPLAY) | release binary, links only libc/libgcc_s |
| Legacy commands and PTY suites (terminal editor, TUI, workspace, native CLI, serial bridge on a pseudo-terminal) | — |
| `embed` IDE, keyboard only through a PTY: no-argument launch, Vim editing and `:w` of real files, `:e`, `:help`, unknown `:` commands, Alt+1/2/3 focus switching, explorer navigation, narrow (80×24) and wide (120×40) layouts, status bar (mode, file, language, compiler, position) | `/bin/sh` as the user shell, no `DISPLAY`/`WAYLAND_DISPLAY`, a trap `node` first on `PATH` (never executed) |
| Integrated terminal: real interactive shell in a PTY, commands create files, Ctrl+C / Esc / Ctrl+D reach the program | `/bin/sh` (bash 5.3), `cat -v` |
| F5 build of an `embedide.toml` project with the configured compiler, incremental "up to date", diagnostics in PROBLEMS, Enter jumps to the reported line/column, F8 cancels a running Make build | gcc 16.2.1, make |
| End-to-end acceptance path through a PTY on the release and the `cargo install`ed binary: create a file from the explorer, edit, `:w`, build in the integrated terminal, read the error, scroll history (Shift+PageUp/Home/End), fix via `:2`, F5 build, run the program, quit with terminal attributes unchanged | gcc 16.2.1, make; and a Cargo project (rustc error → PROBLEMS → jump → fix → F5) with cargo 1.99.1 |
| `embed build` outside the IDE: Make, CMake, Cargo, Zig (`zig init` project), native `embedide.toml`; exit codes preserved (make: 2), compiler errors on stderr, missing command reported, missing configured toolchain names the installed alternatives | make, cmake, ninja, cargo, zig 0.17.0, gcc |
| Integrated terminal: `sh`, `bash`, `fish` sessions, colours/bold/inverse kept, scrollback with colours, stable view while output arrives, typing returns live, full-screen program on the alternate screen keeps Shift+PageUp (`vim -u NONE`), Ctrl+C stops `yes` while it floods (IDE RSS stays bounded), Ctrl+D ends the shell and Enter restarts it, resize reaches `stty size`, no processes survive quit | bash 5.3, fish 4, vim; `less`/`htop` are not installed on this machine |
| Mouse (opt-in `:mouse`): SGR clicks select/open explorer rows and place the editor cursor, wheel scrolls; capture is released on exit | PTY with SGR mouse sequences |
| Editor: `:<line>`, `:w <file>`/`:saveas` (refuses to overwrite), untitled `:w` explains how to name the file, explorer refresh keeps expanded directories and the selection | unit tests on the real workspace |
| Automatic indentation: Enter/`o`/`O` per language (Rust, C, C++, Zig, shell, Python, assembly, plain), Enter between braces, closers aligned with their opener, strings/comments ignored, Tab/Shift+Tab in Insert/Normal/Visual, width/tabs/detect configuration, undo/redo, literal paste | engine tests; typed in the real binary through a PTY |
| Completion popup with real language servers in the real binary through a PTY: `values.le` → `len` from rust-analyzer 1.99.1, `strle` → `strlen` from clangd 23.1.3, `std.me` → `mem` from zls 0.17.0; accepted with Down+Tab and saved | rust-analyzer, clangd, zls installed on this machine |
| LSP protocol: initialize, UTF-8/UTF-16 negotiation and conversion, didOpen/didChange/didClose, server requests answered, superseded requests cancelled and stale answers ignored, crash/garbage handling, process-group cleanup; local word fallback, selection rules, Unicode, Esc/Insert interaction | scripted mock server; workspace unit tests |

## Verified only with mocks / fakes

| Area | How |
|---|---|
| Anthropic Messages API and OpenAI-style Chat Completions (OpenAI, Moonshot, xAI): request format, headers, streaming, usage, model listing, 401/429/5xx mapping, key redaction | local HTTP mock server |
| `ai chat`, `ai complete`, `ai edit` (diff, `--apply`), `ai login/logout`, offline guard, dry run | CLI against the mock server |
| Subscription mode (Claude Code print mode) | fake `claude` script; Codex path not exercised |
| Completion engine debounce/supersede/cancel | in-process fake backend |
| AI panel in `embed`: provider/model from `config.toml`, nothing sent before Enter, `/file` attachment, streamed chat, `/edit` diff review and `/apply` into the buffer, `:complete` ghost text accepted with Tab and rejected with Esc, offline mode | local OpenAI-style mock HTTP server |

## Not verified

- **Any real AI provider request or real subscription CLI** (not authorized: would cost money).
- **Fortran builds**: no gfortran, flang or lfortran is installed on the verification machine.
- **Windows and macOS runtime.** Windows: `cargo clippy -D warnings` passes for
  `x86_64-pc-windows-gnu` on every crate except those depending on `ureq` (its `ring` dependency needs a
  MinGW C compiler that is not installed) — this now includes `embedide-terminal`/`embed`, so the
  Windows build of the TUI (ConPTY terminal via `portable-pty`) is unverified; nothing was executed on
  Windows. macOS: not built.
- Automatic inline suggestions while typing (`ai.inline = true`) in the TUI; only the explicit
  `:complete` path was exercised end to end.
- **Hardware**: no flashing, debugging or physical serial device. The GDB/MI, probe-rs and serial
  services in `embedide-core` are unchanged and not exposed as automatic CLI actions.
- LFortran and Flang command lines (implemented from their documented flags, not run).
- NASM output formats other than elf64.

## Known limitations

- Native compiles run sequentially; `--jobs` applies to Make/CMake/Cargo/Zig.
- Fortran module dependencies are not scanned (compile order = directory order).
- `ai login` reads the key with echo when typed interactively; piping is recommended.
- Credentials are stored in a 0600 file, not in the OS keychain.
- The Electron app (`electron/`, `src/`) is deprecated. The only change: its serial service now looks for
  the `embed` binary (the CLI was renamed from `embedide`).
- Terminal panel: one shell session; 5000 lines of history; programs cannot receive mouse events
  (the IDE's mouse mode is for the IDE itself); the history of a full-screen program is its own.
- Completion: plain-text insertion of snippets, no automatic-import edits, no signature help, hover or
  LSP diagnostics; one server per workspace root; full-text document sync.
- Mouse is off by default; while it is on, selecting text in the outer terminal usually needs
  Shift+drag. No drag selection inside the editor.
- Real terminal emulators (kitty, foot, xterm, VTE, tmux) were not driven by hand in this session; the
  PTY driver sends their documented byte sequences.
- Ghost text shows the first line of a multi-line suggestion plus a line count; Tab inserts all of it.
- Builds use files on disk: modified buffers must be saved before F5.
