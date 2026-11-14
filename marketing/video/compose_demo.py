#!/usr/bin/env python3
"""Video 2 — Technical demonstration (real session + permission/binary footage). VIDEO_LANG=ru: Russian dub."""
from compose_common import *   # noqa
from lib import term as T

L = F.LANDSCAPE
rec = T.load(OUT / "demo_wide.pkl")
ev = {n: t for t, n in rec["events"]}
ap = T.load(OUT / "approval.pkl")
av = {n: t for t, n in ap["events"]}
binr = T.load(OUT / "overview_binary.pkl")
bv = {n: t for t, n in binr["events"]}
film = F.Film(L, lang=i18n.LANG)
EXPLORER, EDITOR, AIPANEL, BOTTOM, PROBLEMS = (0, 2, 24, 14), (24, 2, 66, 21), (66, 1, 118, 22), (0, 20, 64, 31), (0, 21, 76, 25)


def clip(r, segs, narr, chapter=None, labels=(), title="ringbuf — embed", focus=()):
    c = F.Clip(r, segs, title=title, narration=narr, labels=labels, focus=focus)
    c.chapter = chapter
    return c


t0 = F.Card(logo_on=True, headline=tr("Technical demonstration", "Техническая демонстрация"),
            sub=tr("One real development session, recorded from the program", "Одна настоящая сессия разработки, записанная с программы"),
            narration=tr("This is a technical demonstration of EmbedIDE, version 0.1.0-alpha.1. Everything in the "
                         "recorded sections is the real program, not a mockup. Waiting time is sped up, and it is "
                         "always labelled. The project is a small C ring buffer, and we will edit it, build it, and "
                         "then let an AI agent extend it.",
                         "Это техническая демонстрация EmbedIDE версии 0.1.0-alpha.1. Всё в записанных частях — настоящая "
                         "программа, а не макет. Время ожидания ускорено, и это всегда подписано. Проект — небольшой "
                         "кольцевой буфер на Си: мы изменим его, соберём, а потом поручим ИИ-агенту его расширить."),
            min_dur=9.0)
t0.chapter = tr("Introduction", "Введение")
film.add(t0, 0.6)

film.add(clip(rec, [(1.0, ev["opened"] + 0.5, 1)],
              tr("We start EmbedIDE inside the project directory. The explorer is on the left, the editor in the "
                 "middle, and the AI panel on the right. I expand the source folder and open ring.c. Nothing has "
                 "been sent anywhere, and nothing will be until I press Enter in the AI panel.",
                 "Запускаем EmbedIDE в каталоге проекта. Слева проводник, в середине редактор, справа панель ИИ. "
                 "Раскрываю папку с исходниками и открываю ring.c. Ничего никуда не отправлено и не будет отправлено, "
                 "пока я не нажму Enter в панели ИИ."),
              tr("Open a project", "Открываем проект"),
              labels=[(1.0, 4.5, tr("Explorer · Enter opens a file", "Проводник · Enter открывает файл"))],
              focus=[(0.6, 4.2, EXPLORER)]), 0.4)

film.add(clip(rec, [(ev["opened"] + 0.5, ev["jumped"] + 1.6, 1)],
              tr("Press F5 to build with the project's compiler, here GCC. The build fails: a missing semicolon. The "
                 "problems list shows the error and a warning with file, line and column, parsed from the compiler "
                 "output. Enter jumps straight to the line in the editor.",
                 "F5 запускает сборку компилятором проекта — здесь это GCC. Сборка падает: пропущена точка с запятой. "
                 "Список проблем показывает ошибку и предупреждение с файлом, строкой и столбцом, разобранные из вывода "
                 "компилятора. Enter переносит прямо на строку в редакторе."),
              tr("Build, error, jump", "Сборка, ошибка, переход"),
              labels=[(1.0, 4.0, tr("F5 · build with GCC", "F5 · сборка GCC")), (4.5, 8.5, tr("Problems · Enter jumps to the line", "Проблемы · Enter — к строке"))],
              focus=[(3.2, 7.6, PROBLEMS), (8.0, 11.5, EDITOR)]), 0.4, wipe=True)

