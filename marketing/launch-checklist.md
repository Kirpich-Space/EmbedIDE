# Launch checklist

Tick items as you go. "Owner" = needs your decision or your account.

## Before any post

- [ ] README renders correctly on GitHub in light and dark mode (logo, screenshots, badges). *(owner: look at it in a browser)*
- [ ] `install.sh` works on a clean machine/container: `curl -fsSL …/install.sh | sh` then `embed doctor`.
- [ ] A fresh clone builds: `cargo build --release --locked` (Rust 1.85+).
- [ ] `cargo test --workspace --locked`, `clippy -D warnings`, `fmt --check` pass; PTY suites pass.
- [ ] Release `v0.1.0-alpha.1` has the archives and `SHA256SUMS`; notes are accurate.
- [ ] License is GPL-3.0-only everywhere (`LICENSE`, `Cargo.toml`, README).
- [ ] Repo About, topics and (owner) **social preview image** set; Issues enabled; Discussions enabled (owner).
- [ ] `SECURITY.md` private reporting works: Settings → Code security → "Private vulnerability reporting" is on (owner).
- [x] Demo videos recorded and linked (release assets); upload them to YouTube (owner account).
- [ ] Every claim in the posts matches `docs/cli/status.md` (platform, hardware, FreeBSD, AI).
- [ ] Good-first-issue list created (`contributor-outreach.md`).
- [ ] You can spend the first 3 hours after each post answering comments.

## Posting (each needs your explicit approval of text + destination)

- [ ] r/rust — `reddit-posts.md`
- [ ] dev.to — `devto-article.md` (set `published: true` yourself)
- [ ] r/embedded
- [ ] r/commandline
- [ ] Show HN — `show-hn.md`
- [ ] r/FreeBSD
- [ ] Mastodon / X / LinkedIn — `social-posts.md`
- [ ] r/opensource
- [ ] Awesome-list pull requests — `community-targets.md` (check criteria first)
- [ ] Rust Users Forum announcement; This Week in Rust submission

## After each post

- [ ] Reply to every comment and issue the same day; fix doc/README gaps people point out.
- [ ] Record numbers (see `launch-plan.md` → Measuring) at day 1, 3, 7, 14.
- [ ] Note recurring confusions → update README/FAQ.

## Don'ts

- [ ] No asking for stars/upvotes, no multi-posting the same text, no automation, no unrelated communities.
- [ ] No claims about Windows/macOS support, hardware verification or GUI availability.
