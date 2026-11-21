# `embed` — the terminal IDE

`embed` (no arguments) opens the IDE on the current directory; `embed <directory>` opens another
project; `embed <file>` opens that file (in the nearest directory with `embedide.toml`, or the current
directory when the file is inside it). A missing file in an existing directory becomes a new buffer that
is written only on `:w`. Subcommand names win over same-named files: use `embed ./build`. It needs an interactive terminal (`TERM` other than `dumb`); no GUI, Electron or Node.js.

```
EmbedIDE 0.1.0-alpha.1 | Ready | project | target | profile            ← build status, project, target, profile
>main.c  uart.c*                                                ← open documents (* = unsaved)
EXPLORER │ editor (Vim modes)                    │ AI · provider/model · state
> src    │  1 int main(void) {                   │ you: …
  build  │  2     return 0;                      │ ai: …
         │                                        │ > input line
TERMINAL | /bin/zsh | …        (or BUILD OUTPUT / PROBLEMS / TASKS / SERIAL / DEBUG / TOOLS / SETTINGS / HELP)
$ make flash-dry-run
NORMAL | src/main.c | c | arm-none-eabi-gcc | Ln 2, Col 5 | hint | message
```

The status bar shows the editor mode (or the focused panel), file, language, the compiler selected
for that language by the project configuration, and the cursor position. Errors are shown first.

## Layout adapts to the terminal

| Width / height | Behaviour |
|---|---|
| ≥ 100 columns | explorer, editor and AI panel side by side |
| 80–99 columns | AI panel opens over the editor while focused |
| < 80 columns | explorer opens over the editor while focused |
| < 16 rows or < 60 columns | the focused bottom panel takes the whole work area |
| < 6 rows or < 12 columns | editor only |

Hide or show panels: F2 (explorer), `:assistant` (AI), `:toggle_panel` (bottom panel).

## Keys

Global (work in every panel except where noted):