film.add(clip(rec, [(ev["jumped"] + 1.6, ev["build-ok"] + 0.6, 1)],
              tr("I append the semicolon, press Escape, and save with colon w. Builds use the files on disk, so saving "
                 "comes first. F5 again, and this time the build succeeds.",
                 "Дописываю точку с запятой, нажимаю Escape и сохраняю командой двоеточие w. Сборка берёт файлы с "
                 "диска, поэтому сначала сохранение. Снова F5 — и на этот раз сборка успешна."),
              tr("Fix and rebuild", "Исправляем и пересобираем"), focus=[(1.0, 5.0, EDITOR), (9.0, 12.0, BOTTOM)]), 0.4)

film.add(clip(rec, [(ev["build-ok"] + 0.6, ev["ran"] + 3.0, 1)],
              tr("Alt+3 opens the integrated terminal: a real shell in a pseudo-terminal, started in the project "
                 "directory. The program runs and prints the values popped from the ring buffer. Alt+3 again returns "
                 "to the editor.",
                 "Alt+3 открывает встроенный терминал: настоящую оболочку в псевдотерминале, запущенную в каталоге "
                 "проекта. Программа выполняется и печатает значения, извлечённые из кольцевого буфера. Повторное "
                 "Alt+3 возвращает в редактор."),
              tr("Integrated terminal", "Встроенный терминал"),
              labels=[(0.5, 4.0, tr("Alt+3 · integrated terminal (real PTY)", "Alt+3 · встроенный терминал (настоящий PTY)"))],
              focus=[(1.5, 6.5, BOTTOM)]), 0.4)

film.add(clip(ap, [(av["skills"] - 3.5, av["policy"] + 3.5, 1)],
              tr("Now the AI side. In the AI panel, slash skills lists what agents get: EmbedIDE's own working "
                 "instructions and the skills that match this project, chosen automatically. Slash policy shows the "
                 "three permission policies: read-only, workspace-write, and approval-required.",
                 "Теперь про ИИ. В панели ИИ команда /skills показывает, что получают агенты: собственные рабочие "
                 "инструкции EmbedIDE и навыки, подходящие проекту, выбранные автоматически. Команда /policy показывает "
                 "три политики прав: только чтение, запись в рабочей области и подтверждение каждого действия."),
              tr("Skills and policies", "Навыки и политики"), title="AI panel — embed",
              labels=[(0.5, 6.0, "/skills"), (7.0, 13.0, "/policy")], focus=[(0.5, 6.0, AIPANEL), (7.0, 13.5, AIPANEL)]), 0.4, wipe=True)

film.add(clip(rec, [(ev["ran"] + 3.0, ev["agent-start"] + 0.5, 1.25)],
              tr("I select the Codex CLI backend. EmbedIDE needs no API key: it runs the Codex CLI you are already "
                 "logged in with. In agent mode, plain text is a task. I ask for a peek function, a call from main, "
                 "a build, and a run.",
                 "Выбираю бэкенд Codex CLI. EmbedIDE не нужен ключ API: он запускает Codex CLI, в котором вы уже "
                 "вошли. В режиме агента обычный текст — это задача. Прошу добавить функцию peek, вызвать её из main, "
                 "собрать проект и запустить."),
              tr("AI agent: the task", "ИИ-агент: задача"),
              labels=[(1.0, 5.0, tr("Alt+2 · AI panel · /provider codex-cli", "Alt+2 · панель ИИ · /provider codex-cli"))],
              focus=[(1.0, 9.0, AIPANEL)]), 0.4)

film.add(clip(rec, [(ev["agent-start"] + 0.5, ev["agent-done"] + 1.0, 3.0)],
              tr("The agent reads the files, edits them, builds and runs the program. Every command with its output "
                 "and every file change is shown as it happens. Codex works in its own sandbox. This part is sped up.",
                 "Агент читает файлы, правит их, собирает и запускает программу. Каждая команда с её выводом и каждое "
                 "изменение файла показываются по мере выполнения. Codex работает в собственной песочнице. Эта часть "
                 "ускорена."),
              tr("The agent works", "Агент работает"), focus=[(1.0, 13.0, AIPANEL)]), 0.4)

film.add(clip(rec, [(ev["agent-done"] + 1.0, ev["rebuilt"] + 0.2, 1)],
              tr("When it finishes, EmbedIDE shows a summary: files changed on disk, commands run, and time. Slash "
                 "review shows the diff, and slash revert would undo the whole task. Files open in the editor are "
                 "reloaded automatically.",
                 "Когда агент закончил, EmbedIDE показывает сводку: изменённые на диске файлы, выполненные команды и "
                 "время. Команда /review показывает дифф, а /revert откатила бы всю задачу. Открытые в редакторе файлы "
                 "перечитываются автоматически."),
              tr("Review and revert", "Просмотр и откат"), labels=[(0.5, 4.0, "/review · /revert")],
              focus=[(0.5, 6.0, AIPANEL)]), 0.4)

