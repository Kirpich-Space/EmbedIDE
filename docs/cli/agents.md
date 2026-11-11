# Agent mode: Claude Code and Codex CLI working in your project

In agent mode the installed, logged-in **Claude Code** or **Codex CLI** works directly in the
project: it reads and searches files, creates, edits, renames and deletes them, runs builds and
tests, reads the errors and fixes them. EmbedIDE starts the CLI, answers its permission requests
under the policy you chose, shows every step, lets you review and revert what changed, and stops it
(with every process it started) on `/cancel`.

Chat (`text + Enter`, `/edit`, `/complete`) is unchanged and never touches files. Agent mode is
separate and explicit: `/agent <task>`, or `/mode agent` so that plain text starts tasks.

No API key is needed: the CLI uses its own login and plan. See [AI subsystem](ai.md) for backends,
billing and authentication rules, which apply unchanged.

## Quick start

```text
Alt+2                         focus the AI panel
/provider codex-cli           or /provider claude-code
/agent add a unit test for ring_buffer_push and make the tests pass
```

From a shell (same runtime, approvals asked on the terminal):

```sh
embed ai agent --provider claude-code "fix the warnings in src/uart.c"
embed ai agent --provider codex-cli --policy read-only "explain how the bootloader jumps to the app"
embed ai agent --provider claude-code --fix "make test" "the ring buffer tests fail"
```

`embed ai agent` exits 0 when the task (and the `--fix` check) succeeded; `--json` prints a summary
(`changes`, `commands`, `failed_commands`, `denied`, `session`, `errors`). Without a terminal on stdin
every request that needs approval is refused.

## Policies

| Policy | Claude Code | Codex CLI |
|---|---|---|
| `read-only` | tools limited to `Read,Glob,Grep`; changes and commands other than plain reads are refused | `codex exec --sandbox read-only` |
| `workspace-write` (default) | edits inside the workspace and ordinary build/test commands run without asking; everything else asks you | `codex exec --sandbox workspace-write`: Codex's OS sandbox allows writes only inside the workspace (and temp), no network |
| `approval-required` | every file change and every command except plain reads asks you | Codex plans in the read-only sandbox; you approve the plan; then the same thread carries it out with `workspace-write` |

Set the default with `ai.agent.policy`; switch per session with `/policy <name>` (`/policy` alone
lists them) or `embed ai agent --policy`.

**What asks under `workspace-write` (Claude Code).** Every tool call that Claude Code's own rules do
not allow reaches EmbedIDE (Agent SDK host protocol: `--input-format stream-json`,
`--permission-prompt-tool stdio`, `--permission-mode manual`). EmbedIDE parses shell commands without
running a shell (quotes, `&&`, `|`, `;`, redirections, `for … do … done`) and classifies each part:

- allowed: reading/searching (`ls`, `cat`, `grep`, `rg`, `find` without `-delete`/`-exec`, `git
  status/diff/log/show`), builds and tests (`make`, `cmake`, `ninja`, `cargo build/test/check/clippy`,
  `zig build/test`, `gcc`, `clang`, `cc`, `as`, `nasm`, `ld`, `objdump`, cross tools such as
  `arm-none-eabi-gcc`, `python3 -m pytest`, `qemu-*`), running programs inside the workspace
  (`./app`), `mkdir`/`cp`/`mv`/`touch` inside the workspace, and the names in `ai.agent.allow_commands`;
- asks: deleting or overwriting data (`rm`, `dd`, `truncate`, `find -delete`, `git reset/clean/checkout
  /push/commit/stash`), privileges or other processes (`sudo`, `chown`, `kill`, `systemctl`), network
  and installs (`curl`, `wget`, `ssh`, `pip`, `npm`, `cargo install`, `git fetch/pull`), **hardware**
  (`openocd`, `probe-rs`, `st-flash`, `esptool`, `avrdude`, `picotool`, `dfu-util`, serial terminals),
  nested shells and interpreters (`bash -c`, `eval`, `xargs`, `perl`, `node`), command substitution and
  here-documents, any path outside the workspace (absolute, `..`, `~`, `$HOME`, symlinks are resolved),
  `.git` internals, `.env` files and keys, the network tools `WebFetch`/`WebSearch`, MCP tools, and any
  command not on the list;
- refused: interactive questions (`AskUserQuestion`; the agent asks in its reply instead).

These rules only narrow what Claude Code allows. EmbedIDE never passes
`--dangerously-skip-permissions`, `bypassPermissions` or Codex's `--dangerously-bypass-approvals-and-sandbox`,
and Claude Code's own safety checks (for example on its configuration directory) still apply.

**Codex under `workspace-write`.** `codex exec` has no channel for per-command approval, so EmbedIDE
runs it with approvals `never`: whatever the sandbox does not allow fails and the model is told so;
nothing waits for an answer nobody can give. Deleting a file *inside* the workspace is possible in the
sandbox; review and `/revert` cover that. Use Claude Code or `approval-required` when every step
should be confirmed.