| Key | Action |
|---|---|
| **Alt+1** | Explorer — press again to return to the editor |
| **Alt+2** | AI assistant — press again to return to the editor |
| **Alt+3** | Terminal — press again to return to the editor |
| **Alt+1/2/3 + Delete** | Hold Alt and the digit, press Delete: hide that panel (its state and shell are kept; kitty keyboard protocol) |
| Space 1/2/3, Space E/A/T, Ctrl+] | Without Alt (the FreeBSD console sends Alt+1 as a plain `1`): open the panels / hide them from Normal mode, and move focus on from any panel (Ctrl+] arrives as Ctrl+5 in a legacy terminal) |
| Ctrl+Alt+1/2/3 | Same as Alt+1/2/3 |
| `Alt+\` | AI inline suggestion at the cursor (Insert mode; Tab accepts, Esc dismisses) |
| Ctrl+K | Command palette (every command below, plus targets, profiles, tasks and files) |
| Ctrl+P | Find a file among loaded explorer entries |
| Ctrl+S | Save the active document |
| Ctrl+W / Ctrl+Q | Close document / quit (unsaved changes ask for confirmation) |
| F1 | Help panel |
| F2 | Show / hide explorer |
| F3 | Problems (compiler diagnostics and search results) |
| F4 | Cycle bottom panels |
| F5 / F7 / F8 | Build / `cargo check` (legacy Cargo projects) / cancel the running task or AI request |
| F6 | Cycle focus: editor → explorer → bottom panel → AI → editor |
| F9 / F10 | Select target / next document |

### What terminals can report

Alt+1/2/3 work in every terminal (they arrive as `ESC 1`…). Ctrl+Alt+digit and "digit still held"
exist only with the
[kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) (for example kitty, foot,
WezTerm, Ghostty, Alacritty 0.13+). `embed` asks the terminal at startup and enables
it only when the terminal answers; `EMBEDIDE_KEYBOARD=legacy` turns it off.

Without it (commonly xterm, tmux/screen and the Linux console; check `:help`, which says which mode
is active) the terminal cannot report these chords at all: Ctrl+Alt+1 arrives as `ESC 1` — the same
bytes as **Alt+1** — and xterm-style terminals send Ctrl+Alt+3 as `ESC ESC` (indistinguishable from
two Esc presses). There:

* **Alt+1 / Alt+2 / Alt+3** switch panels as everywhere (`ESC <digit>` from Ctrl+Alt+1 works as well);
* the hide chord is never guessed from separate presses; use `:hide_explorer`, `:hide_assistant`,
  `:hide_terminal` (also in Ctrl+K), or bind them, e.g. in `embedide.json`:
  `{"native": {"keys": {"hide_terminal": "alt+6", "hide_explorer": "f12"}}}`. Bindings may be
  F1–F12, `ctrl+a`…`ctrl+z`, `ctrl+space`, `alt+0`…`alt+9` or `ctrl+alt+0`…`ctrl+alt+9` (a
  `ctrl+alt+N` binding answers to Alt+N as well).

Ctrl+Alt+Delete is a system shortcut on many desktops and on the Linux console (it may reboot); such
a system takes it before any terminal program sees it, so prefer a bound key there.

In the **terminal** every key — Ctrl+C, Ctrl+D, Ctrl+K, Tab, Esc, F-keys, arrows — goes to your
shell; only the panel keys (Alt+1/2/3, Ctrl+Alt+1/2/3) and keys you bound
to `hide_*` commands are taken by the IDE. Colours and attributes (bold, underline, inverse) of the
program's output are kept. Paste is forwarded as bracketed paste when the program asks for it. When the shell exits, Enter starts a new one; `:terminal_restart` restarts it.

**Terminal history** (5000 lines, colours kept):

| Key | Action |
|---|---|
| Shift+PageUp | Start scrolling back (when there is history); then one page further back |
| Shift+PageDown | One page forward |
| Shift+Up / Shift+Down | One line back / forward |
| Shift+Home / Shift+End | Oldest line / back to the live screen |
| Any other key, or paste | Back to the live screen, and the key goes to the shell |

While scrolled back the title shows `HISTORY n/m lines up` and new output does not move the view. On
the live screen only Shift+PageUp is taken (Shift+Up/Down/Home/End still reach the shell). Full-screen
programs (`less`, `vim`, `htop` — the alternate screen) have no history of their own here; these keys go
to the program. If your terminal emulator keeps Shift+PageUp for its own scrollback (the
key then never reaches `embed`), use the wheel (mouse support is on by default).

A program that floods output (`yes`) cannot freeze the IDE or grow its memory: output is applied in
bounded slices between frames, and the reader waits for the screen instead of buffering without limit.

**Editor** (Vim/Kenit modal editing, shared engine `embedide-editor`):

| Mode | Keys |
|---|---|
| Normal | `h j k l`, `w b e`, `0 $`, `gg G`, counts, `x`, `dd`, `yy`, `p`, `u`, Ctrl+R, `/ ? n N`, `v` / `V` |
| Insert | `i a o O` enter, Esc leaves |
| Command | `:w`, `:q`, `:q!`, `:wq`, `:<line>` (go to line, clamped), `:w <file>` / `:saveas <file>` (save as a **new** file — an existing file is never overwritten; this is also how an untitled buffer is saved), and IDE commands (below) |
| Leader (Space) | `e` explorer · `b` build · `d` debug · `t` terminal · `s` serial · `f` find file · `1`/`2`/`3` explorer/AI/terminal focus · `a` AI panel · `E`/`A`/`T` hide explorer/AI/terminal |

IDE commands in Command mode: `:build`, `:clean`, `:cancel`, `:e <file>` (also `:edit`, `:open`),
`:help`, `:settings`, `:compilers`, `:complete`, `:explain`, `:ai <question>`, `:assistant`,
`:edit_project_config`, `:edit_user_config`, `:reload`, `:profile`, `:target`, `:theme`, and every
other palette command id. Unknown commands report an error and change nothing.

**Explorer:** `j`/`k` or arrows, Enter/Right open or expand, Left collapse, `n` new file, Delete
delete (files and empty directories, always confirmed with `y`; open documents must be closed first),
`r` refresh (expanded directories and the selection are kept), `/` find, Esc editor. Ctrl+K also has
**Create Directory** and **Rename**. Files changed or deleted on disk are never overwritten by a save:
the save fails with an error and the buffer stays.

**Bottom panels:** `j`/`k`, PgUp/PgDn, End follows output, `/` filters, Tab next panel, Enter in
Problems jumps to the file, line and column, Esc editor.

**Mouse** (on by default; `:mouse` toggles it for the session, `"mouse": false` in `embedide.json` → `native` turns it off at start): click focuses a panel, places the cursor, selects or opens explorer rows; the wheel scrolls. A drag inside the AI panel selects only that panel's text (not the editor or explorer beside it) and copies it to the clipboard through the terminal (OSC 52: kitty, foot, WezTerm, Alacritty, iTerm2, xterm with `allowWindowOps`, tmux with `set-clipboard on`). While the mouse is captured, your terminal's own selection needs Shift+drag and then selects whole screen rows; turn the mouse off with `:mouse` if you prefer that.

Global bindings can be changed per project in `embedide.json` → `native.keys`
(`{"build": "f12", "focus_terminal": "alt+9"}`; F1–F12, `ctrl+a`–`ctrl+z`, `alt+0`–`alt+9`,
`ctrl+alt+0`–`ctrl+alt+9`). Invalid or duplicate chords are configuration errors; a binding to an
unknown command id is reported at startup (`:settings`). The table lives in one place
(`Workspace::shortcut`); Vim keys stay in the editor's `Keymap`.

**Keys that work in every xterm-compatible terminal** (no kitty protocol needed): Alt+1/2/3, F1–F12,
Ctrl+letter, Shift+PageUp/PageDown/Home/End/Up/Down, arrows, Esc, Enter, Tab, Backspace, Delete,
bracketed paste. Ctrl+Alt+digit and the held-digit hide chords need the kitty keyboard protocol.
Alt+1/2/3 therefore never reach programs in the integrated terminal.


## Theme

`theme = "dark"` (default, **Carbon**), `"light"` (**Bone**) or `"mono"` in the user config; `:theme`
cycles them. All colours live in `crates/embedide-terminal/src/tui/theme.rs` and come from the brand
palette `Desige/embedide-palette.json`:

| Role | Carbon (dark) | Bone (light) | Used for |
|------|---------------|--------------|----------|
| bars | `#F4F1EC` on `#1C1C20` | `#0B0B0C` on `#EAE5DC` | title/status bars, selections, popups |
| accent | `#E0703F` | `#A3401A` | AI prompt `›`, your messages, changed files, edited bytes |
| error / warning / success | `#F2645F` / `#E5AE45` / `#5DB57A` | `#B8302C` / `#7E5500` / `#276B3E` | diagnostics, build result, diff `-`/`+`, agent steps |
| AI | `#86B9CC` | `#2C6A80` | AI answers, agent actions, inline suggestions (italic) |
| muted | `#A8A6A2` | `#5C5A55` | hints, notes, command output |

