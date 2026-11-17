# Changelog

## Unreleased

### Security
- **A project manifest can no longer choose what the build loads.** `embedide.toml` `[env]` refuses `LD_*`, `DYLD_*`,
  `PATH`, `SHELL`, `IFS`, `BASH_ENV`, `ENV`, `COMPILER_PATH` and `GCC_EXEC_PREFIX` (an `LD_PRELOAD` in a cloned
  project's manifest used to run its library inside the compiler on F5 or `embed build`). Compiler flags that load or run
  project code (`-fplugin`, `-wrapper`, `-specs`, `-B…`, `-load`) are reported as warnings by `embed build` and
  `embed project info`; they stay allowed because embedded projects use `-specs=nano.specs`. Build only projects you trust.

### Fixed
- **Backspace on the FreeBSD console (and any terminal with `stty erase ^H`):** the key arrives as BS (0x08, `^H`), which the
  key parser reports as Ctrl+H; no mode handled it, so nothing was deleted and the status line said "Key not handled by the
  editor". Ctrl+H (and a literal BS or DEL character) now mean Backspace everywhere except the shell panel, which still
  receives the byte your terminal sent.
- **FreeBSD (verified on 15.1 amd64, UFS):** the disassembly view failed because LLVM's `objdump` (the base `/usr/bin/objdump`)
  rejects the `--` argument; a `tools.make` setting in `embedide.json` was ignored because GNU make is `gmake` there; and
  saving twice in quick succession reported "file changed externally" (the change time of a freshly renamed file still moves
  on UFS with soft updates, so it is no longer compared on the BSDs). Three tests that assumed GNU/Linux (`make` name,
  `wc -l` output) were made portable. The full suite (383 tests) and the PTY checks pass on FreeBSD.
- **The screen no longer freezes on F5 when a compiler hangs.** Probing every compiler's version (5 s timeout each, one
  after another), downloading required toolchains and planning the build now run on a background thread; before, a hung
  `gcc` froze the TUI for 5 s (measured).
- `embed --help`, the Settings panel and code comments no longer claim that EmbedIDE never installs anything.

## `embed` 0.1.0-alpha.1 — update of 2026-11-16 (same version, rebuilt archives)

The `0.1.0-alpha.1` release was updated in place: its archives were rebuilt with the changes below. The
version string is unchanged, so check `SHA256SUMS` to tell the builds apart.

### Fixed
- **Fortran builds failed with gfortran** (`f951: Fatal Error: To enable preprocessing, use '-cpp'`): EmbedIDE
  asked for a dependency file that needs the preprocessor. Fortran no longer uses one.
- **Fortran modules** are compiled in dependency order (EmbedIDE reads `module`, `submodule`, `use` and
  `include` statements, free and fixed form), a changed module rebuilds the files that use it, and cycles or
  duplicate modules are reported. Before, files were compiled in directory order.
- **gfortran diagnostics** (location, source excerpt and message on separate lines) now reach the problem
  list and jump-to-error; before, Fortran errors produced no entries.

