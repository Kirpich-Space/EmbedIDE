const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, cpuFlags, memLength, DEFAULT_BOARD_ID } = require('./boards');

const ALLOWED_HIDDEN = new Set(['.cargo', '.gitignore', '.editorconfig', 'rust-toolchain.toml', 'embedide.json'])

function linkerMemory(board) {
  return `MEMORY
{
    FLASH (rx)  : ORIGIN = ${board.flashOrigin}, LENGTH = ${memLength(board.flashKb)}
    RAM (xrw)   : ORIGIN = ${board.ramOrigin}, LENGTH = ${memLength(board.ramKb)}
}`
}

function linkerScript(board) {
  return `${linkerMemory(board)}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .isr_vector : { KEEP(*(.isr_vector)) } > FLASH
    .text : { *(.text*) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM AT > FLASH
    .bss : { *(.bss*) *(COMMON) } > RAM
}
`
}

function asmLinkerScript(board) {
  return `${linkerMemory(board)}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .isr_vector : { KEEP(*(.isr_vector)) } > FLASH
    .text : { *(.text*) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM AT > FLASH
    .bss : { *(.bss*) *(COMMON) } > RAM
}
`
}

function rustMemory(board) {
  return `MEMORY
{
  FLASH : ORIGIN = ${board.flashOrigin}, LENGTH = ${memLength(board.flashKb)}
  RAM   : ORIGIN = ${board.ramOrigin}, LENGTH = ${memLength(board.ramKb)}
}
`
}

function startupAsm(board) {
  return `.syntax unified
.cpu ${board.cpu}
.thumb

.global g_pfnVectors
.global Default_Handler
.global Reset_Handler

.section .text.Reset_Handler
.thumb_func
.weak Reset_Handler
.type Reset_Handler, %function
Reset_Handler:
    ldr r0, =_estack
    mov sp, r0
    bl SystemInit
    bl main
1:  b 1b
.size Reset_Handler, .-Reset_Handler

.section .text.Default_Handler,"ax",%progbits
.thumb_func
Default_Handler:
    b Default_Handler

.section .isr_vector,"a",%progbits
.type g_pfnVectors, %object
g_pfnVectors:
    .word _estack
    .word Reset_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
.size g_pfnVectors, .-g_pfnVectors
`
}

