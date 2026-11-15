"""Narration (one multilingual neural voice for all languages: Microsoft Emma via edge-tts) and original synthesised music."""
import hashlib, json, os, re, subprocess, wave
from pathlib import Path

import numpy as np

SR = 48000
VOICE_DIR = Path(os.environ.get("EMBEDIDE_VOICE_DIR", Path.home() / ".cache" / "embedide-video"))
CACHE = VOICE_DIR / "narration"

VOICE_ID = "edge-emma-mtl-1"   # bump to invalidate the narration cache


def _resample(a, sr_from):
    from scipy.signal import resample_poly
    from math import gcd
    g = gcd(SR, sr_from)
    return resample_poly(a, SR // g, sr_from // g).astype(np.float32)


def _tail_trim(a):
    nz = np.where(np.abs(a) > 0.01)[0]
    if len(nz):
        a = a[max(0, nz[0] - 800): nz[-1] + 1600]
    return a


SPOKEN = [
    (r"0\.1\.0-alpha\.1", "zero point one point zero, alpha one"),
    (r"GPL-3\.0", "G P L three"),
    (r"\bF5\b", "F five"), (r"\bAlt\+(\d)", lambda m: "Alt plus " + ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"][int(m.group(1))]),
    (r"\b(\w+)\.([ch])\b", lambda m: f"{m.group(1)} dot {m.group(2)}"),
    (r"/review", "slash review"), (r"/revert", "slash revert"), (r"/policy", "slash policy"), (r"/skills", "slash skills"),
    (r"\bGCC\b", "G C C"), (r"\bPTY\b", "P T Y"), (r"\bCLIs\b", "C L Is"), (r"\bCLI\b", "C L I"),
    (r"\bTUI\b", "T U I"), (r"\bGUI\b", "G U I"), (r"\bAPI\b", "A P I"), (r"\bI/O\b", "I O"),
    (r"\bFreeBSD\b", "Free B S D"), (r"\bobjdump\b", "obj dump"), (r"\bELF\b", "elf"), (r"\bHEX\b", "hex"),
    (r"\bNASM\b", "nasm"), (r"\bGDB\b", "G D B"), (r"\bOpenOCD\b", "Open O C D"),
    (r"\bGitHub\b", "Git Hub"), (r"\bzig cc\b", "zig C C"), (r"\bLSP\b", "L S P"),
]


def spoken(text):
    for pat, rep in SPOKEN:
        text = re.sub(pat, rep, text)
    return text


SPOKEN_RU = [
    (r"EmbedIDE", "Ембедъидъе"), (r"Claude Code", "Клод Код"), (r"Codex", "Кодекс"), (r"\bRust\b", "Раст"),
    (r"0\.1\.0-alpha\.1", "ноль точка один точка ноль, альфа один"), (r"GPL-3\.0|GPL", "Джи-Пи-Эл три"),
    (r"\bF5\b", "эф пять"), (r"\bAlt\+(\d)", lambda m: "Альт плюс " + ["ноль", "один", "два", "три", "четыре", "пять", "шесть", "семь", "восемь", "девять"][int(m.group(1))]),
    (r"\b(\w+)\.([ch])\b", lambda m: f"{m.group(1)} точка {'си' if m.group(2) == 'c' else 'аш'}"),
    (r"/review", "слэш ревью"), (r"/revert", "слэш ревёрт"), (r"/policy", "слэш полиси"), (r"/skills", "слэш скиллс"),
    (r"\bGCC\b", "Джи-Си-Си"), (r"\bPTY\b", "пи-ти-вай"), (r"\bCLI\b", "си-эл-ай"), (r"\bTUI\b", "ти-ю-ай"),
    (r"\bGUI\b", "гуи"), (r"\bAPI\b", "эй-пи-ай"), (r"\bFreeBSD\b", "Фри-Би-Эс-Ди"), (r"\bELF\b", "эльф"),
    (r"\bHEX\b", "хекс"), (r"\bobjdump\b", "обж-дамп"), (r"\bNASM\b", "насм"), (r"\bGDB\b", "Джи-Ди-Би"),
    (r"\bOpenOCD\b", "Оупен-О-Си-Ди"), (r"\bGitHub\b", "Гитхаб"), (r"\bLinux\b", "Линукс"),
    (r"\bElectron\b", "Электрон"), (r"\bzig cc\b", "зиг си-си"), (r"\bClang\b", "Клэнг"), (r"\bCMake\b", "Си-Мэйк"),
    (r"\bMake\b", "Мэйк"), (r"\bCargo\b", "Карго"), (r"\bZig\b", "Зиг"), (r"\bCortex-M\b", "Кортекс-М"),
    (r"\bC\b", "Си"), (r"\bVim\b", "Вим"), (r"\bAI\b", "эй-ай"), (r"\bembed\b", "эмбэд"), (r"\bGit\b", "Гит"),
    (r"\bring_pop\b", "ринг-поп"), (r"\bmain\b", "майн"), (r"\bdoctor\b", "доктор"),
]


def spoken_ru(text):
    for pat, rep_ in SPOKEN_RU:
        text = re.sub(pat, rep_, text)
    return text


def _key(text, lang):
    return hashlib.sha1(f"{lang}|{VOICE_ID}|{text}".encode()).hexdigest()[:16]


def narrate(text, lang="en"):
    """Mono float32 audio at SR for `text`. One voice for every language (Microsoft's free multilingual neural voice
    Emma via edge-tts, see lib/tts_worker.py). Audio comes from the cache; on a miss the text is queued in
    $CACHE/jobs.jsonl and silence is returned so a dry run can collect every line (see `prefill`)."""
    CACHE.mkdir(parents=True, exist_ok=True)
    spoken_text = spoken(text) if lang == "en" else spoken_ru(text)
    key = _key(spoken_text, lang)
    f = CACHE / f"{key}.npy"
    if f.exists():
        return np.load(f)
    with open(CACHE / "jobs.jsonl", "a", encoding="utf-8") as jobs:
        jobs.write(json.dumps({"key": key, "lang": lang, "text": spoken_text}, ensure_ascii=False) + "\n")
    if os.environ.get("EMBEDIDE_TTS_DRYRUN"):
        return np.zeros(int(max(2.0, len(spoken_text) / 14) * SR), np.float32)
    raise SystemExit(f"narration for {text[:50]!r} is not synthesised yet: run `python3 prefill_voice.py`")


def sentences(text):
    parts = re.split(r"(?<=[.!?])\s+", text.strip())
    return [p for p in parts if p]


def chunks(text, limit=100):
    """Caption pieces: sentences, long ones split at a comma or space so each fits two lines."""
    out = []
    for sent in sentences(text):
        while len(sent) > limit:
            cut = max(sent.rfind(", ", 0, limit), sent.rfind("; ", 0, limit))
            if cut < 30:
                cut = sent.rfind(" ", 0, limit)
            else:
                cut += 1
            out.append(sent[:cut].strip())
            sent = sent[cut:].strip()
        if sent:
            out.append(sent)
    return out


def fit_durations(film, pad=1.0, lead=0.45):
    """Make each narrated scene at least as long as its narration."""
    for s, _ in film.scenes:
        if s.narration:
            s.narr = narrate(s.narration, film.lang)
            if hasattr(s, "fit_to"):
                s.fit_to(len(s.narr) / SR + lead + pad)
            s.duration = max(s.duration, len(s.narr) / SR + lead + pad)


def captions_and_voice(film, lead=0.45):
    """Return (voice track, narration windows) and fill film.captions."""
    total = int(film.total * SR) + SR
    voice = np.zeros(total, dtype=np.float32)
    windows = []
    film.captions = []
    for p in film.placed:
        s = p.scene
        if not s.narration:
            continue
        a = s.narr
        i0 = int((p.start + lead) * SR)
        voice[i0:i0 + len(a)] += a[: len(voice) - i0]
        t0, t1 = p.start + lead, p.start + lead + len(a) / SR
        windows.append((t0, t1))
        sents = chunks(s.narration)
        weights = np.array([len(x) + 8 for x in sents], dtype=np.float64)
        edges = np.concatenate([[0], np.cumsum(weights) / weights.sum()])
        for k, sent in enumerate(sents):
            film.captions.append((t0 + (t1 - t0) * edges[k], t0 + (t1 - t0) * edges[k + 1] - 0.05, sent))
    return voice, windows


# ----------------------------------------------------------------- music

def _bell(freq, dur, vel):
    """Soft electric-piano-like note: three decaying partials, 6 ms attack. No noise, no aliasing."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    y = (np.sin(2 * np.pi * freq * t) + 0.28 * np.sin(2 * np.pi * 2 * freq * t) * np.exp(-t * 3.0)
         + 0.08 * np.sin(2 * np.pi * 3 * freq * t) * np.exp(-t * 5.0))
    env = np.minimum(1.0, t / 0.006) * np.exp(-t * 2.4)
    return (y * env * vel).astype(np.float32)


def music(duration, bpm=84, seed=7):
    """Calm original ambient bed in A minor, stereo (n, 2): warm sine pad, soft bass, sparse bell arpeggio.
    No drums and no sawtooth/noise sources, so nothing harsh competes with the voice."""
    n = int(duration * SR)
    out = np.zeros((n, 2), dtype=np.float32)
    spb = 60.0 / bpm
    bar = spb * 4
    chords = [(110.0, [220.0, 261.63, 329.63]), (87.31, [174.61, 261.63, 349.23]),
              (130.81, [196.0, 261.63, 329.63]), (98.0, [196.0, 246.94, 293.66])]
    rng = np.random.default_rng(seed)
    for b in range(int(duration / bar) + 2):
        root, notes = chords[b % 4]
        i0 = int(b * bar * SR)
        if i0 >= n:
            break
        L = int((bar + 1.6) * SR)
        t = np.arange(L) / SR
        env = np.minimum(1.0, t / 1.4) * np.minimum(1.0, (L / SR - t) / 1.6)
        pads = [np.zeros(L, np.float32), np.zeros(L, np.float32)]
        for f in notes:
            for ch, det in ((0, 0.0025), (1, -0.0025)):
                ph = rng.random() * 6.28
                ff = f * (1 + det)
                pads[ch] += (np.sin(2 * np.pi * ff * t + ph) + 0.30 * np.sin(4 * np.pi * ff * t + ph)
                             + 0.10 * np.sin(6 * np.pi * ff * t + ph)).astype(np.float32) * 0.06
        bass = (np.sin(2 * np.pi * root * t) * 0.20 + np.sin(4 * np.pi * root * t) * 0.03).astype(np.float32)
        j1 = min(n, i0 + L)
        for ch in (0, 1):
            out[i0:j1, ch] += ((pads[ch] + bass) * env)[: j1 - i0]
        for st in range(4):                                   # sparse arpeggio on beats, alternating sides
            idx = int((b * bar + st * spb) * SR)
            if idx >= n:
                break
            note = _bell(notes[[0, 2, 1, 2][st]] * (2 if st == 2 else 1), 2.0, 0.07 if st % 2 == 0 else 0.05)
            j1 = min(n, idx + len(note))
            out[idx:j1, st % 2] += note[: j1 - idx] * 0.8
            out[idx:j1, 1 - st % 2] += note[: j1 - idx] * 0.35
    fi, fo = min(n, int(3.0 * SR)), min(n, int(4.0 * SR))
    out[:fi] *= np.linspace(0, 1, fi)[:, None]
    out[-fo:] *= np.linspace(1, 0, fo)[:, None]
    out /= max(1e-6, np.abs(out).max())
    return out * 0.5


def whoosh(dur=0.8, seed=3):
    """Soft transition swell: band-passed noise with a slow rise and fall (no hiss)."""
    n = int(dur * SR)
    noise = np.random.default_rng(seed).uniform(-1, 1, n).astype(np.float32)
    lo = np.convolve(noise, np.ones(60, dtype=np.float32) / 60, mode="same")
    lo = lo - np.convolve(lo, np.ones(1200, dtype=np.float32) / 1200, mode="same")
    env = np.sin(np.linspace(0, np.pi, n, dtype=np.float32)) ** 3
    return lo * env * 0.9


def duck(music_track, windows, depth=0.38, ramp=0.25):
    env = np.ones(len(music_track), dtype=np.float32)
    for a, b in windows:
        i0, i1 = int((a - ramp) * SR), int((b + ramp) * SR)
        i0, i1 = max(0, i0), min(len(env), i1)
        seg = np.full(i1 - i0, depth, dtype=np.float32)
        r = int(ramp * SR)
        if r > 0 and len(seg) > 2 * r:
            seg[:r] = np.linspace(1, depth, r)
            seg[-r:] = np.linspace(depth, 1, r)
        env[i0:i1] = np.minimum(env[i0:i1], seg)
    # smooth
    k = np.ones(int(0.05 * SR), dtype=np.float32)
    k /= k.sum()
    env = np.convolve(env, k, mode="same")
    return music_track * (env[:, None] if music_track.ndim == 2 else env)


def write_wav(path, stereo):
    pcm = np.clip(stereo, -1, 1)
    pcm = (pcm * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def _ffmpeg_filter(x, filt, channels):
    """Run float audio (n,) or (n, 2) at SR through an ffmpeg -af chain and return it with the same shape."""
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        src, dst = Path(d) / "in.wav", Path(d) / "out.wav"
        pcm = (np.clip(x, -1, 1) * 32767).astype("<i2")
        with wave.open(str(src), "wb") as w:
            w.setnchannels(channels)
            w.setsampwidth(2)
            w.setframerate(SR)
            w.writeframes(pcm.tobytes())
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(src), "-af", filt, "-ar", str(SR),
                        "-c:a", "pcm_s16le", str(dst)], check=True)
        with wave.open(str(dst), "rb") as w:
            y = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768.0
        y = y.reshape(-1, channels) if channels > 1 else y
    out = np.zeros_like(x)
    m = min(len(out), len(y))
    out[:m] = y[:m]
    return out


# Voice chain: remove rumble and TTS hiss, tame the 2-5 kHz harshness, de-ess, gentle compression, slight warmth.
VOICE_FX = ("highpass=f=85,lowpass=f=11000,equalizer=f=250:t=q:w=1.2:g=1.5,equalizer=f=3200:t=q:w=1.0:g=-2.5,"
            "equalizer=f=6500:t=q:w=2:g=-3,deesser=i=0.35:m=0.5:f=0.5,"
            "acompressor=threshold=-21dB:ratio=2.6:attack=12:release=180:makeup=2.5")
MUSIC_FX = "lowpass=f=5500,highpass=f=40,aecho=0.8:0.7:70|140:0.28|0.2"


def mix(film, out_wav):
    voice, windows = captions_and_voice(film)
    voice = _ffmpeg_filter(voice.astype(np.float32), VOICE_FX, 1)
    mus = music(film.total + 1.0)
    mus = _ffmpeg_filter(mus, MUSIC_FX, 2)
    mus = np.pad(mus, ((0, max(0, len(voice) - len(mus))), (0, 0)))[: len(voice)]
    mus = duck(mus, windows, depth=0.42) * 0.55
    voice = voice[: len(mus)]
    sfx = np.zeros(len(mus), dtype=np.float32)
    for i, p in enumerate(film.placed[1:], 1):
        w = whoosh(0.9, seed=i)
        i0 = max(0, int((p.start - 0.2) * SR))
        sfx[i0:i0 + len(w)] += w[: max(0, len(sfx) - i0)]
    stereo = mus + (voice * 0.95 + sfx * 0.30)[:, None]
    stereo = stereo[: int(film.total * SR)]
    write_wav(out_wav, stereo)
    return windows


def mux(video, wav, out):
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(video), "-i", str(wav),
                    "-af", "loudnorm=I=-16:TP=-1.5:LRA=9,alimiter=limit=0.89", "-c:v", "copy", "-ar", "48000", "-c:a", "aac", "-b:a", "192k",
                    "-shortest", "-movflags", "+faststart", str(out)], check=True)
