# `embedide.toml` (schema 1)

Unknown keys are errors, so typos are caught. Paths are relative to the project and must stay inside
it. Credentials do not belong here (`config set` refuses credential-looking keys; `project info`
warns about credential-looking `[env]` names).

```toml
schema = 1

[project]
name = "sensor-node"          # letters, digits, '-', '_', '.'
language = "c"                # c | cpp | rust | zig | asm | fortran
description = "optional"

[build]
system = "native"             # native | make | cmake | cargo | zig
sources = ["src"]             # scanned recursively (hidden dirs, target/, zig-out/ skipped)
include = ["include"]
output = "build"              # artifacts in build/<profile>/, objects in build/<profile>/obj/
artifact = "sensor-node.elf"  # default: project name (+ .exe for Windows host builds)
defines = ["BOARD_REV=2"]
flags = ["-Wall", "-Wextra"]  # every C/C++/ASM/Fortran compile
link_flags = []
libs = ["m"]                  # → -lm
linker_script = "linker.ld"   # → -T linker.ld

[toolchain]                   # language → toolchain id (see `embedide toolchain list`)
c = "arm-none-eabi-gcc"
asm = "arm-none-eabi-gcc"     # .S/.s go through a C driver or GNU as; .asm uses NASM

[target]
triple = "thumbv7em-none-eabihf"   # Clang/zig cc/rustc get --target; GCC must already target it
flags = ["-mcpu=cortex-m4", "-mthumb", "-mfpu=fpv4-sp-d16", "-mfloat-abi=hard"]
board = "stm32f407vg"         # catalog id, informational

[profiles.debug]
flags = ["-O0", "-g"]
cargo_profile = "dev"

[profiles.release]
flags = ["-O2", "-g"]
defines = ["NDEBUG"]
cargo_profile = "release"

[profiles.clang-check]        # several configurations of one project
flags = ["-O1", "-g", "-fsanitize=address"]
toolchain = { c = "clang" }

[env]                         # for builds and `embedide terminal`
SDK_ROOT = "/opt/vendor sdk"

[ai]
exclude = ["vendor/**", "*.bin", "keys/**"]   # never sent to an AI provider

[debug]                       # kept verbatim for debugger front ends; never auto-started
openocd_interface = "stlink"
openocd_target = "stm32f4x"
gdb = "arm-none-eabi-gdb"
```

## Build systems

- `native` — EmbedIDE drives the compilers. C (`.c`), C++ (`.cpp .cc .cxx`), GNU assembly
  (`.S .s .sx`), NASM (`.asm .nasm`) and Fortran (`.f .f90 .f95 .f03 .f08` and upper-case variants)
  are compiled per file and linked by the driver of the highest language present
  (Fortran > C++ > C). For `language = "zig"` EmbedIDE runs `zig build-exe src/main.zig`; for
  `language = "rust"` with `native` it runs `rustc src/main.rs`.
- `make`, `cmake`, `cargo`, `zig` — the project's own build files are used unchanged.

Limitations: Fortran module dependencies are compiled in directory order (no module dependency
scan yet); native compiles run sequentially.
