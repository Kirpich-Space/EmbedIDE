"""Build a finished video: fit narration, lay out scenes, render frames, mix audio, mux, thumbnail."""
import json, subprocess, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from lib import audio as A, brand as B, film as F  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

OUT = Path(__file__).resolve().parent / "out"


def build(film, name, crf=18):
    A.fit_durations(film)
    film.layout()
    wav = OUT / f"{name}.wav"
    A.mix(film, wav)
    film.layout()  # captions now filled
    silent = OUT / f"{name}.silent.mp4"
    print(f"{name}: {film.total:.1f}s, {len(film.captions)} captions")
    film.render_video(silent, crf=crf)
    final = OUT / f"{name}.mp4"
    A.mux(silent, wav, final)
    film.write_subs(OUT / f"{name}.srt", OUT / f"{name}.vtt")
    chapters = []
    for p in film.placed:
        title = getattr(p.scene, "chapter", None)
        if title:
            chapters.append((p.start, title))
    (OUT / f"{name}.chapters.txt").write_text(
        "\n".join(f"{int(t // 60)}:{int(t % 60):02d} {title}" for t, title in chapters) + "\n")
    probe(final)
    return final


def probe(path):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries",
                        "stream=codec_type,codec_name,width,height,r_frame_rate,duration,sample_rate,channels",
                        "-of", "json", str(path)], capture_output=True, text=True, check=True)
    info = json.loads(r.stdout)
    print(json.dumps(info["streams"], indent=1))
    return info


def thumbnail(path, frame_img, title_lines, kicker="EmbedIDE · alpha", size=(1280, 720)):
    """YouTube-style thumbnail: real screenshot on the right, plain title on the left (brand colours only)."""
    W, H = size
    img = Image.new("RGB", size, B.OBSIDIAN)
    d = ImageDraw.Draw(img)
    shot = frame_img.copy()
    # crop the terminal window area of a 1920x1080 frame and scale to the right 60%
    shot = shot.crop((120, 14, 1810, 950)).resize((int(W * 0.62), int(W * 0.62 * 936 / 1690)))
    img.paste(shot, (W - shot.width - 30, (H - shot.height) // 2))
    d.rectangle([W - shot.width - 33, (H - shot.height) // 2 - 3, W - 27, (H + shot.height) // 2 + 2],
                outline=B.COPPER, width=3)
    lg = F.logo(int(W * 0.30))
    img.paste(lg, (36, 40), lg)
    f1 = F.font(B.FONT_SANS_BOLD, 74)
    y = 250
    for ln in title_lines:
        d.text((40, y), ln, font=f1, fill=B.WARM_WHITE)
        y += 92
    d.rectangle([40, y + 12, 220, y + 18], fill=B.COPPER)
    d.text((40, y + 40), kicker, font=F.font(B.FONT_MONO_BOLD, 30), fill=B.COPPER_LIGHT)
    img.save(path)


def metadata(name, title, description, tags, film, extra=""):
    """Write upload-ready metadata (title, description with chapters, tags) next to the video."""
    ch = (OUT / f"{name}.chapters.txt").read_text().strip() if (OUT / f"{name}.chapters.txt").exists() else ""
    text = f"# {name}\n\n## Title\n\n{title}\n\n## Description\n\n{description}\n"
    if ch:
        text += f"\n{ch}\n"
    text += f"\n{extra}\n\n## Tags\n\n{', '.join(tags)}\n"
    (OUT / f"{name}.metadata.md").write_text(text)
    return text
