# AI subsystem

Crate `embedide-ai`. Provider ≠ model ≠ authentication method ≠ IDE feature: you can use different
providers and models for chat, completion and edits.

## Providers

| Provider | Models | API protocol (default base URL) | Key variable | Subscription mode |
|---|---|---|---|---|
| `anthropic` | Claude | Messages API (`https://api.anthropic.com`, `x-api-key`, `anthropic-version: 2023-06-01`) | `ANTHROPIC_API_KEY` | Claude Code CLI, `claude -p --output-format text` |
| `openai` | GPT / Codex | Chat Completions (`https://api.openai.com/v1`, `max_completion_tokens`) | `OPENAI_API_KEY` | Codex CLI, `codex exec --sandbox read-only` |
| `moonshot` | Kimi | Chat Completions (`https://api.moonshot.ai/v1`) | `MOONSHOT_API_KEY` | not integrated |
| `xai` | Grok | Chat Completions (`https://api.x.ai/v1`) | `XAI_API_KEY` | not integrated |

Subscription mode runs only the provider's **official CLI**, logged in with its own login. EmbedIDE
never reads browser cookies, session tokens or private credentials, and does not claim that a
ChatGPT/Claude subscription grants API access. Kimi Code CLI exists, but its non-interactive
interface has not been verified for EmbedIDE; no verified official subscription CLI interface was found
for xAI. Both use API keys. Subscription CLIs run in an empty temporary directory and receive the
prompt on stdin, so they cannot read your project by themselves. They do not report token usage.

## Models

EmbedIDE does not invent model names. Pick one per provider or per task:

```sh
embedide ai models --provider anthropic                     # one request with your key
embedide config set ai.providers.anthropic.model <model-id>
embedide config set ai.models.complete "openai:<model-id>"   # task-specific provider:model
embedide config set ai.provider anthropic                    # default provider
```

## Credentials

Order: the provider's environment variable, then `credentials.toml` in the configuration directory
(written by `embedide ai login <provider>` with mode 0600; refused if readable by others). Keys are
never stored in `config.toml` or `embedide.toml`, never printed, and removed from provider error
messages. The key is read from stdin, which is visible when typed. To avoid that, pipe it:
`printf %s "$KEY" | embedide ai login openai`.

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

## For the Qt editor

`embedide_ai::complete::Engine` is the inline-completion service: `submit()` on every edit,
`poll()` for results. It debounces (`ai.completion_debounce_ms`, 250 ms by default), keeps one
request in flight, cancels superseded requests and never returns stale results.
`embedide_ai::backend()` builds a provider backend from configuration; `api_backend()` takes an
explicit base URL.

## What was verified

The Anthropic and Chat Completions wire formats, streaming, model listing, error mapping and
redaction, completion, edit and the CLI commands were tested only against a local mock HTTP
server. The subscription path was tested with a fake `claude` executable. **No request was sent to
a real provider or real subscription CLI.**
