"""Record a real program in a pseudo-terminal and render its screen to images.

Everything shown comes from the real binary: the PTY output is fed to a terminal emulator (pyte),
every screen change is stored with its real timestamp, and the renderer draws those stored screens.
"""
import fcntl, os, pickle, pty, select, struct, subprocess, termios, time
from pathlib import Path

import pyte
from PIL import Image, ImageDraw, ImageFont

from . import brand as B

NAMES = {
    "black": "#1C1C20", "red": "#E5534B", "green": "#57AB5A", "brown": "#D9A441", "blue": "#539BF5",
    "magenta": "#B083F0", "cyan": "#4FC1D8", "white": "#D6CFC4",
    "brightblack": "#6E6E76", "brightred": "#FF7B72", "brightgreen": "#7EE787", "brightbrown": "#E3B341",
    "brightblue": "#79C0FF", "brightmagenta": "#D2A8FF", "brightcyan": "#56D4DD", "brightwhite": "#F4F1EC",
}


def colour(c, default):
    if c == "default":
        return default
    if c in NAMES:
        return NAMES[c]
    if len(c) == 6:
        return "#" + c
    return default


class Recorder:
    """Spawn `argv` in a PTY of rows x cols and record every screen change with its timestamp."""

    def __init__(self, argv, cwd, env, rows, cols):
        self.rows, self.cols = rows, cols
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.t0 = time.monotonic()
        self.proc = subprocess.Popen(list(map(str, argv)), cwd=cwd, env=env, stdin=slave, stdout=slave,
                                     stderr=slave, start_new_session=True)
        os.close(slave)
        self.raw = bytearray()
        self.answered = 0
        self.frames = []            # (t, rows, cursor)
        self._rows = [None] * rows  # cached row tuples
        self.events = []            # (t, name) markers placed by the scenario script
        self.screen.dirty.update(range(rows))
        self._snap()

    def now(self):
        return time.monotonic() - self.t0

    def mark(self, name):
        self.events.append((self.now(), name))

    def _snap(self):
        s = self.screen
        for y in s.dirty:
            line = s.buffer[y]
            self._rows[y] = tuple(
                (c.data, c.fg, c.bg, c.bold, c.italics, c.reverse) for c in (line[x] for x in range(self.cols)))
        s.dirty.clear()
        cur = (s.cursor.x, s.cursor.y, s.cursor.hidden)
        if not self.frames or self.frames[-1][1] != self._rows or self.frames[-1][2] != cur:
            self.frames.append((self.now(), tuple(self._rows), cur))

    def pump(self, seconds=0.2):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            r, _, _ = select.select([self.master], [], [], 0.02)
            if r:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    return
                if not data:
                    return
                self.raw += data
                self.stream.feed(data)
                while self.raw.count(b"\x1b[?u") > self.answered:      # kitty keyboard query
                    self.answered += 1
                    os.write(self.master, b"\x1b[?0u\x1b[?62;22c")
                self._snap()
            else:
                self._snap()

    def keys(self, *chunks, pause=0.06):
        for c in chunks:
            os.write(self.master, c if isinstance(c, bytes) else c.encode())
            self.pump(pause)

    def type(self, text, pause=0.055, jitter=True):
        for i, ch in enumerate(text):
            self.keys(ch, pause=pause * (1.6 if jitter and ch in " ,.(" else 1.0))

    def text(self):
        return "\n".join(self.screen.display)

    def until(self, needle, timeout=40):
        """Wait until `needle` is on screen (whitespace-insensitive: panels wrap text)."""
        flat = " ".join(needle.split())
        end = time.monotonic() + timeout
        while flat not in " ".join(self.text().split()):
            if time.monotonic() > end:
                raise RuntimeError(f"timeout waiting for {needle!r}; screen:\n{self.text()}")
            self.pump(0.1)

    def close(self):
        try:
            os.killpg(self.proc.pid, 9)
        except OSError:
            pass

    def save(self, path):
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with open(path, "wb") as f:
            pickle.dump({"rows": self.rows, "cols": self.cols, "frames": self.frames, "events": self.events}, f)


def load(path):
    with open(path, "rb") as f:
        return pickle.load(f)


class TermRenderer:
    """Draw recorded screens with cell size (cw, ch); rows are cached by content."""

    def __init__(self, cw, ch, font_size):
        self.cw, self.ch = cw, ch
        self.f = ImageFont.truetype(B.FONT_MONO, font_size)
        self.fb = ImageFont.truetype(B.FONT_MONO_BOLD, font_size)
        self.fi = ImageFont.truetype(B.FONT_MONO_IT, font_size)
        # DejaVu Sans Mono has no Braille patterns (the spinner); the proportional DejaVu Sans does.
        self.fbr = ImageFont.truetype("/usr/share/fonts/TTF/DejaVuSans.ttf", font_size)
        self.cache = {}

    def row(self, row):
        img = self.cache.get(row)
        if img is not None:
            return img
        cols = len(row)
        img = Image.new("RGB", (cols * self.cw, self.ch), B.OBSIDIAN)
        d = ImageDraw.Draw(img)
        for x, (data, fg, bg, bold, ital, rev) in enumerate(row):
            f_, b_ = colour(fg, B.WARM_WHITE), colour(bg, B.OBSIDIAN)
            if rev:
                f_, b_ = b_, f_
            px = x * self.cw
            if b_ != B.OBSIDIAN:
                d.rectangle([px, 0, px + self.cw - 1, self.ch - 1], fill=b_)
            if data.strip():
                if "\u2800" <= data <= "\u28ff":
                    d.text((px + 1, 1), data, font=self.fbr, fill=f_)
                else:
                    d.text((px, 1), data, font=self.fb if bold else self.fi if ital else self.f, fill=f_)
        if len(self.cache) > 3000:
            self.cache.clear()
        self.cache[row] = img
        return img

    def draw(self, canvas, x0, y0, rows, cursor, show_cursor=True):
        for y, row in enumerate(rows):
            canvas.paste(self.row(row), (x0, y0 + y * self.ch))
        if show_cursor and not cursor[2]:
            cx, cy, _ = cursor
            d = ImageDraw.Draw(canvas)
            d.rectangle([x0 + cx * self.cw, y0 + cy * self.ch, x0 + (cx + 1) * self.cw - 1,
                         y0 + (cy + 1) * self.ch - 1], outline=B.COPPER, width=2)
