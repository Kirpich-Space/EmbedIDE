# EmbedIDE 2.0: решения первого этапа

> Историческое состояние предыдущего этапа. Актуальный проектный workspace,
> графемное редактирование и hardware services: [workspace.md](workspace.md).

Дата: 2026-10-16. Статус: фундамент миграции; нативного GUI ещё нет.
Текущие проверки и ограничения: [fullscreen-editor.md](fullscreen-editor.md).
Первоначальный аудит: [session-report.md](session-report.md).

## 1. Миграция рядом с работающим продуктом

Electron-версия остаётся в `electron/`, `src/`, `public/` и npm-сборке.
Rust workspace имеет независимую сборку и не требует Node.js для работы CLI.
Нельзя считать перенос завершённым, пока нет проверенных замен редактора,
генераторов, сборки, прошивки, UART, настроек и упаковки.

Во время аудита уже существовали незакоммиченные `Cargo.toml`, `Cargo.lock`,
`embedide-project`, `embedide-core`, `embedide-cli` и изменения `.gitignore`.
Они сохранены при миграции. В manifests добавлены локальные пакеты редактора
и терминального frontend; существующие Rust-сервисы дополняются и форматируются.
Версии и контрольные суммы registry-зависимостей сохранены. Лицензия MIT остаётся прежней.

## 2. Границы Cargo crates

| Crate | Ответственность сейчас | Зависимости от GUI |
| --- | --- | --- |
| `embedide-project` | Открытие каталога, `embedide.json`, обнаружение manifest, безопасный `init` | Нет |
| `embedide-core` | План и синхронное выполнение сборки, инвентаризация PATH, каталог MCU | Нет |
| `embedide-cli` | Разбор аргументов, JSON, stdout/stderr, коды завершения | Нет |
| `embedide-editor` | Piece Table, Vim parser/executor, cursor/selection/register/search и file bindings | Нет |
| `embedide-terminal` | Полноэкранный TUI, экранные координаты, viewport, terminal guard; также line/batch | Нет |

Направление зависимостей: CLI → core → project/editor. Core реэкспортирует
Editor и его API; `embedide edit` передаёт Editor в `embedide-terminal`.
Terminal зависит от editor, не наоборот; его standalone binary использует тот же
frontend. Workspace и основной CLI собираются с locked dependencies. Отдельные `build`, `lsp`,
`debug`, `serial`, `targets`, `config` выделяются при появлении содержательной
реализации. Пустых crates и общего framework для плагинов сейчас не требуется.

Дальнейшая модель: frontend отправляет типизированные команды сервисам,
получает результаты и события задач. Операции с файлом и редактором не должны
напрямую вызывать виджеты. CLI может ожидать задачу; GUI получает события через
ограниченную очередь и запрашивает перерисовку по событию. Протокол событий,
идентификаторы задач и отмена ещё не реализованы.

## 3. Запуск инструментов и offline-first

Существующий `BuildPlan` хранит программу, массив аргументов, рабочий каталог
и порядок шагов. Исполнение использует `std::process::Command` без shell.
`--dry-run` не запускает сборку; ненулевой результат останавливает шаги.
Сам Make/CMake/Cargo может выполнять код проекта: план не является sandbox.

CLI поддерживает Cargo, Make, CMake и Zig через их manifest. Для Cargo по
умолчанию передаётся `--offline`. Make/CMake/Zig используют правила проекта:
глобального запрета сети для этих процессов нет. GCC/Clang выбирает сам проект.
До отдельного toolchain adapter не надо незаметно добавлять флаги MCU.

`run_build` пока синхронный и неотменяемый. Его нельзя вызывать из GUI event
thread. До GUI нужен worker с потоковым чтением обоих каналов, ограничением
накопления логов, завершением дочерних процессов и гарантированным ожиданием
выхода. Для Unix process groups и Windows Job Objects нужны отдельные адаптеры.
Не следует подменять это простым `kill` одного непосредственного процесса.

AI не участвует в загрузке, редактировании или сборке. Будущий AI adapter должен
выдавать предлагаемый diff; применение изменений остаётся обычной транзакцией
проекта. Сетевые зависимости не включаются в базовый CLI и ядро редактора.

## 4. Текстовый буфер: Piece Table и Rope

Выбран небольшой Piece Table как первый проверяемый контракт, без новых
внешних зависимостей. Исходный String передаётся во владение; вставки добавляются
в отдельный String. Редактирование перестраивает список диапазонов, undo хранит
метаданные диапазонов, а не полную копию документа. Последовательные смежные
диапазоны одного источника объединяются.

Ограничения реализации намеренно явны: плоский `Vec<Piece>`, линейный поиск и
замена по числу pieces, копирование метаданных для истории, отсутствие индекса
строк. История по умолчанию ограничена 128 транзакциями, но это **не лимит RAM**:
append-only хранилище вставок и число pieces могут расти до закрытия буфера.
До длительной GUI-сессии нужны бюджет памяти, compaction и сравнение с Rope.