## Approvals

A request that needs your decision appears in the panel:

```text
  ⚠ Approval needed: $ rm -rf build
    why: rm deletes or overwrites data
    y allow once · a allow similar Bash requests · n deny · /cancel stop
```

Press **y**, **a** (allow this and similar requests — same tool and program — for the rest of the
session) or **n** while the input line is empty, or type `/approve`, `/allow-similar`, `/deny
[reason]`. The status bar shows the request and the AI panel opens even if it was hidden. Nothing else
can be typed to the agent until you answer. A denial is final for that turn; the agent is told not to
work around it. Time spent waiting for you does not count against `ai.agent.timeout_secs`.

## What the panel shows

```text
▍You
  /agent Add int mul(int a, int b) to util.h/util.c and print mul(4, 5) …
▍Agent · Claude Code · workspace-write
  claude-opus-5-5 · session 78649bf1
  Read util.h
  Edit util.h
  ✎ util.h (edit)
  $ gcc -Wall -Werror -o app main.c util.c && ./app
    │ 5
    │ 20
    ✓ ok
  ⊘ Bash: The user refused this action
  I added mul() … (the agent's summary)
  ■ done · 4 file(s) changed · 3 command(s) · 1 approval(s) · 1 refused · 21.4 s · tokens 61236 in, 1102 out
  ✎ main.c (modified)
  ✎ util.c (modified)
  …
  · /review shows the diffs · /revert <file>|all undoes · open buffers without unsaved edits were reloaded
```

The title shows `⠋ agent 12s · step 5 · workspace-write` (or `⚠ approval needed`); a running command
has a spinner. Commands show the last lines of their output (more when they fail) and their exit
status. The final line counts files changed **on disk** (from the snapshot, not only what the CLI
reported), commands and failures, approvals, refusals, time and tokens as reported by the CLI.

## Commands

| Command | Action |
|---|---|
| `/agent <task>` | Run a task (the active editor file and line are mentioned to the agent; `/file` attachments are included) |
| `/agent` | Agent status: backend, policy, mode, session, active skills |
| `/mode agent` / `/mode chat` | What plain text does |
| `/policy [read-only\|workspace-write\|approval-required]` | Show / set the policy |
| `y` / `a` / `n` | Answer an approval |
| `/changes` | Files the last task changed |
| `/review [file]` | Unified diffs of those changes |
| `/revert <file>` / `/revert all` | Restore a file (or delete one the agent created); refused if you changed the file afterwards |
| `/fix [command]` | Run the build (or the command, no shell), give the errors to the agent, rebuild — up to `ai.agent.max_fix_iterations` rounds |
| `/new` | Forget the CLI session; the next task starts without earlier context |
| `/skills`, `/skill <name>`, `/skill off <name>`, `/skill clear` | [Skills](skills.md) |
| `/cancel` | Stop the task and every process it started |

`/fix` without a command uses `ai.agent.fix_command`, else `embed build .`.

## Sessions and context

The first task starts a CLI session (`claude --session-id <uuid>`; Codex reports a thread id); later
tasks in the same panel continue it (`claude --resume <id>`, `codex exec resume <id>`), so the agent
remembers earlier work. `/new` starts over; switching backend or project starts a new session.
`ai.agent.persist_sessions = false` runs every turn without saved sessions
(`--no-session-persistence`, `--ephemeral`) and therefore without shared context. `embed ai agent
--resume <id>` continues a session from a shell.

## Review, revert and the editor

Before each task EmbedIDE snapshots the workspace's text files (generated directories such as `.git`,
`target`, `build`, `node_modules`, `zig-out` are skipped; files over 1 MiB are tracked by size and
time only). Afterwards it compares, whatever tool or command made the change. Limits:
`ai.agent.snapshot_max_bytes` (64 MiB) and `snapshot_max_files` (20 000); beyond them files are tracked
by metadata only and the panel says the review is partial.

Open buffers whose files changed are reloaded when they have no unsaved edits; a buffer with unsaved
edits is never replaced (the status bar names it). Saving such a buffer later is protected by the
editor's usual check against files changed on disk.

## Cancellation and limits

`/cancel` (or F8, or quitting the IDE) stops the CLI's whole process group: SIGTERM, then SIGKILL after
0.5 s, including compilers or programs the agent started. A turn longer than `ai.agent.timeout_secs`
(1800 s) is stopped the same way. Output lines from the CLI are bounded (64 MiB per line).

## Security notes

- The CLI's credentials are never read, copied or shown; API-key variables are removed from its
  environment unless `ai.cli.allow_api_key = true`. If Claude Code nevertheless reports an API-key
  source (`apiKeySource` in its start event), the task stops before anything is sent.
