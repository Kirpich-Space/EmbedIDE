# EmbedIDE documentation

`embed` is a terminal IDE for embedded firmware, drivers, systems code and assembly, with optional AI
agents. Start with the README in the repository root, then pick a guide.

## Using EmbedIDE

| Guide | Read it when you want to… |
|---|---|
| [Getting started](cli/getting-started.md) | install `embed`, open a first project, learn the first keys |
| [Tutorials](cli/tutorials.md) | follow complete walkthroughs: C, Rust, Zig, assembly, Cortex-M firmware, AI agent, skills |
| [Terminal IDE guide](cli/tui.md) | understand panels, editing, completion, assembly, HEX/ELF view, AI panel |
| [Key and command reference](cli/keys.md) | look up a key, a `:` command, a panel command or a command ID |
| [Configuration reference](cli/configuration.md) | change settings in `config.toml`, `embedide.json` or `embedide.toml` |
| [Command-line reference](cli/commands.md) | script `embed build`, `embed toolchain`, `embed config`, … |
| [`embedide.toml`](cli/embedide-toml.md) | describe a project: sources, profiles, toolchains, target |
| [Troubleshooting and FAQ](cli/troubleshooting.md) | fix a problem or find out what is (not) supported |

## AI

| Guide | Topic |
|---|---|
| [AI subsystem](cli/ai.md) | providers, Claude Code / Codex CLI backends, billing and credentials, inline suggestions, config |
| [Agent mode](cli/agents.md) | agents that edit files and run builds: policies, approvals, review, revert, cancellation |
| [Skills](cli/skills.md) | built-in and custom `SKILL.md` instructions |

## Project

| Document | Topic |
|---|---|
| [Verified and unverified functionality](cli/status.md) | what was tested, with which tools, and what was not |
| [Architecture](cli/architecture.md) | crates, data flow, process and file safety rules |
| [Contributing](../CONTRIBUTING.md) | build, test, style, commits |
| [Security policy](../SECURITY.md) | reporting vulnerabilities |
| [Changelog](../CHANGELOG.md) | what changed in each version |
| [`native/`](native/) | historical design notes from the migration to Rust (in Russian) |