function buildTemplates(board) {
  const cpu = cpuFlags(board)
  const flashCfg = `-f interface/${board.defaultAdapter}.cfg -f target/${board.openocdTarget}.cfg`

  return {
    rust: {
      name: 'Rust (Cortex-M)',
      ext: '.rs',
      files: {
        'Cargo.toml': (name) => `[package]
name = "${name}"
version = "0.1.0"
edition = "2021"

[dependencies]
cortex-m-rt = "0.7"
cortex-m-semihosting = "0.5"
panic-halt = "0.2"

[[bin]]
name = "${name}"
path = "src/main.rs"
`,
        'src/main.rs': () => `#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;

#[entry]
fn main() -> ! {
    // ${board.mcu} — EmbedIDE bare-metal entry
    loop {}
}
`,
        '.cargo/config.toml': () => `[target.${board.rustTarget}]
rustflags = ["-C", "link-arg=-Tlink.x"]

[build]
target = "${board.rustTarget}"
`,
        'memory.x': () => rustMemory(board),
        'build.rs': () => `fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-search=.");
}
`,
      },
    },

    c: {
      name: 'C (ARM Cortex-M)',
      ext: '.c',
      files: {
        'Makefile': (name) => `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build

C_SOURCES = $(wildcard $(SRC_DIR)/*.c)
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)

OBJECTS = $(C_SOURCES:$(SRC_DIR)/%.c=$(BUILD_DIR)/%.o) \\
         $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)

PREFIX = arm-none-eabi-
CC = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = ${cpu}
DEFINES = -D${board.cDefine}
CFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections -Wl,-Map=$(BUILD_DIR)/$(TARGET).map

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.c
\t@mkdir -p $(BUILD_DIR)
\t$(CC) $(CFLAGS) $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(CC) $(CFLAGS) $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(CC) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
        'src/main.c': () => `#include <stdint.h>

/* Target: ${board.mcu} (${board.family}) */

int main(void) {
    while (1) {
        /* Application loop */
    }
}

void SystemInit(void) {}
`,
        'src/startup.S': () => startupAsm(board),
        'src/uart.c': () => `#include "uart.h"

void uart_init(void) {
    /* UART initialization for ${board.mcu} */
}

int uart_send(const uint8_t *data, uint16_t len) {
    (void)data;
    (void)len;
    return 0;
}
`,
        'src/uart.h': () => `#ifndef UART_H
#define UART_H

#include <stdint.h>

void uart_init(void);
int uart_send(const uint8_t *data, uint16_t len);

#endif
`,
        'linker.ld': () => linkerScript(board),
      },
    },

    cpp: {
      name: 'C++ (ARM Cortex-M)',
      ext: '.cpp',
      files: {
        'Makefile': (name) => `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build

CXX_SOURCES = $(wildcard $(SRC_DIR)/*.cpp)
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)

OBJECTS = $(CXX_SOURCES:$(SRC_DIR)/%.cpp=$(BUILD_DIR)/%.o) \\
         $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)

PREFIX = arm-none-eabi-
CXX = $(PREFIX)g++
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = ${cpu}
DEFINES = -D${board.cDefine}
CXXFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections -fno-exceptions -fno-rtti
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections -Wl,-Map=$(BUILD_DIR)/$(TARGET).map

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.cpp
\t@mkdir -p $(BUILD_DIR)
\t$(CXX) $(CXXFLAGS) $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(PREFIX)gcc $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(CXX) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
        'src/main.cpp': () => `#include <cstdint>

/* Target: ${board.mcu} (${board.family}) */

class Application {
public:
    void init() {}
    void run() {
        while (1) {}
    }
};

int main() {
    Application app;
    app.init();
    app.run();
}

extern "C" void SystemInit() {}
`,
        'src/startup.S': () => startupAsm(board),
        'linker.ld': () => linkerScript(board),
      },
    },

    asm: {
      name: 'Assembly (ARM Cortex-M)',
      ext: '.S',
      files: {
        'Makefile': (name) => `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build

ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)

OBJECTS = $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)

PREFIX = arm-none-eabi-
AS = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = -mcpu=${board.cpu} -mthumb
ASFLAGS = $(CPU) -c -g -Wall
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(AS) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
        'src/main.S': () => `.syntax unified
.cpu ${board.cpu}
.fpu softvfp
.thumb

/* Target: ${board.mcu} */

.global _start
.global Default_Handler
.global main

.section .text

.thumb_func
_start:
    ldr r0, =_estack
    mov sp, r0
    bl main
    b .

.thumb_func
main:
    b main

.thumb_func
Default_Handler:
    b Default_Handler

.section .isr_vector,"a",%progbits
g_pfnVectors:
    .word _estack
    .word _start
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
    .word Default_Handler
`,
        'linker.ld': () => asmLinkerScript(board),
      },
    },

    zig: {
      name: 'Zig (Cortex-M)',
      ext: '.zig',
      files: {
        'Makefile': (name) => zigMakefile(name, board, flashCfg),
        'src/main.zig': () => zigMainSource(board, `//! Target: ${board.mcu} (${board.family})
//! Bare-metal Zig entry for Cortex-M

export fn _start() callconv(.C) noreturn {
    // EmbedIDE Zig firmware entry
    while (true) {
        asm volatile ("nop");
    }
}
`),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}

Zig bare-metal firmware for **${board.mcu}**.

\`\`\`bash
make        # requires zig + arm-none-eabi-binutils
make flash  # OpenOCD
\`\`\`
`,
      },
    },

    ...buildDriverTemplates(board, cpu, flashCfg),
    ...buildOsTemplates(board, cpu, flashCfg),
  }
}

function cFamilyMakefile(name, board, cpu, flashCfg, { cxx = false } = {}) {
  if (cxx) {
    return `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build
CXX_SOURCES = $(wildcard $(SRC_DIR)/*.cpp)
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)
OBJECTS = $(CXX_SOURCES:$(SRC_DIR)/%.cpp=$(BUILD_DIR)/%.o) \\
         $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)
PREFIX = arm-none-eabi-
CXX = $(PREFIX)g++
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size
CPU = ${cpu}
DEFINES = -D${board.cDefine}
CXXFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections -fno-exceptions -fno-rtti -Iinclude
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.cpp
\t@mkdir -p $(BUILD_DIR)
\t$(CXX) $(CXXFLAGS) $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(PREFIX)gcc $(CPU) -c $(DEFINES) -O2 -g -Wall $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(CXX) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`
  }
  return `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build
C_SOURCES = $(wildcard $(SRC_DIR)/*.c)
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)
OBJECTS = $(C_SOURCES:$(SRC_DIR)/%.c=$(BUILD_DIR)/%.o) \\
         $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)
PREFIX = arm-none-eabi-
CC = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size
CPU = ${cpu}
DEFINES = -D${board.cDefine}
CFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections -Iinclude
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.c
\t@mkdir -p $(BUILD_DIR)
\t$(CC) $(CFLAGS) $< -o $@

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(CC) $(CFLAGS) $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(CC) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`
}

function zigTargetTriple(board) {
  return board.floatAbi === 'hard' ? 'thumb-freestanding-eabihf' : 'thumb-freestanding-eabi'
}

