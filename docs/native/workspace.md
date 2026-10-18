# Native project workspace — EmbedIDE 2.0

Этот этап добавляет рабочее проектное TUI к прежнему редактору. Desktop/Electron
не переписывался. Файл можно по-прежнему открыть минимальным `edit`, а каталог —
новым workspace. Все операции локальные, без аккаунта и сетевых запросов при старте.

```sh
cargo build --release --workspace --locked --offline
./target/release/embedide .
./target/release/embedide open /path/to/project
./target/release/embedide open examples/native/firmware-logic
./target/release/embedide edit example.rs
```

`--offline` при компиляции самого IDE требует уже загруженных Cargo-зависимостей.
`open --json` и `open` с перенаправленным вводом/выводом сохраняют прежний контракт
инспекции проекта. Batch/line-протоколы и защита сохранения не изменены.
`embedide PATH` — сокращение для одного аргумента, являющегося существующим каталогом;
для дополнительных флагов используйте `embedide open PATH`.

## Экран и документы

Есть верхняя строка проекта/цели/профиля/сборки, переключатель документов с `*`,
ленивое дерево, редактор, панели Problems, Build Output, Task Console, Serial,
Debug, Tools, Settings и Help. Текущий документ первым показывается в переключателе.
Каждый документ сохраняет собственные Vim mode, курсор, viewport, регистр и undo.
`:q` закрывает только текущий документ; последний закрытый документ завершает workspace.
Ctrl+Q защищает все несохранённые документы. Ctrl+W предлагает явное отбрасывание
изменённого документа. Esc/n отменяют подтверждение; только `y` принимает его.

Проводник читает один каталог за раз: до 4096 записей за чтение и 20000 видимых
записей. Не сканирует всё дерево при старте или каждой перерисовке. Поддерживает
раскрытие/сворачивание, открытие, создание файлов и каталогов, переименование файлов
и пустых каталогов, удаление файлов и пустых каталогов с подтверждением, обновление
и Reveal Current File. Для переименования/удаления сначала закройте документы под
выбранным путём. Переименование файла использует hard link + unlink, не заменяет
существующий destination; требует поддержки hard links файловой системой.
Ошибка unlink может оставить оба имени. Непустые каталоги не переименовываются.

Symlink показывается как `[link]`, но не раскрывается и не открывается через дерево.
Создание/переименование не принимают абсолютные пути, `..` и symlink parents.
Проверки не устраняют все гонки при одновременной замене каталогов другим процессом;
это не изоляция от вредоносного процесса с правами того же пользователя.
`.git`, `target`, `node_modules`, `.embedide-build` исключены. Из `.gitignore`
учитываются только простые имена без glob/negation; полная семантика Git не реализована.
Конфигурации вроде `memory.x`, `.cargo` и `.embedide` намеренно не скрываются по точке.

При ширине менее 81 колонки дерево становится экраном, доступным через F2/F6.
Панель при маленькой высоте скрывается, но F4/F6 возвращают её в полный размер.
Для размеров меньше 12×6 используется минимальная раскладка редактора; диалог
остаётся активным, Esc его отменяет. Очень маленький экран не позволяет прочитать
весь preview — увеличьте терминал перед аппаратными операциями.
Размер кадра ограничен 1024×256, последняя колонка зарезервирована.

## Клавиши

| Клавиша | Действие |
| --- | --- |
| Ctrl+K | Палитра: фильтр по подстроке, стрелки, Enter, Esc |
| Ctrl+P | Фильтр уже загруженных записей проводника; каталоги можно раскрывать |
| Ctrl+S | Сохранить текущий документ |
| Ctrl+W / Ctrl+Q | Закрыть документ / выйти из workspace |
| F1 / F2 / F3 | Help / Explorer / Problems |
| F4 | Переключить панель и передать ей фокус |
| F5 / F7 / F8 | Build / Cargo Check / Cancel |
| F6 | Editor → Explorer → Panel → Editor |
| F9 / F10 | Выбрать настроенную цель / следующий документ |
| Explorer: j/k, стрелки, Enter, Left | Навигация, раскрытие, открытие, сворачивание |
| Explorer: n, Delete, r, / | Создать файл, удалить с подтверждением, refresh, поиск по загруженным именам |
| Panel: j/k, PgUp/PgDn, End, / | Прокрутка, возврат к концу, фильтр вывода |
| Problems: j/k, Enter | Выбрать диагностику / открыть её источник |
| Esc | Закрыть диалог или вернуть фокус редактору |

