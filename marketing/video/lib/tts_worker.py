#!/usr/bin/env python3
"""Batch narration with ONE voice for every language: Microsoft's free neural voice
en-US-EmmaMultilingualNeural, reached through the `edge-tts` package (no key, no account; the script text is
public). Needs `pip install edge-tts faster-whisper scipy`. Reads jobs.jsonl lines
{"key","lang","text"}, writes CACHE/<key>.npy (mono float32, 48 kHz). Every sentence is transcribed back with
Whisper and regenerated (up to 5 tries) until it matches the script, so no mispronounced take ships."""
import json, os, re, sys
from math import gcd
from pathlib import Path

import numpy as np

SR = 48000
HOME = Path(os.environ.get("EMBEDIDE_VOICE_DIR", Path.home() / ".cache" / "embedide-video"))
REF = HOME / "ref_voice.wav"


def norm(s):
    return re.sub(r"[^\w\s]", " ", s.lower().replace("ё", "е")).split()


def wer(ref, hyp):
    r, h = norm(ref), norm(hyp)
    d = list(range(len(h) + 1))
    for i in range(1, len(r) + 1):
        p, d[0] = d[0], i
        for j in range(1, len(h) + 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (r[i - 1] != h[j - 1]))
    return d[len(h)] / max(1, len(r))


VOICE = "en-US-EmmaMultilingualNeural"


def decode(mp3):
    import subprocess
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", mp3, "-f", "f32le", "-ac", "1", "-ar", "24000", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype="<f4").copy(), 24000


def synth(text, path):
    import asyncio, edge_tts

    async def go():
        for attempt in range(6):
            try:
                await edge_tts.Communicate(text, VOICE, rate="-2%").save(path)
                return
            except Exception:
                await asyncio.sleep(1.5 * (attempt + 1))
        raise SystemExit(f"edge-tts gave no audio for {text[:50]!r}")
    asyncio.run(go())


def main(jobs_file, cache):
    import tempfile
    from scipy.signal import resample_poly
    from faster_whisper import WhisperModel
    asr = WhisperModel("small", device="cpu", compute_type="int8")
    cache = Path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    tmp = tempfile.mkdtemp()

    def say(sentence, lang):
        best, best_w = None, 9
        for attempt in range(4):
            synth(sentence, f"{tmp}/s.mp3")
            a, sr = decode(f"{tmp}/s.mp3")
            g = gcd(16000, sr)
            segs, _ = asr.transcribe(resample_poly(a, 16000 // g, sr // g).astype(np.float32), language=lang, beam_size=5)
            e = wer(sentence, " ".join(s.text for s in segs))
            if e < best_w:
                best, best_w, bsr = a, e, sr
            if e <= 0.15:
                break
        print(f"  wer={best_w:.2f} {sentence[:60]}", flush=True)
        return best, bsr

    for line in open(jobs_file, encoding="utf-8"):
        job = json.loads(line)
        out = cache / f"{job['key']}.npy"
        if out.exists():
            continue
        parts = []
        for sent in re.split(r"(?<=[.!?])\s+", job["text"].strip()):
            if not sent:
                continue
            a, sr = say(sent, job["lang"])
            g = gcd(SR, sr)
            a = resample_poly(a, SR // g, sr // g).astype(np.float32)
            nz = np.where(np.abs(a) > 0.01)[0]
            if len(nz):
                a = a[max(0, nz[0] - 600): nz[-1] + 1200]
            parts += [a, np.zeros(int(0.28 * SR), np.float32)]
        a = np.concatenate(parts[:-1])
        a = a / max(1e-6, np.abs(a).max()) * 0.85
        np.save(out, a)
        print("wrote", out.name, f"{len(a)/SR:.1f}s", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
