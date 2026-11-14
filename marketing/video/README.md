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
          audio.py (neural narration + synthesised music + mix), i18n.py (English / Russian dub)
compose_overview.py   Video 1 — official overview            (1920x1080, ~4 min)
compose_demo.py       Video 2 — technical demonstration     (1920x1080, ~3.5 min)
compose_short.py      Video 3 — short promo for Shorts       (1080x1920, ~42 s)
VIDEO_LANG=ru python compose_*.py   the Russian dub (*_ru.mp4, Russian captions and on-screen text)
metadata/             upload-ready titles, descriptions, tags, chapters, subtitles (.srt/.vtt), thumbnails
out/                  recordings, WAVs and the rendered MP4s (git-ignored; MP4s are release assets)
```

## Requirements

Linux, `ffmpeg` (libx264, aac), `rsvg-convert`, Python 3.11+ with `pyte pillow numpy scipy`, fonts DejaVu Sans Mono /
DejaVu Sans / Noto Sans. Narration runs **locally** (no cloud, no API) and uses **one voice for every language**:
Chatterbox Multilingual (MIT) cloned from a single reference clip, `~/.cache/embedide-video/ref_voice.wav`
(a short Kokoro-82M sample, Apache-2.0). It needs a separate Python with torch (CUDA), `chatterbox-tts` and
`faster-whisper`; a GPU with 8 GB is enough. Run `python3 prefill_voice.py --tts-python /path/to/that/python`
once: it queues every line of all storyboards, synthesises them sentence by sentence and re-takes any sentence
that Whisper does not read back correctly. The compose scripts then only read the cache.
`finalize.sh` copies the finished renders into `../../videos/`.

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
for L in en ru; do for v in demo overview short; do VIDEO_LANG=$L python compose_$v.py; done; done
```

Check the screens that were recorded before composing (the scripts print the final screen and events),
update the narration if behaviour changed, and look at the rendered frames (`ffmpeg -i out/X.mp4 -vf
fps=1/10,tile=4x4 sheet.png`). Narration is generated locally; `lib/audio.py` has pronunciation tables
(`SPOKEN`, `SPOKEN_RU`) and captions keep the written text. Scenes can zoom into terminal regions (`focus=`),
use wipe transitions, kinetic text (`Words`) and animated counters (`Stats`). Music is synthesised in `lib/audio.py` (original; no
third-party audio). Audio is normalised to -16 LUFS.

## Publishing

- GitHub: `sh publish_release.sh v0.1.0-alpha.1 0.1.0-alpha.1` (uploads English and Russian videos and subtitles).
- YouTube / Shorts and other platforms: upload the MP4, paste the title/description/tags from
  `metadata/<name>.metadata.md`, attach `<name>.srt` as English subtitles and the thumbnail. Disclose in
  the video description that it is an official Kirpich Space production (already in the text).
- Do not claim features that `docs/cli/status.md` marks unverified; update the narration instead.
