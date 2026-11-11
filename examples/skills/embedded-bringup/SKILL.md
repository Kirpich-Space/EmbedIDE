---
name: embedded-bringup
description: Board bring-up and debugging for microcontroller projects (STM32, nRF, RP2040, ESP32, AVR). Use when a task touches clocks, pins, peripherals, linker scripts, startup code or flashing.
license: MIT
---
# Embedded bring-up

## Before changing anything
- Find the target: MCU, board, toolchain and build system (embedide.toml, Makefile, CMakeLists.txt, platformio.ini).
- Read the existing startup code, linker script and clock setup; state the memory map you are assuming.
- Cite the register or reference-manual section you rely on in a comment next to the code.

## Changes
- Change one peripheral at a time and keep the previous configuration recoverable.
- Pin and clock changes must not affect pins used by the debugger (SWD/JTAG) or the boot pins.
- Respect existing HAL/LL or bare-register style; do not mix them in one driver.

## Build, flash, observe
- Build with the project build and report the size (`arm-none-eabi-size`, `size`).
- Flashing, erasing, resetting a board and opening serial ports are actions the user approves:
  propose the exact command (openocd, probe-rs, st-flash, esptool, avrdude, picotool) and let EmbedIDE ask.
- Describe how to verify on hardware: expected serial output, LED pattern, logic-analyzer trace.
