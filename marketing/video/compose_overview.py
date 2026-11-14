#!/usr/bin/env python3
"""Video 1 — Official EmbedIDE overview. VIDEO_LANG=ru renders the Russian dub."""
from compose_common import *   # noqa
from lib import term as T

L = F.LANDSCAPE
demo = T.load(OUT / "demo_wide.pkl")
dv = {n: t for t, n in demo["events"]}
inst = T.load(OUT / "overview_install.pkl")
cl = T.load(OUT / "overview_claude.pkl")
cv = {n: t for t, n in cl["events"]}
binr = T.load(OUT / "overview_binary.pkl")
bv = {n: t for t, n in binr["events"]}
ap = T.load(OUT / "approval.pkl")
av = {n: t for t, n in ap["events"]}
film = F.Film(L, lang=i18n.LANG)

# terminal cell boxes (118x31 layout)
EXPLORER, EDITOR, AIPANEL, BOTTOM, PROBLEMS = (0, 2, 24, 14), (24, 2, 66, 21), (66, 1, 118, 22), (0, 20, 64, 31), (0, 21, 76, 25)


def clip(rec, segs, narr, chapter=None, title="embed", labels=(), focus=()):
    c = F.Clip(rec, segs, title=title, narration=narr, labels=labels, focus=focus)
    c.chapter = chapter
    return c


hook = F.Words(tr(["Edit.", "Build.", "Inspect.", "Delegate."], ["Пиши.", "Собирай.", "Изучай.", "Делегируй."]),
               closing=tr("A native Rust IDE — in your terminal.", "Нативная IDE на Rust — в вашем терминале."),
               narration=tr("Edit. Build. Inspect. Delegate. EmbedIDE is a native Rust IDE for embedded and systems "
                            "development, and it lives in your terminal.",
                            "Пиши. Собирай. Изучай. Делегируй. EmbedIDE — нативная IDE на Rust для встраиваемой и "
                            "системной разработки, и она живёт в вашем терминале."), min_dur=8.5)
hook.chapter = tr("What is EmbedIDE", "Что такое EmbedIDE")
film.add(hook, 0.6)

film.add(F.Card(logo_on=True, headline=tr("One binary. No Electron. No browser.", "Один бинарник. Без Electron. Без браузера."),
                bullets=tr(["Vim-style editor, explorer, integrated terminal", "Builds with jump-to-source diagnostics",
                            "ELF, HEX and disassembly view for what you build",
                            "Optional AI agents through Claude Code and Codex CLI"],
                           ["Редактор в стиле Vim, проводник, встроенный терминал", "Сборка с переходом к ошибке в исходниках",
                            "ELF, HEX и дизассемблер для того, что вы собрали",
                            "Необязательные ИИ-агенты через Claude Code и Codex CLI"]),
                narration=tr("It combines a Vim-style editor, an explorer, an integrated terminal, builds with "
                             "jump-to-source diagnostics, a view for the binaries you produce, and optional AI agents. "
                             "One Rust binary, with no Electron and no browser.",
                             "Редактор в стиле Vim, проводник, встроенный терминал, сборка с переходом к ошибке в "
                             "исходниках, просмотр собранных программ и необязательные ИИ-агенты. Один бинарник на Rust, "
                             "без Electron и без браузера."), min_dur=9.0), 0.6, wipe=True)

film.add(F.Stats([(9, tr("Rust crates", "крейтов на Rust"), ""), (361, tr("automated tests", "автоматических теста"), ""),
                  (0, tr("unsafe blocks", "блоков unsafe"), ""), (8, tr("MB: one static binary", "МБ: один статический бинарник"), "")],
                 headline=tr("Small and strict", "Небольшой и строгий"),
                 narration=tr("The project is small and strict: nine crates, three hundred sixty-one automated tests, "
                              "no unsafe code allowed, and a single static binary of about eight megabytes.",
                              "Проект небольшой и строгий: девять крейтов, триста шестьдесят один автоматический тест, "
                              "ни одного блока unsafe и один статический бинарник около восьми мегабайт."), min_dur=10.0), 0.5)

d = F.Diagram(narration=tr("Under the hood it is a Cargo workspace of nine crates. The editor engine has no I/O and no "
                           "terminal types, so a native GUI could reuse it later. The build engine makes a reviewable "
                           "plan and then runs it, without a shell. There is no unsafe code, processes are launched "
                           "with argument vectors, and files are written atomically. The GUI itself is planned, not "
                           "started: this alpha is a terminal application.",
                           "Внутри — Cargo-воркспейс из девяти крейтов. Движок редактора ничего не знает ни о вводе-выводе, "
                           "ни о терминале, поэтому позже его сможет использовать нативный графический интерфейс. "
                           "Движок сборки сначала составляет проверяемый план, а потом выполняет его без оболочки. "
                           "Небезопасного кода нет, процессы запускаются с явными аргументами, файлы пишутся атомарно. "
                           "Сам графический интерфейс только планируется: эта альфа-версия — терминальное приложение."),
              min_dur=14.0)