Syntax: keywords `#E0703F`, types `#E3B98F`, strings `#A6C48E`, numbers `#E8C27A`, comments
`#8A8A91`, preprocessor/registers `#86B9CC`, jumps `#F3DCCB`; code itself stays ink. `dark` keeps
your terminal's background (set it to `#0B0B0C` for the exact look); `light` paints Bone. With
`COLORTERM=truecolor` the exact colours are used, otherwise the nearest xterm-256 colours; terminals
without 256 colours (Linux console, plain `xterm`) and `mono` use inverse video for bars.

## Indentation

Indentation follows the language of the file (detected like syntax highlighting). Rules look only at
code: braces inside strings and comments are ignored, and nothing is re-indented inside a block comment
or string. **Pasted text is inserted exactly as pasted.**

| Language | Enter adds a level after | A typed closer on a blank line aligns with its opener |
|---|---|---|
| Rust, C, C++, Zig, JavaScript/TypeScript, JSON | `{`, `[`, `(` at the end of the line | `}`, `]`, `)` |
| Shell | `{`, `(`, `then`, `do`, `else` | `}` |
| Python | `:` | — |
| Assembly (GNU as, NASM) | a label (`reset:`) at column 0 | — |
| Fortran, TOML, other text | — (the current indentation is kept) | — |