В палитре также есть Open File для точного относительного/абсолютного пути,
Create Directory, Rename, Reveal, Toggle Output Panel, Search Project,
Select Build Profile, Discover Toolchains, Discover Probes и аппаратные команды.
Ctrl+P не является рекурсивным fuzzy finder: он ищет только загруженные записи.

Сохранены Vim `i/a`, `h/j/k/l`, `w/b/e`, `gg/G`, операторы, Visual, `u`, Ctrl+R,
`/`, `?`, `n/N`, `:w`, `:q`, `:q!`, `:wq`. В Normal пробел + e/b/d/t/s/f
передаёт workspace запрос explorer/build/debug/task console/serial/file finder.
Обычная печать в Insert не исполняет имена команд. Dialog/panel/editor input
маршрутизируются отдельно; интерактивного shell/PTY в Task Console нет.

Горизонтальные перемещения, Backspace, Delete и символьное выделение используют
extended grapheme clusters, включая combining, ZWJ-emoji, flags и CRLF. Обход
работает через фрагменты piece table без копирования всего документа. API диапазонов
TextBuffer сохраняет байтовые/scalar boundaries для программных замен. `set_cursor`
приводит позицию к началу графемы. Вертикальные движения сохраняют номер графемы,
не визуальную колонку: tab/wide characters могут менять экранную позицию.
Статус `line:column` пока показывает Unicode scalar column. Enter копирует начальный
отступ текущей строки до курсора (до 4096 символов). O/o не делают smart indentation.
Matching brackets, синтаксическая подсветка, отдельная подсветка current line,
LSP и session persistence пока не добавлены. Строки >16 KiB отображаются escaped
scalar fallback; табуляция и курсор учитываются, но non-ASCII виден как escape.

## Настройки

Новый schema version=1 находится в объекте `native` прежнего `embedide.json`.
Необязательный `embedide.local.json` содержит тот же объект напрямую и рекурсивно
перекрывает project settings; он исключён из Git. Массивы заменяются целиком.
Неизвестные поля верхнего `embedide.json` сохраняются; внутри `native` опечатки
отвергаются. IDE не переписывает эти файлы автоматически. Reload Settings читает
изменения; активные hardware sessions сохраняют параметры, с которыми стартовали.

```json
{
  "version": 1,
  "name": "blink",
  "buildSystem": "cargo",
  "native": {
    "version": 1,
    "theme": "dark",
    "tab_width": 4,
    "profiles": {
      "debug": { "cargo_profile": "dev", "cmake_type": "Debug" },
      "release": { "cargo_profile": "release", "features": [], "cmake_type": "Release" }
    },
    "targets": [
      {
        "name": "My configured Pico",
        "compiler_target": "thumbv6m-none-eabi",
        "chip": "RP2040",
        "probe": "1234:5678:REPLACE_WITH_ACTUAL_SERIAL",
        "artifact": "target/thumbv6m-none-eabi/release/blink"
      }
    ],
    "build_directory": ".embedide-build",
    "tools": {},
    "environment": {},
    "keys": { "build": "ctrl+b", "palette": "ctrl+k" },
    "tasks": { "Tests": { "program": "cargo", "args": ["test", "--offline"] } }
  }
}
```

Пример target требует реального установленного Rust target, совместимой платы,
probe-rs и действительного selector. Placeholder не означает наличие оборудования.
Для обычных Cargo/Make/CMake/Zig проектов создавать `embedide.json` не обязательно.
При нескольких root manifests выберите верхний `buildSystem` явно.
Настройки `theme=dark|light|mono` и `tab_width=1..16` не требуют перекомпиляции.
Cycle Theme меняет тему только на текущий сеанс. `keys` задаёт уникальные
`f1..f12` либо `ctrl+a..ctrl+z`; переопределённая команда теряет default shortcut.
Идентификаторы команд находятся в `workspace.rs` COMMANDS; дополнительно доступны
`save`, `palette`, `focus`, `panel`, `next_document` для глобальных bindings.
`tools` заменяет executable имени адаптера (cargo, make, cmake, rg, probe-rs).
Debug paths настраиваются отдельно в `debug`. Не коммитьте секреты в environment/tasks.

## Сборки и диагностика