film.add(clip(rec, [(ev["rebuilt"] + 0.2, ev["ran-again"] + 3.2, 1)],
              tr("Back in the editor, the build is already up to date, because the agent built it. Running the program "
                 "shows the new function at work: it prints the peeked value first.",
                 "Возвращаемся в редактор: сборка уже актуальна, потому что её выполнил агент. Запуск программы "
                 "показывает новую функцию в деле: сначала она печатает подсмотренное значение."),
              tr("Run the result", "Запускаем результат"), focus=[(4.0, 9.5, BOTTOM)]), 0.6)

film.add(clip(ap, [(av["task"] - 1.0, av["done"] + 2.0, 1)],
              tr("Permissions are the other half. Here I ask Claude Code to delete the build directory. Deleting data "
                 "is not something the workspace-write policy allows silently, so EmbedIDE stops and asks. The request "
                 "shows the exact command and why it needs approval. I press n. The agent is told it was refused, and "
                 "the directory is still there.",
                 "Вторая половина — права. Здесь я прошу Claude Code удалить каталог сборки. Удаление данных политика "
                 "записи в рабочей области молча не разрешает, поэтому EmbedIDE останавливается и спрашивает. Запрос "
                 "показывает точную команду и причину. Я нажимаю n. Агенту сообщают об отказе, и каталог остаётся на "
                 "месте."),
              tr("Permission prompts", "Запросы разрешений"), title="AI panel — embed",
              labels=[(5.0, 11.0, tr("Approval needed · y allow · a allow similar · n deny", "Нужно подтверждение · y · a · n"))],
              focus=[(5.0, 11.5, AIPANEL)]), 0.5, wipe=True)

film.add(clip(binr, [(bv["hex"] - 1.0, bv["disasm"] + 3.0, 1.3)],
              tr("And what you build can be inspected without leaving the IDE. This is the compiled program in the "
                 "hex view, then the ELF inspector with its symbols, then the disassembly of ring_pop, produced by "
                 "objdump. The file is only read; editing bytes is an explicit mode with a confirmed, atomic save.",
                 "А собранное можно изучать, не выходя из IDE. Это скомпилированная программа в hex-просмотре, затем "
                 "инспектор ELF с символами, затем дизассемблер функции ring_pop, полученный через objdump. Файл только "
                 "читается; правка байтов — явный режим с подтверждаемым атомарным сохранением."),
              tr("Binary view", "Просмотр бинарников"), title="build/debug/ringbuf — embed"), 0.6)

summary = F.Card(headline=tr("What you just saw", "Что вы увидели"), bullets=tr([
    "Native Rust, one binary, no Electron", "GCC diagnostics with jump to source", "Integrated terminal (real PTY)",
    "Codex CLI agent: edits, builds, review and revert", "Permission prompts for risky actions", "ELF, HEX and disassembly view"], [
    "Нативный Rust, один бинарник, без Electron", "Диагностика GCC с переходом к исходнику", "Встроенный терминал (настоящий PTY)",
    "Агент Codex CLI: правки, сборка, просмотр и откат", "Запросы разрешений для рискованных действий", "Просмотр ELF, HEX и дизассемблера"]),
    narration=tr("That was one real session: editing, compiler diagnostics, an integrated terminal, an AI agent that "
                 "edited files and built the project with a diff you can review and revert, a permission prompt for a "
                 "risky action, and a view of the compiled program. EmbedIDE is an alpha. Linux is verified, FreeBSD "
                 "builds but has not been run, and the native GUI is only planned.",
                 "Это была одна настоящая сессия: правка, диагностика компилятора, встроенный терминал, ИИ-агент, "
                 "который изменил файлы и собрал проект, с диффом, который можно просмотреть и откатить, запрос "
                 "разрешения для рискованного действия и просмотр скомпилированной программы. EmbedIDE — альфа. Linux "
                 "проверен, FreeBSD собирается, но не запускалась, нативный GUI только запланирован."), min_dur=13.0)
summary.chapter = tr("Summary", "Итоги")
film.add(summary, 0.6)