* Enter always keeps the current line's indentation; Enter between `{` and `}` puts the cursor on an
  indented empty line and the `}` on its own line. An Enter on a line that holds only indentation
  leaves no trailing blanks behind. `o` and `O` follow the same rules.
* **Tab** in Insert mode: in the leading blanks of a line it adds one level (snapping to the grid);
  after text it inserts a tab or spaces up to the next stop (measured in terminal cells, so wide
  characters are respected). **Shift+Tab** removes one level from the line wherever the cursor is.
* **Tab / Shift+Tab** in Normal mode shift the current line (a count shifts several: `3 Tab`); in Visual
  mode they shift the selected lines. Empty lines are not indented. A multi-line shift is one undo
  step; indentation typed in Insert mode is part of that Insert's undo step.

Configuration (`embedide.json` → `native.indent`, defaults shown):

```json
{"native": {"indent": {"auto": true, "width": 4, "style": "detect"}}}
```

`width` is the number of columns per level (1–16). `style` is `detect` (follow the file: tab-indented
files use tabs, space-indented files use spaces; Makefiles and `*.mk` always use tabs), `spaces` or
`tabs` (one tab per level). `auto: false` keeps only "Enter preserves indentation". The existing
`tab_width` setting is still the display width of tab characters.

## Code completion

While you type in Insert mode, a compact popup appears under the cursor (above it near the bottom of
the window) once a word has `completion.min_chars` characters (default 2), and after `.`, `::` or `->`
when a language server is running. It shows the name, the kind (`method`, `struct`, `word`, …) and a
short detail (type or signature); the last line names the source.

| Key (while the popup is visible) | Action |
|---|---|
| Down / Ctrl+N, Up / Ctrl+P | Select the next / previous item (wraps) |
| Tab or Enter | Accept the **selected** item |
| Tab or Enter with nothing selected | Their usual meaning (indent, new line); the popup closes |
| Esc | Close the popup and leave Insert mode (as in Vim, so `Esc :w` always saves) |
| Any other key | Goes to the editor; typing narrows the list, other characters close it |
| **Ctrl+Space** (also when `completion.auto` is off) | Ask explicitly; the first item is selected |

An automatic popup starts with **nothing selected**, so it never takes Enter or Tab away from you.
Accepting replaces the word you typed (or the range the language server names) and is part of the
current Insert undo step.

**clangd names.** If `clangd` is not on PATH, EmbedIDE also looks for the names packages give it: `clangd21` or
`clangd-21` on PATH, `/usr/local/llvm21/bin/clangd` (FreeBSD), `/usr/lib/llvm-21/bin/clangd` (Debian). The newest
version found is used; set `completion.servers.c.command` to choose another.

**Sources.** For Rust, C, C++ and Zig files EmbedIDE starts the matching language server the first time
completion is needed in such a file (never when a directory is merely opened), one process per server
for the workspace:

