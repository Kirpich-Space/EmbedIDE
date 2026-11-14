# video1_overview_ru

## Title

EmbedIDE — нативная IDE на Rust для встраиваемой и системной разработки (обзор альфы)

## Description

EmbedIDE (`embed`) — нативная IDE на Rust для встраиваемой и системной разработки, которая работает в терминале: редактор в стиле Vim, проводник, встроенный терминал, сборка с переходом к ошибке, просмотр ELF/HEX/дизассемблера собранных программ и необязательные ИИ-агенты, которые работают в вашем проекте через уже установленные Claude Code или Codex CLI — с политикой прав, которую вы контролируете, и диффами, которые можно просмотреть и откатить.

Официальный ролик Kirpich Space / EmbedIDE. Записанные части — настоящая программа (0.1.0-alpha.1); схема архитектуры — анимация, и это подписано на экране. Ожидание ускорено и помечено. Русский дубляж; оригинал на английском.

Статус: альфа. Linux проверен, FreeBSD собирается, но не запускалась, Windows и macOS не поддерживаются. Нативный GUI запланирован, не начат.

Репозиторий (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Установка (Linux x86_64, статическая сборка, проверка контрольной суммы):
curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Сборка из исходников: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Нашли ошибку? https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

Голос (дубляж): Piper, голос Denis (датасет CC0). Музыка: оригинальная, синтезирована для ролика.

0:00 Что такое EmbedIDE
0:36 Архитектура
1:06 Редактор и сборка
1:20 Встроенный терминал
1:30 Просмотр бинарников
1:46 Claude и Codex
2:04 ИИ-агент правит файлы
2:29 Запросы разрешений
3:06 Статус
3:21 Установка
3:39 Сборка из исходников



## Tags

EmbedIDE, Rust, embedded, IDE, terminal, TUI, firmware, systems programming, open source, Claude Code, Codex CLI, FreeBSD, Linux
