---
name: c-firmware
description: Rules for bare-metal and RTOS C firmware (C99/C11). Use when writing or changing C code for microcontrollers, drivers, interrupt handlers or startup code.
license: GPL-3.0-only
---
# C firmware

## Code
- Use `<stdint.h>`/`<stdbool.h>` fixed-width types for registers, protocol fields and sizes on the wire.
- Hardware registers and variables shared with interrupt handlers are `volatile`; read-modify-write
  of shared state happens with interrupts masked or with an atomic primitive.
- No dynamic allocation (`malloc`, `free`, VLAs) unless the project already uses an allocator.
- Interrupt handlers are short: acknowledge, record, defer the work. No blocking calls, no `printf`.
- Every function that can fail returns a status; callers check it. No silent `(void)` on errors.
- Keep `static` everything that is not part of a module's interface; one header per module with an include guard.
- Bit manipulation uses named masks (`#define UART_SR_TXE (1u << 7)`), never magic numbers.

## Build and check
- Build with warnings as errors: `-Wall -Wextra -Werror` (add `-Wconversion` if the project already passes it).
- Use the project's build (`embed build`, `make`) rather than inventing a compiler command line.
- After a change, build; if host tests exist (`make test`, `ctest`), run them.

## Never
- Never flash, erase or reset a connected board yourself; say what to run instead.
- Never change linker scripts, vector tables or clock setup unless the task asks for it.
