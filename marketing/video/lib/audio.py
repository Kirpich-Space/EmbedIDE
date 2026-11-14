"""Narration (Piper TTS, voice: LibriTTS-R, CC BY 4.0) and original synthesised music."""
import hashlib, json, os, re, subprocess, wave
from pathlib import Path

import numpy as np

SR = 48000
VOICE_DIR = Path(os.environ.get("EMBEDIDE_VOICE_DIR", Path.home() / ".cache" / "embedide-video"))
VOICE = VOICE_DIR / "en_US-libritts_r-medium.onnx"
SPEAKER = 6
CACHE = VOICE_DIR / "narration"

_piper = {}
_kokoro = None
KOKORO_VOICE = "af_heart"      # Kokoro-82M (Apache-2.0), best-rated English voice
PIPER_RU = VOICE_DIR / "ru_RU-denis-medium.onnx"   # Piper, CC0 dataset


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
    (r"EmbedIDE", "Эмбэд-Ай-Ди-И"), (r"Claude Code", "Клод Код"), (r"Codex", "Кодекс"), (r"\bRust\b", "Раст"),
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


def narrate(text, lang="en"):
    """Mono float32 audio at SR for `text`, cached on disk. English: Kokoro-82M; Russian: Piper (CC0 voice)."""
    global _kokoro
    CACHE.mkdir(parents=True, exist_ok=True)
    spoken_text = spoken(text) if lang == "en" else spoken_ru(text)
    key = hashlib.sha1(f"{lang}|{KOKORO_VOICE}|{spoken_text}".encode()).hexdigest()[:16]
    f = CACHE / f"{key}.npy"
    if f.exists():
        return np.load(f)
    if lang == "en":
        from kokoro_onnx import Kokoro
        if _kokoro is None:
            _kokoro = Kokoro(str(VOICE_DIR / "kokoro-v1.0.onnx"), str(VOICE_DIR / "voices-v1.0.bin"))
        a, sr = _kokoro.create(spoken_text, voice=KOKORO_VOICE, speed=1.02, lang="en-us")
        a = _resample(np.asarray(a, dtype=np.float32), sr)
    else:
        from piper import PiperVoice, SynthesisConfig
        if "ru" not in _piper:
            _piper["ru"] = PiperVoice.load(str(PIPER_RU))
        cfg = SynthesisConfig(length_scale=1.05, noise_scale=0.6, noise_w_scale=0.7)
        chunks = [c.audio_int16_array for c in _piper["ru"].synthesize(spoken_text, syn_config=cfg)]
        a = _resample(np.concatenate(chunks).astype(np.float32) / 32768.0, 22050)
    a = _tail_trim(a)
    a = a / max(1e-6, np.abs(a).max()) * 0.85
    np.save(f, a)
    return a


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

def _saw(freq, n, phase=0.0):
    t = (np.arange(n) / SR) * freq + phase
    return 2.0 * (t - np.floor(t + 0.5))


