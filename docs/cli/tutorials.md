# Tutorials

Complete walkthroughs. Every shell command here was run with `embed 0.1.0-alpha.1` on Linux; the
IDE steps are keyboard sequences you type in `embed`. Use a scratch directory.

Contents: [1 C project](#1-a-c-project-from-scratch) · [2 Rust and Zig](#2-rust-and-zig) ·
[3 Assembly](#3-assembly-x86-64) · [4 Cortex-M firmware](#4-bare-metal-firmware-for-a-cortex-m-board) ·
[5 Choose compilers](#5-choose-your-compilers) · [6 AI agent](#6-let-an-ai-agent-do-the-work) ·
[7 Skills](#7-your-own-skill) · [8 Inline suggestions](#8-inline-ai-suggestions) ·
[9 Inspect a binary](#9-inspect-a-binary)

## 1. A C project from scratch

```sh
embed project init hello --language c      # creates embedide.toml and src/main.c
embed build hello                          # → hello/build/debug/hello
./hello/build/debug/hello
embed hello                                # open the IDE in the project
```

In the IDE:

1. `Alt+1` is the explorer; `Enter` on `src` expands it, `Enter` on `main.c` opens it.
2. In the editor press `i`, change the text, press `Esc`, then `:w` `Enter` to save.
3. `F5` builds (builds use the files on disk: save first). Errors appear in the bottom panel;
   `F3` lists problems and `Enter` on one jumps to the line and column.
4. `Alt+3` is the integrated terminal: run `./build/debug/hello`. `Alt+3` again returns to the editor.
5. `Ctrl+Q` quits (it asks if something is unsaved).

`build/` is EmbedIDE's output directory (marked so `embed clean` only removes what it created).
Flags and profiles live in `embedide.toml`; `embed build hello --release` builds the release profile.

## 2. Rust and Zig

```sh
embed project init rs --language rust        # Cargo project
embed build rs                               # runs cargo build (offline unless --online)
embed project init zg --language zig
embed build zg                               # zig build-exe src/main.zig
```

In a Rust or Zig file the completion popup uses rust-analyzer or zls when installed (otherwise words
from the file): type two letters, move with `Down`, accept with `Tab`; `Esc` closes it. `F7` runs
`cargo check` in Cargo projects.

## 3. Assembly (x86-64)

```sh
embed project init asm1 --template asm       # a C program calling a GNU assembly function
embed build asm1
```

Open the `.S` file: the dialect (NASM, GNU as AT&T or Intel, ARM, AArch64, RISC-V) is detected per
file, branch instructions are coloured, `:label` jumps to a label, and typing a mnemonic or register
completes it. Assembler errors reach the problems list. See
[assembly](tui.md#assembly) and `examples/asm`.

## 4. Bare-metal firmware for a Cortex-M board

Needs a cross compiler (`arm-none-eabi-gcc`) on `PATH` or registered with `embed toolchain add`.

```sh
embed toolchain list                                       # is arm-none-eabi-gcc listed?
embed project init fw --template c-cortex-m --board stm32f407vg
embed build fw                                             # → fw/build/debug/fw.elf
embed fw                                                   # IDE
```

Then open `build/debug/fw.elf` in the IDE (`:e build/debug/fw.elf`): the HEX view opens;
`Tab` → ELF inspector (vector table in flash, sections, symbols) → disassembly.

EmbedIDE never flashes or debugs on its own. With a probe attached, use the IDE's explicit commands
(`Ctrl+K` → *Flash Firmware*, which previews the command and asks), or your own tools from the
integrated terminal (`openocd`, `probe-rs`, `gdb`). Serial: `Ctrl+K` → *Select Serial Port*, then
*Connect Serial Monitor* (receive-only by default).

## 5. Choose your compilers

EmbedIDE uses exactly what you select and never installs anything.

```sh
embed toolchain list                       # what is installed
embed build hello --toolchain c=clang      # one build with clang
embed toolchain default c clang            # your default for projects that do not choose
```

Per project, in `embedide.toml`:

```toml
[toolchain]
c = "arm-none-eabi-gcc"

[profiles.asan]
flags = ["-O1", "-g", "-fsanitize=address"]
toolchain = { c = "clang" }
```

`embed project info hello` shows which toolchain each language would use and why;
`embed build hello --profile asan` builds that profile.

## 6. Let an AI agent do the work

Requires Claude Code or Codex CLI, installed and logged in once in a normal terminal
(`claude` / `codex login`). No API key is involved; usage counts against that CLI's plan.

```sh
embed hello
```

1. `Alt+2` opens the AI panel. Type `/provider codex-cli` (or `claude-code`) and `Enter`. The panel
   checks the CLI and shows how you are logged in.
2. Describe the task in plain text, for example
   `add a function mul(a, b) to a new file mul.c, call it from main.c, build and run it`, `Enter`.
3. Watch the steps: files read, `✎ mul.c (added)`, `$ make` or `$ gcc …` with their output and exit
   status. The title shows the running state and policy.
4. A risky action (deleting files, network, anything outside the project) shows `⚠ Approval needed`:
   press `y`, `a` or `n`.
5. When it finishes, the summary line counts files, commands and time. `/review` shows the diffs,
   `/revert all` undoes everything (it refuses to overwrite files you changed since). Open files
   without unsaved edits reload by themselves.
6. A follow-up (`now add a test for it`) continues the same session; `/new` starts fresh.
7. Stuck or wrong? `/cancel` stops the agent and every process it started.

Policies: `/policy read-only` (look but do not touch), `workspace-write` (the default: edits and
builds inside the project run; risky things ask), `approval-required` (everything asks). From a script:

```sh
embed ai agent --provider codex-cli --policy read-only "explain how main.c works"
embed ai agent --provider claude-code --fix "make test" "the tests fail"   # build → fix loop
```

Details and security notes: [agent mode](agents.md).

## 7. Your own skill

Agents already get EmbedIDE's built-in skills (how to work in the project, C/C++, Rust, Zig, assembly,
firmware). Add your team's rules:

```sh
mkdir -p hello/.embedide/skills/house-style
cat > hello/.embedide/skills/house-style/SKILL.md <<'EOS'
---
name: house-style
description: Our C style. Use for every C change in this repository.
---
- Use uint32_t for register values, never unsigned long.
- Every public function has a comment listing its error codes.
- Build with `make CFLAGS="-Wall -Wextra -Werror"` after each change.
EOS
embed ai skills                       # lists built-in and project skills
```

In the AI panel: `/skills` lists them (`●` = given to agent tasks), `/skill house-style` activates the
new one, `/skill off house-style` removes it. A skill is guidance only: it cannot change the policy or
permissions. More: [skills](skills.md).

## 8. Inline AI suggestions

1. Open a source file, move to an empty line inside a function, press `i`.
2. Press `Alt+\`. After a few seconds a dimmed suggestion appears at the cursor; a multi-line one
   shows its further lines below, pushing the code down.
3. `Tab` inserts it (one undo step); `Esc` dismisses it. Typing the first letters of the suggestion
   keeps the rest. `u` undoes an accepted suggestion.
4. `Ctrl+Space` still opens the normal (LSP) completion popup.

Suggestions come from the selected AI backend (several seconds through the subscription CLIs). For
automatic suggestions while typing: `/inline auto` in the AI panel, or `ai.completion.mode = "auto"`.

## 9. Inspect a binary

```sh
embed hello/build/debug/hello          # opens in the HEX view (the file is only read)
```

`Tab` switches HEX → ELF inspector (`1` header, `2` segments, `3` sections, `4` symbols; `/` finds a
name) → disassembly (needs `objdump` or `llvm-objdump`). `g` jumps to an offset, `/` searches bytes
(`7f 45 4c`) or text (`"ELF"`). Editing is explicit (`i`), replaces bytes only, and saving asks first.
