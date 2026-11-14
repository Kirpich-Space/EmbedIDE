"""Scenes, captions and the renderer that turns them into video frames (piped to ffmpeg)."""
import bisect, math, subprocess, textwrap
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

from . import brand as B
from . import term as T
from . import i18n


def font(path, size):
    return ImageFont.truetype(path, size)


def logo(width, on="dark"):
    """The official wordmark rendered from Desige/ (never redrawn)."""
    import subprocess as sp, io
    svg = B.DESIGE / f"embedide-wordmark-on-{on}.svg"
    png = sp.run(["rsvg-convert", "-w", str(width), str(svg)], capture_output=True, check=True).stdout
    return Image.open(io.BytesIO(png)).convert("RGBA")


def mark(size):
    import subprocess as sp, io
    png = sp.run(["rsvg-convert", "-w", str(size), str(B.DESIGE / "embedide-mark.svg")], capture_output=True, check=True).stdout
    return Image.open(io.BytesIO(png)).convert("RGBA")


def ease(x):
    x = max(0.0, min(1.0, x))
    return 1 - (1 - x) ** 3


@dataclass
class Layout:
    w: int
    h: int
    cols: int
    rows: int
    cw: int
    ch: int
    fs: int
    x0: int
    y0: int          # terminal top-left
    cap_y: int       # captions block top
    cap_w: int       # caption max pixel width
    chip_y: int


