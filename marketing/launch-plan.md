# Launch plan — EmbedIDE 0.1.0-alpha.1

Goal: find the first genuinely interested developers and alpha testers, and turn their feedback into
fixes. Not a traffic goal. Nothing here is published until the owner approves the exact text and
destination.

## Stage 1 — Preparation (done / in this repository)

- [x] Repository presentation: README with logo, positioning, real screenshots, platform statement,
  roadmap, contributing section; GitHub topics and description.
- [x] Branding: only the approved identity in `Desige/` (Carbon/Bone/rust). Social-preview concept in
  `product-description.md`.
- [x] Documentation: tutorials, key/config references, troubleshooting/FAQ, architecture, CONTRIBUTING.
- [x] Install path for testers: `install.sh` (static build, checksum-verified), issue templates.
- [ ] **Demo video** (owner): 60–90 s screen recording, link in README and posts. Not available yet.
- [ ] Enable GitHub Discussions; create the "good first issue" list (`contributor-outreach.md`).
- [ ] Upload the social preview image (browser only).
- [ ] Owner reads and approves each post in this directory.

## Stage 2 — Initial visibility (week 1–2, one channel at a time)

Order and spacing (never the same day):

1. **Day 0 — r/rust** (architecture angle), plus the dev.to article. Stay online for replies.
2. **Day 2–3 — r/embedded** (hardware testers wanted), r/commandline.
3. **Day 4–5 — Show HN** on a weekday morning US time (about 13:00–16:00 UTC). One shot; prepare the first
   comment; reply to everything for the first hours.
4. **Day 6+ — r/FreeBSD** (testers), Mastodon/X/LinkedIn posts, r/opensource.
5. **Week 2 — Awesome-list pull requests** where criteria are met; Rust Users Forum announcement.

If a post gets no traction, do not repost; improve the README/screenshots and try a different angle next
release instead.

## Stage 3 — Feedback (week 2–6)

- Triage issues within a few days; label; reproduce; thank reporters.
- Every confirmed bug becomes a regression test; ship fixes as `0.1.0-alpha.2` (needs the owner's
  decision on version/tag).
- Keep a public "Known issues" list in the README/status page honest.
- Collect: terminal compatibility matrix (which emulators work), FreeBSD results, hardware reports.
- Invite repeat reporters to review docs or take a good-first issue.

## Stage 4 — Long-term growth

- A short devlog per release (what changed, what was learned) — dev.to or your own site.
- Technical demos: "inspect your firmware ELF in the terminal", "agent permission rules explained",
  "how the editor engine avoids I/O" — each as a short post/video.
- Packaging by others (AUR, FreeBSD port, Nix) — welcome contributions, not a priority for the author.
- Regular releases with a changelog; Discussions for questions.
- Prepare the GUI release: reuse the engine crates; announce only when there is something to run.

## Measuring real interest (no vanity metrics)

Quality signals matter more than stars:

| Signal | How to read it |
|---|---|
| Issues from people you do not know, with `embed doctor` attached | the best signal: someone actually ran it |
| Distinct reporters, and reporters who come back | retention |
| Pull requests and docs fixes from outside | community |
| Release asset downloads | rough install count (includes your own tests) |
| Unique cloners / viewers and referrers | where interest comes from |
| Stars, forks, watchers | awareness only; never a goal |

Commands (need `gh` logged in as a repo admin; traffic data covers only the last 14 days — record it
weekly in a private note):

```sh
R=Kirpich-Space/EmbedIDE
gh api repos/$R --jq '{stars:.stargazers_count,forks:.forks_count,watchers:.subscribers_count,issues:.open_issues_count}'
gh api repos/$R/releases --jq '.[] | {tag:.tag_name, downloads:[.assets[]|{name,count:.download_count}]}'
gh api repos/$R/traffic/views  --jq '{views:.count, unique:.uniques}'
gh api repos/$R/traffic/clones --jq '{clones:.count, unique:.uniques}'
gh api repos/$R/traffic/popular/referrers --jq '.[] | "\(.referrer): \(.count) (\(.uniques) unique)"'
gh api -H "Accept: application/vnd.github.star+json" repos/$R/stargazers --paginate --jq '.[].starred_at' | cut -c1-10 | sort | uniq -c
gh issue list -R $R --state all --json author,createdAt,labels --jq 'group_by(.author.login) | map({user:.[0].author.login, issues:length})'
```

Do not report numbers you did not measure. Compare to the day before launch, not to other projects.
The two downloads currently on the release are from the author's own tests.

## Principles

Real software, real developers. Be specific, be honest about limits, answer people, thank reporters,
never ask for stars, never automate posting.