end = F.Card(logo_on=True, headline=tr("Try it. Tell us what breaks.", "Попробуйте. Расскажите, что сломалось."), code=[
    "$ curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh",
    "github.com/Kirpich-Space/EmbedIDE"],
    sub=tr("Official Kirpich Space production · GPL-3.0 · Linux · FreeBSD (compiled, not yet run)",
           "Официальный ролик Kirpich Space · GPL-3.0 · Linux · FreeBSD (собирается, не запускалась)"),
    narration=tr("Try it, and tell us what breaks. The repository link and the install command are in the description.",
                 "Попробуйте и расскажите, что сломалось. Ссылка на репозиторий и команда установки — в описании."),
    min_dur=8.0, note=tr("Alpha software. The native GUI is planned, not available.", "Альфа-версия. Нативный GUI запланирован, его пока нет."))
end.chapter = tr("Install", "Установка")
film.add(end, 0.0)

TITLE = tr("EmbedIDE technical demo: edit, build, AI agent and permissions in a Rust terminal IDE (alpha)",
           "EmbedIDE: техническая демонстрация — правка, сборка, ИИ-агент и права в терминальной IDE на Rust (альфа)")
DESC = tr("""A real development session in EmbedIDE (`embed`), the native Rust terminal IDE for embedded and systems work. Every recorded section is the actual program, version 0.1.0-alpha.1; waiting time is sped up and labelled. Nothing is mocked.

What happens: open a C project, build with GCC, read the diagnostics and jump to the error, fix it, use the integrated terminal, then let the Codex CLI agent add a function, build and run it — with a summary, a reviewable diff and revert. Then a Claude Code request to delete a directory is stopped by a permission prompt, and the compiled program is inspected in the ELF/HEX/disassembly view.

Official Kirpich Space / EmbedIDE video. Status: alpha. Linux verified, FreeBSD compiles but not yet run, Windows/macOS not supported, native GUI planned (not available).

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64): curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Bugs and feedback: https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

The AI agents use the Claude Code / Codex CLI you are logged in with; EmbedIDE needs no API key.
Voice: Kokoro-82M (Apache-2.0). Music: original, synthesised for this video.""",
          """Настоящая сессия разработки в EmbedIDE (`embed`) — нативной терминальной IDE на Rust для встраиваемой и системной работы. Каждая записанная часть — настоящая программа версии 0.1.0-alpha.1; ожидание ускорено и подписано. Ничего не имитируется.

Что происходит: открываем проект на Си, собираем GCC, читаем диагностику и переходим к ошибке, исправляем, пользуемся встроенным терминалом, затем агент Codex CLI добавляет функцию, собирает и запускает её — со сводкой, дифф-просмотром и откатом. Потом запрос Claude Code на удаление каталога останавливается запросом разрешения, а скомпилированная программа изучается в просмотре ELF/HEX/дизассемблера.

Официальный ролик Kirpich Space / EmbedIDE. Русский дубляж; оригинал на английском. Статус: альфа. Linux проверен, FreeBSD собирается, но не запускалась, Windows/macOS не поддерживаются, нативный GUI запланирован (его нет).

Репозиторий (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Установка (Linux x86_64): curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Сборка из исходников: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Ошибки и отзывы: https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

ИИ-агенты используют Claude Code / Codex CLI, в которых вы вошли; ключ API EmbedIDE не нужен.
Голос (дубляж): Piper, голос Denis (датасет CC0). Музыка: оригинальная.""")
TAGS = ["EmbedIDE", "Rust", "terminal IDE", "TUI", "embedded", "C programming", "GCC", "AI coding agent",
        "Claude Code", "Codex CLI", "ELF", "disassembly", "open source", "Linux", "FreeBSD"]

if __name__ == "__main__":
    final = build(film, "video2_technical_demo")
    c = F.Clip(rec, [(ev["jumped"], ev["jumped"] + 1, 1)], title="ringbuf — embed")
    thumbnail(OUT / (sfx("video2") + "_thumbnail.png"), c.frame(0.5, L),
              tr(["Real C workflow", "with an AI agent"], ["Настоящий C-процесс", "с ИИ-агентом"]),
              kicker=tr("EmbedIDE · technical demo", "EmbedIDE · техдемо"))
    metadata("video2_technical_demo", TITLE, DESC, TAGS, film)
    print("done", final)
