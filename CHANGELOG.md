# Changelog

## `embed` 0.2.0 — in development (`0.2.0-dev`, not released)

### Changed
- Official EmbedIDE theme (`Desige/embedide-palette.json`) in one module,
  `crates/embedide-terminal/src/tui/theme.rs`: Carbon (dark) and Bone (light) tokens, syntax colours
  (`syn-*`: rust keywords, ink code, ceramic jumps) and semantic UI roles — errors red, warnings amber,
  success green, AI cyan, notes muted, rust accent only for prompts and changed files. Bars are bone on
  Carbon `#1C1C20`. Build output and PROBLEMS lines are coloured by severity; AI inline suggestions
  are cyan italics instead of an inverse block. Truecolor terminals get exact tokens, 256-colour
  terminals the nearest ones; tests check WCAG contrast (≥ 4.5:1) of every text colour.

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