| Language | Server | Install (examples) |
|---|---|---|
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` or your distribution's package |
| C, C++ | `clangd` | the `clang` / `clang-tools` / `clangd` package of your distribution, or LLVM releases |
| Zig | `zls` | the zigtools release matching your Zig version, or your distribution's package |

Until a server is ready, when it is not installed, crashed, or for other languages (assembly, shell,
Python, …) and untitled buffers, the popup offers **words from the current file**, labelled
`words (local, no types)`: plain identifiers already in the document, with no knowledge of types,
methods or signatures. Server items are labelled with the server name. A missing or failed server is
reported once in the status bar; `:lsp_restart` (Ctrl+K: Restart Language Servers) tries again.

Servers run without a shell in their own process group and are stopped when the IDE exits. Requests
never block the editor: only the newest completion request is answered (older ones are cancelled),
and documents are sent to the server only when completion asks for them. clangd uses
`compile_commands.json` when the project has one. **rust-analyzer starts with build scripts and
procedural macros disabled** so that completion does not execute project code; zls may evaluate the
project's `build.zig` to discover modules.

Configuration (`embedide.json` → `native.completion`, defaults shown):

```json
{"native": {"completion": {
  "auto": true, "min_chars": 2, "lsp": true,
  "servers": {
    "rust": {"command": "rust-analyzer",
             "initialization_options": {"cargo": {"buildScripts": {"enable": true}},
                                        "procMacro": {"enable": true}}},
    "c":    {"command": "/opt/llvm/bin/clangd", "args": ["--query-driver=/opt/arm/bin/*"]},
    "zig":  {"command": ""}
  }
}}}
```

`servers` overrides the built-in command for `rust`, `c`, `cpp` or `zig` (the example re-enables build
scripts for rust-analyzer, uses another clangd, and turns zls off with an empty command).
`lsp: false` uses local words only; `auto: false` shows the popup only on Ctrl+Space, which can be
rebound like any command (`"keys": {"suggest": "f12"}`). The existing `:complete` command is the
separate, optional AI suggestion (ghost text).

**Limitations:** snippet placeholders are inserted as plain text; extra edits a server attaches to an
item (automatic imports) are not applied; there is no signature help, hover or LSP diagnostics yet;
indentation rules look back at most 2000 lines; shell `fi`/`done`/`esac` are not dedented as you type.

## Assembly

Files ending in `.asm`, `.nasm`, `.inc` (NASM) and `.s`, `.S`, `.sx` (GNU as) are assembly. The
dialect is detected per file and changes comments, registers and immediates:

| Dialect | Detected by | Comments | Notes |
|---|---|---|---|
| NASM (x86/x86-64, Intel) | `.asm`/`.nasm`/`.inc`, `bits`, `%define` | `;` | `%` preprocessor, bare registers |
| GNU as, AT&T (x86/x86-64) | other `.s` files (GNU as's default) | `#`, `/* */` | `%rax`, `$imm`; `;` separates statements |
| GNU as, Intel | `.intel_syntax` | `#`, `/* */` | bare registers |
| GNU as, ARM (32-bit/Thumb) | `.syntax unified`, `.thumb`, `r0`–`r15`, `bx` | `@`, `//` | `#imm` |
| GNU as, AArch64 | `.arch armv8`, `x0`–`x30`, `svc`, `adrp` | `//` | `#imm`, `b.eq` |
| GNU as, RISC-V | `a0`, `t0`, `ecall`, `li`, `la` | `#` | |

Override detection in `embedide.json` → `native.asm.syntax` (`auto`, `nasm`, `gas-att`, `gas-intel`,
`arm`, `aarch64`, `riscv`).

* **Highlighting:** mnemonics, **jumps/calls/returns/branches in their own colour** with their target
  label, registers, labels, directives, numbers, strings, comments.
* **Labels:** `:label name` jumps to a label, `:label` to the label under the cursor; Ctrl+K → *Go to
  Label* lists the file's labels (also typing `label:` in the palette).
* **Completion** (popup source `asm: labels, mnemonics, registers (local)`): this file's labels first,
  then the dialect's instructions (branches marked), directives and registers. These are fixed lists;
  the assembler is the authority on what is valid.
* **Indentation:** after a label at column 0, Enter indents the next instruction.
* **Errors:** NASM (`file:line: error:`) and GNU as (`file:line: Error:`) messages appear in PROBLEMS;
  Enter jumps to the line.
