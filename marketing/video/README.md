# EmbedIDE video production

Reproducible pipeline for the official EmbedIDE videos. **Every screen recording is the real program**:
a script drives the released `embed` binary in a pseudo-terminal, the output goes through a terminal
emulator (pyte), each screen change is stored with its real timestamp, and the renderer draws those
stored screens. Nothing is typed into a mock-up; if a step fails, the recording script fails. Animated
graphics (the architecture diagram) carry an "ANIMATION · not application footage" chip, instruction
cards (build from source) are labelled as instructions, recordings carry "REAL RECORDING · embed <version>"
and sped-up waits carry "TIME-LAPSE ×N".

```
record/   scripts that run the real program and save out/*.pkl (screen states + timestamps + events)
lib/      brand.py (Desige palette), term.py (recorder/renderer), film.py (scenes, captions, ffmpeg),
          audio.py (Piper narration + synthesised music + mix)
compose_overview.py   Video 1 — official overview            (1920x1080, ~3 min)
compose_demo.py       Video 2 — technical demonstration     (1920x1080, ~3.5 min)
compose_short.py      Video 3 — short promo for Shorts       (1080x1920, ~37 s)
metadata/             upload-ready titles, descriptions, tags, chapters, subtitles (.srt/.vtt), thumbnails
out/                  recordings, WAVs and the rendered MP4s (git-ignored; MP4s are release assets)
```

## Requirements

Linux, `ffmpeg` (libx264, aac), `rsvg-convert`, Python 3.11+ with `pyte`, `pillow`, `numpy`, `piper-tts`
(`pip install pyte pillow numpy piper-tts` in a virtualenv), fonts DejaVu Sans Mono / DejaVu Sans / Noto Sans,
the Piper voice `en_US-libritts_r-medium` (CC BY 4.0) in `~/.cache/embedide-video/`:

```sh
mkdir -p ~/.cache/embedide-video && cd ~/.cache/embedide-video
for f in en_US-libritts_r-medium.onnx en_US-libritts_r-medium.onnx.json; do
  curl -fsSLO https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts_r/medium/$f; done
```

The agent recordings need the **Codex CLI** and **Claude Code** installed and logged in (they use your
subscriptions: a few short tasks per run). `install.sh` is run for real against GitHub.

## Make the videos for a new release

```sh
cargo build --release --locked                       # the binary that is filmed (EMBED_BIN overrides)
cd marketing/video
python record/demo_wide.py            # 1920x1080 session: edit, build error, fix, terminal, Codex agent
python record/approval.py             # skills, policies, Claude Code permission prompt (rm -rf build -> n)
python record/overview_extra.py       # install.sh, /providers + Claude Code agent, ELF/HEX/disassembly
python record/short_narrow.py         # 72x46 terminal for the vertical short
python compose_demo.py && python compose_overview.py && python compose_short.py
```

Check the screens that were recorded before composing (the scripts print the final screen and events),
update the narration if behaviour changed, and look at the rendered frames (`ffmpeg -i out/X.mp4 -vf
fps=1/10,tile=4x4 sheet.png`). Narration is spoken by Piper; `lib/audio.py` has a pronunciation table
(`SPOKEN`) and captions keep the written text. Music is synthesised in `lib/audio.py` (original; no
third-party audio). Audio is normalised to -16 LUFS.

## Publishing

- GitHub: `gh release upload <tag> out/video*.mp4 --repo Kirpich-Space/EmbedIDE` (done for v0.1.0-alpha.1).
- YouTube / Shorts and other platforms: upload the MP4, paste the title/description/tags from
  `metadata/<name>.metadata.md`, attach `<name>.srt` as English subtitles and the thumbnail. Disclose in
  the video description that it is an official Kirpich Space production (already in the text).
- Do not claim features that `docs/cli/status.md` marks unverified; update the narration instead.
