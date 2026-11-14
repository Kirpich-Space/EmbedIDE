#!/usr/bin/env python3
"""Real screenshots of `embed` for the README and launch material.

Runs the real binary in a pseudo-terminal, types keys into it, feeds its output to a terminal emulator
(pyte) and draws the resulting screen to PNG with Pillow. Nothing is mocked or composited: every
picture is what the program drew. Needs `pip install pyte pillow` (not project dependencies).

  python3 scripts/render-screenshots.py --binary target/release/embed --out docs/img
  python3 scripts/render-screenshots.py --binary ... --out docs/img --only editor,hex
  python3 scripts/render-screenshots.py --binary ... --out docs/img --only agent --live-codex
"""
import argparse, fcntl, os, pty, select, shutil, struct, subprocess, sys, tempfile, termios, time
from pathlib import Path

import pyte
from PIL import Image, ImageDraw, ImageFont

COLS, ROWS = 118, 34
BG, FG = "#0B0B0C", "#F4F1EC"  # carbon-950 / bone-100 from Desige/embedide-palette.json
NAMES = {  # pyte colour names -> a Carbon-friendly ANSI palette
    "black": "#1C1C20", "red": "#E5534B", "green": "#57AB5A", "brown": "#D9A441", "blue": "#539BF5",
    "magenta": "#B083F0", "cyan": "#4FC1D8", "white": "#D6CFC4",
    "brightblack": "#6E6E76", "brightred": "#FF7B72", "brightgreen": "#7EE787", "brightbrown": "#E3B341",
    "brightblue": "#79C0FF", "brightmagenta": "#D2A8FF", "brightcyan": "#56D4DD", "brightwhite": "#F4F1EC",
}
FONT = "/usr/share/fonts/TTF/DejaVuSansMono.ttf"
FONT_BOLD = "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf"
FONT_IT = "/usr/share/fonts/TTF/DejaVuSansMono-Oblique.ttf"


def colour(c, default):
    if c == "default":
        return default
    if c in NAMES:
        return NAMES[c]
    if len(c) == 6:
        return "#" + c
    return default


class Session:
    def __init__(self, binary, cwd, env, rows=ROWS, cols=COLS):
        self.rows, self.cols = rows, cols
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.proc = subprocess.Popen([str(binary)], cwd=cwd, env=env, stdin=slave, stdout=slave, stderr=slave,
                                     start_new_session=True)
        os.close(slave)
        self.raw = bytearray()
        self.answered = 0

    def pump(self, seconds=0.3):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            r, _, _ = select.select([self.master], [], [], 0.05)
            if r:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    return
                if not data:
                    return
                self.raw += data
                self.stream.feed(data)
                while self.raw.count(b"\x1b[?u") > self.answered:  # keyboard capability query
                    self.answered += 1
                    os.write(self.master, b"\x1b[?0u\x1b[?62;22c")

    def keys(self, *chunks, pause=0.05):
        for c in chunks:
            os.write(self.master, c if isinstance(c, bytes) else c.encode())
            self.pump(pause)

    def type(self, text, pause=0.02):
        for ch in text:
            self.keys(ch, pause=pause)

    def text(self):
        return "\n".join(self.screen.display)

    def until(self, needle, timeout=30):
        end = time.monotonic() + timeout
        while needle not in self.text():
            if time.monotonic() > end:
                raise SystemExit(f"timeout waiting for {needle!r}; screen:\n{self.text()}")
            self.pump(0.2)

    def close(self):
        try:
            os.killpg(self.proc.pid, 9)
        except OSError:
            pass

    def png(self, path, title=None):
        cw, ch = 10, 21
        f, fb, fi = (ImageFont.truetype(p, 16) for p in (FONT, FONT_BOLD, FONT_IT))
        pad = 18
        bar = 34
        img = Image.new("RGB", (self.cols * cw + 2 * pad, self.rows * ch + 2 * pad + bar), BG)
        d = ImageDraw.Draw(img)
        d.rectangle([0, 0, img.width, bar], fill="#141416")
        for i, c in enumerate(("#E5534B", "#D9A441", "#57AB5A")):
            d.ellipse([14 + i * 22, 11, 26 + i * 22, 23], fill=c)
        if title:
            d.text((img.width // 2 - len(title) * 4, 9), title, font=f, fill="#6E6E76")
        for y in range(self.rows):
            line = self.screen.buffer[y]
            for x in range(self.cols):
                cell = line[x]
                fg, bg = colour(cell.fg, FG), colour(cell.bg, BG)
                if cell.reverse:
                    fg, bg = bg, fg
                px, py = pad + x * cw, bar + pad + y * ch
                if bg != BG:
                    d.rectangle([px, py, px + cw, py + ch], fill=bg)
                if cell.data.strip():
                    font = fb if cell.bold else fi if cell.italics else f
                    d.text((px, py), cell.data, font=font, fill=fg)
        cx, cy = self.screen.cursor.x, self.screen.cursor.y
        if not self.screen.cursor.hidden:
            px, py = pad + cx * cw, bar + pad + cy * ch
            d.rectangle([px, py, px + cw, py + ch], outline="#C8562A")
        img.save(path)
        print("wrote", path)


def make_env(cfg):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("CLAUDE", "ANTHROPIC", "OPENAI", "CODEX", "DISPLAY", "WAYLAND"))}
    env.update(TERM="xterm-256color", COLORTERM="truecolor", SHELL="/bin/sh", EMBEDIDE_CONFIG_DIR=str(cfg),
               PS1="$ ", LANG="C.UTF-8")
    return env


