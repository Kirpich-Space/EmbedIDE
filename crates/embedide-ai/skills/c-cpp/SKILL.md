---
name: c-cpp
description: Writing, building and debugging C and C++ on the host (GCC, Clang, Make, CMake). Use for any C or C++ source in the project.
license: GPL-3.0-only
---
# C and C++

## Building
- Use the project's build first (`embed build .`, `make`, `cmake --build build`). For loose sources:
  `cc -std=c11 -Wall -Wextra -Wpedantic -g -o app *.c` (C++: `c++ -std=c++17 -Wall -Wextra -g -o app *.cpp`).
- Keep the compiler and standard the project uses (`-std=` in the Makefile/CMake/embedide.toml).
- For a debugging session on the host add sanitizers to a separate build:
  `-fsanitize=address,undefined -fno-omit-frame-pointer -g`, then run with `timeout 10`.
- Makefile recipes are indented with **tabs**; keep variables (`CC`, `CFLAGS`, `LDFLAGS`, `SRCS`).
- Link order: objects first, then libraries (`-lm` after the objects that use it).

## Reading diagnostics
- `implicit declaration of function` → missing `#include` or prototype; add the header, do not
  declare library functions by hand.
- `undefined reference to` → the defining `.c` is not compiled/linked, or a name/signature mismatch
  (C++: missing `extern "C"` around C headers).
- `format '%d' expects argument of type 'int'` → use the matching specifier (`%zu` for `size_t`,
  `%ld` for `long`, `PRIu32` from `<inttypes.h>` for fixed-width types).
- `comparison of integer expressions of different signedness` → fix the types, not with a cast
  hiding a real bug.
- Fix the first error first; later errors are often consequences.

## Correctness rules
- No undefined behaviour: signed overflow, out-of-range shifts, reading uninitialised memory,
  out-of-bounds indexing, use after free, data races. Watch loop counters against their bounds
  (an `int` counter compared with a constant larger than `INT_MAX` overflows — use a wider type or
  fix the bound).
- Check every allocation, `fopen`, `read`/`write` and library call that can fail; free what you
  allocate on every path; close files.
- Prefer `size_t` for sizes and indexes, `const` for pointers that are not written, `static` for
  file-local functions and data.
- Headers: include guards (or `#pragma once` where the project uses it), only declarations,
  include what you use.
- Strings: bounded functions (`snprintf`, `strncpy` with explicit termination); never `gets`.
- C++: RAII and standard containers instead of raw `new`/`delete`; `std::string_view`/`std::span`
  for non-owning views; no `using namespace std;` in headers.

## Testing
- Use the project's tests (`make test`, `ctest`); otherwise write a small test program or `assert`s
  next to the change and run it with `timeout 10`.
