#!/usr/bin/env python3
"""Pick the narrator before spending credits.

  python3 audition_voices.py list                    # free: shows your quota and the voices you can use
  python3 audition_voices.py say VOICE_ID [VOICE_ID…]  # one ~100-character Russian + English line per voice

Samples go to ~/.cache/embedide-video/audition/<name>_<lang>.mp3 (about 200 characters per voice)."""
import os, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from lib import elevenlabs as E  # noqa: E402

RU = "Ембедъидъе — нативная среда разработки на Rust. Она живёт в вашем терминале."
EN = "EmbedIDE is a native Rust IDE. It lives in your terminal."
OUT = Path.home() / ".cache" / "embedide-video" / "audition"

if len(sys.argv) < 2 or sys.argv[1] == "list":
    used, limit = E.quota()
    print(f"credits used {used} of {limit}\n")
    for v in E.voices():
        lab = v.get("labels") or {}
        print(f"{v['voice_id']}  {v['name']:<22} {lab.get('gender', ''):<7} {lab.get('age', ''):<12} "
              f"{lab.get('descriptive', lab.get('description', ''))}  [{v.get('category', '')}]")
elif sys.argv[1] == "say":
    OUT.mkdir(parents=True, exist_ok=True)
    names = {v["voice_id"]: v["name"] for v in E.voices()}
    for vid in sys.argv[2:]:
        name = names.get(vid, vid).split()[0]
        for lang, text in (("ru", RU), ("en", EN)):
            E.speak(text, vid, OUT / f"{name}_{lang}.mp3")
            print("wrote", OUT / f"{name}_{lang}.mp3")
    print("credits used / limit:", *E.quota())