function zigMakefile(name, board, flashCfg) {
  const zigCpu = String(board.cpu || 'cortex-m4').replace(/-/g, '_')
  const zigTarget = zigTargetTriple(board)
  return `TARGET = ${name}
BUILD_DIR = build
ZIG = zig
CPU = ${zigCpu}

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/$(TARGET).elf: src/main.zig linker.ld
\t@mkdir -p $(BUILD_DIR)
\t$(ZIG) build-exe src/main.zig \\
\t  -target ${zigTarget} \\
\t  -mcpu=$(CPU) \\
\t  -O ReleaseSafe \\
\t  -fentry=_start \\
\t  -fno-strip \\
\t  -T linker.ld \\
\t  --cache-dir $(BUILD_DIR)/zig-cache \\
\t  --name $(TARGET) \\
\t  -femit-bin=$(BUILD_DIR)/$(TARGET).elf
\t@arm-none-eabi-size $(BUILD_DIR)/$(TARGET).elf 2>/dev/null || true

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\tarm-none-eabi-objcopy -O binary $< $@ 2>/dev/null || \\
\t  $(ZIG) objcopy -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`
}

/** Wrap Zig application body with Cortex-M vector table at FLASH origin. */
function zigMainSource(_board, body) {
  return `//! Cortex-M vector table must sit at FLASH origin (linker .isr_vector).
extern const _estack: anyopaque;

export const vector_table: [16]usize align(4) linksection(".isr_vector") = .{
    @intFromPtr(&_estack),
    @intFromPtr(&_start),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
    @intFromPtr(&Default_Handler),
};

export fn Default_Handler() callconv(.C) noreturn {
    while (true) {}
}

${body.trim()}
`
}

function rustProjectFiles(board, mainRs) {
  return {
    'Cargo.toml': (name) => `[package]
name = "${name}"
version = "0.1.0"
edition = "2021"

[dependencies]
cortex-m-rt = "0.7"
panic-halt = "0.2"

[[bin]]
name = "${name}"
path = "src/main.rs"
`,
    'src/main.rs': () => mainRs,
    '.cargo/config.toml': () => `[target.${board.rustTarget}]
rustflags = ["-C", "link-arg=-Tlink.x"]

[build]
target = "${board.rustTarget}"
`,
    'memory.x': () => rustMemory(board),
    'build.rs': () => `fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-search=.");
}
`,
  }
}

