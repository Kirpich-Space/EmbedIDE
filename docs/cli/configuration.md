# Configuration reference

EmbedIDE has three configuration layers. Unknown keys are rejected, so typos are reported instead of
ignored. Nothing here ever stores a secret: API keys live in the environment or in the private
credentials file (`embed ai login`), never in these files.

| Layer | File | Scope | Format |
|---|---|---|---|
| User | `config.toml` in the configuration directory | every project | TOML |
| Project manifest | `embedide.toml` in the project root | one project (build) | TOML |
| IDE settings | `embedide.json` → `"native"` and `embedide.local.json` in the project root | one project (IDE behaviour) | JSON |

The configuration directory is `$XDG_CONFIG_HOME/embedide` or `~/.config/embedide` on Linux,
`~/Library/Application Support/EmbedIDE` on macOS and `%APPDATA%\EmbedIDE` on Windows.
`EMBEDIDE_CONFIG_DIR` overrides it. `embed config path` prints the locations; in the IDE
`:edit_user_config` / `:edit_project_config` (Ctrl+K) open the files and `:reload` applies changes.

Edit user and project TOML from a shell without losing comments:

```sh
embed config list                       # user config.toml
embed config set ai.agent.policy read-only
embed config get ai.agent.policy
embed config unset ai.agent.policy
embed config set build.flags '["-Wall", "-Wextra"]' --project .   # embedide.toml
```

`VALUE` is parsed as TOML (`true`, `3`, `["-O2"]`) and stored as a string otherwise. The file is
validated before it is written.

## User config: `config.toml`

### `[toolchains]`

| Key | Default | Meaning |
|---|---|---|
| `search_dirs` | `[]` | extra directories searched for compilers after `PATH` |
| `custom` | `[]` | toolchains registered with `embed toolchain add` (id, kind, path) |
| `defaults.<language>` | — | default toolchain id per language (`c = "clang"`) |

### `[ai]`

The complete table is in the [AI subsystem](ai.md#configuration-reference). Groups:

- backend: `provider`, `models.<task>`, `providers.<id>.{model,auth,base_url,cli}`
- limits: `max_tokens`, `max_context_bytes`, `cli.{timeout_secs,max_output_bytes,allow_api_key}`
- inline suggestions: `completion.*`, `completion_debounce_ms`, `inline`
- agents: `agent.{policy,mode,timeout_secs,snapshot_max_bytes,snapshot_max_files,max_fix_iterations,fix_command,allow_commands,persist_sessions}`
- skills: `skills.{enabled,dirs,max_bytes,auto,exclude}`

Example:

```toml
[toolchains]
search_dirs = ["/opt/xpack-arm-none-eabi-gcc/bin"]
[toolchains.defaults]
c = "gcc"

[ai]
provider = "codex-cli"
[ai.agent]
policy = "workspace-write"
timeout_secs = 1800
[ai.completion]
mode = "manual"
[ai.skills]
exclude = ["asm-review"]
```

## Project manifest: `embedide.toml`

Described in [embedide-toml.md](embedide-toml.md): `[project]`, `[build]`, `[toolchain]`, `[target]`,
`[profiles.*]`, `[env]`, `[ai] exclude`, `[debug]`.

## IDE settings: `embedide.json` → `native`

`embedide.json` may hold other data (kept untouched); only the `"native"` object is read.
`embedide.local.json` has the same shape (without the `native` wrapper), is merged on top, and is
ignored by git: use it for machine-specific values.

```json
{
  "native": {
    "theme": "dark",
    "tab_width": 4,
    "mouse": true,
    "keys": { "build": "F12", "focus_assistant": "alt+5" },
    "indent": { "auto": true, "width": 4, "style": "detect" },
    "completion": { "auto": true, "min_chars": 2, "lsp": true,
      "servers": { "c": { "command": "clangd", "args": ["--background-index"] } } },
    "asm": { "syntax": "auto" },
    "binary": { "bytes_per_row": 16, "allow_edit": true, "disasm_syntax": "att" },
    "serial": { "port": "/dev/ttyUSB0", "baud": 115200 },
    "debug": { "gdb": "arm-none-eabi-gdb", "remote": "127.0.0.1:3333", "elf": "build/debug/app.elf" },
    "tasks": { "test": { "program": "make", "args": ["test"] } }
  }
}
```

| Key | Default | Meaning |
|---|---|---|
| `version` | `1` | schema version (must be 1) |
| `theme` | `dark` | `dark`, `light` or `mono` (inverse video only); `:theme` cycles |
| `tab_width` | `4` | display width of a tab, 1–16 |
| `mouse` | `true` | capture the mouse: click, wheel, drag-select in the AI panel; `:mouse` toggles |
| `keys.<command>` | — | rebind a command ID (see [keys](keys.md#command-ids)) to `F1`–`F12`, `ctrl+<letter>`, `ctrl+space`, ``alt+\``, `alt+<digit>` or `ctrl+alt+<digit>`; chords must be unique |
| `indent.auto` | `true` | language-aware Enter and closers |
| `indent.width` | `4` | columns per level, 1–16 |
| `indent.style` | `detect` | `detect` (follow the file), `spaces`, `tabs` (Makefiles always use tabs) |
| `completion.auto` | `true` | popup while typing |
| `completion.min_chars` | `2` | letters typed before the popup opens |
| `completion.lsp` | `true` | use language servers (rust-analyzer, clangd, zls) when installed |
| `completion.servers.<lang>` | — | `command`, `args`, `initialization_options` of a language server |
| `asm.syntax` | `auto` | assembly dialect: `auto` (detected per file), `nasm`, `gas-att`, `gas-intel`, `arm`, `aarch64`, `riscv` |
| `binary.bytes_per_row` | `16` | HEX view width |
| `binary.allow_edit` | `true` | permit HEX editing (saves still need confirmation) |
| `binary.disasm_syntax` | `att` | `att` or `intel` |
| `binary.objdump`, `binary.llvm_objdump` | auto | disassembler paths |
| `serial.*` | 115200 8N1, receive-only | `port`, `baud`, `data_bits`, `parity`, `stop_bits`, `flow_control`, `line_ending`, `receive_only` |
| `debug.*` | gdb on `127.0.0.1:3333` | `gdb`, `openocd`, `configs`, `remote`, `elf` — started only by explicit commands |
| `tasks.<name>` | — | explicit tasks (`program`, `args`; no shell) shown in the command palette |
| `profiles`, `targets`, `environment`, `tools`, `build_directory`, `cmake_*` | — | legacy build settings for Make/CMake/Cargo projects |

## Environment variables

| Variable | Effect |
|---|---|
| `EMBEDIDE_CONFIG_DIR` | configuration directory |
| `EMBEDIDE_AI_OFFLINE=1` | every AI request is refused; the IDE works fully offline |
| `EMBEDIDE_KEYBOARD=legacy` | do not request the kitty keyboard protocol |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `MOONSHOT_API_KEY`, `XAI_API_KEY` | API keys for the API providers; removed from the Claude Code / Codex CLI environment unless `ai.cli.allow_api_key = true` |
| `COLORTERM=truecolor` | exact theme colours (otherwise the nearest 256 colours) |
| `SHELL` | shell of the integrated terminal |
| `EMBEDIDE_PROJECT` | set by EmbedIDE (in the integrated terminal and `embed terminal`) to the project root |