Проект обнаруживается по Cargo.toml, Makefile/GNUmakefile/makefile, CMakeLists.txt,
build.zig или embedide.json. Определение не запускает никаких hooks. Только явная
Build/Check/Clean/task-команда исполняет проектный код. Одновременно выполняется
один build/search/flash/task; serial и debugger имеют отдельные владельцы.
Build не сохраняет буферы автоматически: сначала сохраните изменённые документы.

- Cargo: `build`/`check`, offline, `--message-format=json`, `--profile`, `--features`,
  `--target`, два jobs. Профиль и target выбираются в палитре. Неустановленные
  зависимости/target показывают настоящую ошибку Cargo; загрузки IDE не запускает.
- CMake: configure `-S . -B build_directory -DCMAKE_BUILD_TYPE=...`, затем
  `--build ... --config ... --parallel 2`. Поддержаны `cmake_generator` и
  `cmake_toolchain` (путь к настоящему toolchain file). Конфигурация CMake сама
  может содержать download-команды: свойство offline IDE не sandbox для build scripts.
- Make: `make -j 2`, опциональный `profiles.NAME.make_target`.
- Zig: сохранён прежний `zig build`; новые native profile settings к нему не применяются.
- Clean: Cargo clean, CMake clean target, Make clean. Для Zig задайте явный task.
- Tasks: executable + literal args, cwd=root, configured environment, stdin=null.
  Проекты и их tool paths должны быть доверенными перед запуском.

CLI `embedide build` сохраняет прежние flags `--release/--jobs/--online/--dry-run`;
расширенные native profile/target settings на этом этапе применяет workspace,
не legacy CLI build. Это ограничение интерфейса, backend plan общий.

stdout/stderr читаются отдельными workers, очередь ограничена 256×4 KiB; console
хранит до 2000 строк и 2 MiB на pane, строка до 8192 bytes. При отставании frontend
вывод отбрасывается с явным счётчиком omitted bytes; полнота Problems при overflow
не гарантируется. Финальный outcome имеет отдельный слот и не теряется при отмене.
Никаких придуманных процентов прогресса: показываются статус, command, elapsed и exit.
Вывод сохраняется до следующей сборки; доступны прокрутка и фильтр по подстроке.
Task console — вывод процессов, без stdin forwarding, shell и job control.

Cargo compiler-message JSON преобразуется в severity/message/file/line/column/code/source.
GCC/Clang распознаются по `file:line:column: severity: message`, в том числе с Windows
drive и пробелами. CMake Error без надёжной координаты остаётся problem без файла.
Неподдержанные форматы остаются текстом; notes/child spans пока не структурируются.
До 1000 problems, Enter открывает primary location; внешние файлы разрешены явно,
недоступный или не-UTF-8 источник даёт видимую ошибку.

Search Project использует локальный `rg --json --fixed-strings`, с case-sensitive
и ignore-case вариантами. Чтение результатов фоновое и отменяемое F8; до 100 matches
на файл и 1000 результатов в UI. При отсутствии rg есть сообщение зависимости;
fallback scanner не реализован. Поиск в текущем файле остаётся буквальным Vim-поиском
с wrap; отдельной подсветки всех совпадений и case-toggle для него пока нет.

## Архитектура и следующие этапы

`embedide-editor`: документы, Unicode, history, сохранение; без UI и hardware.
`embedide-project`: прежняя discovery/schema compatibility.
`embedide-core`: config, process manager, diagnostics, explorer file services,
probe-rs planning, serialport worker, GDB/MI/OpenOCD worker.
`embedide-terminal::tui::workspace`: фокус, команды, диалоги, композиция панелей,
владение документами и presentation state. Сервисы можно переиспользовать в GUI.
Новые crate для каждой интеграции и async runtime не вводились.

Далее: реальные hardware acceptance tests; настоящая проверка CMake/FreeBSD/Windows;
tool versions и capability checks; полная .gitignore/fuzzy indexing; binding current
line/search styles; named registers и стабильный breakpoint list; единые CLI/TUI
native build options; PTY terminal; PlatformIO/Arduino adapters. Это планы,
а не поддерживаемые сейчас возможности.

Аппаратные настройки: [hardware.md](hardware.md). Результаты и измерения:
[workspace-validation.md](workspace-validation.md). Историческое состояние отдельного
редактора: [fullscreen-editor.md](fullscreen-editor.md).
