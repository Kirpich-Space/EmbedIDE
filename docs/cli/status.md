# CLI status — verified vs. unverified (2026-11-14, `embed` 0.1.0-alpha.1)

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
| Claude Code / Codex CLI backends with the **real installed CLIs** through the release binary in a PTY: login state (`claude.ai (pro)`, `ChatGPT`), attached file, a real answer from each, `/cancel` of a running Codex request (launcher and native process gone in ~0.3 s, twice) | Claude Code 2.1.322, Codex CLI 0.163.0 (a few short prompts on the user's subscription) |
| Agent mode with the **real CLIs** (`embed ai agent`, disposable projects in `/tmp`): Codex created `add.c`, built it with `gcc -Wall` and ran it (workspace-write sandbox); Claude Code edited `util.h`, `util.c`, `main.c`, built with `gcc -Wall -Werror`, ran the program, and its `rm app` was asked about and refused; a project skill reached Claude Code under `read-only`. The `can_use_tool` host protocol and Codex's `--json` item events were inspected first with one minimal probe each | Claude Code 2.1.322, Codex CLI 0.163.0, gcc 16.2.1 (a handful of short tasks on the user's subscriptions) |
| Inline AI suggestion latency through the real CLIs (`embed ai complete`, 2 runs each): Codex 9.3–10.8 s, Claude Code 6.8–10.7 s; both returned `return a + b;` | same |
| CLI backends with a mock CLI: streaming, plain-text fallback, login/limit/crash errors, broken pipe, oversized output, timeout, cancellation of the whole process group, concurrency, no API keys or parent-session variables passed, edit → diff → reject/apply, quitting during a request | scripted mock CLI; PTY test of the panel |
| Assembly: dialect detection (incl. the example files), per-dialect comments/registers, branch highlighting, labels, `:label`, completion of labels/mnemonics, NASM and GNU as errors in PROBLEMS with jump, F5 build of `examples/asm` and running the program; AT&T/Intel programs built and run; AArch64/RISC-V assembled to objects | nasm 3.02, GNU as/ld 2.47, llvm-mc |
| Binary files: paged HEX view, goto/search, edit mode with byte undo/redo, confirmed byte-exact save, refusal when changed on disk, permissions kept; ELF64 from GCC, ELF32 and big-endian (PowerPC64) objects, truncated/garbage files without panics; objdump listing agreeing with the ELF reader; GCC build → HEX → ELF → `main` → disassembly → address → bytes, file unchanged | gcc 16.2.1, objdump 2.47, llvm-objdump |
| LSP protocol: initialize, UTF-8/UTF-16 negotiation and conversion, didOpen/didChange/didClose, server requests answered, superseded requests cancelled and stale answers ignored, crash/garbage handling, process-group cleanup; local word fallback, selection rules, Unicode, Esc/Insert interaction | scripted mock server; workspace unit tests |

## Release check 0.1.0-alpha.1 (2026-11-14, Linux x86_64, glibc)

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` | pass (see CHANGELOG for the two racy tests fixed) |
| `scripts/check-embed-pty.py` (real `embed` in a PTY) | 33 pass, 4 skipped: nasm, clangd, rust-analyzer, zls not installed |
| `scripts/check-tui-pty.py` (standalone and `--cli`), `check-terminal-pty.py`, `check-workspace-pty.py` | pass |
| `embed project init` + `embed build` for `c`, `cpp`, `rust`, `asm` templates with gcc 16 / g++ / cargo 1.99 / GNU as; the C program ran; a syntax error gave a parsed diagnostic (file, line 5, column 13) | pass |
| **Live agent, Claude Code 2.1.322**: `embed ai agent --provider claude-code "Create … analyze.py … run it"` created `analyze.py`, ran it (3 rows, sum 6) and reported the change | pass |
| **Live agent, Codex CLI 0.167.2**: the local Codex install lacks `codex-code-mode-host`; `codex exec` alone fails the same way, so no file was written. Not an EmbedIDE fault; Codex agent mode is verified with the mock CLI only | blocked |
| Plain text (no `/agent`) after `/provider codex-cli` runs an agent task and writes a file | mock Codex CLI (unit test) |

| **Fortran with gfortran 16.2.1**: a project with three files whose modules depend on each other (the program sorts first) builds in dependency order, runs, rebuilds only what a changed module affects, and a syntax error gives a diagnostic with file, line and column (`tests/workflows.rs`) | pass |
| **Toolchain download**: built-in `zig` 0.17.1 (54 MiB) downloaded over HTTPS, SHA-256 verified, unpacked, `zig version` printed `0.17.1`; a wrong checksum was rejected with nothing left on disk; an `http://` URL was refused; F5 in the TUI downloaded in the background and then built | pass (real network) |

Not available on this machine: cross GCCs, OpenOCD, probes, boards — rows for those tools come from earlier
verification runs.

## Verified only with mocks / fakes

