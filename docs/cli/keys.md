# Key and command reference

Quick lookup. Explanations and context are in the [terminal IDE guide](tui.md); `F1` or `:help` shows
the same keys inside the IDE and `Ctrl+K` opens the command palette with every command below.

## Panels

| Key | Action |
|---|---|
| `Alt+1` / `Alt+2` / `Alt+3` | explorer / AI panel / terminal; again returns to the editor (`Ctrl+Alt+1/2/3` works too) |
| hold `Alt+<n>`, press `Delete` | hide that panel (needs the kitty keyboard protocol); use `:hide_explorer`, `:hide_assistant`, `:hide_terminal` otherwise |
| `F2` | show / hide the explorer |
| `F3` | problems (diagnostics, search results); `Enter` jumps to the location |
| `F4` | cycle the bottom panels (build output, task log, serial, …) |
| `F6` | cycle focus: editor → explorer → bottom panel → AI → editor |
| `Esc` | leave a panel and return to the editor |

## Global

| Key | Action |
|---|---|
| `Ctrl+K` | command palette |
| `Ctrl+P` | filter explorer entries by name |
| `Ctrl+S` | save |
| `Ctrl+W` / `Ctrl+Q` | close document / quit (unsaved changes ask first) |
| `Ctrl+Space` | LSP / word completion popup (Insert mode) |
| `Alt+\` | AI inline suggestion (Insert mode); `Tab` accepts, `Esc` dismisses |
| `F1` | help |
| `F5` | build |
| `F7` | `cargo check` (Cargo projects) |
| `F8` | cancel the running build, task or AI request |
| `F9` / `F10` | select target / next document |

## Editor (Vim-style)

Modes: Normal, Insert, Visual (`v` characters, `V` lines), Command (`:`), Search (`/`, `?`).

| Keys | Action |
|---|---|
| `h j k l`, arrows | move |
| `w b e`, `0 ^ $` | word and line motions |
| `gg`, `G`, `12G`, `:12` | first line, last line, line 12 |
| `i a I A o O` | enter Insert (before, after, line start, line end, new line below / above) |
| `x`, `dd`, `dw`, `d$`, `cc`, `cw`, `ciw`, `yy`, `yw`, `yiw` | delete, change, yank with counts (`3dd`, `2d3w`) |
| `p` / `P` | paste after / before |
| `u` / `Ctrl+R` | undo / redo |
| `/text`, `?text`, `n`, `N` | search forward / backward, repeat |
| `v`, `V` + motion + `y d c` | Visual selection operators |
| `Tab` / `Shift+Tab` | Insert: indent / dedent; Normal and Visual: shift the line(s) |
| `Esc` | leave Insert / Visual / prompt |

Pasted text is inserted literally; it is never run as commands.

## `:` commands

| Command | Action |
|---|---|
| `:w`, `:w <file>`, `:saveas <file>` | save (to a new file; an existing file is never overwritten by `:w <file>`) |
| `:q`, `:q!`, `:wq` | close, close without saving, save and close |
| `:<n>` | go to line *n* |
| `:e <path>` | open a file (`:e` alone opens a prompt) |
| `:label [name]` | go to an assembly label |
| `:hex [path]`, `:elf [path]`, `:disasm [path]` | open the binary view |
| `:complete` | AI suggestion at the cursor |
| `:explain` | ask the AI panel to explain the file or selection |
| `:ai <text>` | send a message to the AI panel |
| `:mouse` | toggle mouse capture |
| `:reload`, `:settings`, `:compilers`, `:theme`, `:help` | reload / show configuration, show chosen compilers, cycle theme, help |
| `:assistant`, `:terminal_restart`, `:hide_*` | any command ID below can be typed after `:` |

`/` in the editor searches the file; AI commands go in the AI panel.

## Explorer, panels, terminal

| Where | Keys |
|---|---|
| Explorer | `j`/`k` move, `Enter`/`Right` open or expand, `Left` collapse, `n` new file, `Delete` delete, `r` refresh, `Esc` editor |
| Output panels | `j`/`k` scroll, `PgUp`/`PgDn`, `End` follow, `/` filter, `Tab` next panel |
| Terminal | everything goes to the shell; `Shift+PgUp`/`PgDn`/`Home`/`End` scroll history |
| Binary view | HEX: arrows / `h j k l` move, `g` goto offset, `/` + `n` search, `+`/`-` bytes per row, `i` edit mode (hex digits replace bytes), `u` / `Ctrl+R` undo / redo, `w` save (asks first), `q` close; `Tab`, `x`, `e`, `d` switch HEX / ELF / disassembly — see [binary files](tui.md#binary-files-hex-elf-and-disassembly) |

## AI panel (`Alt+2`)

Typing: `←/→`, `Home/End`, `Delete`, `Backspace`, `Ctrl+←/→` words, `Ctrl+A/E/U`; `↑/↓` recall earlier
inputs; `PgUp/PgDn`, `Shift+↑/↓` and the wheel scroll the transcript; dragging with the mouse selects
and copies the panel's text. Approvals: `y` allow, `a` allow similar, `n` deny.

| Command | Action |
|---|---|
| `/providers`, `/provider <id>`, `/status`, `/model <id>`, `/auth <mode>` | choose and inspect the backend |
| `/mode agent\|chat`, `/policy [name]`, `/agent <task>`, `/new`, `/cancel` | agent mode and its policy |
| `/changes`, `/review [file]`, `/revert <file>\|all`, `/fix [command]` | review, undo and fix loop |
| `/skills`, `/skill <name>`, `/skill off <name>`, `/skill clear` | skills |
| `/file [paths]`, `/selection`, `/explain`, `/edit <instruction>`, `/diff`, `/apply`, `/reject` | chat context and edit proposals |
| `/complete`, `/inline [off\|manual\|auto]` | inline suggestions |
| `/history`, `/clear`, `/help` | conversation |

## Command IDs

Usable after `:`, in the palette, and as keys in `native.keys` (for example
`"keys": {"build": "F12"}`):

| Area | IDs |
|---|---|
| Panels | `focus_explorer`, `focus_assistant`, `focus_terminal`, `hide_explorer`, `hide_assistant`, `hide_terminal`, `assistant`, `explorer`, `toggle_panel`, `terminal_restart` |
| Files | `open`, `find`, `reveal`, `refresh`, `new_file`, `new_directory`, `rename`, `delete`, `save`, `close`, `next_document`, `quit` |
| Build | `build`, `check`, `clean`, `cancel`, `target`, `profile`, `problems`, `search`, `search_ignore_case`, `compilers`, `tools` |
| Editing | `suggest` (LSP completion), `ai_suggest` (AI suggestion), `lsp_restart`, `labels`, `hex`, `elf`, `disasm`, `close_binary` |
| AI | `assistant`, `explain` |
| Hardware (explicit only) | `probes`, `flash`, `ports`, `serial_port`, `serial`, `serial_disconnect`, `serial_send`, `serial_pause`, `serial_timestamps`, `serial_clear`, `serial_export`, `debug`, `debug_stop`, `continue`, `interrupt`, `step`, `next`, `breakpoint`, `break_delete`, `backtrace`, `registers` |
| Settings | `settings`, `edit_project_config`, `edit_user_config`, `reload`, `theme`, `mouse`, `help`, `palette` |

Allowed chords: `F1`–`F12`, `ctrl+<letter>`, `ctrl+space`, ``alt+\``, `alt+<digit>`,
`ctrl+alt+<digit>`. Each chord can be bound once.