* **Building:** use a Makefile (see `examples/asm`, with NASM, AT&T and Intel GNU as programs and
  AArch64/RISC-V sources) or `embedide.toml` with `[toolchain] asm = "nasm"` or a GNU as driver.

## Binary files: HEX, ELF and disassembly

A file with a NUL byte or invalid UTF-8 in its first 8 KiB opens in the **binary view** instead of the
text editor (`embed prog`, `:e prog`, the explorer); `:hex [path]`, `:elf [path]`, `:disasm [path]`
(or Ctrl+K) open any file there. The bytes are never decoded or normalised as text, and **viewing never
writes**. The file is read in 64 KiB pages, so large images open instantly; offsets are 64-bit.

```text
OFFSET    HEX BYTES                                         ASCII
00000000  7F 45 4C 46 02 01 01 00  00 00 00 00 00 00 00 00  .ELF............
```

| Key (HEX) | Action |
|---|---|
| arrows / `h j k l`, PgUp/PgDn, `0` / `$`, `G` | Move the byte cursor |
| `g` | Go to offset (`0x1f00`, `1f00h` or decimal) |
| `/` then `n` | Search bytes (`7f 45 4c`, `0x7f454c`) or text in quotes (`"ELF"`, `\x00` escapes); wraps |
| `+` / `-` | 8, 16 or 32 bytes per row |
| `i` | **Edit mode**: two hex digits replace one byte (the first shows as `A_`), the cursor advances; Esc returns to navigation |
| `u` / Ctrl+R | Undo / redo one byte |
| `w` (Ctrl+S) | Write the changes — asks first and names the number of bytes |
| `q` (Ctrl+W) | Close the view (asks first when there are unsaved changes) |
| Tab, `x` / `e` / `d` | HEX / ELF / disassembly |

Edited bytes are coloured and counted in the status bar (`[+2 bytes changed]`); quitting the IDE with
changes asks for confirmation. **Saving** replaces bytes only — the length never changes, untouched
bytes are copied unchanged — and refuses if the file changed on disk since it was opened. The new
content is written to a temporary file next to the original, flushed, and renamed over it (atomic on
POSIX filesystems); permissions are kept, but ownership, extended attributes, ACLs and hard links are not.
`native.binary.allow_edit = false` makes every binary read-only.

**ELF inspector** (`e`): ELF32/ELF64, little and big endian. `1` header (class, endianness, machine,
type, entry point), `2` program headers, `3` sections (name, type, flags, address, offset, size), `4`
symbols (`.symtab` and `.dynsym`). `/` finds a name (an exact match first). Enter opens a segment or data
section in HEX, and an executable section or function symbol in the disassembly. Truncated or malformed
files show what could be read, with notes; nothing is executed. The reader is EmbedIDE's own
bounds-checked parser (no extra dependency).

**Disassembly** (`d`): uses the built-in pure-Rust `iced-x86` disassembler for x86 / x86-64 binaries (no
external `objdump` required), and falls back to `llvm-objdump -d` / `objdump -d -w` for other architectures
(AArch64, RISC-V, ARM, …) or when custom paths are configured; the program is never run.
Each line shows address, instruction bytes and the instruction; symbols (`<main>:`) and sections are
headers. `g` goes to an address (hex), `/` searches instructions or `<symbol>`, `n` repeats, `m` jumps to
`main`, `[` / `]` previous / next section, `s` toggles Intel / AT&T syntax on x86, Enter shows the bytes in
HEX. Stripped binaries have no symbols; no source mapping is invented. Settings: `native.binary`
(`bytes_per_row`, `allow_edit`, `disasm_syntax` = `att`/`intel`, `objdump`, `llvm_objdump`).

## Build and diagnostics

F5 builds the project with the same engine as `embed build`:

- with `embedide.toml`: the compilers selected in `[toolchain]` / `[profiles.<name>.toolchain]`
  (or your user defaults), incremental native builds, or the project's own Make/CMake/Cargo/Zig;
