# EmbedIDE bundled toolchains

Heavy IDE mode: compilers and debuggers ship inside the app so users do not install system packages.

## Fetch (developers / release builds)

```bash
npm run toolchain:fetch
# optional Rust (large):
npm run toolchain:fetch:rust
```

Downloads into `vendor/toolchain/<platform>/`:

| Tool | Source |
|------|--------|
| arm-none-eabi-gcc / g++ / gdb / objcopy / size | xPack |
| openocd | xPack |
| zig | ziglang.org |
| make, python3 | copied from host when present |

Packaged apps put this tree under `resources/toolchain/`. At runtime EmbedIDE prepends those `bin` dirs to `PATH` and prefers them over system tools.

## Layout

```
vendor/toolchain/linux-x64/
  bin/           # symlinks/copies of tools
  gcc/           # unpacked xPack GCC
  openocd/       # unpacked OpenOCD (+ scripts)
  zig/
  manifest.json
```