def _pluck(freq, dur, vel=1.0):
    n = int(dur * SR)
    N = max(2, int(SR / freq))
    y = np.zeros(n + 2 * N, dtype=np.float32)
    rng = np.random.default_rng(int(freq * 10) % 2**31)
    y[:N] = rng.uniform(-1, 1, N)
    decay = 0.996
    for k in range(1, (n // N) + 2):
        a, b = y[(k - 1) * N:k * N], np.roll(y[(k - 1) * N:k * N], 1)
        y[k * N:(k + 1) * N] = decay * 0.5 * (a + b)
    y = y[:n]
    env = np.exp(-np.arange(n) / SR * 2.2)
    return y * env * vel


def music(duration, bpm=100, seed=7):
    """Original ambient-industrial bed in A minor: pad, bass, soft pluck arpeggio, quiet pulse."""
    n = int(duration * SR)
    out = np.zeros((n,), dtype=np.float32)
    spb = 60.0 / bpm
    bar = spb * 4
    chords = [(110.0, [220.0, 261.63, 329.63]),      # Am
              (87.31, [174.61, 261.63, 349.23]),     # F
              (130.81, [196.0, 261.63, 329.63]),     # C
              (98.0, [196.0, 246.94, 293.66])]       # G
    nbars = int(duration / bar) + 2
    rng = np.random.default_rng(seed)
    for b in range(nbars):
        root, notes = chords[b % 4]
        t0 = b * bar
        i0 = int(t0 * SR)
        L = int((bar + 1.2) * SR)
        if i0 >= n:
            break
        # pad: detuned saws, slow attack/release, filtered via simple smoothing
        pad = np.zeros(L, dtype=np.float32)
        for f in notes:
            for det in (-0.004, 0.0, 0.004):
                pad += _saw(f * (1 + det), L, rng.random()) * 0.05
        env = np.minimum(1.0, np.arange(L) / (SR * 1.0)) * np.minimum(1.0, (L - np.arange(L)) / (SR * 1.2))
        # one-pole smoothing (cheap): moving average of 24 samples approximates a ~1 kHz low pass
        k = np.ones(24, dtype=np.float32) / 24
        pad = np.convolve(pad, k, mode="same") * env
        # bass: sine + a little 2nd harmonic
        tt = np.arange(L) / SR
        bass = (np.sin(2 * np.pi * root * tt) * 0.30 + np.sin(4 * np.pi * root * tt) * 0.05) * env
        seg = pad + bass
        j1 = min(n, i0 + L)
        out[i0:j1] += seg[: j1 - i0]
        # arpeggio plucks on 8ths
        pattern = [0, 2, 1, 2, 0, 1, 2, 1]
        for s in range(8):
            ts = t0 + s * spb / 2
            idx = int(ts * SR)
            if idx >= n:
                break
            f = notes[pattern[s]] * (2 if s % 4 == 2 else 1)
            p = _pluck(f, 1.0, 0.16 if s % 2 == 0 else 0.10)
            j1 = min(n, idx + len(p))
            out[idx:j1] += p[: j1 - idx]
        # rhythm from the second bar on: soft kick on 1 and 3, snare on 2 and 4, hats on the 8ths
        if b >= 1:
            for beat in range(4):
                idx = int((t0 + beat * spb) * SR)
                if idx >= n:
                    break
                if beat in (0, 2):
                    m = int(0.28 * SR)
                    tt = np.arange(m) / SR
                    kick = np.sin(2 * np.pi * (45 + 90 * np.exp(-tt * 28)) * tt) * np.exp(-tt * 9) * 0.55
                else:
                    m = int(0.18 * SR)
                    tt = np.arange(m) / SR
                    kick = (rng.uniform(-1, 1, m) * 0.5 + np.sin(2 * np.pi * 190 * tt) * 0.4) * np.exp(-tt * 22) * 0.22
                j1 = min(n, idx + m)
                out[idx:j1] += kick[: j1 - idx].astype(np.float32)
            for e in range(8):
                idx = int((t0 + e * spb / 2) * SR)
                if idx >= n:
                    break
                m = int(0.04 * SR)
                hat = np.diff(rng.uniform(-1, 1, m + 1)).astype(np.float32) * np.exp(-np.arange(m) / (SR * 0.01)) * (0.07 if e % 2 else 0.04)
                j1 = min(n, idx + m)
                out[idx:j1] += hat[: j1 - idx]
    # slow master fade in / out
    fi = min(n, int(2.5 * SR))
    out[:fi] *= np.linspace(0, 1, fi)
    fo = min(n, int(3.0 * SR))
    out[-fo:] *= np.linspace(1, 0, fo)
    out /= max(1e-6, np.abs(out).max())
    return out * 0.35


def whoosh(dur=0.8, seed=3):
    """Short rising-then-falling noise sweep for scene transitions."""
    n = int(dur * SR)
    noise = np.random.default_rng(seed).uniform(-1, 1, n).astype(np.float32)
    lo = np.convolve(noise, np.ones(80, dtype=np.float32) / 80, mode="same")
    hi = np.convolve(noise, np.ones(6, dtype=np.float32) / 6, mode="same")
    sweep = np.linspace(0, 1, n, dtype=np.float32)
    env = np.sin(np.linspace(0, np.pi, n, dtype=np.float32)) ** 2
    return (lo * (1 - sweep) + hi * sweep) * env * 0.5


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
    return music_track * env


def write_wav(path, stereo):
    pcm = np.clip(stereo, -1, 1)
    pcm = (pcm * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def mix(film, out_wav):
    voice, windows = captions_and_voice(film)
    mus = music(film.total + 1.0)
    mus = np.pad(mus, (0, max(0, len(voice) - len(mus))))[: len(voice)]
    mus = duck(mus, windows)
    voice = voice[: len(mus)]
    sfx = np.zeros(len(mus), dtype=np.float32)
    for i, p in enumerate(film.placed[1:], 1):
        w = whoosh(0.9, seed=i)
        i0 = max(0, int((p.start - 0.2) * SR))
        sfx[i0:i0 + len(w)] += w[: max(0, len(sfx) - i0)]
    mono = voice * 0.95 + mus + sfx * 0.55
    stereo = np.stack([mono, mono * 0.98], axis=1)[: int(film.total * SR)]
    write_wav(out_wav, stereo)
    return windows


def mux(video, wav, out):
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(video), "-i", str(wav),
                    "-af", "loudnorm=I=-16:TP=-1.5:LRA=11", "-c:v", "copy", "-ar", "48000", "-c:a", "aac", "-b:a", "192k",
                    "-shortest", "-movflags", "+faststart", str(out)], check=True)