[Ropey](https://docs.rs/ropey/latest/ropey/) предоставляет UTF-8 rope и операции
по Unicode scalar indices. Это кандидат для индекса строк и крупных файлов.
Из доступности библиотеки не следует превосходство в нашем workload: решение
нужно проверить одинаковой последовательностью редактирований и измерением RAM.

Текущий API использует смещения в **байтах**, проверяет UTF-8 boundaries до
изменения состояния, сохраняет BOM/CRLF/последний перевод строки. Grapheme и
display-column навигация принадлежат следующему слою. `write_to` передаёт точные
байты caller-owned writer; это не атомарное сохранение файла. `mark_saved`
вызывается только после успешной записи и фиксации на диске.

Слой `file` реализует UTF-8 open/create, baseline fingerprint/metadata,
staged save с sync и заменой через платформенный adapter. Конечные symlinks
отклоняются, Unix mode bits копируются; несовпадение owner/group и hard links
приводит к отказу записи. Остаточная check-to-rename race, ACL/xattr и платформы
без проверки перечислены в [отчёте движка](editor-engine.md).

## 5. GUI: выбор для следующего этапа

Оценка основана на документации проектов, а не на измерениях EmbedIDE.

| Кандидат | Полезные свойства | Цена и оставшиеся проверки |
| --- | --- | --- |
| [egui/eframe](https://github.com/emilk/egui) | Rust API, нативный backend, MIT/Apache-2.0, инженерные панели удобно описывать кодом | Собственный viewport редактора и focus routing; проверить idle CPU, OpenGL/драйвер X230 и FreeBSD |
| [Slint](https://github.com/slint-ui/slint) | Декларативный UI, стабильная ветка 1.x, несколько renderers | Дополнительный DSL, интеграция редактора и docking; отдельно оценить условия лицензирования зависимости |
| [GPUI](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) | Retained/immediate модель, механизмы для редактора, GPU renderer | Upstream предупреждает о breaking changes до 1.0; проверить графические зависимости и старое GPU |
| Qt через Rust binding | Зрелые меню, docking и desktop widgets | FFI, дополнительная C++ сборка, сопровождение binding и развёртывание Qt |

Рабочий выбор: **нативный eframe/egui**, первоначально оценить renderer `glow`
без Chromium и web assets. Это архитектурный выбор для будущего GUI, зависимости
GUI пока не добавлены. Eframe upstream перечисляет Linux и Windows; это не
подтверждает работоспособность EmbedIDE на них. FreeBSD остаётся отдельной
задачей проверки backend, clipboard, IME, шрифтов и упаковки. GPUI upstream
сейчас также описывает FreeBSD backend; он не исключён по предположению об ОС.

Перед закреплением версии GUI проверить её MSRV, совместимость с workspace,
нативную сборку на Void musl и X11, набор текста, mouse selection, запуск на
X230, idle CPU и RSS. Если эти критерии не выполнены, вернуться к выбору renderer
или framework. Доказанных преимуществ производительности пока нет.

Первая компоновка: menu/toolbar сверху, resizable Explorer слева, вкладки и
редактор в центре, переключаемый output снизу, hideable debugger справа,
status bar снизу. Dock layout и видимость панелей сохраняются отдельно от
`embedide.json`. Для больших файлов рендерить только видимые строки.

## 6. Модальный ввод — следующий слой

В `embedide-editor::vim` реализован независимый `VimInput`: состояния `Normal`,
`Insert`, `Visual`, `Command`, префиксы, счётчики, operator и text object.
Parser выдаёт типизированные editor/IDE actions и не зависит от GUI key events.
`TextBuffer::begin_edit_group` / `end_edit_group` объединяют изменения Insert
или change-оператора в одну undo-транзакцию. Применение действий теперь реализуют
`session`, `document`, `executor`, `motion`, `search`, `file`; mode остаётся только
в parser. Индекс строк и cursor history обновляются вместе с text history.

Добавлены contract tests разбора `5j`, `gg`, `dd`, `yy`, `p`, `ciw`, `u`, Ctrl+r,
`/`, `n`, `N`, `:w`, `:q`, `:wq`; эти тесты реально прошли на Linux musl.
Document session исполняет `:w/:q/:wq`, проверяет dirty-close и ошибки файловой
системы. Editor и terminal frontend скомпилированы и проверены 103 Rust-тестами и Clippy;
полная workspace verification заблокирована registry dependencies. Leader mappings
применяются только в Normal; IME/paste передаются отдельным текстовым событием.
Aliases и leader-конфигурация существуют в API, их сохранение в settings ещё
не реализовано. Детальный [контракт ввода](editor-input.md).

## 7. Embedded и платформы

Каталог 67 MCU — миграционные данные, не доказательство аппаратной поддержки
и не ограничение на новые семейства. Rust CLI пока не реализует flash/debug/
serial. Legacy Debug — batch GDB (`load`, registers, backtrace, detach), не
интерактивный отладчик и не полноценный GDB/MI.

Сначала реализуются протоколы и transcript tests для GDB/MI, LSP framing и
ограниченных очередей UART. Затем подключаются реальные процессы и порты.
Mock transport помечается как тестовый и не выдаётся за соединение с MCU.
При закрытии serial task обязана освобождать порт и завершать worker.

| Платформа | Статус текущей сессии |
| --- | --- |
| Linux x86_64 musl | Editor/terminal пересобраны Rust 1.99.0: 103 Rust tests, 4 PTY tests и Clippy passed. Прежние проверки готового CLI/Make/GCC не заменяют текущую workspace сборку |
| Linux glibc | Новый CI workflow подготовлен, ещё не запускался |
| Windows | Новый CI workflow подготовлен, ещё не запускался |
| FreeBSD | Не проверено; нужен настоящий runner/VM, одного Unix cfg недостаточно |

Пути обрабатываются через Path/OsStr. PowerShell/Fish/Bash не являются
исполнительным движком CLI. Никакой зависимости от systemd или package manager.
Hardware, serial и GUI не объявляются поддерживаемыми до соответствующих проверок.

Текущий построчный frontend, проверенные команды и оставшийся workspace blocker:
[terminal-editor.md](terminal-editor.md). Полноэкранный TUI/raw input пока отсутствует.
