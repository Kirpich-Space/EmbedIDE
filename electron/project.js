const path = require('path');
const fs = require('fs');

const TEMPLATES = {
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
embedded-hal = "1.0"
panic-halt = "0.2"

[[bin]]
name = "${name}"
path = "src/main.rs"
`,
      'src/main.rs': () => `#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;
use embedded_hal::digital::v2::OutputPin;

#[entry]
fn main() -> ! {
    loop {}
}
`,
      '.cargo/config.toml': () => `[target.thumbv7em-none-eabihf]
rustflags = ["-C", "link-arg=-Tlink.x"]

[build]
target = "thumbv7em-none-eabihf"
`,
      'memory.x': () => `MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}
`,
      'build.rs': () => `fn main() {
    println!("cargo:rerun-if-changed=memory.x");
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
LD = $(PREFIX)ld
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = -mcpu=cortex-m4 -mthumb -mfpu=fpv4-sp-d16 -mfloat-abi=hard
DEFINES = -DSTM32F407xx
CFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections
LDFLAGS = $(CPU) -T linker.ld -Wl,--gc-sections -Wl,-Map=$(BUILD_DIR)/$(TARGET).map

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
\topenocd -f interface/stlink.cfg -f target/stm32f4x.cfg \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
      'src/main.c': () => `#include <stdint.h>

int main(void) {
    while (1) {
        // Your code here
    }
}

void SystemInit(void) {}
`,
      'src/uart.c': () => `#include "uart.h"

void uart_init(void) {
    // UART initialization
}

int uart_send(const uint8_t *data, uint16_t len) {
    // UART send
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
      'linker.ld': () => `MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM (xrw)   : ORIGIN = 0x20000000, LENGTH = 128K
}

SECTIONS
{
    .text : { *(.text*) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
`,
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
LD = $(PREFIX)g++
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = -mcpu=cortex-m4 -mthumb -mfpu=fpv4-sp-d16 -mfloat-abi=hard
DEFINES = -DSTM32F407xx
CXXFLAGS = $(CPU) -c $(DEFINES) -O2 -g -Wall -ffunction-sections -fdata-sections -fno-exceptions -fno-rtti
LDFLAGS = $(CPU) -T linker.ld -Wl,--gc-sections -Wl,-Map=$(BUILD_DIR)/$(TARGET).map

all: $(BUILD_DIR)/$(TARGET).elf $(BUILD_DIR)/$(TARGET).bin

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.cpp
\t@mkdir -p $(BUILD_DIR)
\t$(CXX) $(CXXFLAGS) $< -o $@

$(BUILD_DIR)/$(TARGET).elf: $(OBJECTS)
\t$(CXX) $(LDFLAGS) $^ -o $@
\t$(SIZE) $@

$(BUILD_DIR)/$(TARGET).bin: $(BUILD_DIR)/$(TARGET).elf
\t$(OBJCOPY) -O binary $< $@

clean:
\trm -rf $(BUILD_DIR)

flash: $(BUILD_DIR)/$(TARGET).elf
\topenocd -f interface/stlink.cfg -f target/stm32f4x.cfg \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
      'src/main.cpp': () => `#include <cstdint>

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
      'linker.ld': () => `MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM (xrw)   : ORIGIN = 0x20000000, LENGTH = 128K
}

SECTIONS
{
    .text : { *(.text*) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
`,
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
LD = $(PREFIX)ld
OBJCOPY = $(PREFIX)objcopy
SIZE = $(PREFIX)size

CPU = -mcpu=cortex-m4 -mthumb
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
\topenocd -f interface/stlink.cfg -f target/stm32f4x.cfg \\
\t  -c "program $(BUILD_DIR)/$(TARGET).elf verify reset exit"

.PHONY: all clean flash
`,
      'src/main.S': () => `.syntax unified
.cpu cortex-m4
.fpu softvfp
.thumb

.global _start
.global Default_Handler

.section .text

_start:
    ldr r0, =_estack
    mov sp, r0
    bl main
    b .

main:
    // Your code here
    b main

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
      'linker.ld': () => `MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM (xrw)   : ORIGIN = 0x20000000, LENGTH = 128K
}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .text : { *(.text*) *(.isr_vector) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
`,
    },
  },
};

function createProject(rootDir, name, type) {
  const template = TEMPLATES[type];
  if (!template) throw new Error(`Unknown template type: ${type}`);

  const projectDir = path.join(rootDir, name);
  if (fs.existsSync(projectDir)) {
    throw new Error(`Directory ${projectDir} already exists`);
  }

  fs.mkdirSync(projectDir, { recursive: true });

  for (const [filePath, contentFn] of Object.entries(template.files)) {
    const fullPath = path.join(projectDir, filePath);
    fs.mkdirSync(path.dirname(fullPath), { recursive: true });
    const content = contentFn(name);
    fs.writeFileSync(fullPath, content, 'utf8');
  }

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
      if (entry.name.startsWith('.')) continue;
      if (entry.isDirectory()) {
        result.push({ id: relPath, name: entry.name, type: 'directory' });
        walk(path.join(dir, entry.name), relPath);
      } else {
        const ext = path.extname(entry.name).slice(1);
        const langMap = { rs: 'rust', c: 'c', cpp: 'cpp', S: 'asm', s: 'asm', h: 'c', hpp: 'cpp', ld: 'linker', toml: 'toml' };
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
  const ext = ['.rs', '.c', '.cpp', '.h', '.hpp', '.S', '.s', '.toml', '.ld', '.yaml', '.json', '.md', '.txt'];

  function walk(dir) {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.name.startsWith('.') || entry.name === 'target' || entry.name === 'build' || entry.name === 'node_modules') continue;
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

module.exports = { TEMPLATES, createProject, listProjectFiles, readProjectFile, writeProjectFile, createProjectFile, deleteProjectFile, renameProjectFile, searchInFiles };
