# Skills

EmbedIDE's own skills are built into the binary and given to agent tasks automatically when they fit
the project; their sources are in [`crates/embedide-ai/skills`](../../crates/embedide-ai/skills):

| Skill | Given when |
|---|---|
| `embedide-agent` | always (how to work inside EmbedIDE) |
| `c-cpp` | C/C++ sources |
| `c-firmware` | C sources in a firmware project |
| `embedded-bringup` | firmware markers (linker scripts, startup files, cross toolchains, board names) |
| `rust-crate` | `Cargo.toml` |
| `zig-build` | `build.zig` |
| `asm-review` | assembly sources |

To change one for a project, put a skill with the same name in `.embedide/skills/<name>/SKILL.md`
(or `~/.config/embedide/skills/` for all projects); it replaces the built-in one. `embed ai skills`
lists every skill, `embed ai skills <name>` prints one.

Your own skills use the same layout — a directory with `SKILL.md` (frontmatter with `name` and
`description`, then the instructions) — and are activated with `/skill <name>` in the AI panel or
`embed ai agent --skill <name>`. They are quoted to the agent as untrusted guidance and cannot change
the policy or permissions. See [docs/cli/skills.md](../../docs/cli/skills.md).