function buildDriverTemplates(board, cpu, flashCfg) {
  return {
    'driver-c': {
      name: 'Driver — C',
      ext: '.c',
      files: {
        'Makefile': (name) => cFamilyMakefile(name, board, cpu, flashCfg),
        'include/driver.h': () => `#ifndef DRIVER_H
#define DRIVER_H
#include <stdint.h>
#include <stdbool.h>
typedef enum { DRIVER_OK = 0, DRIVER_ERR_PARAM = -1, DRIVER_ERR_BUSY = -2, DRIVER_ERR_TIMEOUT = -3, DRIVER_ERR_HW = -4 } driver_status_t;
typedef struct { uint32_t base; uint32_t irq; uint32_t clock_hz; } driver_config_t;
driver_status_t driver_init(const driver_config_t *cfg);
driver_status_t driver_write(const uint8_t *data, uint16_t len);
driver_status_t driver_read(uint8_t *data, uint16_t len, uint32_t timeout_ms);
void driver_irq_handler(void);
#endif
`,
        'src/driver.c': () => `#include "driver.h"
/* Device driver for ${board.mcu} */
static driver_config_t g_cfg;
static volatile bool g_ready;
driver_status_t driver_init(const driver_config_t *cfg) {
    if (!cfg || !cfg->base) return DRIVER_ERR_PARAM;
    g_cfg = *cfg; g_ready = true;
    return DRIVER_OK;
}
driver_status_t driver_write(const uint8_t *data, uint16_t len) {
    if (!g_ready || !data) return DRIVER_ERR_PARAM; (void)len; return DRIVER_OK;
}
driver_status_t driver_read(uint8_t *data, uint16_t len, uint32_t timeout_ms) {
    if (!g_ready || !data) return DRIVER_ERR_PARAM; (void)len; (void)timeout_ms; return DRIVER_OK;
}
void driver_irq_handler(void) {}
`,
        'src/main.c': () => `#include "driver.h"
int main(void) {
    const driver_config_t cfg = { .base = 0x40011000u, .irq = 0, .clock_hz = 16000000u };
    driver_init(&cfg);
    while (1) {}
}
void SystemInit(void) {}
`,
        'src/startup.S': () => startupAsm(board),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nC device driver for **${board.mcu}**.\n`,
      },
    },
    'driver-cpp': {
      name: 'Driver — C++',
      ext: '.cpp',
      files: {
        'Makefile': (name) => cFamilyMakefile(name, board, cpu, flashCfg, { cxx: true }),
        'include/driver.hpp': () => `#pragma once
#include <cstdint>
enum class DriverStatus : int { Ok = 0, Param = -1, Busy = -2, Timeout = -3, Hw = -4 };
struct DriverConfig { std::uint32_t base; std::uint32_t irq; std::uint32_t clock_hz; };
class Driver {
public:
    DriverStatus init(const DriverConfig &cfg);
    DriverStatus write(const std::uint8_t *data, std::uint16_t len);
    DriverStatus read(std::uint8_t *data, std::uint16_t len, std::uint32_t timeout_ms);
    void onIrq();
private:
    DriverConfig cfg_{};
    bool ready_ = false;
};
`,
        'src/driver.cpp': () => `#include "driver.hpp"
/* C++ driver for ${board.mcu} */
DriverStatus Driver::init(const DriverConfig &cfg) {
    if (!cfg.base) return DriverStatus::Param;
    cfg_ = cfg; ready_ = true; return DriverStatus::Ok;
}
DriverStatus Driver::write(const std::uint8_t *data, std::uint16_t len) {
    if (!ready_ || !data) return DriverStatus::Param; (void)len; return DriverStatus::Ok;
}
DriverStatus Driver::read(std::uint8_t *data, std::uint16_t len, std::uint32_t timeout_ms) {
    if (!ready_ || !data) return DriverStatus::Param; (void)len; (void)timeout_ms; return DriverStatus::Ok;
}
void Driver::onIrq() {}
`,
        'src/main.cpp': () => `#include "driver.hpp"
int main() {
    Driver drv;
    drv.init({0x40011000u, 0, 16000000u});
    while (true) {}
}
extern "C" void SystemInit() {}
`,
        'src/startup.S': () => startupAsm(board),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nC++ device driver for **${board.mcu}**.\n`,
      },
    },
    'driver-rust': {
      name: 'Driver — Rust',
      ext: '.rs',
      files: {
        ...rustProjectFiles(board, `#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;

pub struct DriverConfig {
    pub base: u32,
    pub irq: u32,
    pub clock_hz: u32,
}

pub struct Driver {
    ready: bool,
}

impl Driver {
    pub const fn new() -> Self {
        Self { ready: false }
    }
    pub fn init(&mut self, cfg: DriverConfig) -> Result<(), ()> {
        if cfg.base == 0 {
            return Err(());
        }
        // TODO: clocks / pins / reset for ${board.mcu}
        let _ = cfg;
        self.ready = true;
        Ok(())
    }
    pub fn write(&self, data: &[u8]) -> Result<(), ()> {
        if !self.ready {
            return Err(());
        }
        let _ = data;
        Ok(())
    }
}

#[entry]
fn main() -> ! {
    let mut drv = Driver::new();
    let _ = drv.init(DriverConfig {
        base: 0x4001_1000,
        irq: 0,
        clock_hz: 16_000_000,
    });
    loop {}
}
`),
        'README.md': (name) => `# ${name}\n\nRust device driver for **${board.mcu}**.\n`,
      },
    },
    'driver-asm': {
      name: 'Driver — ASM',
      ext: '.S',
      files: {
        'Makefile': (name) => `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)
OBJECTS = $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)
PREFIX = arm-none-eabi-
AS = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size
CPU = -mcpu=${board.cpu} -mthumb
ASFLAGS = $(CPU) -c -g -Wall
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin
$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(AS) $(ASFLAGS) $< -o $@
$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(AS) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@
$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@
clean:
\trm -rf $(BUILD_DIR)
flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"
.PHONY: all clean flash
`,
        'src/main.S': () => `.syntax unified
.cpu ${board.cpu}
.thumb
/* Bare-metal driver stub for ${board.mcu} */
.equ PERIPH_BASE, 0x40011000
.global _start
.global Default_Handler
.section .text
.thumb_func
_start:
    ldr r0, =_estack
    mov sp, r0
    bl driver_init
1:  b 1b
.thumb_func
driver_init:
    ldr r0, =PERIPH_BASE
    /* TODO: program peripheral registers */
    bx lr
.thumb_func
Default_Handler:
    b Default_Handler
.section .isr_vector,"a",%progbits
g_pfnVectors:
    .word _estack
    .word _start
    .word Default_Handler
`,
        'linker.ld': () => asmLinkerScript(board),
        'README.md': (name) => `# ${name}\n\nAssembly driver stub for **${board.mcu}**.\n`,
      },
    },
    'driver-zig': {
      name: 'Driver — Zig',
      ext: '.zig',
      files: {
        'Makefile': (name) => zigMakefile(name, board, flashCfg),
        'src/main.zig': () => zigMainSource(board, `//! Device driver skeleton for ${board.mcu}
const DriverError = error{ Param, Busy, Timeout, Hw };
const Config = struct { base: u32, irq: u32, clock_hz: u32 };
var ready: bool = false;
fn driverInit(cfg: Config) DriverError!void {
    if (cfg.base == 0) return error.Param;
    ready = true;
}
export fn _start() callconv(.C) noreturn {
    driverInit(.{ .base = 0x40011000, .irq = 0, .clock_hz = 16_000_000 }) catch {};
    while (true) asm volatile ("nop");
}
`),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nZig device driver for **${board.mcu}**.\n`,
      },
    },
  }
}

function buildOsTemplates(board, cpu, flashCfg) {
  const rustOsMain = `#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;

const MAX_TASKS: usize = 8;

type TaskFn = fn();

struct Kernel {
    tasks: [Option<TaskFn>; MAX_TASKS],
    ticks: u32,
}

impl Kernel {
    const fn new() -> Self {
        Self { tasks: [None; MAX_TASKS], ticks: 0 }
    }
    fn spawn(&mut self, task: TaskFn) -> Result<(), ()> {
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                *slot = Some(task);
                return Ok(());
            }
        }
        Err(())
    }
    fn run(&mut self) -> ! {
        loop {
            for slot in self.tasks.iter() {
                if let Some(task) = slot {
                    task();
                    self.ticks = self.ticks.wrapping_add(1);
                }
            }
        }
    }
}

fn task_blink() {}
fn task_telemetry() {}

#[entry]
fn main() -> ! {
    // Cooperative mini-OS for ${board.mcu}
    let mut k = Kernel::new();
    let _ = k.spawn(task_blink);
    let _ = k.spawn(task_telemetry);
    k.run()
}
`

  return {
    'os-c': {
      name: 'OS / Kernel — C',
      ext: '.c',
      files: {
        'Makefile': (name) => cFamilyMakefile(name, board, cpu, flashCfg),
        'include/kernel.h': () => `#ifndef KERNEL_H
#define KERNEL_H
#include <stdint.h>
#include <stddef.h>
#define KERNEL_MAX_TASKS 8
#define KERNEL_STACK_WORDS 256
typedef void (*task_fn_t)(void *);
typedef enum { TASK_UNUSED = 0, TASK_READY, TASK_RUNNING, TASK_BLOCKED } task_state_t;
typedef struct {
    uint32_t *sp; task_fn_t entry; void *arg; task_state_t state;
    uint32_t stack[KERNEL_STACK_WORDS];
} task_t;
void kernel_init(void);
int kernel_create_task(task_fn_t entry, void *arg);
void kernel_start(void) __attribute__((noreturn));
void kernel_yield(void);
uint32_t kernel_ticks(void);
#endif
`,
        'src/kernel.c': () => `#include "kernel.h"
/* Cooperative mini-kernel for ${board.mcu} */
static task_t g_tasks[KERNEL_MAX_TASKS];
static volatile uint32_t g_ticks;
static int g_current = -1;
void kernel_init(void) {
    for (int i = 0; i < KERNEL_MAX_TASKS; i++) g_tasks[i].state = TASK_UNUSED;
    g_ticks = 0; g_current = -1;
}
int kernel_create_task(task_fn_t entry, void *arg) {
    for (int i = 0; i < KERNEL_MAX_TASKS; i++) {
        if (g_tasks[i].state != TASK_UNUSED) continue;
        g_tasks[i].entry = entry; g_tasks[i].arg = arg; g_tasks[i].state = TASK_READY;
        g_tasks[i].sp = &g_tasks[i].stack[KERNEL_STACK_WORDS - 16];
        return i;
    }
    return -1;
}
void kernel_yield(void) {}
uint32_t kernel_ticks(void) { return g_ticks; }
void kernel_start(void) {
    for (;;) {
        for (int i = 0; i < KERNEL_MAX_TASKS; i++) {
            if (g_tasks[i].state != TASK_READY) continue;
            g_current = i; g_tasks[i].state = TASK_RUNNING;
            g_tasks[i].entry(g_tasks[i].arg);
            if (g_tasks[i].state == TASK_RUNNING) g_tasks[i].state = TASK_READY;
            g_ticks++;
        }
    }
}
`,
        'src/main.c': () => `#include "kernel.h"
static void task_blink(void *arg) { (void)arg; kernel_yield(); }
static void task_telemetry(void *arg) { (void)arg; kernel_yield(); }
int main(void) {
    kernel_init();
    kernel_create_task(task_blink, 0);
    kernel_create_task(task_telemetry, 0);
    kernel_start();
}
void SystemInit(void) {}
`,
        'src/startup.S': () => startupAsm(board),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nMinimal cooperative OS for **${board.mcu}** (C).\n`,
      },
    },
    'os-cpp': {
      name: 'OS / Kernel — C++',
      ext: '.cpp',
      files: {
        'Makefile': (name) => cFamilyMakefile(name, board, cpu, flashCfg, { cxx: true }),
        'include/kernel.hpp': () => `#pragma once
#include <cstdint>
#include <array>
constexpr std::size_t kMaxTasks = 8;
using TaskFn = void (*)(void *);
enum class TaskState { Unused, Ready, Running, Blocked };
struct Task {
    TaskFn entry = nullptr;
    void *arg = nullptr;
    TaskState state = TaskState::Unused;
};
class Kernel {
public:
    void init();
    int spawn(TaskFn entry, void *arg = nullptr);
    [[noreturn]] void start();
    void yield();
    std::uint32_t ticks() const { return ticks_; }
private:
    std::array<Task, kMaxTasks> tasks_{};
    std::uint32_t ticks_ = 0;
};
`,
        'src/kernel.cpp': () => `#include "kernel.hpp"
/* Cooperative C++ kernel for ${board.mcu} */
void Kernel::init() {
    for (auto &t : tasks_) t = {};
    ticks_ = 0;
}
int Kernel::spawn(TaskFn entry, void *arg) {
    for (std::size_t i = 0; i < tasks_.size(); ++i) {
        if (tasks_[i].state != TaskState::Unused) continue;
        tasks_[i] = { entry, arg, TaskState::Ready };
        return static_cast<int>(i);
    }
    return -1;
}
void Kernel::yield() {}
void Kernel::start() {
    for (;;) {
        for (auto &t : tasks_) {
            if (t.state != TaskState::Ready) continue;
            t.state = TaskState::Running;
            t.entry(t.arg);
            if (t.state == TaskState::Running) t.state = TaskState::Ready;
            ++ticks_;
        }
    }
}
`,
        'src/main.cpp': () => `#include "kernel.hpp"
static void task_blink(void *) {}
static void task_telemetry(void *) {}
int main() {
    Kernel k;
    k.init();
    k.spawn(task_blink);
    k.spawn(task_telemetry);
    k.start();
}
extern "C" void SystemInit() {}
`,
        'src/startup.S': () => startupAsm(board),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nMinimal cooperative OS for **${board.mcu}** (C++).\n`,
      },
    },
    'os-rust': {
      name: 'OS / Kernel — Rust',
      ext: '.rs',
      files: {
        ...rustProjectFiles(board, rustOsMain),
        'README.md': (name) => `# ${name}\n\nMinimal cooperative OS for **${board.mcu}** (Rust).\n`,
      },
    },
    'os-asm': {
      name: 'OS / Kernel — ASM',
      ext: '.S',
      files: {
        'Makefile': (name) => `TARGET = ${name}
SRC_DIR = src
BUILD_DIR = build
ASM_SOURCES = $(wildcard $(SRC_DIR)/*.S)
OBJECTS = $(ASM_SOURCES:$(SRC_DIR)/%.S=$(BUILD_DIR)/%.o)
PREFIX = arm-none-eabi-
AS = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size
CPU = -mcpu=${board.cpu} -mthumb
ASFLAGS = $(CPU) -c -g -Wall
LDFLAGS = $(CPU) -T linker.ld -nostartfiles -Wl,--gc-sections
all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin
$(BUILD_DIR)/%.o: $(SRC_DIR)/%.S
\t@mkdir -p $(BUILD_DIR)
\t$(AS) $(ASFLAGS) $< -o $@
$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(AS) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@
$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@
clean:
\trm -rf $(BUILD_DIR)
flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd ${flashCfg} \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"
.PHONY: all clean flash
`,
        'src/main.S': () => `.syntax unified
.cpu ${board.cpu}
.thumb
/* Tiny cooperative scheduler stub for ${board.mcu} */
.global _start
.global Default_Handler
.section .bss
.align 3
task_idx: .word 0
.section .text
.thumb_func
_start:
    ldr r0, =_estack
    mov sp, r0
    bl kernel_init
    b kernel_run
.thumb_func
kernel_init:
    movs r0, #0
    ldr r1, =task_idx
    str r0, [r1]
    bx lr
.thumb_func
kernel_run:
1:  bl task0
    bl task1
    b 1b
.thumb_func
task0:
    bx lr
.thumb_func
task1:
    bx lr
.thumb_func
Default_Handler:
    b Default_Handler
.section .isr_vector,"a",%progbits
g_pfnVectors:
    .word _estack
    .word _start
    .word Default_Handler
`,
        'linker.ld': () => asmLinkerScript(board),
        'README.md': (name) => `# ${name}\n\nAssembly OS / scheduler stub for **${board.mcu}**.\n`,
      },
    },
    'os-zig': {
      name: 'OS / Kernel — Zig',
      ext: '.zig',
      files: {
        'Makefile': (name) => zigMakefile(name, board, flashCfg),
        'src/main.zig': () => zigMainSource(board, `//! Cooperative mini-OS for ${board.mcu}
const MAX_TASKS: usize = 8;
const TaskFn = *const fn () void;
var tasks: [MAX_TASKS]?TaskFn = .{null} ** MAX_TASKS;
var ticks: u32 = 0;
fn spawn(task: TaskFn) bool {
    for (&tasks) |*slot| {
        if (slot.* == null) {
            slot.* = task;
            return true;
        }
    }
    return false;
}
fn taskBlink() void {}
fn taskTelemetry() void {}
fn run() noreturn {
    while (true) {
        for (tasks) |maybe| {
            if (maybe) |task| {
                task();
                ticks +%= 1;
            }
        }
    }
}
export fn _start() callconv(.C) noreturn {
    _ = spawn(taskBlink);
    _ = spawn(taskTelemetry);
    run();
}
`),
        'linker.ld': () => linkerScript(board),
        'README.md': (name) => `# ${name}\n\nMinimal cooperative OS for **${board.mcu}** (Zig).\n`,
      },
    },
  }
}

