# Troubleshooting and FAQ

## Installing and starting

**`embed: command not found`** — the install directory is not on `PATH`. For `~/.local/bin`:
bash/zsh: `export PATH="$HOME/.local/bin:$PATH"` in `~/.profile`; fish:
`fish_add_path ~/.local/bin`. Open a new terminal. `which embed` shows what runs.

**`embed` exits immediately with status 2** — the IDE needs an interactive terminal (a TTY on stdin
and stdout and `TERM` other than `dumb`). Inside scripts or pipes use the subcommands
(`embed build`, `embed doctor`, …). `embed --version` works anywhere.

**`version GLIBC_2.39 not found`** — the release archive needs glibc 2.39+ (Ubuntu 24.04+,
Debian 13+, Fedora 40+, Arch). On older or musl systems build from source: `cargo build --release
--locked` (Rust 1.85+).

**Check your setup:** `embed doctor` prints the OS, the configuration file and the toolchains found per
language. `embed config path` prints the file locations.

**Start from scratch:** quit the IDE and move `~/.config/embedide` away (user settings and the
credentials file). Project files are never touched by this. **Uninstall:** delete the `embed` binary and
that directory.

## Display and keys

**Wrong or missing colours** — set `COLORTERM=truecolor` if your terminal supports 24-bit colour,
otherwise the nearest 256 colours are used. `"theme": "mono"` in `embedide.json` → `native` (or
`:theme`) uses only your terminal's own colours.

**Alt+1/2/3 do nothing** — some terminals or window managers take Alt+digit. Use `Ctrl+Alt+1/2/3`
or `F6` (cycle focus), or bind other chords: `"keys": {"focus_assistant": "alt+5"}`. Inside `tmux`
enable extended keys (`set -g extended-keys on`) or use `Ctrl+Alt` / `F6`.

**Hold-Alt-and-Delete does not hide a panel** — your terminal does not report a held key (needs the
kitty keyboard protocol). Use `:hide_explorer`, `:hide_assistant`, `:hide_terminal`.
`EMBEDIDE_KEYBOARD=legacy` forces the legacy key encoding if enhanced keys misbehave.

**`/provider …` typed in the editor shows "search not found"** — in the editor `/` is Vim search.
Press `Alt+2` first and type AI commands in the panel.

**Selecting text selects the whole screen** — with the mouse captured (default), drag inside the AI
panel to select its text; elsewhere use `Shift`+drag for the terminal's own selection, or turn capture
off with `:mouse`.

**Drag-selected AI text is not on the clipboard** — copying uses OSC 52. kitty, foot, WezTerm,
Alacritty and iTerm2 support it; xterm needs `allowWindowOps`; tmux needs `set -g set-clipboard on`;
many VTE-based terminals ignore it. Use `Shift`+drag with your terminal's own copy instead.

**Terminal panel: can't scroll** — `Shift+PgUp` / `Shift+PgDn`; if your terminal keeps those for its own
scrollback, use the mouse wheel over the panel.

## FreeBSD

FreeBSD is supported and verified on 15.1 amd64 (UFS); please report problems on other versions or filesystems. The base `objdump` is LLVM's; EmbedIDE works with either.

- Install: `sudo pkg install rust gmake git` (a C compiler is in the base system as `cc`; `pkg install gcc`
  for GCC), then `cargo install --path crates/embedide-cli --locked --root ~/.local`.
- **Makefile projects fail with syntax errors** — BSD `make` cannot read GNU Makefiles. EmbedIDE uses
  `gmake` automatically when it is on `PATH`; in the terminal run `gmake`.
- **Serial ports** are `/dev/cuaU0`, `/dev/cuaU1`, … (USB) and `/dev/cuau0` (UART). Add your user to the
  `dialer` group and log in again if opening a port is denied.
- **Compilers**: `cc` is Clang; `embed toolchain list` shows what is found, `embed toolchain recommend c`
  prints `pkg install` suggestions.

## Editing and building

**The build ignores my last edit** — builds use files on disk. Save (`:w` or `Ctrl+S`) before `F5`.

**"file changed externally; reopen or reconcile before saving"** — a save refuses to overwrite a file
that changed outside the IDE (another editor, an agent, `git checkout`). Reopen the file (`:q!` then
`:e <path>`), or save your version under a new name with `:w <file>`.

**Toolchain not installed or not configured** — EmbedIDE downloads a compiler only if your `config.toml` describes it (`[toolchains.install.<name>]`) and something requires it; otherwise it installs nothing. `embed toolchain
list` shows what it found; `embed toolchain recommend c` prints install suggestions (not executed);
`embed toolchain add <id> --kind gcc --path /path/to/gcc` registers one; `embed toolchain search-dir
/opt/x/bin` adds a directory.