- The agent works with your user's permissions inside the CLI's own sandbox (Codex) or under the rules
  above (Claude Code). Under `workspace-write`, builds and tests run without asking, and building means
  running code from the project (Makefiles, build scripts, tests). Use `approval-required` or
  `read-only` for projects you do not trust.
- Skill text and project files are guidance for the model, never permissions.
- Hardware is never touched without your approval (Claude Code); Codex's sandbox blocks device access
  only as far as its sandbox does — flash from the IDE yourself.

## Configuration

```toml
[ai.agent]
policy = "workspace-write"      # read-only | workspace-write | approval-required
mode = "chat"                   # what plain text does in the panel: chat | agent
timeout_secs = 1800             # per turn, not counting time waiting for approvals
snapshot_max_bytes = 67108864
snapshot_max_files = 20000
max_fix_iterations = 3
fix_command = "make test"       # default: `embed build .`
allow_commands = ["just"]       # extra command names allowed without asking (Claude Code)
persist_sessions = true
```

## Examples

### 1. Codex implements a C function (`codex-cli`, `workspace-write`)

`main.c` calls `add()`, which does not exist yet. Real run on 2026-11-12 (Codex CLI 0.163.0, ChatGPT
login), abridged:

```text
$ embed ai agent --provider codex-cli "Implement add() in a new file add.c so main.c links. Build with:
  gcc -Wall -o app main.c add.c, run ./app and report the output."
Agent: Codex CLI · policy workspace-write · workspace /tmp/…/codex-c
· started · session 01a1cc60-…
$ /usr/bin/bash -lc "pwd && rg --files -g 'AGENTS.md' -g 'main.c' -g 'add.c' -g '*.h'"
  └ ok
✎ /tmp/…/codex-c/add.c (add)
$ /usr/bin/bash -lc 'gcc -Wall -o app main.c add.c && ./app'
  │ 5
  └ ok
Created add.c implementing `add(int a, int b)`.
- `gcc -Wall -o app main.c add.c` — succeeded without warnings.
- `./app` — output: `5`
added add.c
added app
```

In the IDE the same task is `/provider codex-cli` then `/agent Implement add() …`; the panel shows
the commands without Codex's `bash -lc` wrapper.

### 2. Claude Code edits several files and runs GCC (`claude-code`, `workspace-write`)

Real run on 2026-11-12 (Claude Code 2.1.322, claude.ai login), without a terminal for approvals,
abridged:

```text
$ embed ai agent --provider claude-code "Add int mul(int a, int b): declare it in util.h, implement it
  in util.c, and make main.c also print mul(4, 5). Build with: gcc -Wall -Werror -o app main.c util.c
  and run ./app. Finally try to delete the app binary with rm app."
· Read …/util.h   · Read …/util.c   · Read …/main.c
✎ …/util.h (edit)
✎ …/util.c (edit)
✎ …/main.c (edit)
$ gcc -Wall -Werror -o app main.c util.c && ./app
  │ 5
  │ 20
  └ ok
$ rm app
⚠ Approval needed: $ rm app
  why: rm deletes or overwrites data
  refused (no terminal to ask on)
… Deleting the binary didn't happen: EmbedIDE refused `rm app` …
modified main.c
modified util.c
modified util.h
```

The three edits and the build ran without questions; only the deletion needed approval. (Its first
command, a `for` loop over the sources, was asked about too in that run; shell loops are now
recognised.)

## Verified

- Mock CLIs (`crates/embedide-ai/tests/agent.rs`, `tests/fixtures/mock_agent.py`, the AI panel unit
  tests and `scripts/check-embed-pty.py`): the Claude Code host protocol (initialize, `can_use_tool`,
  allow/deny responses), all three policies for both CLIs, approvals answered with one key in the real
  binary, the Codex plan → approval → resume flow, session resume arguments, API-key source refusal,
  `/cancel` killing the CLI and a grandchild process, the fix loop for both CLIs, review/revert, reload
  of an open buffer, skills reaching the agent quoted as untrusted text.
- Real CLIs (2026-11-12, the user's subscriptions, a handful of short tasks): examples 1 and 2 above;
  Claude Code with a custom skill under `read-only`; the Claude Code `can_use_tool` protocol and the
  Codex `--json` event format were first inspected with a minimal probe in a disposable directory.
- Not verified with real CLIs: `approval-required` (mock only), `/fix` and session resume (mock only),
  Windows and macOS.

## Limitations

- Codex CLI: no per-command approval in `exec` mode (see above); commands are reported when they
  finish, file changes after they are applied; no streamed text (whole messages).
- Claude Code: decisions are per tool call; "allow similar" matches tool and program name only.
- The command rules are heuristics over a parsed command line; they ask rather than allow when unsure,
  but they cannot see what a build script or program does once allowed.
- The snapshot covers text files up to the limits; changes to skipped generated directories are not
  listed.
- One agent task at a time per IDE window.