const TEMPLATES_META = {
  rust: { name: 'Firmware — Rust', ext: '.rs', category: 'firmware', needsBoard: true, lang: 'rust' },
  c: { name: 'Firmware — C', ext: '.c', category: 'firmware', needsBoard: true, lang: 'c' },
  cpp: { name: 'Firmware — C++', ext: '.cpp', category: 'firmware', needsBoard: true, lang: 'cpp' },
  asm: { name: 'Firmware — ASM', ext: '.S', category: 'firmware', needsBoard: true, lang: 'asm' },
  zig: { name: 'Firmware — Zig', ext: '.zig', category: 'firmware', needsBoard: true, lang: 'zig' },
  'driver-c': { name: 'Driver — C', ext: '.c', category: 'driver', needsBoard: true, lang: 'c' },
  'driver-cpp': { name: 'Driver — C++', ext: '.cpp', category: 'driver', needsBoard: true, lang: 'cpp' },
  'driver-rust': { name: 'Driver — Rust', ext: '.rs', category: 'driver', needsBoard: true, lang: 'rust' },
  'driver-asm': { name: 'Driver — ASM', ext: '.S', category: 'driver', needsBoard: true, lang: 'asm' },
  'driver-zig': { name: 'Driver — Zig', ext: '.zig', category: 'driver', needsBoard: true, lang: 'zig' },
  'os-c': { name: 'OS / Kernel — C', ext: '.c', category: 'os', needsBoard: true, lang: 'c' },
  'os-cpp': { name: 'OS / Kernel — C++', ext: '.cpp', category: 'os', needsBoard: true, lang: 'cpp' },
  'os-rust': { name: 'OS / Kernel — Rust', ext: '.rs', category: 'os', needsBoard: true, lang: 'rust' },
  'os-asm': { name: 'OS / Kernel — ASM', ext: '.S', category: 'os', needsBoard: true, lang: 'asm' },
  'os-zig': { name: 'OS / Kernel — Zig', ext: '.zig', category: 'os', needsBoard: true, lang: 'zig' },
}