d.chapter = tr("Architecture", "Архитектура")
film.add(d, 0.6, wipe=True)

film.add(clip(demo, [(1.0, dv["opened"] + 0.5, 1.4), (dv["opened"] + 0.5, dv["jumped"] + 1.6, 1)],
              tr("This is the real program. Open a file from the explorer, press F5, and GCC's error appears in the "
                 "problems list with file, line and column. Enter jumps to it.",
                 "Это настоящая программа. Откройте файл в проводнике, нажмите F5 — и ошибка GCC появится в списке "
                 "проблем с файлом, строкой и столбцом. Enter переносит вас к ней."),
              tr("Editor and builds", "Редактор и сборка"), title="ringbuf — embed",
              labels=[(0.5, 3.0, tr("Explorer", "Проводник")), (4.0, 7.0, tr("F5 · build", "F5 · сборка")),
                      (7.5, 10.5, tr("Problems · Enter jumps", "Проблемы · Enter — к строке"))],
              focus=[(0.4, 3.2, EXPLORER), (6.0, 9.2, PROBLEMS), (9.6, 12.6, EDITOR)]), 0.5)

film.add(clip(demo, [(dv["jumped"] + 1.6, dv["ran"] + 3.0, 1.3)],
              tr("Fix, build again, and use the integrated terminal: a real shell in a pseudo-terminal, one key away.",
                 "Исправьте, соберите снова и откройте встроенный терминал: настоящую оболочку в псевдотерминале, "
                 "в одно нажатие."),
              tr("Integrated terminal", "Встроенный терминал"), title="ringbuf — embed",
              focus=[(6.0, 10.0, BOTTOM)]), 0.5)

film.add(clip(binr, [(bv["hex"] - 1.0, bv["disasm"] + 3.0, 1.6)],
              tr("Open the program you just built. A hex viewer and editor, an ELF inspector with sections and "
                 "symbols, and a disassembly view that runs objdump. The file is only read, unless you explicitly edit "
                 "bytes and confirm the save.",
                 "Откройте собранную программу. Hex-просмотр и редактор, инспектор ELF с секциями и символами и "
                 "дизассемблер, который запускает objdump. Файл только читается, пока вы явно не включите правку "
                 "байтов и не подтвердите сохранение."),
              tr("Binary view", "Просмотр бинарников"), title="build/debug/ringbuf — embed"), 0.5, wipe=True)

film.add(clip(cl, [(cv["providers"] - 2.0, cv["claude"] + 3.0, 1)],
              tr("AI integration uses the Claude Code and Codex command-line tools you already have installed and "
                 "logged in. EmbedIDE needs no API key, never reads their credentials, and removes API key "
                 "variables from their environment, so a subscription request can never silently become billed "
                 "API usage.",
                 "ИИ работает через уже установленные Claude Code и Codex CLI, в которых вы вошли. EmbedIDE не нужен "
                 "ключ API, он не читает их учётные данные и убирает переменные с ключами из окружения, поэтому "
                 "запрос по подписке не превратится незаметно в платный запрос через API."),
              tr("Claude and Codex", "Claude и Codex"), title="ringbuf — embed",
              labels=[(2.0, 6.5, tr("/providers · installed CLIs and login state", "/providers · установленные CLI и вход"))],
              focus=[(2.0, 7.0, AIPANEL)]), 0.5)

film.add(clip(cl, [(cv["claude"] + 3.0, cv["agent-start"] + 0.5, 1.6), (cv["agent-start"] + 0.5, cv["agent-done"] + 1.5, 2.2)],
              tr("In agent mode, a plain message is a task. Here Claude Code adds a function, edits three files, builds "
                 "with the project's compiler and runs the program. EmbedIDE answers each permission request from a "
                 "policy: edits and builds inside the project run, while deleting files, network access, hardware "
                 "tools and anything outside the project ask first.",
                 "В режиме агента обычное сообщение — это задача. Здесь Claude Code добавляет функцию, правит три "
                 "файла, собирает проект его компилятором и запускает программу. Каждый запрос разрешения EmbedIDE "
                 "решает по политике: правки и сборка внутри проекта выполняются, а удаление файлов, сеть, аппаратные "
                 "инструменты и всё за пределами проекта спрашивают подтверждения."),
              tr("AI agent edits files", "ИИ-агент правит файлы"), title="ringbuf — embed",
              focus=[(1.0, 9.0, AIPANEL)]), 0.5)