**Make refuses my compiler path** — Make cannot take a compiler path that contains spaces; EmbedIDE
refuses instead of running a broken command. Put the compiler on `PATH` or in a path without spaces.

**`embed clean` refuses to clean** — it removes only directories it created (marked
`.embedide-output`) inside the project. That is deliberate.

**No completion popup** — it needs two typed letters (`completion.min_chars`). Language servers
(rust-analyzer, clangd, zls) are optional; without them you get words from the open file.
`:lsp_restart` restarts a stuck server; `Ctrl+Space` asks explicitly.

**Tab inserts spaces/tabs in an unexpected way** — `indent.style = "detect"` follows the file
(tabs in tab-indented files and Makefiles). Set `"spaces"` or `"tabs"` to force it.

**Serial port permission denied** (Linux) — add your user to the group that owns the device (often
`dialout` or `uucp`) and log in again. EmbedIDE only opens a port when you connect.

## AI

**Which backend should I pick?** `claude-code` or `codex-cli` if you already pay for Claude or
ChatGPT: they use that login, need no API key, and can run agent tasks. API providers
(`anthropic`, `openai`, `moonshot`, `xai`) need a key and are billed per request; they chat, propose
edits and complete code but never edit files by themselves.

**"Not logged in" / "token expired"** — log in with the CLI in a normal terminal (`claude` or `claude
auth login`; `codex login`), then `/status` in the panel.

**Usage or rate limit reached** — the CLI's plan limit. EmbedIDE does not retry, pool accounts or fall
back to a paid API key; wait or switch backend.

**Plain text only chats** — the backend is an API provider, or `/mode chat` is set. Use
`/provider codex-cli`/`claude-code` and `/mode agent`.

**The agent says it cannot edit files** — that is chat mode; run it as an agent (`/mode agent`, or
`/agent <task>`).

**Agent does nothing / "tool host failed to start"** — run the same task with the CLI alone
(`codex exec "…"`, `claude -p "…"`). If that fails too, update or log in to that CLI.

**A command was refused / `Approval needed` keeps appearing** — see
[what asks under each policy](agents.md#policies). `a` (allow similar) stops repeated questions for
the rest of the session; `ai.agent.allow_commands = ["just"]` allows extra command names permanently
(Claude Code).

**The agent changed something I did not want** — `/review` shows the diffs, `/revert <file>` or
`/revert all` restores them (it refuses files you edited afterwards).

**Will it send my secrets?** Only files you attach or the agent reads in the workspace reach the
provider. Project `[ai] exclude` patterns and built-in secret patterns (`.env`, `*.pem`, keys) apply to
chat attachments; the agent's own rules ask before reading or changing sensitive files. For projects
you do not trust, use `read-only` or `approval-required`, or `EMBEDIDE_AI_OFFLINE=1` to disable AI.

**Does a subscription include API credits?** No. A Claude or ChatGPT subscription does not pay for the
`anthropic` / `openai` API backends. EmbedIDE removes API-key variables from the CLIs' environment so a
CLI request never silently becomes a billed API call (`ai.cli.allow_api_key` changes that).

**Inline suggestions are slow** — through a subscription CLI each suggestion takes several seconds.
Keep `ai.completion.mode = "manual"` (ask with `Alt+\`) or point `ai.completion.provider` at a fast API
model.

## FAQ

**Which platforms work?** Linux and FreeBSD. Linux x86_64 is the verified platform and the only one with
published binaries. FreeBSD (15.1 amd64) is verified too: there is no published FreeBSD binary yet, so build it from source (`pkg install rust`, then
`cargo build --release --locked`) and report what breaks. Windows and macOS are not supported.

**Is there a GUI?** No. `embed` is a terminal application. A native GUI is planned, not started.

**Can it flash or debug my board?** Through explicit commands only (flash preview with confirmation,
GDB/MI with OpenOCD, serial monitor), and these have not been verified on physical hardware in this
release. Nothing starts automatically.

**Does it need the network?** No. Editing, building and the integrated terminal are fully offline. AI
features and `cargo` with `--online` use the network only when you ask.

**Can I use my own editor keys?** Rebind commands in `native.keys`; the editor's Vim-style modal input
itself is fixed. See [keys](keys.md).

**Where are the settings?** [Configuration reference](configuration.md).

**Where do I report bugs?** GitHub issues; vulnerabilities privately (see [SECURITY.md](../../SECURITY.md)).
Include `embed --version`, your terminal and OS, and the steps to reproduce.
