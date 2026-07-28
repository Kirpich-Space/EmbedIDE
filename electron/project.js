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

SECTIONS
{
    .text : { *(.text*) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
}
`
}

function asmLinkerScript(board) {
  return `${linkerMemory(board)}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .text : { *(.text*) *(.isr_vector) } > FLASH
    .rodata : { *(.rodata*) } > FLASH
    .data : { *(.data*) } > RAM
    .bss : { *(.bss*) } > RAM
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
        'link.x': () => `INCLUDE memory.x
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

.section .text

_start:
    ldr r0, =_estack
    mov sp, r0
    bl main
    b .

main:
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
        'linker.ld': () => asmLinkerScript(board),
      },
    },
  }
}

const TEMPLATES_META = {
  rust: { name: 'Rust (Cortex-M)', ext: '.rs' },
  c: { name: 'C (ARM Cortex-M)', ext: '.c' },
  cpp: { name: 'C++ (ARM Cortex-M)', ext: '.cpp' },
  asm: { name: 'Assembly (ARM Cortex-M)', ext: '.S' },
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

  const board = getBoardOrDefault(boardId);
  const templates = buildTemplates(board);
  const template = templates[type];

  const projectDir = path.join(rootDir, name);
  if (fs.existsSync(projectDir)) {
    throw new Error(`Directory ${projectDir} already exists`);
  }

  fs.mkdirSync(projectDir, { recursive: true });

  for (const [filePath, contentFn] of Object.entries(template.files)) {
    const fullPath = path.join(projectDir, filePath);
    fs.mkdirSync(path.dirname(fullPath), { recursive: true });
    fs.writeFileSync(fullPath, contentFn(name), 'utf8');
  }

  writeProjectMeta(projectDir, {
    name,
    type,
    boardId: board.id,
    version: 1,
  });

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
        const langMap = { rs: 'rust', c: 'c', cpp: 'cpp', S: 'asm', s: 'asm', h: 'c', hpp: 'cpp', ld: 'linker', toml: 'toml', json: 'json', x: 'linker' };
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
