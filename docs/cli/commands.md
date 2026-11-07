# `embedide` command reference

Global flag: `--json` — print one JSON envelope on stdout
(`{"ok": true, "data": …}` or `{"ok": false, "error": {"kind": "usage"|"operation", "message": …}}`);
compiler and tool output always goes to stderr.
Exit codes: `0` success, `1` operation error, `2` usage error, a failing build returns the compiler's
exit code, `130` when a build is cancelled with Ctrl+C.

Configuration: user settings in `config.toml` inside the configuration directory
(`$XDG_CONFIG_HOME/embedide` or `~/.config/embedide` on Linux, `~/Library/Application Support/EmbedIDE`
on macOS, `%APPDATA%\EmbedIDE` on Windows; `EMBEDIDE_CONFIG_DIR` overrides it). Projects use
[`embedide.toml`](embedide-toml.md).

## Environment

| Command | What it does |
|---|---|
| `embedide doctor` | OS, configuration file, toolchains available per language. Never probes hardware. |
| `embedide targets` | The 67-entry MCU catalog (metadata only, not hardware validation). |
| `embedide toolchains` | Legacy PATH inventory (does not execute tools). |

## Toolchains — you choose, nothing is installed

| Command | What it does |
|---|---|
| `toolchain list [--language L] [--no-probe]` | Installed/configured toolchains: id, kind, languages, host/cross, version, path, target. |
| `toolchain detect` | Same, plus languages that have no toolchain. |
| `toolchain info <id>` | Details for one id (`gcc`, `clang`, `zig-cc`, `arm-none-eabi-gcc`, a custom id). |
| `toolchain add <id> --kind K --path P` | Register any executable (absolute path; spaces are fine). It must answer a version query. |
| `toolchain remove <id>` | Remove a toolchain added with `add`. |
| `toolchain default <language> <id>` | Your default for projects that do not choose. |
| `toolchain search-dir <dir>` | Search an extra directory after PATH (e.g. a vendor `bin/`). |
| `toolchain recommend [language]` | Installation suggestions for your package manager. **Printed, never executed.** |

Kinds: `gcc`, `g++`, `clang`, `clang++`, `zig-cc` (`zig cc`), `zig-c++` (`zig c++`), `rustc`,
`cargo`, `zig`, `gas`, `nasm`, `gfortran`, `flang`, `lfortran`. Triple-prefixed GCCs
(`arm-none-eabi-gcc`, `riscv64-unknown-elf-gcc`, …) are found automatically as cross compilers.

Selection order for a language: `--toolchain` on the command line → profile → project
`[toolchain]` → your `toolchain default` → (host builds only) the first installed of
gcc/clang/zig-cc (C), g++/clang++/zig-c++ (C++), gfortran/flang/lfortran (Fortran). Projects with a
`[target]` never get an automatic choice. GCC cannot switch targets, so a GCC whose default target
differs from `target.triple` is refused with an explanation; Clang and `zig cc` receive
`--target` / `-target`. A host GCC is refused for a board project.

## Projects

| Command | What it does |
|---|---|
| `project init [PATH] [--name N] [--language L] [--template T] [--toolchain lang=id]… [--board B]` | Create a project from a template. In an existing non-empty directory it **adopts** the project: only `embedide.toml` is written, with the detected build system; no other file is touched. |
| `project info [PATH] [--profile P]` | Manifest summary and the toolchain each language would use (and why). |
| `project templates` | `c`, `cpp`, `rust`, `zig`, `asm`, `fortran`, `c-cortex-m`. |
| `init`, `open`, `edit` | Legacy: register with `embedide.json`; terminal workspace UI; terminal editor. |

## Build

| Command | What it does |
|---|---|
| `build [PATH] [--profile P] [--toolchain lang=id]… [--dry-run] [--jobs N] [--online]` | Build with the selected toolchains. `--dry-run` prints every command. Ctrl+C cancels the whole process group. |
| `build --release` | Shorthand for `--profile release`. |
| `build --build-system S` | Only for projects without `embedide.toml` (legacy path). |
| `clean [PATH] [--dry-run]` | Native: removes the output directory **only** if it carries EmbedIDE's `.embedide-output` marker and lies inside the project. CMake: removes the EmbedIDE CMake build dir. Make/Cargo: runs `make clean` / `cargo clean`. |
| `terminal [PATH] [--profile P]` | Starts `$SHELL` (`%COMSPEC%` on Windows) in the project with its `[env]` and the selected toolchains' directories first on PATH. |

Native builds compile each source with the chosen compiler (no shell, absolute paths) and rebuild
incrementally: an object is recompiled when it is missing, its exact command line changed, or the
source or any header listed in the compiler's depfile is newer. Make/CMake/Cargo/Zig projects keep
their build system; EmbedIDE passes `CC=`/`CXX=`/`FC=` (Make) or `-DCMAKE_<LANG>_COMPILER=` (CMake)
when you selected a compiler. Make cannot take a compiler path containing spaces; EmbedIDE refuses
instead of producing a broken command.

## Configuration

| Command | What it does |
|---|---|
| `config list [--project DIR]` | Every key of the user config (or the project's `embedide.toml`). |
| `config get KEY [--project DIR]` | One value. |
| `config set KEY VALUE [--project DIR]` | VALUE is parsed as TOML (`true`, `3`, `["-O2"]`), otherwise stored as a string. The file is validated before it is written; comments are preserved. Keys that look like credentials are refused. |
| `config unset KEY [--project DIR]` | Remove a key. |
| `config path` | File locations. |
| `config show [PATH]` | Legacy `embedide.json` metadata. |

## AI

See [ai.md](ai.md). `ai providers` never uses the network; `ai models`, `ai chat`,
`ai complete` and `ai edit` do, only when you run them, and print what they send first.

## Firmware, debugging, serial

EmbedIDE does not flash or start debuggers on its own. Use your tools from `embedide terminal`
(`openocd`, `gdb`, `probe-rs`, `qemu-system-*`). The Rust crates keep the GDB/MI service,
probe-rs flash planning (explicit confirmation) and the serial monitor; `embedide serial-bridge`
exposes the serial monitor to front ends as JSON lines on stdin/stdout.