def project(root):
    (root / "src").mkdir(parents=True)
    (root / "src" / "main.c").write_text(
        '#include <stdint.h>\n#include <stdio.h>\n\n'
        '/* Moving average over the last N samples. */\n'
        '#define WINDOW 4\n\n'
        'static uint32_t samples[WINDOW];\nstatic unsigned head;\n\n'
        'uint32_t average(uint32_t sample)\n{\n'
        '    samples[head++ % WINDOW] = sample;\n'
        '    uint32_t sum = 0;\n'
        '    for (unsigned i = 0; i < WINDOW; i++)\n'
        '        sum += samples[i];\n'
        '    return sum / WINDOW;\n}\n\n'
        'int main(void)\n{\n'
        '    for (uint32_t v = 100; v < 140; v += 10)\n'
        '        printf("%u\\n", (unsigned)average(v));\n'
        '    return 0;\n}\n')
    (root / "embedide.toml").write_text(
        'schema = 1\n[project]\nname = "sensor"\nlanguage = "c"\n[build]\nsystem = "native"\nsources = ["src"]\n'
        'flags = ["-Wall", "-Wextra"]\n[toolchain]\nc = "gcc"\n[profiles.debug]\nflags = ["-O0", "-g"]\n')


def shot_editor(binary, out, tmp):
    root = tmp / "sensor"
    project(root)
    cfg = tmp / "cfg-editor"
    cfg.mkdir()
    s = Session(binary, root, make_env(cfg))
    try:
        s.until("EmbedIDE")
        s.keys(b"\x1b[27u")                     # Esc: focus the editor
        s.type(":e src/main.c")
        s.keys(b"\r")
        s.until("average")
        s.keys(b"\x1b[15~")                     # F5: build
        s.until("Finished: exit", 40)
        s.type("12G")
        s.pump(0.5)
        s.png(out / "editor-build.png", "embed — sensor/ (C, GCC, native build)")
    finally:
        s.close()


def shot_hex(binary, out, tmp):
    root = tmp / "sensor-bin"
    project(root)
    subprocess.run([str(binary), "build", str(root)], check=True, capture_output=True, env=make_env(tmp / "cfg-hex"))
    elf = root / "build" / "debug" / "sensor"
    cfg = tmp / "cfg-hex"
    cfg.mkdir(exist_ok=True)
    s = Session(binary, root, make_env(cfg))
    try:
        s.until("EmbedIDE")
        s.keys(b"\x1b[27u")
        for hide in (":hide_explorer", ":hide_assistant"):   # give the binary view the whole width
            s.type(hide)
            s.keys(b"\r")
        s.type(":e build/debug/sensor")
        s.keys(b"\r")
        s.until("OFFSET")
        s.keys(b"\t")                           # ELF inspector
        s.pump(0.5)
        s.keys("4")                             # symbols
        s.pump(0.5)
        s.type("/average")
        s.keys(b"\r")
        s.pump(0.5)
        s.png(out / "elf-symbols.png", "embed — ELF inspector: symbols of the built program")
        s.keys(b"\r")                           # open in disassembly
        s.pump(1.5)
        s.png(out / "disassembly.png", "embed — disassembly of average() (objdump)")
    finally:
        s.close()


def shot_agent(binary, out, tmp, live_codex):
    if not live_codex:
        print("skipping agent screenshot (pass --live-codex to run one short real task)")
        return
    root = tmp / "agent"
    (root).mkdir()
    (root / "main.c").write_text('#include <stdio.h>\nint add(int a, int b);\nint main(void) { printf("%d\\n", add(2, 3)); return 0; }\n')
    (root / "Makefile").write_text("app: main.c add.c\n\tcc -Wall -o app main.c add.c\n")
    cfg = tmp / "cfg-agent"
    cfg.mkdir()
    s = Session(binary, root, make_env(cfg), rows=40)
    try:
        s.until("EmbedIDE")
        s.keys(b"\x1b[27u")
        s.keys(b"\x1b2")                        # Alt+2: AI panel
        s.type("/provider codex-cli")
        s.keys(b"\r")
        s.until("logged in", 40)
        s.type("add.c is missing: implement add() so the program builds, then build it with make and run ./app")
        s.keys(b"\r")
        deadline = time.monotonic() + 240
        while "done" not in s.text() and time.monotonic() < deadline:
            s.pump(1)
        s.pump(1)
        s.png(out / "agent-codex.png", "embed — Codex CLI agent working in the project")
    finally:
        s.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--only", default="editor,hex,agent")
    ap.add_argument("--live-codex", action="store_true", help="run one real, short Codex CLI task (uses your subscription)")
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    tmp = Path("/tmp/embedide-demo")
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir()
    try:
        for name in a.only.split(","):
            {"editor": lambda: shot_editor(a.binary.resolve(), a.out, tmp),
             "hex": lambda: shot_hex(a.binary.resolve(), a.out, tmp),
             "agent": lambda: shot_agent(a.binary.resolve(), a.out, tmp, a.live_codex)}[name]()
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
