# `embed` — the terminal IDE

`embed` (no arguments) opens the IDE on the current directory; `embed <directory>` opens another
project; `embed <file>` opens that file (in the nearest directory with `embedide.toml`, or the current
directory when the file is inside it). A missing file in an existing directory becomes a new buffer that
is written only on `:w`. Subcommand names win over same-named files: use `embed ./build`. It needs an interactive terminal (`TERM` other than `dumb`); no GUI, Electron or Node.js.

```
EmbedIDE 0.1.0 | Ready | project | target | profile            ← build status, project, target, profile
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
| **Ctrl+Alt+1** | Explorer — press again to return to the editor |
| **Ctrl+Alt+2** | AI assistant — press again to return to the editor |
| **Ctrl+Alt+3** | Terminal — press again to return to the editor |
| **Ctrl+Alt+1/2/3 + Delete** | Hold the digit, press Delete: hide that panel (its state and shell are kept) |
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

`Ctrl+Alt+<digit>` and "digit still held" exist only with the
[kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) (for example kitty, foot,
WezTerm, Ghostty, Alacritty 0.13+). `embed` asks the terminal at startup and enables
it only when the terminal answers; `EMBEDIDE_KEYBOARD=legacy` turns it off.

Without it (commonly xterm, tmux/screen and the Linux console; check `:help`, which says which mode
is active) the terminal cannot report these chords at all: Ctrl+Alt+1 arrives as `ESC 1` — the same
bytes as **Alt+1** — and xterm-style terminals send Ctrl+Alt+3 as `ESC ESC` (indistinguishable from
two Esc presses). There:

* **Alt+1 / Alt+2 / Alt+3** switch panels (and `ESC <digit>` from Ctrl+Alt+1 works as well);
* the hide chord is never guessed from separate presses; use `:hide_explorer`, `:hide_assistant`,
  `:hide_terminal` (also in Ctrl+K), or bind them, e.g. in `embedide.json`:
  `{"native": {"keys": {"hide_terminal": "alt+6", "hide_explorer": "f12"}}}`. Bindings may be
  F1–F12, `ctrl+a`…`ctrl+z`, `alt+0`…`alt+9` or `ctrl+alt+0`…`ctrl+alt+9` (in a legacy terminal a
  `ctrl+alt+N` binding answers to Alt+N).

Ctrl+Alt+Delete is a system shortcut on many desktops and on the Linux console (it may reboot); such
a system takes it before any terminal program sees it, so prefer a bound key there.

In the **terminal** every key — Ctrl+C, Ctrl+D, Ctrl+K, Tab, Esc, F-keys, arrows — goes to your
shell; only the panel keys (Ctrl+Alt+1/2/3, Alt+1/2/3 without the kitty protocol) and keys you bound
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
key then never reaches `embed`), turn on `:mouse` and use the wheel.

A program that floods output (`yes`) cannot freeze the IDE or grow its memory: output is applied in
bounded slices between frames, and the reader waits for the screen instead of buffering without limit.

**Editor** (Vim/Kenit modal editing, shared engine `embedide-editor`):

| Mode | Keys |
|---|---|
| Normal | `h j k l`, `w b e`, `0 $`, `gg G`, counts, `x`, `dd`, `yy`, `p`, `u`, Ctrl+R, `/ ? n N`, `v` / `V` |
| Insert | `i a o O` enter, Esc leaves |
| Command | `:w`, `:q`, `:q!`, `:wq`, `:<line>` (go to line, clamped), `:w <file>` / `:saveas <file>` (save as a **new** file — an existing file is never overwritten; this is also how an untitled buffer is saved), and IDE commands (below) |
| Leader (Space) | `e` explorer · `b` build · `d` debug · `t` terminal · `s` serial · `f` find file |

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

**Mouse** (off by default, because capturing the mouse takes text selection away from your terminal):
`:mouse` toggles it for the session, `"mouse": true` in `embedide.json` → `native` turns it on at start.
A click focuses the panel under it, places the editor cursor, selects an explorer row (a second click
opens or expands it); the wheel scrolls the editor, explorer, AI chat, output panels and terminal
history. With the mouse on, most terminals still select text with Shift+drag.

Global bindings can be changed per project in `embedide.json` → `native.keys`
(`{"build": "f12", "focus_terminal": "alt+9"}`; F1–F12, `ctrl+a`–`ctrl+z`, `alt+0`–`alt+9`,
`ctrl+alt+0`–`ctrl+alt+9`). Invalid or duplicate chords are configuration errors; a binding to an
unknown command id is reported at startup (`:settings`). The table lives in one place
(`Workspace::shortcut`); Vim keys stay in the editor's `Keymap`.

**Keys that work in every xterm-compatible terminal** (no kitty protocol needed): Alt+1/2/3, F1–F12,
Ctrl+letter, Shift+PageUp/PageDown/Home/End/Up/Down, arrows, Esc, Enter, Tab, Backspace, Delete,
bracketed paste. Ctrl+Alt+digit and the held-digit hide chords need the kitty keyboard protocol.

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
EmbedIDE detects and reuses installed toolchains; it never installs or downloads one.

## AI assistant (Ctrl+Alt+2)

Optional. Without a provider, or with `EMBEDIDE_AI_OFFLINE=1`, the IDE works fully offline and the
panel only explains what is missing. Providers, models and keys are configured exactly as for
`embed ai` ([AI subsystem](ai.md)); four providers: Anthropic (Claude), OpenAI (GPT/Codex),
Moonshot (Kimi), xAI (Grok), via API key or, for Anthropic/OpenAI, the official subscription CLI.

Nothing is sent until you press Enter. Before every request the panel shows how many bytes go to
which provider and which files are attached.

| Input | Action |
|---|---|
| text + Enter | Chat (history kept until `/clear`), streamed |
| `/file` | Attach the current buffer (project `[ai] exclude` and secret patterns apply) |
| `/sel` | Attach the Visual selection |
| `/explain [question]` | Explain the selection or the current file |
| `/edit <instruction>` | Propose a new version of the current file; the unified diff is shown |
| `/apply` / `/reject` | Put the proposal into the editor buffer (undo with `u`, save with `:w`) / discard it |
| `/complete` | Completion at the editor cursor |
| `/provider <id>`, `/model <id>`, `/auth api-key\|subscription` | Switch for this session |
| `/cancel`, `/clear`, `/status`, `/help` | |
| Up/Down, PgUp/PgDn | Scroll the transcript |
| Esc | Back to the editor |

**Inline completion:** `:complete` (or `/complete`) requests one suggestion at the cursor. It is
shown in inverse video in the editor without changing the buffer: **Tab** inserts it, **Esc**
rejects it, moving the cursor or editing discards it. Automatic suggestions while typing are off;
`ai.inline = true` in `config.toml` enables them (debounced by `ai.completion_debounce_ms`; each
suggestion is a provider request).

## For the Qt frontend

The terminal IDE is a thin adapter. Editing (`embedide-editor`), builds (`embedide-build`:
`prepare` → `Prepared::begin` / steps / `commit`), toolchains (`embedide-toolchains`,
`embedide_build::registry`), configuration (`embedide-workspace`), AI (`embedide-ai`: backends,
`complete::Engine`, `edit::propose`) and processes, serial and GDB/MI (`embedide-core`) are
frontend-independent crates.
