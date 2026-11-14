#!/usr/bin/env python3
"""Video 3 — Short promotional video (1080x1920, ~40 s). VIDEO_LANG=ru: Russian dub."""
from compose_common import *   # noqa
from lib import term as T

V = F.VERTICAL
rec = T.load(OUT / "short_narrow.pkl")
ev = {n: t for t, n in rec["events"]}
film = F.Film(V, lang=i18n.LANG)


def clip(segs, narr, labels=()):
    return F.Clip(rec, segs, title="ringbuf — embed", narration=narr, labels=labels)


film.add(F.Words(tr(["Edit.", "Build.", "Delegate."], ["Пиши.", "Собирай.", "Делегируй."]),
                 closing=tr("A native Rust IDE for embedded work", "Нативная IDE на Rust для embedded"),
                 narration=tr("Edit. Build. Delegate. A native Rust IDE for embedded development, in your terminal.",
                              "Пиши. Собирай. Делегируй. Нативная IDE на Rust для встраиваемой разработки — в вашем терминале."),
                 min_dur=5.0, step=0.55), 0.4)
film.add(clip([(1.5, ev["error"] + 2.5, 1.5)], tr("Build with GCC, and the error is one keypress away.",
                                                  "Сборка GCC — и ошибка в одном нажатии от вас."),
              labels=[(0.5, 3.5, tr("F5 · build", "F5 · сборка"))]), 0.3, wipe=True)
film.add(clip([(ev["error"] + 2.5, ev["built"] + 1.0, 1.7)], tr("Fix it, and build again.", "Исправьте и соберите снова.")), 0.3)
film.add(clip([(ev["built"] + 1.0, ev["agent-start"] + 0.5, 2.4)],
              tr("Then hand a task to an AI agent. EmbedIDE drives the Claude Code or Codex CLI you already use.",
                 "Затем поручите задачу ИИ-агенту. EmbedIDE управляет Claude Code или Codex CLI, которые у вас уже есть."),
              labels=[(0.3, 3.5, tr("Alt+2 · AI panel", "Alt+2 · панель ИИ"))]), 0.3, wipe=True)
film.add(clip([(ev["agent-start"] + 0.5, ev["agent-done"] + 1.0, 6.5)],
              tr("It edits files, builds, and runs your program, and every step is visible.",
                 "Он правит файлы, собирает и запускает программу, и каждый шаг виден.")), 0.3)
film.add(clip([(ev["agent-done"] + 1.0, ev["agent-done"] + 3.0, 1)],
              tr("Review every change, or revert it.", "Просмотрите каждое изменение или откатите его.")), 0.4)
film.add(F.Card(logo_on=True, headline=tr("Open source. Alpha.", "Открытый код. Альфа."),
                sub=tr("Try it, and tell us what breaks.", "Попробуйте и расскажите, что сломалось."),
                code=["github.com/Kirpich-Space/EmbedIDE"],
                narration=tr("EmbedIDE is open source and in alpha. Try it, and tell us what breaks.",
                             "EmbedIDE — открытый код, пока в альфе. Попробуйте и расскажите, что сломалось."), min_dur=6.0,
                note=tr("Linux · FreeBSD (untested) · GUI planned", "Linux · FreeBSD (не проверялась) · GUI запланирован")), 0.0)

TITLE = tr("EmbedIDE in 45 seconds: a native Rust terminal IDE with AI agents #shorts",
           "EmbedIDE за 45 секунд: нативная терминальная IDE на Rust с ИИ-агентами #shorts")
DESC = tr("""EmbedIDE (`embed`) is a native Rust IDE for embedded and systems development that runs in your terminal. Real recording of version 0.1.0-alpha.1: build with GCC, jump to the error, fix it, then let an AI agent (Claude Code / Codex CLI, your own login) add a function, build and run it.

Official Kirpich Space / EmbedIDE video. Alpha: Linux verified, FreeBSD compiles but not yet run, no Windows/macOS, native GUI planned.

https://github.com/Kirpich-Space/EmbedIDE  (GPL-3.0)
Install: curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh

#shorts #rust #embedded #opensource #terminal""",
          """EmbedIDE (`embed`) — нативная IDE на Rust для встраиваемой и системной разработки, работающая в терминале. Настоящая запись версии 0.1.0-alpha.1: сборка GCC, переход к ошибке, исправление, затем ИИ-агент (Claude Code / Codex CLI, ваш собственный вход) добавляет функцию, собирает и запускает её.

Официальный ролик Kirpich Space / EmbedIDE. Русский дубляж. Альфа: Linux проверен, FreeBSD собирается, но не запускалась, без Windows/macOS, нативный GUI запланирован.

https://github.com/Kirpich-Space/EmbedIDE  (GPL-3.0)
Установка: curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh

#shorts #rust #embedded #opensource #terminal""")
TAGS = ["EmbedIDE", "Rust", "embedded", "terminal IDE", "TUI", "AI coding agent", "open source", "shorts"]

if __name__ == "__main__":
    final = build(film, "video3_short")
    c = F.Clip(rec, [(ev["error"] + 1.5, ev["error"] + 2.5, 1)], title="ringbuf — embed")
    c.frame(0.3, V).save(OUT / (sfx("video3") + "_thumbnail.png"))
    metadata("video3_short", TITLE, DESC, TAGS, film)
    print("done", final)