film.add(clip(ap, [(av["approval"] - 6.0, av["done"] + 1.5, 1)],
              tr("When an action is risky, EmbedIDE stops and asks. Here the agent wants to delete the build directory. "
                 "The request shows the exact command and the reason. I answer n; the agent is told it was refused, "
                 "and the directory stays.",
                 "Если действие рискованно, EmbedIDE останавливается и спрашивает. Здесь агент хочет удалить каталог "
                 "сборки. Запрос показывает точную команду и причину. Я отвечаю «n», агент получает отказ, и каталог "
                 "остаётся на месте."),
              tr("Permission prompts", "Запросы разрешений"), title="AI panel — embed",
              labels=[(3.0, 9.0, tr("Approval needed · y / a / n", "Нужно подтверждение · y / a / n"))],
              focus=[(2.5, 9.0, AIPANEL)]), 0.5, wipe=True)

film.add(F.Card(headline=tr("You choose the toolchain", "Компиляторы выбираете вы"),
                bullets=tr(["GCC, Clang, zig cc, arm-none-eabi-gcc, NASM", "Make, CMake, Cargo, Zig, or EmbedIDE's native build",
                            "Never installs or flashes anything by itself",
                            "GDB, OpenOCD and serial: explicit commands, not yet verified on hardware"],
                           ["GCC, Clang, zig cc, arm-none-eabi-gcc, NASM", "Make, CMake, Cargo, Zig или собственная сборка EmbedIDE",
                            "Ничего не устанавливает и не прошивает само",
                            "GDB, OpenOCD и serial: только явные команды, на железе пока не проверены"]),
                narration=tr("You choose the compilers: GCC, Clang, zig cc, a cross GCC for Cortex-M, NASM. EmbedIDE "
                             "detects what is installed and never installs or flashes anything on its own. Debugging "
                             "with GDB and OpenOCD, and a serial monitor, exist as explicit commands, but they are "
                             "not yet verified on real hardware.",
                             "Компиляторы выбираете вы: GCC, Clang, zig cc, кросс-GCC для Cortex-M, NASM. EmbedIDE находит "
                             "установленное и ничего не устанавливает и не прошивает сам. Отладка через GDB и OpenOCD и "
                             "монитор последовательного порта есть как явные команды, но на реальном железе пока не "
                             "проверены."), min_dur=12.0), 0.6)

st = F.Card(headline=tr("Honest status: an alpha", "Честный статус: альфа"),
            bullets=tr(["Linux: verified", "FreeBSD: compiles, not yet run", "Windows and macOS: not supported",
                        "Native GUI: planned, not started"],
                       ["Linux: проверено", "FreeBSD: собирается, ещё не запускалась", "Windows и macOS: не поддерживаются",
                        "Нативный GUI: запланирован, не начат"]),
            narration=tr("This is an alpha. Linux is the verified platform. FreeBSD compiles but has not been run yet. "
                         "Windows and macOS are not supported, and the native GUI is planned, not started.",
                         "Это альфа-версия. Проверена только Linux. На FreeBSD проект собирается, но ещё ни разу не "
                         "запускался. Windows и macOS не поддерживаются, а нативный графический интерфейс запланирован, "
                         "но не начат."), min_dur=9.0)
st.chapter = tr("Status", "Статус")
film.add(st, 0.6)

film.add(clip(inst, [(0.5, 24.0, 1.3)],
              tr("To try it, one command downloads a static build into your home directory and verifies its checksum. "
                 "Then embed doctor shows what EmbedIDE found on your machine.",
                 "Чтобы попробовать, одна команда скачивает статическую сборку в домашний каталог и проверяет "
                 "контрольную сумму. Затем embed doctor показывает, что EmbedIDE нашёл на вашей машине."),
              tr("Install", "Установка"), title="/bin/sh",
              labels=[(1.0, 6.0, tr("install.sh · checksum verified", "install.sh · контрольная сумма проверена"))]), 0.5, wipe=True)

