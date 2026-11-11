# Example skills

Skills are reusable instructions for EmbedIDE's agent mode. Each one is a
directory with a `SKILL.md` file (frontmatter with `name` and `description`,
then the instructions), the same layout as Agent Skills.

| Skill | For |
|---|---|
| `c-firmware` | bare-metal / RTOS C |
| `rust-crate` | Rust crates and workspaces |
| `zig-build` | Zig with build.zig |
| `asm-review` | x86-64, AArch64 and RISC-V assembly |
| `embedded-bringup` | clocks, pins, peripherals, flashing |

Install one for every project (global) or for one project:

```sh
mkdir -p ~/.config/embedide/skills && cp -r examples/skills/c-firmware ~/.config/embedide/skills/
mkdir -p .embedide/skills && cp -r examples/skills/c-firmware .embedide/skills/
```

Then, in the AI panel: `/skills` lists them, `/skill c-firmware` activates one
for the next agent tasks, `/skill off c-firmware` removes it. From a shell:
`embed ai skills` and `embed ai agent --skill c-firmware "…"`.

Skill text is guidance only. It is quoted into the agent's task as untrusted
content and cannot change the policy, permissions or configuration;
`allowed-tools` in frontmatter is ignored. See `docs/cli/skills.md`.
