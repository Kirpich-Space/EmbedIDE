---
name: zig-build
description: Working on Zig projects with build.zig. Use when editing Zig code, build.zig or Zig tests.
license: MIT
---
# Zig

- Build and test with `zig build` and `zig build test`; format with `zig fmt .` (check: `zig fmt --check .`).
- Functions that can fail return error unions; propagate with `try`, handle at the boundary. No `catch unreachable`
  on input that can really fail.
- Allocation goes through an explicit `std.mem.Allocator` parameter; every allocation has a matching `defer`/`errdefer` free.
- Prefer slices over pointer+length pairs; use `std.debug.assert` for invariants.
- Add `test "…" { … }` blocks next to the code they test.
- Match the project's Zig version (see `build.zig.zon` / `minimum_zig_version`); do not use newer std APIs.
- For cross builds use `-Dtarget=` and `-Doptimize=` options the build script already defines.
