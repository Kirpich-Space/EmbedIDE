# Architecture

EmbedIDE is a Cargo workspace. Everything that is not drawing characters on a terminal lives in
frontend-independent crates, so another frontend (a future native GUI) can reuse it.

```text
                         embed (embedide-cli)
                 subcommands, JSON envelopes, `embed` IDE launcher
                                  │
        ┌─────────────────────────┼───────────────────────────┐
 embedide-terminal           embedide-build                 embedide-ai
 TUI: panels, keys,          prepare → plan → run:          providers, CLI backends,
 theme, PTY terminal         native / Make / CMake /        agent runtime, policy,
        │                    Cargo / Zig                    skills, completion engine
        │                           │                            │
 embedide-editor             embedide-toolchains          embedide-workspace
 piece table, undo,          discover / select            embedide.toml, config.toml,
 Vim input, syntax,          compilers (installs          templates, atomic writes
 indent, completion          nothing)
        │
 embedide-core ── processes, LSP client, diagnostics, ELF/HEX/disassembly,
                  explorer, serial, GDB/MI, native config
 embedide-project ── project discovery, legacy embedide.json compatibility
```

## Crates

| Crate | Responsibility | Notes |
|---|---|---|
| `embedide-editor` | UTF-8 piece-table buffer with bounded undo, Vim-style input parser (`Key` → `Action`), executor, syntax lexers and incremental highlighter, indentation, completion ranking, assembly dialects | no I/O, no terminal types; offsets are bytes on scalar boundaries |
| `embedide-core` | cancellable processes (own process group, no shell), LSP client, diagnostics parsers (GCC/Clang/rustc/NASM/GAS), paged binary file and ELF reader, objdump wrapper, explorer, serial, GDB/MI, `NativeConfig` | shared by CLI and TUI |
| `embedide-workspace` | `embedide.toml` manifest, user `config.toml` (including `[ai]`), project templates, atomic file writes | unknown project metadata is preserved |
| `embedide-toolchains` | detection, versions, targets and selection of compilers | never downloads or installs |
| `embedide-build` | turns project + profile + toolchain choice into a reviewable plan, then runs it | no shell; process-group cancel; incremental via depfiles |
| `embedide-ai` | provider wire formats, Claude Code / Codex CLI backends, agent runtime and permission rules, snapshots, skills, inline completion engine | credentials never read from CLI config |
| `embedide-terminal` | the full-screen TUI (crossterm, vt100 + portable-pty for the terminal panel) and the line/batch editor frontends | thin adapter over the crates above |
| `embedide-cli` | `embed` binary: clap subcommands, `--json` envelopes, serial bridge | |
| `embedide-project` | non-destructive project discovery and legacy `embedide.json` support | |

Workspace policy: `unsafe_code = "forbid"`, `missing_docs = "warn"`, Rust 1.85+.

## The TUI

`tui/workspace.rs` owns the application state and key routing. One event loop: poll terminal events
(20–50 ms), handle them, `tick()` to collect results from background services, redraw only when
something changed (the screen writer diffs frames and sends only changed cells).

Key routing order: panel chords → dialogs → completion popup → global shortcuts → focused panel
(terminal passes keys through; AI panel; explorer; output panels; editor). The editor branch lets an
AI ghost suggestion take `Tab`/`Esc` first, then the Vim input.

Background services run on threads and report through channels, never blocking the loop:
builds (`ProcessTask`), LSP servers, the terminal PTY, serial, the AI worker (chat, agent, inline
engine, backend checks). `Drop`/shutdown stops all of them, including child process groups.

## AI and agents

```text
 AI panel ──► Assistant ──► chat / edit / complete ──► Backend (HTTP API | Claude Code CLI | Codex CLI)
                  │
                  └──► agent::Session::run(task, Host, Cancel)
                          ├─ Snapshot (before) ─────────────► changes / diff / revert (after)
                          ├─ Claude Code: stream-json host protocol; each can_use_tool request
                          │     → Rules (policy) → allow | ask the user via Host | deny
                          ├─ Codex: `exec --json` in its sandbox; plan → approve → resume
                          └─ events (steps, commands, files, denials) → Host → panel
```

- The permission rules (`agent/policy.rs`) parse shell commands without running a shell and ask when
  unsure; they only narrow what the CLI allows and never pass bypass flags.
- Skills (`skills.rs`) are discovered by frontmatter, loaded on demand, and quoted into the task;
  built-in ones are EmbedIDE's own text, file skills are untrusted and never reach the rules.
- Child CLIs run in their own process group with a scrubbed environment (no parent-session variables,
  no API keys unless `ai.cli.allow_api_key`); stop = SIGTERM, 0.5 s, SIGKILL on the group.

## Safety rules the code keeps

- Processes are launched with explicit argument vectors, never through shell interpolation.
- Files are written atomically (temporary file, flush, rename); saves refuse to overwrite a file that
  changed on disk; new files are created with exclusive opens.
- Hardware actions (flash, debug, serial) start only from explicit commands.
- Secrets are never written to configuration, logs or error messages (`redact`, `sanitize`).
- Output from programs, files and models is stripped of terminal control sequences before display.
- Everything bounded: output buffers, line lengths, history, scrollback, snapshot size.

## Testing layers

| Layer | Where |
|---|---|
| unit and integration tests | `crates/*/src` (`#[cfg(test)]`) and `crates/*/tests/` |
| mock CLIs and servers | `crates/embedide-ai/tests/fixtures/mock_cli.py`, `mock_agent.py`; a local HTTP mock for the API providers |
| the real binary through a pseudo-terminal | `scripts/check-embed-pty.py`, `check-workspace-pty.py`, `check-tui-pty.py`, `check-terminal-pty.py` |
| CLI contract | `scripts/check-native-cli.py`, `check-serial-bridge.py` |
| real tools | tests skip themselves when gcc, clang, zig, nasm, rust-analyzer, … are missing |

See [status.md](status.md) for what has been verified with real tools and what has not.