- without it: the legacy adapters (Make, CMake, Cargo, Zig detected from the project files,
  `embedide.json` profiles and tools).

Output streams into BUILD OUTPUT while you keep editing (builds use files on disk, so modified
buffers must be saved first). Diagnostics appear in PROBLEMS; Enter opens the file at the
reported line and column. F8 cancels the whole process group. Debugging, flashing and the serial
monitor run only from explicit commands (`debug`, `flash` with confirmation, `serial`).

## Choosing compilers

There is no settings screen in the terminal edition; compilers are configured in files, and the
future Qt application will edit the same files through the same Rust services.

```toml
# <project>/embedide.toml — per project and per profile
[toolchain]
c = "arm-none-eabi-gcc"
asm = "arm-none-eabi-gcc"

[profiles.debug]
flags = ["-O0", "-g"]

[profiles.host-test]
flags = ["-O1", "-g", "-fsanitize=address"]
toolchain = { c = "clang" }
```

```toml
# ~/.config/embedide/config.toml (EMBEDIDE_CONFIG_DIR overrides) — per user
[toolchains]
search_dirs = ["/opt/xpack-arm-none-eabi-gcc/bin"]   # searched after PATH
defaults = { c = "clang", cpp = "clang++" }           # for projects that do not choose

[[toolchains.custom]]
id = "my-gcc"
kind = "gcc"
path = "/opt/vendor sdk/bin/gcc"
```

`:edit_project_config` and `:edit_user_config` open these files in the editor; `:reload` applies
them; `:compilers` (and the Settings panel) shows which compiler each language uses and why.
EmbedIDE detects and reuses installed toolchains. It downloads one only if your `config.toml` requires it (progress appears in the build panel, then the build starts).

## AI assistant (Alt+2)

Optional. Without a provider, or with `EMBEDIDE_AI_OFFLINE=1`, the IDE works fully offline and the
panel only explains what is missing. Providers, models and keys are configured exactly as for
`embed ai` ([AI subsystem](ai.md)); four providers: Anthropic (Claude), OpenAI (GPT/Codex),
Moonshot (Kimi), xAI (Grok), via API key or, for Anthropic/OpenAI, the official subscription CLI.

The panel is the right column; Alt+2 moves the cursor to its input line (`›`). **Commands typed in
the editor are Vim commands** (`/` searches the file), so type AI commands in the panel. Start with
`/provider claude-code` or `/provider codex-cli` (your logged-in CLI, no API key) or an API provider.

Nothing is sent until you press Enter. The conversation is laid out as messages: a coloured header per
speaker (`▍You`, `▍Codex CLI`), indented text wrapped at word boundaries, code blocks framed and in a
code colour (`╭─ c` … `╰─`), headings highlighted, diffs in green/red, and quiet grey notes (bytes sent,
attachments, answer time and tokens). While a request runs, the panel title and the answer show an
animated spinner with the elapsed time (`⠙ thinking 3s`); streamed text ends in a blinking caret.
The empty input line shows what to type next.