function readProjectMeta(projectDir) {
  const metaPath = path.join(projectDir, 'embedide.json')
  try {
    if (fs.existsSync(metaPath)) {
      return JSON.parse(fs.readFileSync(metaPath, 'utf8'))
    }
  } catch {}
  return null
}

function writeProjectMeta(projectDir, meta) {
  fs.writeFileSync(
    path.join(projectDir, 'embedide.json'),
    JSON.stringify(meta, null, 2) + '\n',
    'utf8',
  )
}

function createProject(rootDir, name, type, boardId = DEFAULT_BOARD_ID) {
  if (!TEMPLATES_META[type]) throw new Error(`Unknown template type: ${type}`);
  if (typeof name !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(name)) {
    throw new Error('Invalid project name: use letters, digits, _ and - only');
  }

  const metaTpl = TEMPLATES_META[type];
  const needsBoard = metaTpl.needsBoard !== false;
  const board = needsBoard ? getBoardOrDefault(boardId) : getBoardOrDefault(DEFAULT_BOARD_ID);
  const templates = buildTemplates(board);
  const template = templates[type];
  if (!template) throw new Error(`Template not implemented: ${type}`);

  const root = path.resolve(rootDir);
  const projectDir = path.resolve(root, name);
  const rel = path.relative(root, projectDir);
  if (rel.startsWith('..') || path.isAbsolute(rel) || rel === '') {
    throw new Error('Invalid project path');
  }

  if (fs.existsSync(projectDir)) {
    throw new Error(`Directory ${projectDir} already exists`);
  }

  fs.mkdirSync(projectDir, { recursive: true });

  for (const [filePath, contentFn] of Object.entries(template.files)) {
    const fullPath = path.join(projectDir, filePath);
    fs.mkdirSync(path.dirname(fullPath), { recursive: true });
    fs.writeFileSync(fullPath, contentFn(name), 'utf8');
  }

  const meta = {
    name,
    type,
    version: 1,
  };
  if (needsBoard) meta.boardId = board.id;

  writeProjectMeta(projectDir, meta);

  return projectDir;
}