LANDSCAPE = Layout(1920, 1080, 118, 31, 14, 28, 23, (1920 - 118 * 14) // 2, 62, 954, 1500, 20)
VERTICAL = Layout(1080, 1920, 72, 46, 14, 28, 23, (1080 - 72 * 14) // 2, 210, 1560, 960, 70)


class Scene:
    narration = ""
    duration = 3.0
    kind = "card"         # "card" (graphic), "anim" (animation), "real" (recording)
    note = ""

    def frame(self, t, L):  # -> PIL RGB
        raise NotImplementedError

    def key(self, t):
        return None


def base(L):
    return Image.new("RGB", (L.w, L.h), B.OBSIDIAN)


def chip(img, L, text, accent, right=True, inside=None):
    """A label chip. `inside=(x_left, x_right, y)` puts a small one in a window title bar."""
    d = ImageDraw.Draw(img)
    if inside:
        xl, xr, y = inside
        f = font(B.FONT_MONO_BOLD, 16)
        w = d.textlength(text, font=f) + 24
        x = xr - w - 10 if right else xl + 10
        d.rounded_rectangle([x, y, x + w, y + 28], radius=7, fill=B.GRAPHITE2, outline=accent, width=2)
        d.text((x + 12, y + 4), text, font=f, fill=accent)
        return
    f = font(B.FONT_MONO_BOLD, 20 if L.w > 1500 else 22)
    w = d.textlength(text, font=f) + 36
    x = L.w - w - 40 if right else 40
    y = L.chip_y
    d.rounded_rectangle([x, y, x + w, y + 38], radius=8, fill=B.GRAPHITE2, outline=accent, width=2)
    d.text((x + 18, y + 7), text, font=f, fill=accent)


class Card(Scene):
    """Title/graphic card: optional logo, headline, sub lines, bullet list, code block."""

    def __init__(self, headline="", sub="", bullets=(), code=(), logo_on=False, narration="", min_dur=3.0,
                 accent_line=True, kind="card", note="", big=False):
        self.headline, self.sub, self.bullets, self.code = headline, sub, list(bullets), list(code)
        self.logo_on, self.narration, self.min_dur, self.accent_line = logo_on, narration, min_dur, accent_line
        self.kind, self.note, self.big = kind, note, big
        self.duration = min_dur
        self._logo = {}

    def key(self, t):
        return round(t * 30)

    def frame(self, t, L):
        img = base(L)
        d = ImageDraw.Draw(img)
        vertical = L.h > L.w
        cx = L.w // 2
        y = int(L.h * (0.30 if not self.code and not self.bullets else 0.22))
        if self.logo_on:
            lw = int(L.w * (0.62 if vertical else 0.34))
            if lw not in self._logo:
                self._logo[lw] = logo(lw)
            lg = self._logo[lw]
            a = ease(t / 0.8)
            tmp = lg.copy()
            tmp.putalpha(tmp.getchannel("A").point(lambda v: int(v * a)))
            img.paste(tmp, (cx - lg.width // 2, y - int((1 - a) * 18)), tmp)
            y += lg.height + 40
        if self.headline:
            size = (70 if self.big else 56) if not vertical else 60
            f = font(B.FONT_SANS_BOLD, size)
            lines = textwrap.wrap(self.headline, 26 if vertical else 40)
            a = ease((t - 0.15) / 0.7)
            for ln in lines:
                tw = d.textlength(ln, font=f)
                col = tuple(int(c * a + o * (1 - a)) for c, o in zip(_hex(B.WARM_WHITE), _hex(B.OBSIDIAN)))
                d.text((cx - tw / 2, y + (1 - a) * 14), ln, font=f, fill=col)
                y += int(size * 1.25)
            if self.accent_line:
                lw = int(180 * ease((t - 0.3) / 0.7))
                d.rectangle([cx - lw // 2, y + 10, cx + lw // 2, y + 14], fill=B.COPPER)
            y += 44
        if self.sub:
            f = font(B.FONT_SANS, 32 if not vertical else 34)
            a = ease((t - 0.5) / 0.7)
            for ln in textwrap.wrap(self.sub, 56 if not vertical else 34):
                tw = d.textlength(ln, font=f)
                col = tuple(int(c * a + o * (1 - a)) for c, o in zip(_hex(B.BONE300), _hex(B.OBSIDIAN)))
                d.text((cx - tw / 2, y), ln, font=f, fill=col)
                y += 46
            y += 20
        if self.bullets:
            f = font(B.FONT_SANS, 36 if not vertical else 36)
            x = int(L.w * (0.22 if not vertical else 0.10))
            for i, b in enumerate(self.bullets):
                a = ease((t - 0.7 - i * 0.35) / 0.5)
                col = tuple(int(c * a + o * (1 - a)) for c, o in zip(_hex(B.WARM_WHITE), _hex(B.OBSIDIAN)))
                d.rectangle([x, y + 14, x + 12, y + 26], fill=B.COPPER)
                for k, ln in enumerate(textwrap.wrap(b, 52 if not vertical else 30)):
                    d.text((x + 34 - (1 - a) * 20, y + k * 46), ln, font=f, fill=col)
                y += 66 + 46 * (len(textwrap.wrap(b, 52 if not vertical else 30)) - 1)
        if self.code:
            f = font(B.FONT_MONO, 30 if not vertical else 26)
            bw = int(L.w * (0.70 if not vertical else 0.90))
            bh = 56 + 44 * len(self.code)
            x = (L.w - bw) // 2
            a = ease((t - 0.6) / 0.6)
            d.rounded_rectangle([x, y, x + bw, y + bh], radius=14, fill=B.GRAPHITE, outline=B.GRAPHITE3, width=2)
            for i, ln in enumerate(self.code):
                col = B.COPPER_LIGHT if ln.startswith("$") else B.WARM_WHITE
                shown = ln[: int(len(ln) * min(1.0, max(0.0, (t - 0.8 - i * 0.5) / 0.9)))] if a > 0 else ""
                d.text((x + 28, y + 28 + i * 44), shown, font=f, fill=col)
        if self.note:
            f = font(B.FONT_MONO, 22)
            tw = d.textlength(self.note, font=f)
            d.text((cx - tw / 2, int(L.h * (0.80 if L.w > L.h else 0.86))), self.note, font=f, fill=B.MUTED)
        if self.kind == "anim":
            chip(img, L, i18n.chip("ANIMATION · not application footage"), B.COPPER_LIGHT)
        return img


def _hex(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


class Words(Scene):
    """Kinetic typography: words pop in one by one (graphic, not application footage)."""

    def __init__(self, words, closing="", narration="", min_dur=5.0, step=0.75):
        self.words, self.closing, self.narration, self.duration, self.step = list(words), closing, narration, min_dur, step

    def key(self, t):
        return round(t * 30)

    def frame(self, t, L):
        img = base(L)
        d = ImageDraw.Draw(img)
        vertical = L.h > L.w
        size = 120 if not vertical else 130
        f = font(B.FONT_SANS_BOLD, size)
        y = int(L.h * (0.26 if not vertical else 0.28))
        for i, w in enumerate(self.words):
            a = ease((t - 0.3 - i * self.step) / 0.45)
            if a <= 0:
                continue
            tw = d.textlength(w, font=f)
            col = tuple(int(c * a + o * (1 - a)) for c, o in zip(_hex(B.COPPER_LIGHT if i == len(self.words) - 1 else B.WARM_WHITE), _hex(B.OBSIDIAN)))
            d.text(((L.w - tw) / 2, y + (1 - a) * 40), w, font=f, fill=col)
            y += int(size * 1.15)
        a = ease((t - 0.5 - len(self.words) * self.step) / 0.6)
        if self.closing and a > 0:
            f2 = font(B.FONT_SANS, 40 if not vertical else 44)
            tw = d.textlength(self.closing, font=f2)
            col = tuple(int(c * a) for c in _hex(B.BONE300))
            d.text(((L.w - tw) / 2, y + 40), self.closing, font=f2, fill=col)
            lw = int(240 * a)
            d.rectangle([L.w / 2 - lw / 2, y + 20, L.w / 2 + lw / 2, y + 25], fill=B.COPPER)
        return img


class Stats(Scene):
    """Animated counters (facts taken from the repository: crates, tests, binary size, unsafe blocks)."""

    def __init__(self, items, headline="", narration="", min_dur=9.0):
        self.items, self.headline, self.narration, self.duration = list(items), headline, narration, min_dur

    def key(self, t):
        return round(t * 30)

    def frame(self, t, L):
        img = base(L)
        d = ImageDraw.Draw(img)
        vertical = L.h > L.w
        if self.headline:
            f = font(B.FONT_SANS_BOLD, 50)
            tw = d.textlength(self.headline, font=f)
            d.text(((L.w - tw) / 2, 110), self.headline, font=f, fill=B.WARM_WHITE)
        n = len(self.items)
        cols = 1 if vertical else min(n, 4)
        rows_ = (n + cols - 1) // cols
        cw_, ch_ = (L.w - 160) // cols, 300
        top = int(L.h * 0.30) if not vertical else 420
        for i, (val, label, suffix) in enumerate(self.items):
            r, c = divmod(i, cols)
            x = 80 + c * cw_
            y = top + r * (ch_ + 30)
            a = ease((t - 0.4 - i * 0.35) / 0.5)
            if a <= 0:
                continue
            cur = int(round(val * ease((t - 0.5 - i * 0.35) / 1.6)))
            fnum = font(B.FONT_SANS_BOLD, 130)
            txt = f"{cur}{suffix}"
            tw = d.textlength(txt, font=fnum)
            col = tuple(int(cc * a + o * (1 - a)) for cc, o in zip(_hex(B.COPPER_LIGHT), _hex(B.OBSIDIAN)))
            d.text((x + (cw_ - tw) / 2, y), txt, font=fnum, fill=col)
            fl = font(B.FONT_SANS, 32)
            lines = textwrap.wrap(label, 24)
            for k, ln in enumerate(lines):
                lw_ = d.textlength(ln, font=fl)
                colw = tuple(int(cc * a) for cc in _hex(B.BONE300))
                d.text((x + (cw_ - lw_) / 2, y + 170 + k * 40), ln, font=fl, fill=colw)
        return img


class Diagram(Scene):
    """Animated architecture diagram (motion graphics, labelled as animation)."""

    kind = "anim"

    def __init__(self, narration="", min_dur=9.0):
        self.narration, self.duration = narration, min_dur
        self.min_dur = min_dur

    def key(self, t):
        return round(t * 30)

    def frame(self, t, L):
        img = base(L)
        d = ImageDraw.Draw(img)
        f = font(B.FONT_SANS_BOLD, 38)
        fs = font(B.FONT_SANS, 25)
        fm = font(B.FONT_MONO_BOLD, 29)
        W = L.w
        title = i18n.tr("Nine crates. One engine. Thin frontends.", "Девять крейтов. Один движок. Тонкие интерфейсы.")
        t_ = i18n.tr
        d.text((W // 2 - d.textlength(title, font=f) / 2, 70), title, font=f, fill=B.WARM_WHITE)

        def mix(c, a):
            return tuple(int(x * a + o * (1 - a)) for x, o in zip(_hex(c), _hex(B.OBSIDIAN)))

        def box(x, y, w, h, name, lines, t0, accent=False, planned=False):
            a = ease((t - t0) / 0.6)
            if a <= 0:
                return
            off = int((1 - a) * 22)
            edge = mix(B.COPPER if accent else (B.MUTED if planned else B.GRAPHITE3), a)
            d.rounded_rectangle([x, y + off, x + w, y + h + off], radius=12, fill=mix(B.GRAPHITE2, a), outline=edge,
                                width=3)
            d.text((x + 24, y + 16 + off), name, font=fm, fill=mix(B.MUTED if planned else B.WARM_WHITE, a))
            for i, ln in enumerate(lines):
                d.text((x + 24, y + 64 + i * 34 + off), ln, font=fs, fill=mix(B.BONE300, a))

        cw, gap, bh = 480, 40, 142
        x1 = (W - (3 * cw + 2 * gap)) // 2
        xs = [x1, x1 + cw + gap, x1 + 2 * (cw + gap)]
        y1, y2, y3 = 160, 430, 700
        d.text((xs[0], y1 - 38), t_("frontends", "интерфейсы"), font=fs, fill=B.MUTED)
        box(xs[0], y1, cw, bh, "embedide-terminal", [t_("TUI: panels, keys, theme, PTY", "TUI: панели, клавиши, тема, PTY")], 0.4)
        box(xs[1], y1, cw, bh, "embedide-cli", [t_("binary `embed` · subcommands", "бинарник `embed` · подкоманды")], 0.8, True)
        box(xs[2], y1, cw, bh, t_("native GUI", "нативный GUI"), [t_("planned — not started", "запланирован — не начат")], 1.2, planned=True)
        d.text((xs[0], y2 - 38), t_("engines", "движки"), font=fs, fill=B.MUTED)
        box(xs[0], y2, cw, bh, "embedide-editor", [t_("piece table · Vim input · syntax", "кусочная таблица · Vim · синтаксис"), t_("no I/O, no terminal types", "без ввода-вывода и типов терминала")], 1.9)
        box(xs[1], y2, cw, bh, "embedide-build", [t_("plan, then run · no shell · cancel", "план, затем запуск · без shell · отмена"), "Make · CMake · Cargo · Zig"], 2.5)
        box(xs[2], y2, cw, bh, "embedide-ai", [t_("agents · permission policy · skills", "агенты · политика прав · навыки"), "Claude Code · Codex CLI"], 3.1, True)
        d.text((xs[0], y3 - 38), t_("services", "сервисы"), font=fs, fill=B.MUTED)
        box(xs[0], y3, cw, bh, "embedide-core", [t_("processes · LSP · ELF/HEX · serial", "процессы · LSP · ELF/HEX · serial")], 3.8)
        box(xs[1], y3, cw, bh, "embedide-toolchains", [t_("detects compilers", "находит компиляторы"), t_("installs nothing", "ничего не устанавливает")], 4.2)
        box(xs[2], y3, cw, bh, "embedide-workspace", ["embedide.toml · config.toml", t_("atomic writes", "атомарная запись")], 4.6)
        for t0, ya, yb in ((1.5, y1 + bh, y2), (3.4, y2 + bh, y3)):
            a = ease((t - t0) / 0.8)
            if a > 0:
                col = mix(B.COPPER, a)
                mid = (ya + yb) // 2
                d.line([xs[0] + cw // 2, ya, xs[0] + cw // 2, yb], fill=col, width=3)
                d.line([xs[1] + cw // 2, ya, xs[1] + cw // 2, yb], fill=col, width=3)
                d.line([xs[2] + cw // 2, ya, xs[2] + cw // 2, yb], fill=col, width=3)
        a = ease((t - 5.4) / 0.8)
        if a > 0:
            msg = t_("No unsafe code  ·  processes launched with argument vectors  ·  atomic file writes", "Без unsafe  ·  процессы запускаются с явными аргументами  ·  атомарная запись файлов")
            d.text((W // 2 - d.textlength(msg, font=fs) / 2, 930), msg, font=fs, fill=mix(B.BONE300, a))
        chip(img, L, i18n.chip("ANIMATION · not application footage"), B.COPPER_LIGHT)
        return img


class Clip(Scene):
    """A segment of a real recording. segments = [(src_start, src_end, speed)]."""

    kind = "real"

    def __init__(self, rec, segments, title="embed", narration="", labels=(), min_dur=0.0, freeze_tail=0.0,
                 timelapse_note=None, focus=()):
        self.rec = rec if isinstance(rec, dict) else T.load(rec)
        self.segments = segments
        self.title, self.narration, self.labels = title, narration, list(labels)
        self.freeze_tail = freeze_tail
        self.clip_dur = sum((b - a) / s for a, b, s in segments) + freeze_tail
        self.duration = max(self.clip_dur, min_dur)
        self.times = [f[0] for f in self.rec["frames"]]
        self._r = {}
        self.timelapse_note = timelapse_note
        self.focus = list(focus)     # (t0, t1, (col0, row0, col1, row1)) zoom targets in terminal cells

    def fit_to(self, target, max_slow=2.4):
        """Retime the recording so it lasts about `target` s (slow motion up to `max_slow`x) instead of freezing
        on the last frame while the narration talks on. Zoom windows and labels are scaled with it."""
        base = sum((b - a) / sp for a, b, sp in self.segments)
        if getattr(self, "_fitted", False) or target <= base + 0.3:
            return
        f = min(max_slow, target / base)
        self._fitted = True
        self.segments = [(a, b, sp / f) for a, b, sp in self.segments]
        self.focus = [(a * f, b * f, box) for a, b, box in self.focus]
        self.labels = [(a * f, b * f, *rest) for a, b, *rest in self.labels]
        self.clip_dur = base * f + self.freeze_tail
        self.duration = max(self.duration, self.clip_dur)

    def src_time(self, t):
        acc = 0.0
        for a, b, s in self.segments:
            seg = (b - a) / s
            if t < acc + seg:
                return a + (t - acc) * s, s
            acc += seg
        return self.segments[-1][1], 1.0

    def _frame_idx(self, ts):
        return max(0, bisect.bisect_right(self.times, ts) - 1)

    def zoom_state(self, t):
        """(progress 0..1, cell box) of the active focus, with 0.7 s ease in and out."""
        for a, b, box in self.focus:
            if a - 0.01 <= t < b:
                p = min(ease((t - a) / 0.7), ease((b - t) / 0.7))
                return p, box
        return 0.0, None

    def key(self, t):
        ts, s = self.src_time(t)
        p, box = self.zoom_state(t)
        return (self._frame_idx(ts), s, round(t * 2), round(p * 30) if p > 0 else 0, box)

    def frame(self, t, L):
        p, box = self.zoom_state(t)
        if p > 0.001:
            return self._zoomed(t, L, p, box)
        return self._plain(t, L)

    def _zoomed(self, t, L, p, box):
        """Render the terminal at 2x, crop around `box` (eased by p) and add screen-space chips."""
        ts, speed = self.src_time(t)
        _, rows, cur = self.rec["frames"][self._frame_idx(ts)]
        S = 2
        key = ("hi", L.cw)
        r = self._r.get(key)
        if r is None:
            r = self._r[key] = T.TermRenderer(L.cw * S, L.ch * S, L.fs * S)
        W, H = L.w, L.h
        hi = Image.new("RGB", (W * S, H * S), B.OBSIDIAN)
        d = ImageDraw.Draw(hi)
        pad = 14
        tw, th = self.rec["cols"] * L.cw, self.rec["rows"] * L.ch
        d.rounded_rectangle([(L.x0 - pad) * S, (L.y0 - 46) * S, (L.x0 + tw + pad) * S, (L.y0 + th + pad) * S],
                            radius=14 * S, outline=B.GRAPHITE3, width=2 * S)
        r.draw(hi, L.x0 * S, L.y0 * S, rows, cur)
        c0, r0, c1, r1 = box
        bx0, by0 = L.x0 + c0 * L.cw - 10, L.y0 + r0 * L.ch - 6
        bx1, by1 = L.x0 + c1 * L.cw + 10, L.y0 + r1 * L.ch + 6
        d.rounded_rectangle([bx0 * S, by0 * S, bx1 * S, by1 * S], radius=10 * S, outline=B.COPPER, width=3 * S)
        # target crop: the box with margin, in the frame's aspect ratio, never zoomed past 2.4x
        aspect = W / H
        cw_ = max((bx1 - bx0) * 1.18, (by1 - by0) * 1.18 * aspect, W / 2.4)
        ch_ = cw_ / aspect
        cx, cy = (bx0 + bx1) / 2, (by0 + by1) / 2
        tx0 = min(max(cx - cw_ / 2, 0), W - cw_)
        ty0 = min(max(cy - ch_ / 2, 0), H - ch_)
        x0 = tx0 * p
        y0 = ty0 * p
        x1 = W + (tx0 + cw_ - W) * p
        y1 = H + (ty0 + ch_ - H) * p
        crop = hi.crop((int(x0 * S), int(y0 * S), int(x1 * S), int(y1 * S))).resize((W, H), Image.LANCZOS)
        chip(crop, L, i18n.chip("REAL RECORDING · embed 0.1.0-alpha.1"), B.WARM_WHITE, right=True)
        if speed > 1.05:
            chip(crop, L, i18n.chip("TIME-LAPSE ×{x}", x=f"{speed:g}"), B.COPPER_LIGHT, right=False)
        for (a, b, text) in self.labels:
            if a <= t < b:
                dd = ImageDraw.Draw(crop)
                fc = font(B.FONT_SANS_BOLD, 28)
                w = dd.textlength(text, font=fc) + 40
                dd.rounded_rectangle([W / 2 - w / 2, 22, W / 2 + w / 2, 70], radius=10, fill=B.COPPER)
                dd.text((W / 2 - w / 2 + 20, 27), text, font=fc, fill=B.OBSIDIAN)
        return crop

    def _plain(self, t, L):
        ts, speed = self.src_time(t)
        _, rows, cur = self.rec["frames"][self._frame_idx(ts)]
        r = self._r.get(L.cw)
        if r is None:
            r = self._r[L.cw] = T.TermRenderer(L.cw, L.ch, L.fs)
        img = base(L)
        d = ImageDraw.Draw(img)
        pad = 14
        tw, th = self.rec["cols"] * L.cw, self.rec["rows"] * L.ch
        d.rounded_rectangle([L.x0 - pad, L.y0 - 46, L.x0 + tw + pad, L.y0 + th + pad], radius=14, fill=B.OBSIDIAN,
                            outline=B.GRAPHITE3, width=2)
        d.rounded_rectangle([L.x0 - pad, L.y0 - 46, L.x0 + tw + pad, L.y0 - 8], radius=14, fill=B.GRAPHITE)
        for i, c in enumerate(("#E5534B", "#D9A441", "#57AB5A")):
            d.ellipse([L.x0 + 6 + i * 26, L.y0 - 36, L.x0 + 22 + i * 26, L.y0 - 20], fill=c)
        f = font(B.FONT_MONO, 19)
        active = [x for x in self.labels if x[0] <= t < x[1]]
        if active:   # a callout replaces the window title while it is active
            fc = font(B.FONT_SANS_BOLD, 22)
            text = active[0][2]
            w = d.textlength(text, font=fc) + 36
            cx = L.x0 + tw // 2
            d.rounded_rectangle([cx - w / 2, L.y0 - 41, cx + w / 2, L.y0 - 11], radius=8, fill=B.COPPER)
            d.text((cx - w / 2 + 18, L.y0 - 41), text, font=fc, fill=B.OBSIDIAN)
        else:
            d.text((L.x0 + tw // 2 - d.textlength(self.title, font=f) / 2, L.y0 - 37), self.title, font=f, fill=B.MUTED)
        r.draw(img, L.x0, L.y0, rows, cur)
        if L.h > L.w:   # vertical: official wordmark above the window
            lg = self._logo_small if hasattr(self, "_logo_small") else None
            if lg is None:
                lg = self._logo_small = logo(420)
            img.paste(lg, ((L.w - lg.width) // 2, 60), lg)
        bar = (L.x0 - pad, L.x0 + tw + pad, L.y0 - 41)
        chip(img, L, i18n.chip("REAL RECORDING · embed 0.1.0-alpha.1"), B.WARM_WHITE, right=True, inside=bar)
        if speed > 1.05:
            chip(img, L, i18n.chip("TIME-LAPSE ×{x}", x=f"{speed:g}"), B.COPPER_LIGHT, right=False, inside=(bar[0] + 90, bar[1], bar[2]))
        return img


def _callout(img, L, text):
    d = ImageDraw.Draw(img)
    f = font(B.FONT_SANS_BOLD, 28)
    w = d.textlength(text, font=f) + 44
    x = L.w // 2 - w / 2
    y = L.cap_y - 70
    d.rounded_rectangle([x, y, x + w, y + 52], radius=10, fill=B.COPPER)
    d.text((x + 22, y + 8), text, font=f, fill=B.OBSIDIAN)


# ---------------------------------------------------------------- captions + film

def wrap_caption(text, width_chars):
    return textwrap.wrap(text, width_chars)


@dataclass
class Placed:
    scene: Scene
    start: float
    dur: float
    xf: float


class Film:
    def __init__(self, L=LANDSCAPE, fps=30, lang="en"):
        self.L, self.fps, self.lang = L, fps, lang
        self.scenes = []
        self.captions = []   # (t0, t1, text)

    def add(self, scene, xf=0.5, wipe=False):
        scene.wipe = wipe   # the transition INTO this scene
        self.scenes.append((scene, xf))
        return scene

    def layout(self):
        placed, t = [], 0.0
        for i, (s, xf) in enumerate(self.scenes):
            nxt = self.scenes[i + 1][1] if i + 1 < len(self.scenes) else 0.0
            placed.append(Placed(s, t, s.duration, xf))
            t += s.duration - (self.scenes[i + 1][1] if i + 1 < len(self.scenes) else 0.0)
        self.total = placed[-1].start + placed[-1].dur
        self.placed = placed
        return placed

    def _active(self, t):
        out = []
        for p in self.placed:
            if p.start <= t < p.start + p.dur:
                out.append(p)
        return out

    def _caption_at(self, t):
        for a, b, text in self.captions:
            if a <= t < b:
                return text
        return None

    def render_frame(self, t, cache):
        L = self.L
        act = self._active(t)
        if not act:
            act = [self.placed[-1]]
        imgs = []
        for p in act:
            lt = min(t - p.start, p.dur - 1e-3)
            k = (id(p.scene), p.scene.key(lt))
            if k not in cache:
                cache[k] = np.asarray(p.scene.frame(lt, L), dtype=np.uint8)
                if len(cache) > 8:
                    for old in list(cache)[:3]:
                        cache.pop(old, None)
            imgs.append((p, cache[k]))
        if len(imgs) == 2:
            (p0, a0), (p1, a1) = imgs
            ov = p0.start + p0.dur - p1.start
            alpha = max(0.0, min(1.0, (t - p1.start) / max(ov, 1e-3)))
            if getattr(p1.scene, "wipe", False):
                H_, W_ = a0.shape[:2]
                edge = int(ease(alpha) * (W_ + 80)) - 40
                xs = np.arange(W_)[None, :, None]
                m = np.clip((edge - xs) / 40.0 + 0.5, 0, 1)
                arr = (a0 * (1 - m) + a1 * m).astype(np.uint8)
                if 0 < edge < W_:
                    arr[:, max(0, edge - 3):min(W_, edge + 3)] = _hex(B.COPPER)
            else:
                arr = (a0 * (1 - alpha) + a1 * alpha).astype(np.uint8)
        else:
            arr = imgs[0][1]
        cap = self._caption_at(t)
        if cap:
            arr = self._with_caption(arr, cap)
        # fade in/out of whole film
        if t < 0.5:
            arr = (arr * (t / 0.5)).astype(np.uint8)
        elif t > self.total - 0.7:
            arr = (arr * max(0.0, (self.total - t) / 0.7)).astype(np.uint8)
        return arr

    _capcache = {}

    def _with_caption(self, arr, text):
        key = (text, self.L.w)
        ov = self._capcache.get(key)
        if ov is None:
            L = self.L
            f = font(B.FONT_SANS_BOLD, 38 if L.w > 1500 else 40)
            lines = textwrap.wrap(text, 78 if L.w > 1500 else 34)[:3]
            h = 20 + 52 * len(lines)
            img = Image.new("RGBA", (L.w, L.h), (0, 0, 0, 0))
            d = ImageDraw.Draw(img)
            wmax = max(d.textlength(l, font=f) for l in lines) + 56
            x = (L.w - wmax) / 2
            y = L.cap_y + (110 - h) / 2 if L.w > 1500 else L.cap_y
            d.rounded_rectangle([x, y, x + wmax, y + h], radius=14, fill=(11, 11, 12, 215))
            for i, l in enumerate(lines):
                tw = d.textlength(l, font=f)
                d.text(((L.w - tw) / 2, y + 12 + i * 52), l, font=f, fill=B.WARM_WHITE)
            a = np.asarray(img, dtype=np.float32)
            ov = (a[..., :3], a[..., 3:4] / 255.0)
            self._capcache[key] = ov
        rgb, al = ov
        return (arr * (1 - al) + rgb * al).astype(np.uint8)

    def render_video(self, out_path, crf=18, progress=True):
        L = self.L
        cmd = ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{L.w}x{L.h}",
               "-r", str(self.fps), "-i", "-", "-c:v", "libx264", "-crf", str(crf), "-preset", "medium",
               "-pix_fmt", "yuv420p", "-r", str(self.fps), "-movflags", "+faststart", str(out_path)]
        p = subprocess.Popen(cmd, stdin=subprocess.PIPE)
        n = int(math.ceil(self.total * self.fps))
        cache, last_key, last_bytes = {}, None, None
        for i in range(n):
            t = i / self.fps
            act = self._active(t) or [self.placed[-1]]
            keys = tuple((id(a.scene), a.scene.key(min(t - a.start, a.dur - 1e-3))) for a in act)
            fade = t < 0.5 or t > self.total - 0.7
            xfade = len(act) == 2
            k = (keys, self._caption_at(t), None if not (fade or xfade) else i)
            if k != last_key:
                last_bytes = self.render_frame(t, cache).tobytes()
                last_key = k
            p.stdin.write(last_bytes)
            if progress and i % 300 == 0:
                print(f"  frame {i}/{n}", flush=True)
        p.stdin.close()
        p.wait()
        return n

    def write_subs(self, srt_path, vtt_path):
        def ts(t, sep):
            h, r = divmod(t, 3600)
            m, s = divmod(r, 60)
            return f"{int(h):02d}:{int(m):02d}:{int(s):02d}{sep}{int((s % 1) * 1000):03d}"
        srt, vtt = [], ["WEBVTT\n"]
        for i, (a, b, text) in enumerate(self.captions, 1):
            srt.append(f"{i}\n{ts(a, ',')} --> {ts(b, ',')}\n{text}\n")
            vtt.append(f"{ts(a, '.')} --> {ts(b, '.')}\n{text}\n")
        Path(srt_path).write_text("\n".join(srt))
        Path(vtt_path).write_text("\n".join(vtt))