| Area | How |
|---|---|
| Anthropic Messages API and OpenAI-style Chat Completions (OpenAI, Moonshot, xAI): request format, headers, streaming, usage, model listing, 401/429/5xx mapping, key redaction | local HTTP mock server |
| `ai chat`, `ai complete`, `ai edit` (diff, `--apply`), `ai login/logout`, offline guard, dry run | CLI against the mock server |
| Subscription mode (Claude Code print mode, Codex exec) | fake `claude` and `codex` scripts |
| Completion engine debounce/supersede/cancel | in-process fake backend |
| AI panel in `embed`: provider/model from `config.toml`, nothing sent before Enter, `/file` attachment, streamed chat, `/edit` diff review and `/apply` into the buffer, `:complete` ghost text accepted with Tab and rejected with Esc, offline mode | local OpenAI-style mock HTTP server |
| Inline AI suggestions in the real binary through a PTY: manual mode sends nothing while typing; `Alt+\` (kitty encoding) shows a two-line suggestion with a virtual row; typing its start keeps it without a request; Tab inserts; `u` / `Ctrl+R`; Tab still indents without a suggestion; auto mode sends one debounced request for five keystrokes and none mid-line. Engine cache, timeout, reindent and symbol extraction in unit tests | local mock HTTP server; in-process fake backend |
| Agent runtime: Claude Code host protocol (initialize, `can_use_tool`, allow/deny), all three policies for both CLIs, Codex plan → approval → `exec resume` with `workspace-write`, session ids and `--resume`, API-key source refusal, cancellation of the CLI and a grandchild, `/fix` loop for both CLIs, snapshot diff/revert, command classifier (allowed vs. asked commands, quoting, redirections, loops, symlinks out of the workspace) | `tests/fixtures/mock_agent.py`, `crates/embedide-ai/tests/agent.rs`, unit tests |
| Agent mode in the real binary through a PTY: `/agent` with one-key approvals, step display, buffer reload, `/review`, `/revert all`, Codex `approval-required` plan approval, `/cancel` leaving no processes; skills listed, a broken skill reported, a skill activated and quoted as untrusted text | same mock CLIs |

## Not verified

- **Any request to a paid API** (`anthropic`, `openai`, `moonshot`, `xai` with an API key): not
  authorized, would cost money; mock servers only.
- With the real CLIs: agent `approval-required`, `/fix`, session resume across tasks and agent runs
  inside the TUI (all exercised with mock CLIs only; the real agent runs used `embed ai agent`).
- **Fortran with Flang or LFortran**: only gfortran was run.
- **FreeBSD beyond what was run.** Verified on FreeBSD 15.1-RELEASE amd64 (UFS; Rust 1.98.1, gcc, clang, gfortran 14.2):
  all 383 tests, the PTY terminal checks, `gmake` selection, gfortran module builds with diagnostics, C builds with
  clang. The agent tests and the terminal scripts need an executable named `python3`; FreeBSD installs only
  `python3.12`, so put a `python3` symlink on PATH. Not verified there: serial ports, probes and boards (no hardware), `pkg` install hints, ZFS, other FreeBSD
  versions and architectures, NetBSD and OpenBSD.
- **Windows and macOS are not supported.** Windows-specific code was removed and the crates refuse to
  compile on non-Unix targets.
- **Hardware**: no flashing, debugging or physical serial device. The GDB/MI, probe-rs and serial
  services in `embedide-core` are unchanged and not exposed as automatic CLI actions.
- LFortran and Flang command lines (implemented from their documented flags, not run).
- NASM output formats other than elf64.

## Known limitations

- Native compiles run sequentially; `--jobs` applies to Make/CMake/Cargo/Zig.
- Fortran module order comes from EmbedIDE's own scan of `module`/`use`/`include` lines; modules supplied through the C preprocessor (`#include`) are not followed.
- `ai login` reads the key with echo when typed interactively; piping is recommended.
- Credentials are stored in a 0600 file, not in the OS keychain.
- The former Electron app was removed from the repository (see the git history).
- Terminal panel: one shell session; 5000 lines of history; programs cannot receive mouse events
  (the IDE's mouse mode is for the IDE itself); the history of a full-screen program is its own.
- Completion: plain-text insertion of snippets, no automatic-import edits, no signature help, hover or
  LSP diagnostics; one server per workspace root; full-text document sync.
- CLI backends: Codex answers arrive per message (its `--json` mode has no token deltas); Codex keeps
  read access to files your user can read (its read-only sandbox forbids writes and network only);
  each request starts a new CLI process (a few seconds of start-up); model listing is not available.
- Assembly completion uses fixed mnemonic/register lists; ARM/AArch64/RISC-V were assembled but not
  linked or run (no cross linker or emulator on the test machine).
- Binary view: one binary at a time; replace-only editing (no insert/delete); ownership, xattrs, ACLs
  and hard links are not preserved by a save; no DWARF source mapping or GDB integration in the view.
- Mouse is off by default; while it is on, selecting text in the outer terminal usually needs
  Shift+drag. No drag selection inside the editor.
- Real terminal emulators (kitty, foot, xterm, VTE, tmux) were not driven by hand in this session; the
  PTY driver sends their documented byte sequences.
- Inline suggestions through the subscription CLIs take several seconds each (see above); no warm
  completion session is kept. Virtual rows show at most 16 lines of a suggestion (`… (+N lines)`).
- Agent mode: Codex `exec` has no per-command approval (sandbox instead; `approval-required` is plan
  approval); the command classifier is a heuristic that asks when unsure; one agent task at a time;
  the review snapshot skips generated directories and is limited in size. See
  [agents.md](agents.md#limitations).
- Builds use files on disk: modified buffers must be saved before F5.
