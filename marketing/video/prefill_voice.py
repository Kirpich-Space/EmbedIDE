#!/usr/bin/env python3
"""Synthesise every narration line of all three videos in both languages with the single cloned voice.

  python3 prefill_voice.py [--tts-python PATH]      # PATH: Python with torch, chatterbox-tts, faster-whisper

Step 1 (dry run, no GPU): import each storyboard and queue the lines missing from the cache.
Step 2: lib/tts_worker.py synthesises them (each sentence is verified with Whisper and retried).
"""
import argparse, os, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from lib import audio as A  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("--tts-python", default=os.environ.get("EMBEDIDE_TTS_PYTHON", sys.executable))
a = ap.parse_args()
A.CACHE.mkdir(parents=True, exist_ok=True)
jobs = A.CACHE / "jobs.jsonl"
jobs.unlink(missing_ok=True)
for mod in ("compose_overview", "compose_demo", "compose_short"):
    for lang in ("en", "ru"):
        env = dict(os.environ, VIDEO_LANG=lang, EMBEDIDE_TTS_DRYRUN="1")
        subprocess.run([sys.executable, "-c", f"import {mod} as m; from lib import audio as A; A.fit_durations(m.film)"],
                       cwd=HERE, env=env, check=True)
if not jobs.exists():
    print("nothing to synthesise")
    sys.exit(0)
subprocess.run([a.tts_python, str(HERE / "lib" / "tts_worker.py"), str(jobs), str(A.CACHE)], check=True)
jobs.unlink()
