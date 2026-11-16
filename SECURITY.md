# Security policy

EmbedIDE runs compilers, shells and AI agents on your machine, so security reports matter.

## Supported versions

| Version | Supported |
|---|---|
| 0.1.0-alpha.1 (latest pre-release, updated 2026-11-16) | yes |
| older development builds | no |

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting
("Security" → "Report a vulnerability") in
[Kirpich-Space/EmbedIDE](https://github.com/Kirpich-Space/EmbedIDE/security/advisories/new).

Include the version (`embed --version`), the platform, steps to reproduce and the impact. You will get
an acknowledgement; fixes are released as a new pre-release with a CHANGELOG entry.

## Scope

In scope: escaping the workspace through agent mode or its approval rules, credential or API-key
leaks (logs, prompts, child processes), terminal escape-sequence injection from files or program
output, unsafe file writes (lost data, writes outside the project), and actions on hardware without an
explicit user command, and toolchain downloads (anything that makes EmbedIDE fetch or unpack data that is not
pinned by the user's own SHA-256, or write outside its install directory). Bugs in Claude Code, Codex CLI or other third-party tools should go to their
vendors.