### Added
- **Toolchain downloads from your own config**: `[toolchains.install.<name>]` (HTTPS URL, mandatory SHA-256,
  `strip`, `bin`, `provides`), `toolchains.require`, `toolchains.auto_install`, a built-in checksum-pinned
  `zig` entry, a project-level `[install] toolchains = [...]` that refers to entries by name only, and
  `embed toolchain available | install | uninstall`. Missing required toolchains are downloaded before a
  build (`embed build` and F5; in the TUI on a background thread). Archives are verified before unpacking
  and unpacked with path checks. `EMBEDIDE_OFFLINE=1` forbids downloads. See
  [Toolchain downloads](docs/cli/configuration.md#toolchain-downloads).

### Changed
- The README and docs no longer say that EmbedIDE never downloads anything: it downloads only what your
  `config.toml` (or the built-in pinned `zig` entry) describes and something requires.
- LFortran compiles now pass `-J <module dir>` (from its documented options; not run).

## `embed` 0.1.0-alpha.1 — 2026-11-14 (first public alpha, pre-release)

Version note: development builds were called `0.2.0-dev` and an earlier `0.1.0` release candidate was
never published; `0.1.0-alpha.1` is the first published version of the Rust `embed`. Older
experimental versions (1.0.0 – 2.6.2, Electron) are unrelated and stay in the history.

### Changed
- **Agent mode is the default with a CLI backend**: after `/provider codex-cli` or
  `/provider claude-code`, an ordinary request is a task the agent carries out in the project —
  no `/agent` needed. `ai.agent.mode` defaults to `agent`; `/mode chat` gives read-only answers; API
  backends always chat. The provider note shows the mode and policy.
- Official EmbedIDE theme (`Desige/embedide-palette.json`) in one module,
  `crates/embedide-terminal/src/tui/theme.rs`: Carbon (dark) and Bone (light) tokens, syntax colours
  (`syn-*`: rust keywords, ink code, ceramic jumps) and semantic UI roles — errors red, warnings amber,
  success green, AI cyan, notes muted, rust accent only for prompts and changed files. Bars are bone on
  Carbon `#1C1C20`. Build output and PROBLEMS lines are coloured by severity; AI inline suggestions
  are cyan italics instead of an inverse block. Truecolor terminals get exact tokens, 256-colour
  terminals the nearest ones; tests check WCAG contrast (≥ 4.5:1) of every text colour.

### License
- EmbedIDE is now licensed under the **GNU GPL v3.0 only** (was MIT). All workspace crates, the built-in
  skills and the documentation follow. Earlier published copies (up to `0.1.0-alpha.1`) stay MIT. All
  third-party dependencies are GPL-3.0-compatible (MIT, Apache-2.0, BSD, ISC, Unicode, MPL-2.0, Zlib,
  Unlicense).

### Fixed (agents)
- Claude Code agents were asked for approval to run `embed build .` (EmbedIDE's own command was not on the
  development allow list). `embed build`, `clean`, `doctor`, `targets`, `project info|init`, `toolchain
  list|detect|info|recommend` and `config list|get|path` are now allowed under `workspace-write`;
  `embed ai`, `terminal`, `config set` and `toolchain add` still ask.

### Distribution
- Three official videos (overview, technical demonstration, vertical short) as release assets, produced by the
  reproducible pipeline in `marketing/video/` from recordings of the real binary.
- Static `x86_64-unknown-linux-musl` archive (any Linux distribution, no glibc requirement) next to the
  glibc one; `install.sh` (checksum-verified, user-local); issue templates for bug reports.

### Platforms
- Supported systems are **Linux and FreeBSD**. Windows and macOS are not supported: Windows-specific code
  (`.exe`, `COMSPEC`, `%APPDATA%`, `taskkill`, `winget`, the crossterm `windows` feature) and macOS paths
  were removed, and the crates refuse to compile on non-Unix targets.
- FreeBSD: GNU make is used as `gmake` when present (BSD `make` cannot read GNU Makefiles; also in the
  agent rules and the built-in skill), `pkg install` hints for missing toolchains, `x86_64-unknown-freebsd`
  is type-checked and linted in CI. Not run on FreeBSD yet.

### Documentation
- New guides: documentation index, tutorials (C, Rust, Zig, assembly, Cortex-M, compilers, AI agent,
  skills, inline suggestions, binaries), key and command reference, configuration reference,
  troubleshooting and FAQ, architecture, `CONTRIBUTING.md`.

### Fixed (release preparation)
- Test `cancel_and_timeout_stop_the_whole_process_group` was flaky: `pgrep -f` matched the mock
  directory `…-1` as a prefix of concurrent mocks `…-10`…`…-19`. Pattern anchored.
- Test `terminal_scrollback_…` waited for `DONE`, which the shell echoes with the typed command, before
  any output existed.
- PTY end-to-end checks read files after a stale "Saved" (the status row redraws the previous
  message) and expected English `make` messages under any locale; 4 checks failed before this release.
- UI colours reused syntax categories (errors drawn in the constant colour, diffs in string/constant
  colours); they now have semantic roles.

### Added
- Agent mode (`/agent`, `/mode agent`, `embed ai agent`): Claude Code (Agent SDK host protocol,
  every unapproved tool call answered by EmbedIDE's rules or the user) and Codex CLI (`exec --json`
  in its read-only/workspace-write sandbox) read, search, create, edit and delete files and run builds
  and tests in the project. Policies `read-only`, `workspace-write` (default) and `approval-required`
  (Codex: plan, approve, carry out); one-key approvals (`y`/`a`/`n`) shown in the panel and the status
  bar; step display with commands, output tails, exit codes, file changes and refusals; summary line;
  session continuation (`--resume`, `exec resume`), `/new`; `/changes`, `/review`, `/revert` from a
  workspace snapshot; reload of unmodified open buffers; `/fix` build-and-fix loop without a shell;
  `/cancel` stops the whole process group; `[ai.agent]` settings.
- Inline AI suggestions: `ai.completion.mode` (`off`, `manual` default, `auto`), `Alt+\` (`ai_suggest`),
  multi-line ghost text as virtual rows, re-indentation, type-through, cache, timeout, separate
  backend/model (`ai.completion.provider`), project line and file declarations as context, error
  back-off, `/inline`; `[ai.completion]` settings.
- Built-in skills given to every agent task without `/skill`: `embedide-agent` (detailed working
  instructions: use the tools, build → diagnostics → fix → rebuild, bounded program runs, permissions,
  report) plus `c-cpp`, `c-firmware`, `embedded-bringup`, `rust-crate`, `zig-build`, `asm-review`
  chosen by what the project contains; sent once per CLI session; project/global skills of the same
  name replace them; `ai.skills.auto`, `ai.skills.exclude`, `--no-auto-skills`, `/skill off`.
- Chat mode tells the model it cannot change files and should point to `/agent`; `/agent task`
  (a copied placeholder) is refused with an example.
- Skills: `SKILL.md` discovery in `.embedide/skills`, `ai.skills.dirs` and `~/.config/embedide/skills`
  (in that precedence), frontmatter validation, on-demand loading, `/skills`, `/skill`, `embed ai
  skills`, `--skill`; skill text is quoted as untrusted guidance; `examples/skills` (C firmware, Rust,
  Zig, assembly, embedded bring-up).
- AI backends `claude-code` and `codex-cli`: the installed official CLIs with their own login,
  streaming structured output, cancellation of the whole process group, time and output limits,
  login/limit/error classification, `/providers`, `/status`, `/file <paths>`, `/selection`, `/diff`,
  `/history`; `[ai.cli]` settings. No API keys or parent Claude Code session variables are passed.
- Assembly dialects (NASM, GNU as AT&T/Intel, ARM, AArch64, RISC-V): dialect-aware highlighting with
  branch colour, `:label` / Go to Label, completion of labels/instructions/registers/directives,
  `native.asm.syntax`, `examples/asm`.
- Binary view: paged HEX viewer/editor (search, goto, confirmed atomic save, undo/redo), ELF
  inspector, objdump/llvm-objdump disassembly; `native.binary`.

### Fixed
- Selecting text in the AI panel with the mouse selected the whole IDE row; a drag in the panel now selects
  only its own text and copies it (OSC 52). The mouse is captured by default for this (`:mouse` toggles).
- The AI input line had no cursor: Left/Right, Home/End, Delete, word movement, Ctrl+U, paste at the
  cursor, click to place it and Up/Down input history now work.

### Removed
- The legacy Electron/React desktop application (`electron/`, `src/`, `public/`, `test/`, npm
  and electron-builder configuration, installers, launchers, packaging recipes, Node scripts and the
  Electron CI/release workflows). The repository is pure Rust.

### Changed
- Default theme `dark`: the terminal's own text colours with muted grey bars and selections instead of
  bright inverse video (`mono` keeps inverse video; terminals without 256 colours fall back to it).

### Fixed
- `unified_diff` no longer builds a quadratic table for large files (common head/tail are skipped; very
  large differing regions get a non-minimal diff).
- The inline suggestion engine kept its first backend after `/provider` switched to another one.
- NASM and GNU as error messages (no column) now reach PROBLEMS.
- GNU as `;` (statement separator) and `@function` were highlighted as comments.
- A failed CLI capability probe was cached until restart; the IDE could exit before stopping a running
  CLI request; a backend status check blocked sending a message.

## `embed` 0.1.0 — release candidate (unreleased, not published)

First release of the Rust terminal IDE (`crates/`, binary `embed`). Entries numbered 2.x further
below belong to the deprecated Electron desktop application.

### Added
- Full-screen keyboard IDE: `embed`, `embed <directory>`, `embed <file>` (a missing file in an
  existing directory becomes a buffer written on the first `:w`).
- Editor on the shared `embedide-editor` engine: Vim modes, counts, registers, Visual mode, search,
  undo/redo, Unicode/wide characters/tabs, line numbers, staged saves that never overwrite a file
  changed on disk, `:<line>`, `:w <file>` / `:saveas <file>` (new files only), unsaved-change guards
  on close and quit.
- Automatic indentation: language-aware Enter/`o`/`O` (Rust, C, C++, Zig, JS/TS, JSON, shell, Python,
  assembly), closers that align with their opener, Tab/Shift+Tab in Insert, Normal and Visual mode,
  `native.indent` (`auto`, `width`, `style` = detect/spaces/tabs; Makefiles use tabs).
- Code completion popup while typing (nothing preselected; Down/Ctrl+N, Up/Ctrl+P, Tab/Enter, Esc;
  Ctrl+Space on demand) with semantic items from rust-analyzer, clangd and zls through a small LSP
  client, and words of the current file as a clearly labelled fallback; `native.completion`.
- Syntax highlighting for C, C++, Rust, Zig, assembly (GNU as/NASM, x86/ARM/RISC-V registers),
  Fortran, shell, Python, JavaScript/TypeScript, TOML and JSON; detection by extension, file name and
  shebang.
- Explorer: lazy tree, open/expand/collapse, create file/directory, rename, confirmed delete, refresh
  that keeps expanded directories and the selection, Ctrl+P file finder.
- Integrated terminal: the user's shell in a real PTY (`portable-pty` + `vt100` 0.16), colours and
  text attributes, terminal status-query replies (fish 4), resize, Ctrl+C/Ctrl+D, restart, process-
  group hang-up on exit, **5000-line scrollback** (Shift+PageUp/PageDown/Up/Down/Home/End; the view
  stays put while output arrives), bounded output buffering so flooding programs cannot freeze the UI.
- Builds: Make, CMake, Cargo, Zig and native `embedide.toml` projects with the compilers you select;
  F5/F7/F8, BUILD OUTPUT, PROBLEMS with jump to file:line:column, named tasks.
- Embedded: toolchain inventory (`embed doctor`, `embed toolchain`), Cortex-M templates, serial monitor
  with port/baud selection, probe-rs flashing and GDB/MI (+ OpenOCD) debugging behind explicit
  confirmations that show the exact command.
- Panels: Ctrl+Alt+1/2/3 with the kitty keyboard protocol (held digit + Delete hides a panel),
  Alt+1/2/3 in other terminals, configurable bindings (`native.keys`).
- Opt-in mouse support (`:mouse`, `native.mouse`): click to focus/place the cursor/open explorer rows,
  wheel scrolling.
- Optional AI assistant (Anthropic, OpenAI, Moonshot, xAI); nothing is sent without an explicit action.

### Fixed (during release preparation)
- `vt100` 0.15 panicked when scrolling back more than one screen; upgraded to 0.16.2.
- A flooding program could grow memory without bound and stall the UI (unbounded PTY channel).
- Explorer refresh collapsed every directory and lost the selection.
- Untitled buffers could not be saved; the error now explains `:w <file>`.
- The status bar said "EmbedIDE 2.0" while the binary is 0.1.0.
- Serial open errors now say what to do about missing devices and permissions.
- Key bindings to unknown command ids were silently ignored; they are reported at startup.

### Known limitations
- Verified on Linux x86_64 only; Windows and macOS are untested.
- No real hardware (probes, boards, USB serial) and no real AI provider were used in testing.
- See `docs/cli/status.md` and `docs/cli/getting-started.md`.

---

# Deprecated Electron desktop application

## Earlier versions (1.0.0 – 2.6.2)

These were releases of the Electron desktop application, which has been removed from the repository;
see the git history for its changelog.
