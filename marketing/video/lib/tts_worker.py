#!/usr/bin/env python3
"""Batch narration with ONE voice for every language: Chatterbox Multilingual (MIT) cloned from a single
reference clip. Needs torch + chatterbox-tts + faster-whisper (see README). Reads jobs.jsonl lines
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


def main(jobs_file, cache):
    import perth, torch
    from scipy.signal import resample_poly
    from faster_whisper import WhisperModel
    from chatterbox.mtl_tts import ChatterboxMultilingualTTS

    class NoMark:   # the perth watermark package is unavailable on this Python; narration is disclosed as AI anyway
        def apply_watermark(self, w, sample_rate=None):
            return w
    perth.PerthImplicitWatermarker = NoMark
    tts = ChatterboxMultilingualTTS.from_pretrained(device="cuda")
    asr = WhisperModel("small", device="cpu", compute_type="int8")
    cache = Path(cache)
    cache.mkdir(parents=True, exist_ok=True)

    def to16(a, sr):
        g = gcd(16000, sr)
        return resample_poly(a, 16000 // g, sr // g).astype(np.float32)

    def say(sentence, spoken, lang):
        best, best_w = None, 9
        for attempt in range(5):
            torch.manual_seed(1000 + attempt)
            w = tts.generate(spoken, language_id=lang, audio_prompt_path=str(REF),
                             exaggeration=0.45, cfg_weight=0.5, temperature=0.7)
            a = w.squeeze().cpu().numpy().astype(np.float32)
            segs, _ = asr.transcribe(to16(a, tts.sr), language=lang, beam_size=5)
            e = wer(spoken, " ".join(s.text for s in segs))
            if e < best_w:
                best, best_w = a, e
            if e <= 0.12:
                break
        print(f"  wer={best_w:.2f} {sentence[:60]}", flush=True)
        return best

    for line in open(jobs_file, encoding="utf-8"):
        job = json.loads(line)
        out = cache / f"{job['key']}.npy"
        if out.exists():
            continue
        parts = []
        for sent in re.split(r"(?<=[.!?])\s+", job["text"].strip()):
            if not sent:
                continue
            a = say(sent, sent, job["lang"])
            g = gcd(SR, tts.sr)
            a = resample_poly(a, SR // g, tts.sr // g).astype(np.float32)
            nz = np.where(np.abs(a) > 0.01)[0]
            if len(nz):
                a = a[max(0, nz[0] - 600): nz[-1] + 1200]
            parts += [a, np.zeros(int(0.22 * SR), np.float32)]
        a = np.concatenate(parts[:-1])
        a = a / max(1e-6, np.abs(a).max()) * 0.85
        np.save(out, a)
        print("wrote", out.name, f"{len(a)/SR:.1f}s", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
