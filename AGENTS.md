# Repository Guidelines

## Project Structure & Module Organization

- `crates/` contains the native Rust workspace (nine crates; see `docs/cli/architecture.md`): `embedide-editor` (piece-table buffer, Vim input, syntax), `embedide-core` (processes, LSP, diagnostics, ELF/HEX, serial, GDB/MI), `embedide-workspace` (`embedide.toml`, config, atomic writes), `embedide-toolchains` (compiler discovery, never installs), `embedide-build` (reviewable build plans), `embedide-ai` (providers, Claude Code/Codex backends, agent policy, skills), `embedide-terminal` (full-screen TUI), `embedide-cli` (the `embed` binary), and `embedide-project` (project discovery, legacy `embedide.json`). Keep editor logic independent of presentation; frontends share the existing engine.
- Rust integration tests live in each crate’s `tests/`; editor benchmarks live in `crates/embedide-editor/benches/`. `scripts/` contains verification helpers. Read `docs/native/` (historical design notes from the migration away from the removed Electron app) and `docs/cli/` for verified limitations.

## Build, Test, and Development Commands

Run commands from the repository root:

```sh
cargo check --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p embedide-cli --locked -- doctor --json
```

Rust requires 1.85+; offline workspace checks require cached registry dependencies. Without that cache, use `python3 scripts/check-editor-offline.py --terminal`; this validates only editor/terminal sources, not the full CLI/workspace, and now requires the cached unicode-segmentation dependency. Add `--bench` for editor measurements. See `docs/native/terminal-editor.md` for binary export and launch instructions.

## Coding Style & Naming Conventions

Use rustfmt’s four-space Rust formatting, `snake_case` functions/modules, and `PascalCase` types. Document public APIs, propagate errors explicitly, and retain the workspace’s `unsafe_code = "forbid"` policy.

Avoid unrelated formatting changes.

## Testing Guidelines

Use Rust `#[test]` with descriptive `snake_case` behavior names. Add regression tests for corrected defects, especially Unicode boundaries, undo/redo, file integrity, and frontend key sequences. Use disposable fixtures rather than real projects or hardware.

Run Unix terminal checks with `python3 scripts/check-terminal-pty.py --binary PATH`. Run project workspace PTY checks with `python3 scripts/check-workspace-pty.py --binary target/debug/embed`. No coverage percentage is configured. Report blocked checks explicitly; existing binaries do not validate changed sources.

## Commit & Pull Request Guidelines

History uses imperative subjects such as “Fix …” and “Add …”. Keep commits focused. PRs should explain behavior changes, affected platforms, and validation results, including blocked checks. Link relevant issues; include screenshots for UI changes.

## Migration & File Safety

Preserve uncommitted changes. Retain file-save safeguards and unknown project metadata fields. Launch processes with explicit arguments rather than shell interpolation. Do not assume glibc, systemd, or a particular package manager.