function listProjectFiles(projectDir) {
  const result = [];

  function walk(dir, relative) {
    const entries = fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => {
      if (a.isDirectory() && !b.isDirectory()) return -1;
      if (!a.isDirectory() && b.isDirectory()) return 1;
      return a.name.localeCompare(b.name);
    });

    for (const entry of entries) {
      const relPath = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.name === 'build' || entry.name === 'target' || entry.name === 'node_modules') continue;
      if (entry.name.startsWith('.') && !ALLOWED_HIDDEN.has(entry.name)) continue;
      if (entry.isDirectory()) {
        result.push({ id: relPath, name: entry.name, type: 'directory' });
        walk(path.join(dir, entry.name), relPath);
      } else {
        const ext = path.extname(entry.name).slice(1);
        const langMap = {
          rs: 'rust', c: 'c', cpp: 'cpp', S: 'asm', s: 'asm', h: 'c', hpp: 'cpp',
          ld: 'linker', toml: 'toml', json: 'json', x: 'linker', zig: 'zig',
          md: 'markdown', txt: 'text',
        };
        result.push({ id: relPath, name: entry.name, type: 'file', language: langMap[ext] || ext });
      }
    }
  }

  walk(projectDir, '');
  return result;
}

function readProjectFile(filePath) {
  return fs.readFileSync(filePath, 'utf8');
}