src = F.Card(headline=tr("Or build from source", "Или соберите из исходников"), code=[
    "$ git clone https://github.com/Kirpich-Space/EmbedIDE",
    "$ cd EmbedIDE && cargo build --release --locked",
    "$ ./target/release/embed doctor"],
    sub=tr("Rust 1.85 or newer · instructions, not a recording", "Rust 1.85 или новее · инструкция, а не запись"),
    narration=tr("To build from source you need Rust one point eighty-five or newer. Clone the repository, run cargo "
                 "build with the release flag, and start embed.",
                 "Для сборки из исходников нужен Rust версии один точка восемьдесят пять или новее. Склонируйте "
                 "репозиторий, выполните cargo build с флагом release и запустите embed."), min_dur=9.0)
src.chapter = tr("Build from source", "Сборка из исходников")
film.add(src, 0.6)

end = F.Card(logo_on=True, headline=tr("Try it. Tell us what breaks.", "Попробуйте. Расскажите, что сломалось."),
             sub=tr("Official Kirpich Space production · GPL-3.0", "Официальный ролик Kirpich Space · GPL-3.0"),
             code=["github.com/Kirpich-Space/EmbedIDE"],
             narration=tr("EmbedIDE is open source under the GPL. Try it, and tell us what breaks. The link is in the "
                          "description.",
                          "EmbedIDE — открытый проект под лицензией GPL. Попробуйте и расскажите, что сломалось. Ссылка "
                          "в описании."), min_dur=7.0,
             note=tr("Alpha 0.1.0 · Linux · FreeBSD (compiled, not yet run)", "Альфа 0.1.0 · Linux · FreeBSD (собирается, не запускалась)"))
film.add(end, 0.0)

TITLE = tr("EmbedIDE — a native Rust IDE for embedded and systems development (alpha overview)",
           "EmbedIDE — нативная IDE на Rust для встраиваемой и системной разработки (обзор альфы)")
DESC = tr("""EmbedIDE (`embed`) is a native Rust IDE for embedded and systems development that runs in your terminal: a Vim-style editor, explorer, integrated terminal, builds with jump-to-source diagnostics, an ELF/HEX/disassembly view for the programs you build, and optional AI agents that work in your project through the Claude Code or Codex CLI you already use — with a permission policy you control and diffs you can review and revert.

Official Kirpich Space / EmbedIDE video. Recorded sections are the real program (0.1.0-alpha.1); the architecture slide is an animation and says so. Waiting time is sped up and labelled.

Status: alpha. Linux verified, FreeBSD compiles but has not been run, Windows/macOS not supported. The native GUI is planned, not started.

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64, static build, checksum-verified):
curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Found a bug? https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

Voice: AI-synthesised, one voice for every language (Chatterbox Multilingual, MIT). Music: original, synthesised for this video.""",
          """EmbedIDE (`embed`) — нативная IDE на Rust для встраиваемой и системной разработки, которая работает в терминале: редактор в стиле Vim, проводник, встроенный терминал, сборка с переходом к ошибке, просмотр ELF/HEX/дизассемблера собранных программ и необязательные ИИ-агенты, которые работают в вашем проекте через уже установленные Claude Code или Codex CLI — с политикой прав, которую вы контролируете, и диффами, которые можно просмотреть и откатить.

Официальный ролик Kirpich Space / EmbedIDE. Записанные части — настоящая программа (0.1.0-alpha.1); схема архитектуры — анимация, и это подписано на экране. Ожидание ускорено и помечено. Русский дубляж; оригинал на английском.

Статус: альфа. Linux проверен, FreeBSD собирается, но не запускалась, Windows и macOS не поддерживаются. Нативный GUI запланирован, не начат.

Репозиторий (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Установка (Linux x86_64, статическая сборка, проверка контрольной суммы):
curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Сборка из исходников: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Нашли ошибку? https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

Голос: синтезирован ИИ, один голос на все языки (Chatterbox Multilingual, MIT). Музыка: оригинальная, синтезирована для ролика.""")
TAGS = ["EmbedIDE", "Rust", "embedded", "IDE", "terminal", "TUI", "firmware", "systems programming", "open source",
        "Claude Code", "Codex CLI", "FreeBSD", "Linux"]

if __name__ == "__main__":
    final = build(film, "video1_overview")
    c = F.Clip(demo, [(dv["jumped"], dv["jumped"] + 1, 1)], title="ringbuf — embed")
    thumbnail(OUT / (sfx("video1") + "_thumbnail.png"), c.frame(0.5, L),
              tr(["A native Rust IDE", "for embedded work"], ["Нативная IDE на Rust", "для embedded"]),
              kicker=tr("EmbedIDE · alpha overview", "EmbedIDE · обзор альфы"))
    metadata("video1_overview", TITLE, DESC, TAGS, film)
    print("done", final)
