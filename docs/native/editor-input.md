# Редактор: модальный ввод и групповые транзакции

Продолжение этапа D. Реализация находится в `embedide-editor`; зависимостей
от GUI, CLI, сети и платформенных key codes нет. Основные файлы:

- `src/vim.rs` — `VimInput`, логические `Key`, typed `Action`, `Keymap`.
- `src/lib.rs` — `TextBuffer`, включая `begin_edit_group` / `end_edit_group`.
- `tests/vim.rs` — 23 проверки последовательностей и маршрутизации ввода.
- `tests/edit_groups.rs` — 7 проверок групповой истории.

## Что реализовано

| Ввод | Результат разбора |
| --- | --- |
| `h/j/k/l`, стрелки, `w/b/e`, `0/$` | Motion с положительным repeat |
| `gg`, `12gg`, `G`, `4G` | Первая, заданная или последняя строка |
| `dd`, `yy`, `cc`, `2d3d` | Linewise operator, произведение счётчиков |
| `dw`, `d0`, `c$`, `2d3w` | Operator и motion range |
| `ciw`, `diw`, `yiw`, `2c3iw` | Inner-word operator; Change переводит в Insert |
| `i/a/I/A/o/O` | BeginInsert с позицией и repeat |
| Текст, IME, paste в Insert | InsertText, без интерпретации как команд |
| Enter / Tab в Insert | InsertNewline / InsertTab; политика EOL/отступов остаётся document layer |
| Escape из Insert | FinishInsert и Normal |
| `v`, `V`, motions, `y/d/c`, Escape | Visual actions и переходы режимов |
| `p/P`, `u`, Ctrl+r | Paste с направлением, Undo, Redo |
| `/`, `?`, `n/N` | Поиск/повтор с направлением и repeat |
| `:w`, `:q`, `:q!`, `:wq` | Типизированный запрос host к file session |
| Space `e/b/d/t/s/f` | Запрос фокуса панели, Build или Debug |

`VimInput` остаётся отдельным парсером. К нему подключён `Editor`, исполняющий
действия через `EditorState`, `executor`, индекс строк и файловый слой; см.
[отчёт движка](editor-engine.md). Сам parser не меняет документ и не выполняет
Build. Новый Motion `FirstNonBlank` разбирается из `^`. Keymap хранится через
Arc, чтобы rollback parser state не копировал таблицы на каждом вводе.

Поддержка operators ограничена перечисленными motions и `iw`. `dgg`, block
Visual, macros, registers по именам, recursive mappings и прочие возможности
полного Vim здесь не заявляются реализованными.

## Контракт с frontend и document session

Frontend передаёт layout-aware command keys через `Key::Char`, а committed
IME/paste — одним `Key::Text`. В Normal pasted `dd:wq` поглощается как текстовое
событие и не выполняет последовательность команд. В Insert и Command aliases
и глобальный leader вообще не применяются.

`Dispatch::Consumed` означает обновление prompt/prefix либо неизвестный modal
character. `Dispatch::Unhandled` позволяет frontend обработать системный chord;
оно не является разрешением вставить текст. Неизвестный Ctrl chord сбрасывает
незавершённые Normal prefixes. ОС Alt/Meta shortcuts маршрутизирует frontend.

`Keymap` позволяет менять leader и его bindings, а также задавать однократные
Normal/Visual aliases. Leader suffixes не преобразуются motion aliases.
Смена keymap отменяет незавершённый prefix. Это in-memory API; сериализация
пользовательских settings пока отсутствует.

Command/search prompt ограничен 4096 UTF-8 байт, повторы и произведения —
1 000 000. Переполнение возвращает `InputError`, а не запускает операцию.
Backspace prompt удаляет Unicode scalar целиком. Неизвестные Ex команды,
аргументы и shell escapes отвергаются; `:!make` никогда не исполняется.

При `BeginInsert` session открывает edit group; Change открывает её **до**
удаления. Escape закрывает группу; движение курсора внутри Insert разделяет
транзакции. Counted Insert (`3i` и аналоги) пока возвращает явную ошибку
Unsupported; счётчики motions/operators/paste исполняются. Наличие repeat в
Action не означает поддержки всех возможностей Vim.

Enter использует первый обнаруженный EOL файла (LF/CRLF); autoindent пока нет.
Paste сохраняет переданные байты. Tab вставляет буквальную табуляцию; настройки
tabs/spaces и расчёт экранной ширины остаются следующими возможностями.

Ex actions исполняются файловой session: Write сохраняет, Quit проверяет dirty
state, ForceQuit явно закрывает без записи, WriteQuit закрывает только после
успешного сохранения. При ошибке исполнения Editor возвращает ошибку и
восстанавливает mode/prompt для исправления либо Escape; неудавшийся prefix
и его счётчик сбрасываются, чтобы не влиять на следующую команду.

## История буфера

`begin_edit_group` идемпотентен. Несколько замен создают одну undo-транзакцию;
redo восстанавливает конечный результат группы. Пустая группа и отвергнутая
замена не расходуют историю. История ограничивается числом транзакций, а не
числом символов; это по-прежнему не ограничение RAM вставочного хранилища.

Undo/redo и `mark_saved` закрывают группу. Это сохраняет доступность savepoint:
новая вставка после сохранения не склеивается с сохранённым состоянием.
Новая ветвь редактирования после undo отбрасывает redo. При лимите истории 0
группы не включают историю обратно.

## Проверка этого продолжения

Добавлены 30 Rust tests: 23 parser traces и 7 group contracts. В том числе
есть trace `Insert → Unicode text → Escape → Undo → Redo`, применяющий typed
actions к настоящему `TextBuffer`, а не только проверяющий список событий.

В текущей итерации Rust 1.99.0 доступен: все 23 parser tests и 7 group tests
реально прошли, как и остальные 53 теста редактора. Clippy all-targets с
`-D warnings` и `cargo fmt --all -- --check` прошли. Registry-зависимости
workspace недоступны; editor собран отдельным временным Cargo manifest,
который ссылается на эти же файлы, без копирования/подмены исходников.

Воспроизведение без registry cache (Python 3.11+):

```bash
cargo fmt --all -- --check
python3 scripts/check-editor-offline.py --bench
```

С кешем зависимостей по-прежнему обязательны workspace check/test/Clippy.
Результаты, методика и ограничения: [editor-engine.md](editor-engine.md).

Проверенный построчный frontend и его key notation: [terminal-editor.md](terminal-editor.md).