function writeProjectFile(filePath, content) {
  fs.writeFileSync(filePath, content, 'utf8');
}

function createProjectFile(projectDir, name) {
  const fullPath = path.join(projectDir, name);
  if (fs.existsSync(fullPath)) return false;
  if (name.endsWith('/')) {
    fs.mkdirSync(fullPath, { recursive: true });
  } else {
    fs.mkdirSync(path.dirname(fullPath), { recursive: true });
    fs.writeFileSync(fullPath, '', 'utf8');
  }
  return true;
}

function deleteProjectFile(filePath) {
  if (!fs.existsSync(filePath)) return false;
  fs.rmSync(filePath, { recursive: true });
  return true;
}

function renameProjectFile(oldPath, newPath) {
  if (!fs.existsSync(oldPath)) return false;
  if (fs.existsSync(newPath)) return false;
  fs.mkdirSync(path.dirname(newPath), { recursive: true });
  fs.renameSync(oldPath, newPath);
  return true;
}

function searchInFiles(projectDir, query) {
  const results = [];
  const ext = ['.rs', '.c', '.cpp', '.h', '.hpp', '.S', '.s', '.toml', '.ld', '.yaml', '.json', '.md', '.txt', '.zig'];

  function walk(dir) {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      if ((entry.name.startsWith('.') && !ALLOWED_HIDDEN.has(entry.name)) || entry.name === 'target' || entry.name === 'build' || entry.name === 'node_modules') continue;
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(fullPath);
      } else if (ext.includes(path.extname(entry.name))) {
        try {
          const content = fs.readFileSync(fullPath, 'utf8');
          const lines = content.split('\n');
          for (let i = 0; i < lines.length; i++) {
            if (lines[i].toLowerCase().includes(query.toLowerCase())) {
              results.push({ file: fullPath, line: i + 1, text: lines[i].trim() });
            }
          }
        } catch {}
      }
    }
  }
  walk(projectDir);
  return results;
}

function getTemplateList() {
  return Object.entries(TEMPLATES_META).map(([key, t]) => ({
    id: key,
    name: t.name,
    ext: t.ext,
    category: t.category || 'firmware',
    needsBoard: t.needsBoard !== false,
    lang: t.lang || key,
  }))
}

module.exports = {
  TEMPLATES: TEMPLATES_META,
  createProject,
  listProjectFiles,
  readProjectFile,
  writeProjectFile,
  createProjectFile,
  deleteProjectFile,
  renameProjectFile,
  searchInFiles,
  readProjectMeta,
  writeProjectMeta,
  getTemplateList,
};
