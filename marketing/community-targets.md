# Community targets: where, who, how

Rules of engagement for all of them: post as the author, once, in the right place; read the current
rules the day you post; reply to comments; never ask for stars or upvotes; no cross-posting the same text;
no automation. Verify every link/rule before use — communities change their rules.

## Alpha testers and users

| Audience | Where | Angle | Link |
|---|---|---|---|
| Embedded developers | r/embedded; Rust Embedded Matrix (#rust-embedded:matrix.org); embedded forums you already read | the arm-none-eabi loop + ELF/disassembly view; need hardware testers | https://www.reddit.com/r/embedded/ |
| Rust developers | r/rust; users.rust-lang.org (Announcements); Rust Discord/Matrix show-and-tell channels; This Week in Rust | architecture (editor without I/O, no-unsafe, PTY-driven tests) | https://www.reddit.com/r/rust/ · https://users.rust-lang.org/c/announcements |
| FreeBSD users | r/freebsd; forums.freebsd.org (userland/ports section); FreeBSD Mastodon tags | FreeBSD testers wanted: compiles, never run | https://www.reddit.com/r/freebsd/ · https://forums.freebsd.org/ |
| Linux / terminal users | r/commandline; Mastodon #TUI #CLI; lobste.rs (needs an invite) | keyboard-first IDE, one binary | https://www.reddit.com/r/commandline/ |
| Systems / low-level | Hacker News (Show HN); lobste.rs tags `rust`, `devops`; Hackaday.io project page | ELF/HEX/disassembly + build engine | https://news.ycombinator.com/showhn.html |
| Open-source community | r/opensource; Mastodon #OpenSource | GPL-3.0, contributors wanted | https://www.reddit.com/r/opensource/ |
| AI-agent users | r/ClaudeAI, r/ChatGPTCoding (check rules); official Claude Code / Codex community channels | permission layer, diff review, no API keys | — |

## Directories and Awesome lists (submit by pull request, one each, after reading CONTRIBUTING of the list)

Each list has inclusion criteria (often a minimum age, stars or maturity); check before submitting and
skip a list that you do not meet rather than arguing. Suggested lists to check:

- `rothgar/awesome-tuis` — TUI applications (section: development).
- `agarrharr/awesome-cli-apps` — command-line apps.
- `rust-unofficial/awesome-rust` — applications; has historically required a minimum of stars/activity.
- `rust-embedded/awesome-embedded-rust` — mostly crates/tools for embedded Rust targets; fits only if you
  can argue relevance (EmbedIDE builds Cortex-M firmware in C/Rust).
- A FreeBSD "awesome"/software lists, and the FreeBSD ports tree later (see below).
- Terminal-tools newsletters accept tips by email/form (e.g. Terminal Trove, console.dev) — one polite
  submission each, via their own form.

Entry text (one line, fit to the list's format):
`[EmbedIDE](https://github.com/Kirpich-Space/EmbedIDE) - Terminal IDE in Rust for embedded and systems development: Vim-style editor, builds, ELF/HEX/disassembly view, optional Claude Code/Codex agents.`

## Contributors

- GitHub itself: issues labelled `good first issue` / `help wanted` (see `contributor-outreach.md`).
- Rust Users Forum "Call for participation" threads and TWiR's "Call for Participation" section.
- Distro/port maintainers: an Arch AUR package and a FreeBSD port are natural contribution targets.

## Channels you own

- GitHub Discussions (not enabled yet — enable under Settings → Features to give users a place for
  questions that are not bugs).
- GitHub Releases "Watch → Custom → Releases" for followers; release notes in `CHANGELOG.md`.
- A short devlog post (dev.to or your blog) per release.

## What to avoid

Star-for-star exchanges, "100 stars" campaigns, paid promotion, posting in unrelated communities,
reposting the same announcement, DM-ing maintainers/influencers asking for a share, and claiming support
for platforms that have not been run.
