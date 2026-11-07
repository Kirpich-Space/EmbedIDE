# Repository Guidelines

## Project Structure & Module Organization

- `crates/` contains the native Rust workspace: `embedide-project`, `embedide-core`, `embedide-editor`, `embedide-cli`, and `embedide-terminal`. Keep editor logic independent of presentation; frontends share the existing engine.
- `src/ui/`, `src/core/`, and `src/themes/` contain the existing React/TypeScript desktop interface; `electron/` contains its main process and embedded-tool integrations.
- Rust integration tests live in each crate’s `tests/`; editor benchmarks live in `crates/embedide-editor/benches/`. `scripts/` contains verification and packaging helpers.
- `public/` holds fonts/icons; `build/` and `packaging/` support desktop distribution. Read `docs/native/` for migration decisions and verified limitations.

## Build, Test, and Development Commands

Run commands from the repository root:

```sh
npm run dev                  # Vite and Electron development
npm run typecheck            # Strict TypeScript checks
npm run build                # Build the desktop renderer
cargo check --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p embedide-cli --locked -- doctor --json
```

Install desktop dependencies with `npm install` when needed. Rust requires 1.85+; offline workspace checks require cached registry dependencies. Without that cache, use `python3 scripts/check-editor-offline.py --terminal`; this validates only editor/terminal sources, not the full CLI/workspace, and now requires the cached unicode-segmentation dependency. Add `--bench` for editor measurements. See `docs/native/terminal-editor.md` for binary export and launch instructions.

## Coding Style & Naming Conventions

Use rustfmt’s four-space Rust formatting, `snake_case` functions/modules, and `PascalCase` types. Document public APIs, propagate errors explicitly, and retain the workspace’s `unsafe_code = "forbid"` policy.

Match surrounding TypeScript/JavaScript: two-space indentation, `PascalCase` components, `camelCase` functions, and local semicolon conventions. Electron uses CommonJS. Avoid unrelated formatting changes.

## Testing Guidelines

Use Rust `#[test]` with descriptive `snake_case` behavior names. Add regression tests for corrected defects, especially Unicode boundaries, undo/redo, file integrity, and frontend key sequences. Use disposable fixtures rather than real projects or hardware.

Run Unix terminal checks with `python3 scripts/check-terminal-pty.py --binary PATH`. Run project workspace PTY checks with `python3 scripts/check-workspace-pty.py --binary target/debug/embed`. No coverage percentage or npm test script is configured. Report blocked checks explicitly; existing binaries do not validate changed sources.

## Commit & Pull Request Guidelines

History uses imperative subjects such as “Fix …” and “Add …”. Keep commits focused. PRs should explain behavior changes, affected platforms, and validation results, including blocked checks. Link relevant issues; include screenshots for UI changes.

## Migration & File Safety

Preserve uncommitted changes and the working desktop implementation. Retain file-save safeguards and unknown project metadata fields. Launch processes with explicit arguments rather than shell interpolation. Do not assume glibc, systemd, or a particular package manager.