| Input | Action |
|---|---|
| text + Enter | Chat (history kept until `/clear`), streamed |
| `/file`, `/file a.c b.h` | Attach the current buffer or named project files (project `[ai] exclude` and secret patterns apply) |
| `/selection` (`/sel`) | Attach the Visual selection |
| `/explain [question]` | Explain the selection or the current file |
| `/edit <instruction>` | Propose a new version of the current file; the unified diff is shown |
| `/apply` / `/reject` | Put the proposal into the editor buffer (undo with `u`, save with `:w`) / discard it |
| `/complete` | Completion at the editor cursor |
| `/agent <task>`; `/mode agent\|chat` | Run an **agent task** in the project with Claude Code or Codex CLI ([agent mode](agents.md)) |
| `y` / `a` / `n` | Answer an agent approval (allow once / allow similar / deny) |
| `/policy`, `/changes`, `/review [file]`, `/revert <file>\|all`, `/fix [command]`, `/new` | Agent policy, review and undo of its changes, build-and-fix loop, fresh session |
| `/skills`, `/skill <name>`, `/skill off <name>` | [Skills](skills.md) for agent tasks |
| `/inline [off\|manual\|auto]` | Show / switch inline AI suggestions |
| `/providers`; `/provider claude-code`, `codex-cli`, `anthropic`, …; `/model name`; `/auth api-key\|subscription` | List and switch backends for this session |
| `/diff`, `/history` | Show the pending diff again; recent conversation |
| `/cancel`, `/clear`, `/status`, `/help` | |
| ←/→, Home/End, Delete, Backspace | Edit anywhere in the input line (Ctrl/Alt+←/→ by word, Ctrl+Backspace/Delete erase a word, Ctrl+A/E start/end, Ctrl+U erase to the start); a click in the line places the cursor; paste goes in at the cursor |
| ↑/↓ | Recall earlier inputs (your unfinished text is restored after the last one) |
| PgUp/PgDn, Shift+↑/↓, wheel | Scroll the transcript |
| drag with the left button | Select text **inside the panel only** and copy it (OSC 52) when released; dragging past the top or bottom edge scrolls |
| Esc | Back to the editor |

**Agent tasks** are shown separately from chat: a `▍Agent · Codex CLI · workspace-write` header,
then each step — `$ make` with a spinner while it runs, the last lines of its output, `✓ ok` / `✗ exit
2`, `Read src/uart.c`, `✎ src/uart.c (edit)`, `⊘` refusals and `⚠ Approval needed` boxes — then the
agent's summary and a line such as `■ done · 3 file(s) changed · 4 command(s) (1 failed) · 1
approval(s) · 52.3 s`. The title shows `⠋ agent 12s · step 5 · workspace-write`. Approvals also appear
in the status bar and open the panel if it was hidden. Open buffers the agent changed are reloaded
unless they have unsaved edits. Details: [agents.md](agents.md).

**Inline AI suggestions:** `Alt+\` (or `:complete`, `/complete`) asks for a suggestion at the cursor.
It is shown in inverse video without changing the buffer — further lines of a multi-line suggestion as
virtual rows under the cursor line, the code below moving down. **Tab** inserts it (one undo step),
**Esc** dismisses it or cancels the request, typing its first characters keeps the rest, anything else
discards it. Without a suggestion, Tab and Shift+Tab indent as usual and Ctrl+Space opens the LSP
popup. `ai.completion.mode = "auto"` (or `/inline auto`) asks automatically after typing pauses at the
end of a line; the default is `manual`. Settings, context and measured latency (several seconds with
the subscription CLIs): [AI subsystem](ai.md#inline-suggestions).

*Example 3 — accepting a multi-line suggestion.* In `sum.c`, on the empty line inside
`int sum(int a, int b) { }`, press `A` (Insert at line end), then `Alt+\`:

```text
 2 int sum(int a, int b) {
 3     int total = a + b;          ← ghost text (inverse video), not in the buffer yet
       return total;               ← virtual row
 4 }
```

The status bar says `AI suggestion (2 lines): Tab accepts · Esc dismisses`. Typing `int` keeps the
rest (` total = a + b; …`) without a new request; **Tab** inserts both lines; `Esc`, `u` removes them
again and `Ctrl+R` restores them; `:w` saves. (This sequence is the PTY test
`test_inline_ai_suggestion_alt_backslash_multiline_type_through_tab_and_undo`, run against a local
mock provider.)

## For the Qt frontend

The terminal IDE is a thin adapter. Editing (`embedide-editor`), builds (`embedide-build`:
`prepare` → `Prepared::begin` / steps / `commit`), toolchains (`embedide-toolchains`,
`embedide_build::registry`), configuration (`embedide-workspace`), AI (`embedide-ai`: backends,
`complete::Engine`, `edit::propose`) and processes, serial and GDB/MI (`embedide-core`) are
frontend-independent crates.
