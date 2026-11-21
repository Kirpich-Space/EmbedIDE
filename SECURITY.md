# Security policy

EmbedIDE runs compilers, shells and AI agents on your machine, so security reports matter.

## Supported versions

| Version | Supported |
|---|---|
| 0.1.0-alpha.1 (latest pre-release, updated 2026-11-22) | yes |
| older development builds | no |

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting
("Security" → "Report a vulnerability") in
[Kirpich-Space/EmbedIDE](https://github.com/Kirpich-Space/EmbedIDE/security/advisories/new).

Include the version (`embed --version`), the platform, steps to reproduce and the impact. You will get
an acknowledgement; fixes are released as a new pre-release with a CHANGELOG entry.

## Trust boundary

Opening a project never runs its scripts. **Building** it does run what the project describes: Make, CMake and Cargo
projects execute their own build files, and a native `embedide.toml` chooses compiler flags. EmbedIDE refuses
loader and search-path variables (`LD_*`, `PATH`, …) in a manifest and warns about flags that load project code, but it
cannot make a hostile project safe to build. Build projects you trust, or use a throw-away user or container.

## Scope

In scope: escaping the workspace through agent mode or its approval rules, credential or API-key
leaks (logs, prompts, child processes), terminal escape-sequence injection from files or program
output, unsafe file writes (lost data, writes outside the project), and actions on hardware without an
explicit user command, and toolchain downloads (anything that makes EmbedIDE fetch or unpack data that is not
pinned by the user's own SHA-256, or write outside its install directory). Bugs in Claude Code, Codex CLI or other third-party tools should go to their
vendors.
