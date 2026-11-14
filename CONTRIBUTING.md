# Contributing

Thanks for helping. EmbedIDE is a Rust workspace; read [docs/cli/architecture.md](docs/cli/architecture.md)
first.

## Build and check

```sh
cargo build --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
python3 scripts/check-embed-pty.py --binary target/release/embed          # the IDE through a PTY
python3 scripts/check-workspace-pty.py --binary target/release/embed
python3 scripts/check-tui-pty.py --binary target/release/embed --cli
python3 scripts/check-native-cli.py --binary target/release/embed
python3 scripts/check-serial-bridge.py --binary target/release/embed
```

Rust 1.85+. Offline machines need the registry dependencies cached (`--offline`).
The PTY scripts need Python 3 and a Unix system; checks that need a missing tool (gcc, zig, nasm,
clangd, …) skip themselves. Report blocked checks explicitly in your PR.

## Style

- rustfmt defaults (four spaces), `snake_case` functions and modules, `PascalCase` types.
- Document public items; propagate errors explicitly; no `unsafe` (the workspace forbids it).
- Keep editor logic independent of presentation; frontends are thin adapters.
- Launch processes with explicit arguments, never through a shell string.
- Never write secrets to files, logs or error messages. Keep file saves atomic and refuse to overwrite
  files that changed on disk.
- Do not touch hardware without an explicit user command.

## Tests

- Add a regression test for every fixed bug, especially Unicode boundaries, undo/redo, file integrity
  and key sequences.
- Use disposable fixtures (temporary directories, mock CLIs); never real projects, real credentials
  or connected hardware. Mock the AI CLIs with `crates/embedide-ai/tests/fixtures/`.
- User-visible behaviour belongs in `scripts/check-embed-pty.py` too: it drives the real binary.

## Commits and pull requests

- Imperative subject ("Fix …", "Add …"), a body that explains why, focused commits.
- A PR explains the behaviour change, affected platforms and validation results (including blocked
  checks), links issues, and includes screenshots for UI changes.
- Update `CHANGELOG.md` and the relevant guide in `docs/cli/` with the change.

## Security

Do not open public issues for vulnerabilities: see [SECURITY.md](SECURITY.md).
