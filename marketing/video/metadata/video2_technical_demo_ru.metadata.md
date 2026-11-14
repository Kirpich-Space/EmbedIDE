# video2_technical_demo_ru

## Title

EmbedIDE: техническая демонстрация — правка, сборка, ИИ-агент и права в терминальной IDE на Rust (альфа)

## Description

Настоящая сессия разработки в EmbedIDE (`embed`) — нативной терминальной IDE на Rust для встраиваемой и системной работы. Каждая записанная часть — настоящая программа версии 0.1.0-alpha.1; ожидание ускорено и подписано. Ничего не имитируется.

Что происходит: открываем проект на Си, собираем GCC, читаем диагностику и переходим к ошибке, исправляем, пользуемся встроенным терминалом, затем агент Codex CLI добавляет функцию, собирает и запускает её — со сводкой, дифф-просмотром и откатом. Потом запрос Claude Code на удаление каталога останавливается запросом разрешения, а скомпилированная программа изучается в просмотре ELF/HEX/дизассемблера.

Официальный ролик Kirpich Space / EmbedIDE. Русский дубляж; оригинал на английском. Статус: альфа. Linux проверен, FreeBSD собирается, но не запускалась, Windows/macOS не поддерживаются, нативный GUI запланирован (его нет).

Репозиторий (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Установка (Linux x86_64): curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Сборка из исходников: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Ошибки и отзывы: https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

ИИ-агенты используют Claude Code / Codex CLI, в которых вы вошли; ключ API EmbedIDE не нужен.
Голос (дубляж): Piper, голос Denis (датасет CC0). Музыка: оригинальная.

0:00 Введение
0:22 Открываем проект
0:39 Сборка, ошибка, переход
0:58 Исправляем и пересобираем
1:12 Встроенный терминал
1:28 Навыки и политики
1:49 ИИ-агент: задача
2:06 Агент работает
2:21 Просмотр и откат
2:39 Запускаем результат
2:52 Запросы разрешений
3:15 Просмотр бинарников
3:35 Итоги
4:01 Установка



## Tags

EmbedIDE, Rust, terminal IDE, TUI, embedded, C programming, GCC, AI coding agent, Claude Code, Codex CLI, ELF, disassembly, open source, Linux, FreeBSD
