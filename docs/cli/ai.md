# AI subsystem

Crate `embedide-ai`. Provider ≠ model ≠ authentication method ≠ IDE feature: you can use different
providers and models for chat, completion and edits.

| Feature | Touches files? | Backends | Docs |
|---|---|---|---|
| Chat, explain, `/edit` proposals | no (a proposal goes into a buffer only on `/apply`) | all | below |
| Inline suggestions (ghost text) | no (inserted into the buffer only on Tab) | all | [Inline suggestions](#inline-suggestions) |
| **Agent mode** (`/agent`, `embed ai agent`) | **yes**, in the workspace, under a policy | `claude-code`, `codex-cli` | [agents.md](agents.md) |
| Skills | no (instructions for agent tasks) | agent mode | [skills.md](skills.md) |

## Providers

| Provider | Models | API protocol (default base URL) | Key variable | Subscription mode |
|---|---|---|---|---|
| `anthropic` | Claude | Messages API (`https://api.anthropic.com`, `x-api-key`, `anthropic-version: 2023-06-01`) | `ANTHROPIC_API_KEY` | backend `claude-code` (Claude Code CLI) |
| `openai` | GPT / Codex | Chat Completions (`https://api.openai.com/v1`, `max_completion_tokens`) | `OPENAI_API_KEY` | backend `codex-cli` (Codex CLI) |
| `moonshot` | Kimi | Chat Completions (`https://api.moonshot.ai/v1`) | `MOONSHOT_API_KEY` | not integrated |
| `xai` | Grok | Chat Completions (`https://api.x.ai/v1`) | `XAI_API_KEY` | not integrated |

Kimi Code CLI exists, but its non-interactive interface has not been verified for EmbedIDE; no
verified official subscription CLI interface was found for xAI. Both use API keys.

## Claude Code and Codex CLI backends (`claude-code`, `codex-cli`)

If Claude Code or the OpenAI Codex CLI is installed and logged in on your machine, EmbedIDE can use it
directly: **no API key, no second login**. Select it in the AI panel (`/provider claude-code`,
`/provider codex-cli`), with `ai.provider = "claude-code"` in `config.toml`, or with
`embed ai chat --provider claude-code …`. `/providers` lists every backend; `/status` checks the
selected CLI.

**How it works (chat, explain, edit proposals, suggestions).** EmbedIDE runs the installed CLI as a
child process for each request, with the prompt on stdin, in a fresh empty temporary directory, in its
own process group, without tools. Agent mode runs the CLI differently — in the project, with tools,
under a policy — and is described in [agents.md](agents.md).

| Backend | Command (flags are used only when the installed CLI's `--help` lists them) | Tools the model gets |
|---|---|---|
| `claude-code` | `claude -p --output-format stream-json --verbose --include-partial-messages --tools "" --no-session-persistence` | **none** (`--tools ""`): no file access, no commands |
| `codex-cli` | `codex exec --skip-git-repo-check --sandbox read-only --json --ephemeral --color never -C <empty dir> -` | Codex's shell in its **read-only sandbox**: no writes, no network; it can read files your user can read |

Answers stream into the panel (Claude Code token by token, Codex message by message). `/cancel`,
closing the IDE or the time limit stop the CLI **and every process it started**. Output is bounded.
Sessions are not saved in the CLIs' history (`--no-session-persistence`, `--ephemeral`). An older CLI
without the structured mode is used in plain-text mode (`claude -p --output-format text`, `codex exec
--output-last-message`).

**Authentication stays with the CLI.** EmbedIDE never reads, copies or stores the CLIs' tokens,
cookies or configuration, never talks to their login endpoints and never impersonates them. Login
state comes only from `claude auth status --json` and `codex login status`; the panel shows the method
and plan (for example `claude.ai (pro)`, `ChatGPT`) and nothing personal. If a CLI is not logged in,
the message says what to run: `claude` (or `claude auth login`) or `codex login`, in a terminal.

**Subscription versus API billing.** Requests through `claude-code` / `codex-cli` count against the
plan or credentials **that CLI is logged in with** and its usage limits (EmbedIDE does not bypass or
pool them). A ChatGPT or Claude subscription does not include API credits: the `anthropic` and `openai`
backends always need an API key and are billed per request. To prevent a silent switch to API billing,
EmbedIDE removes `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY` and `CODEX_API_KEY` from
the CLI's environment unless you set `ai.cli.allow_api_key = true`. It also removes the variables that
attach a process to a running Claude Code session (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, …), so `embed`
started inside Claude Code runs an independent, non-interactive CLI instead of joining that session.

**Errors** are classified: not installed (with the setting to point at it), unsupported (no
non-interactive mode in `--help`), not logged in / expired (with the login command), rate or usage
limit, CLI failure (exit code and the CLI's message, credentials redacted), timeout.

Configuration (`config.toml`):

```toml
[ai]
provider = "claude-code"              # or "codex-cli", "anthropic", "openai", …
[ai.providers.claude-code]            # optional: a CLI outside PATH, a model the CLI accepts
cli = "/opt/claude/bin/claude"
model = "sonnet"
[ai.providers.codex-cli]
cli = "/usr/local/bin/codex"
[ai.cli]
timeout_secs = 600                    # per request
max_output_bytes = 8388608
allow_api_key = false
```

`ai.providers.anthropic.cli` / `ai.providers.openai.cli` with `auth = "subscription"` keep working.

## Models

EmbedIDE does not invent model names. Pick one per provider or per task:

```sh
embed ai models --provider anthropic                     # one request with your key
embed config set ai.providers.anthropic.model <model-id>
embed config set ai.models.complete "openai:<model-id>"   # task-specific provider:model
embed config set ai.provider anthropic                    # default provider
```

## Credentials

Order: the provider's environment variable, then `credentials.toml` in the configuration directory
(written by `embed ai login <provider>` with mode 0600; refused if readable by others). Keys are
never stored in `config.toml` or `embedide.toml`, never printed, and removed from provider error
messages. The key is read from stdin, which is visible when typed. To avoid that, pipe it:
`printf %s "$KEY" | embed ai login openai`.

## Features

| Command | Sends | Writes files |
|---|---|---|
| `ai chat [MESSAGE] [--file F]…` | your message and the files you attach | no |
| `ai complete --file F --line L --column C [--related F]…` | up to 6000 chars before / 2000 after the cursor, related files | no |
| `ai edit FILE --instruction "…"` | that file | only with `--apply`, after confirmation (`--yes` for scripts), and only if the file did not change meanwhile |

Common options: `--provider`, `--model`, `--auth api-key|subscription`, `--max-tokens`, `--dry-run`.

- **Nothing is sent without an explicit command.** Before sending, stderr shows the provider,
  model, byte count and the files included or refused.
- `--dry-run` prints the exact request without connecting and without needing a key.
- `EMBEDIDE_AI_OFFLINE=1` refuses every network request.
- Context: only files you name. The project's `[ai] exclude` patterns and built-in secret
  patterns (`.env`, `*.pem`, `*.key`, `id_rsa*`, `credentials*`, `.git/`, `secrets/`, …) are
  always applied; binary files are refused; `ai.max_context_bytes` (32 KiB default) caps the total.
- `ai.max_tokens` (1024 default) caps output tokens per request.
- No automatic retries: a retry can be billed twice. HTTP 429 is reported with the provider's
  `Retry-After`.
- Plain HTTP is accepted only for localhost (mock servers, local proxies).

## In the `embed` IDE

The AI panel (Alt+2) uses the same configuration, credentials and safeguards. Nothing is sent
until you press Enter.

| Command | Action |
|---|---|
| `/providers` | All backends: API providers (key present or not) and the installed CLIs with version and login state |
| `/provider <id>` | `claude-code`, `codex-cli`, `anthropic`, `openai`, `moonshot`, `xai` |
| `/status` | Backend, model and state; checks the selected CLI |
| `/file` / `/file a.c b.h` | Attach the current buffer / named project files (exclusion rules and secret patterns apply) |
| `/selection` (`/sel`) | Attach the Visual selection |
| `/explain [question]` | Explain the selection or file |
| `/edit <instruction>` | Ask for a change: the reply becomes a **diff to review**; nothing is changed yet |
| `/diff` | Show the pending diff again |
| `/apply` / `/reject` | Put the change into the editor buffer (undo with `u`; nothing is written until `:w`) / discard it. If the buffer changed since the request, `/apply` refuses |
| `/history` | Recent conversation |
| `/cancel`, `/clear`, `/model <id>`, `/auth <mode>`, `/complete`, `/help` | as before |

Chat output never runs commands and never writes files: its only path to your files is `/apply` into
a buffer you then save yourself. Agent mode is the explicit exception — `/agent <task>` (or `/mode
agent`), `/policy`, approvals with `y`/`a`/`n`, `/changes`, `/review`, `/revert`, `/fix`, `/new`,
`/skills`, `/skill` — see [agents.md](agents.md). `/inline [off|manual|auto]` shows or switches inline
suggestions.

## Inline suggestions

Ghost text from the AI at the editor cursor, separate from the LSP completion popup:

| Key / setting | Action |
|---|---|
| **`Alt+\`** (command `ai_suggest`, rebindable via `native.keys`) | Ask now, in Insert mode |
| `:complete`, `/complete` | Ask now (also from the panel) |
| **Tab** | Accept the whole suggestion (one undoable edit) — only while a suggestion is shown at the cursor; otherwise Tab indents as always |
| **Esc** | Dismiss the suggestion or cancel the running request |
| typing | Typing the start of the suggestion keeps the rest (no new request); anything else dismisses it |
| Ctrl+Space | Unchanged: the LSP / word completion popup (Tab there accepts the popup item) |

Multi-line suggestions are drawn as dimmed virtual rows under the cursor line (the code below moves
down while it is shown), re-indented to the buffer's tabs or spaces and cut to `max_lines`. Nothing is
written to the buffer or disk before Tab.

Modes (`ai.completion.mode`): `manual` (default) — only on request; `auto` — a debounced request after
you stop typing, only at the end of a line or before closing brackets/quotes, never while another panel
has focus; `off`. The older `ai.inline = true` means `auto`. Auth, configuration and rate-limit errors
pause automatic suggestions (the status bar says why; `Alt+\` still asks); three failures in a row pause
them for a minute.

A request sends the file name, language, the project line (`name (language)`), declarations elsewhere
in the file (function/type heads, labels; at most 40 lines), and `prefix_chars` before / `suffix_chars`
after the cursor. One request runs at a time; a newer one cancels it; answers for a moved cursor are
dropped; the same context is answered from a cache (`cache_entries`); a request is cancelled after
`timeout_secs`.

**Latency.** Measured on 2026-11-12 with `embed ai complete` on a three-line C function: Codex CLI
9.3 s and 10.8 s, Claude Code 6.8 s and 10.7 s per suggestion (each request starts the CLI; neither CLI
offers a supported way for EmbedIDE to keep a completion session warm without growing its context).
That suits `manual` mode; for `auto` an API backend (`ai.completion.provider = "openai"` with a fast
model) is the practical choice. API backends were measured only against a local mock server.

## Configuration reference

All AI settings live in `config.toml` (`embed config set ai.… …`); keys are never stored there.

| Key | Default | Meaning |
|---|---|---|
| `ai.provider` | none | `claude-code`, `codex-cli`, `anthropic`, `openai`, `moonshot`, `xai` |
| `ai.models.<task>` | — | `chat` / `complete` / `edit` → `provider:model` or `model` |
| `ai.providers.<id>.model` / `.auth` / `.base_url` / `.cli` | — | per provider; `.cli` is the path of the official CLI |
| `ai.max_tokens` | 1024 | output cap per request |
| `ai.max_context_bytes` | 32768 | context attached to one chat/edit request |
| `ai.completion_debounce_ms` | 250 | debounce of automatic suggestions |
| `ai.inline` | false | old switch: `true` = `ai.completion.mode = "auto"` |
| `ai.cli.timeout_secs` | 600 | one CLI chat/edit request |
| `ai.cli.max_output_bytes` | 8 MiB | output accepted from one CLI request |
| `ai.cli.allow_api_key` | false | pass API-key variables to the CLIs (otherwise removed) |
| `ai.completion.mode` | `manual` | `off`, `manual`, `auto` |
| `ai.completion.provider` / `.model` | chat backend | backend and model for suggestions only |
| `ai.completion.timeout_secs` | 60 | one suggestion |
| `ai.completion.prefix_chars` / `.suffix_chars` | 4000 / 1500 | context around the cursor |
| `ai.completion.cache_entries` | 64 | suggestions remembered by context |
| `ai.completion.max_lines` | 40 | longest suggestion kept |
| `ai.agent.policy` | `workspace-write` | `read-only`, `workspace-write`, `approval-required` |
| `ai.agent.mode` | `chat` | what plain text does in the panel: `chat` or `agent` |
| `ai.agent.timeout_secs` | 1800 | one agent turn (approval waits excluded) |
| `ai.agent.snapshot_max_bytes` / `.snapshot_max_files` | 64 MiB / 20000 | review snapshot limits |
| `ai.agent.max_fix_iterations` | 3 | `/fix` rounds |
| `ai.agent.fix_command` | `embed build .` | `/fix` check command (no shell) |
| `ai.agent.allow_commands` | [] | extra command names allowed under `workspace-write` (Claude Code) |
| `ai.agent.persist_sessions` | true | continue CLI sessions between tasks |
| `ai.skills.enabled` | true | use skills |
| `ai.skills.dirs` | [] | extra skill directories |
| `ai.skills.max_bytes` | 65536 | largest SKILL.md |

Unknown keys and invalid values (`policy = "yolo"`, `mode = "always"`) are rejected when the file is
loaded or saved with `embed config set`.

## For the Qt editor

`embedide_ai::complete::Engine` is the inline-completion service: `submit()` on every edit,
`submit_now()` for an explicit request, `poll()` for results. It debounces
(`ai.completion_debounce_ms`, 250 ms by default), keeps one request in flight, cancels superseded
requests, answers repeated contexts from its cache, enforces a timeout and never returns stale
results. `embedide_ai::agent::Session` runs agent turns behind a `Host` trait (events and approvals),
`embedide_ai::skills` discovers and loads skills.
`embedide_ai::backend()` builds a provider backend from configuration; `api_backend()` takes an
explicit base URL.

## What was verified

The Anthropic and Chat Completions wire formats, streaming, model listing, error mapping and
redaction, completion, edit and the CLI commands were tested only against a local mock HTTP
server; no request was sent to a paid API.

The CLI backends were tested with a mock CLI (streaming, structured and plain modes, login and limit
errors, crashes, broken pipes, oversized output, ANSI/Unicode, timeouts, cancellation of the process
group, concurrent requests, edit/diff/apply, quitting during a request) and, on 2026-11-11, with the
real installed **Claude Code 2.1.322** (logged in via claude.ai, Pro) and **Codex CLI 0.163.0** (logged
in via ChatGPT) through the release binary in a pseudo-terminal: status detection, an attached file, a
real answer from each (`PONG` in about 5 s from Claude Code and 6 s from Codex), and `/cancel` of a
running Codex request, after which the Node launcher and the native `codex` process (tracked as
descendants of `embed`) were gone within about 0.3 s. These real requests used the user's subscription
(a few short prompts). No manual test in a terminal emulator was made.

On 2026-11-12 the same CLIs were used for agent tasks (see [agents.md](agents.md#verified)), a skill
check, and four inline suggestions for the latency figures above.
