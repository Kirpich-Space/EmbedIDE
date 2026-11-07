# CLI status — verified vs. unverified (2026-11-08)

Environment of record: Linux x86_64 (Artix), rustc 1.99.1. Evidence: `cargo test --workspace`,
`crates/embedide-cli/tests/workflows.rs` (real compilers), `crates/embedide-cli/tests/ai.rs` and
`crates/embedide-ai/tests/mock_providers.rs` (mock servers), `scripts/check-*.py` (PTY/serial).

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

## Verified only with mocks / fakes

| Area | How |
|---|---|
| Anthropic Messages API and OpenAI-style Chat Completions (OpenAI, Moonshot, xAI): request format, headers, streaming, usage, model listing, 401/429/5xx mapping, key redaction | local HTTP mock server |
| `ai chat`, `ai complete`, `ai edit` (diff, `--apply`), `ai login/logout`, offline guard, dry run | CLI against the mock server |
| Subscription mode (Claude Code print mode) | fake `claude` script; Codex path not exercised |
| Completion engine debounce/supersede/cancel | in-process fake backend |

## Not verified

- **Any real AI provider request or real subscription CLI** (not authorized: would cost money).
- **Fortran builds**: no gfortran, flang or lfortran is installed on the verification machine.
- **Windows and macOS runtime.** Windows: `cargo clippy -D warnings` passes for
  `x86_64-pc-windows-gnu` on every crate except those depending on `ureq` (its `ring` dependency needs a
  MinGW C compiler that is not installed); nothing was executed on Windows. macOS: not built.
- **Hardware**: no flashing, debugging or physical serial device. The GDB/MI, probe-rs and serial
  services in `embedide-core` are unchanged and not exposed as automatic CLI actions.
- LFortran and Flang command lines (implemented from their documented flags, not run).
- NASM output formats other than elf64.

## Known limitations

- Native compiles run sequentially; `--jobs` applies to Make/CMake/Cargo/Zig.
- Fortran module dependencies are not scanned (compile order = directory order).
- `ai login` reads the key with echo when typed interactively; piping is recommended.
- Credentials are stored in a 0600 file, not in the OS keychain.
- The Electron app (`electron/`, `src/`) is deprecated and was not changed by the CLI work.
