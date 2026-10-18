# Первый терминальный frontend — 2026-10-16

> Историческое состояние предыдущего этапа. Актуальный проектный workspace,
> графемное редактирование и hardware services: [workspace.md](workspace.md).

> Исторический отчёт построчного этапа. Registry blocker устранён, основной CLI
> собран; по умолчанию теперь запускается полноэкранный TUI. Построчный режим
> доступен через `--line`. Актуальные команды и проверки:
> [fullscreen-editor.md](fullscreen-editor.md).

Проверенный результат: существующий editor engine подключён к работающему
построчному frontend. Это **не полноэкранный TUI**: клавиши выполняются после
отправки строки через Enter. Raw mode, alternate screen и терминальные signal
handlers не включаются. Нужные для полноценного TUI внешние crates сейчас
недоступны; собственная unsafe-реализация termios не добавлялась.

## Сборка и запуск

Работающий путь в текущем окружении, без registry cache:

```bash
python3 scripts/check-editor-offline.py --terminal --release --build-terminal target/editor-offline/embedide-terminal
target/editor-offline/embedide-terminal example.rs
```

Скрипт собирает **те же** library/main/tests sources editor и terminal crates
через временный workspace, затем экспортирует executable, путь которого сообщил
Cargo. Код не копируется/подменяется, основной workspace/lockfile не удаляется.
Python 3.11+ нужен только для проверки/сборки; бинарнику Python не нужен.

В окне `keys>` введите каждую строку, завершая её Enter:

```text
iHello, EmbedIDE!<Esc>
:wq
```

`example.rs` открывается, если существует, иначе создаётся новый буфер.
Каталог должен существовать. Повторный запуск читает сохранённый файл.

При доступных serde/clap и остальных locked dependencies основной CLI:

```bash
cargo build -p embedide-cli --locked
target/debug/embedide edit example.rs
```

Эта команда добавлена в исходники, но **полная сборка основного CLI пока не
подтверждена** из-за registry blocker. Его существующие команды сохранены.
Standalone `embedide-terminal` и `embedide edit` используют одну library frontend
и тот же editor engine; второго редактора нет.

Для скриптов предусмотрен явный batch mode:

```bash
printf 'iHello<Esc>\n:wq\n' | target/editor-offline/embedide-terminal --batch new.rs
```

После пересборки основного CLI также доступен `embedide --json edit --batch FILE`.
В batch нет приглашений и снимков документа, первая ошибка останавливает сессию.
`--json` с интерактивным `edit` отвергается с usage code 2. Без `--batch` stdin и
stdout должны быть терминалами. Standalone возвращает 0 при успешной сессии,
1 при I/O/editor failure, 2 при ошибках аргументов; итог пишет одной строкой.

## Ввод и отображение

Перевод строки stdin — граница порции клавиш, он не вставляется в документ.
В Insert для новой строки используйте `<Enter>`. Для `/pattern`, `?pattern`
и `:command` отправка строки также подтверждает prompt движка.

| Ввод | Действие |
| --- | --- |
| `i`, `a`, `A`, `o`, `O` | Insert/Append/open-line по правилам engine |
| `<Esc>` или физический Esc до Enter | Возврат в Normal |
| `h/j/k/l`, `w/b/e`, `0/^/$`, `gg/G`, counts | Движения engine |
| `dd`, `yy`, `p/P`, `d/c/y`, `ciw` | Изменение текста и unnamed register |
| `u`, `<C-r>` | Undo/redo; Insert/change остаются транзакциями engine |
| `/pattern`, `?pattern`, `n/N` | Буквальный поиск с wrap |
| `:w`, `:q`, `:q!`, `:wq` | Сохранение/закрытие через engine |
| `<Enter>`/`<CR>`, `<Tab>`, `<BS>`, `<Del>` | Явные специальные клавиши |
| `<Left>/<Right>/<Up>/<Down>/<Home>/<End>` | Переносимые обозначения движения |
| `<lt>` | Буквальный `<`, например `<lt>Esc>` вставляет текст `<Esc>` |

Неизвестные `<...>` остаются буквальным текстом; C/C++ `#include <stdint.h>`
и `a << 2` сохраняются. Распознанные token-like строки в коде нужно экранировать
через `<lt>`. Это протокол клавиш, не прозрачная вставка clipboard.
Обычные CSI/SS3 arrows распознаются; неизвестные escape sequences отвергаются
до применения всей текущей порции, чтобы их байты не стали Vim-командами.
Некоторые физические Ctrl/Backspace обрабатывает cooked terminal: используйте
явные обозначения для действий над документом. Модальная логика только в engine.

Снимок показывает filename, dirty `*`, mode, координаты Unicode scalar, номера
строк, курсор `▏`, текущий command/search prompt и ошибки. Вывод ограничен 12
строками и окном 100 Unicode scalars, без материализации всего документа:
`EditorState::line_range` использует существующий индекс строк. Управляющие
символы содержимого, пути и diagnostics экранируются. Графемы, табуляция и
screen cells не считаются равными scalar column; курсор отмечается внутри текста.

Рисование происходит после порции ввода; во время ожидания используется
блокирующий read без polling/фоновых потоков. Размер экранной сетки не захватывается;
перенос вывода обрабатывает терминал. Проверено продолжение работы после PTY
resize. Адаптивного полноэкранного viewport и incremental screen diff пока нет.

## Ошибки и сохранность файлов

Решение open/create принимается один раз по наличию записи пути. Ошибка чтения
наблюдавшегося существующего файла не превращается в пустой новый документ.
Содержимое читает/пишет прежний FileBinding; фронтенд не выполняет shell-команд.

В интерактивном режиме editor errors видны и не закрывают сессию. После failed
Ex/search engine сохраняет prompt для исправления; `<Esc>` выходит из него,
например после отказа `:q` используйте `<Esc>:wq` или `<Esc>:q!`.
`:q` с изменениями отвергается, `:q!` явно отбрасывает их. Failed `:w/:wq` не
снимает dirty flag и не закрывает буфер. EOF с изменениями завершает процесс с
ошибкой, **без автосохранения**; память сессии после выхода не восстанавливается.
При чистом буфере EOF разрешён. Порция ввода ограничена 64 KiB; превышение,
invalid UTF-8 и I/O failure явно завершают транспорт. Скрипт не атомарен:
сохранения выполняются в момент явных команд, последующая ошибка их не откатывает.

Сохраняются прежние проверки symlink, readonly, hard links, fingerprint/inode,
staged write/sync и dirty-close. Остаточное TOCTOU-окно и потеря ACL/xattr при
replacement **не устранены**. Абсолютная atomic/power-loss устойчивость не
обещается. Детали файлового слоя: [editor-engine.md](editor-engine.md).

## Проверки и текущий blocker

Rust/Cargo 1.99.0, rustfmt 1.10.0, Clippy 0.1.99, host/установленный target
`x86_64-unknown-linux-musl`. Проверены Cargo registry config, proxy/registry env,
resolver config и локальный cache. Зеркала/прокси не настроены, cache пуст;
DNS из среды выполнения возвращает `Operation not permitted`, Cargo сообщает
невозможность разрешить `index.crates.io`. Системные настройки и TLS не менялись.

`Cargo.lock` v4 сохранён: добавлены только локальный terminal package и его
связь с CLI; 28 registry packages, их версии/checksums сохранены. Это ещё не
подтверждение полного locked resolution. `cargo metadata --no-deps --locked
--offline` успешно видит 5 workspace members, но не проверяет внешние dependencies.

| Проверка | Результат |
| --- | --- |
| `cargo check --workspace --locked` | Заблокирован crates.io DNS/отсутствием serde |
| `cargo test --workspace --locked` (offline env) | Заблокирован отсутствием serde; workspace tests не запускались |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (offline env) | Та же блокировка dependencies |
| `cargo fmt --all -- --check` | Passed, весь исходный workspace |
| Offline editor + terminal check/test/Clippy all-targets `-D warnings` | Passed, 103 Rust tests: 83 editor + 16 frontend session + 4 binary |
| Release export standalone | Собран из текущих исходников, thin LTO/strip |
| `python3 scripts/check-terminal-pty.py --binary target/editor-offline/embedide-terminal -v` | 4/4 passed |

В этой итерации добавлены 21 Rust test: indexed line ranges и 20 terminal tests.
Есть реальные open/create/edit/save/reopen, navigation/register/history/search,
Unicode/CRLF/empty/large files, unsafe quit, external edits, readonly/symlink,
ошибки input/output и защита от source escape codes. PTY тестирует физический
Esc, save/quit, unsafe quit recovery, EOF, resize и SIGTERM. Termios attributes
после нормального/ошибочного выхода и сигнала совпали с исходными. Raw-mode
cleanup не тестируется: этот frontend raw mode не включает.

Три дополнительных теста основного CLI добавлены в `tests/cli.rs`, но **не
запускались**. Старые binaries не считаются проверкой нового CLI. CI обновлён
для full-workspace и отдельного offline editor/terminal job, но удалённый CI
в этой сессии не запускался.

## Измерения на текущей машине

X230, i5-3320M (2 cores/4 threads), 3 686 272 KiB MemTotal, Void Linux musl,
Linux 6.18.55_1, Rust 1.99.0. Бинарник release с thin LTO/strip. Его SHA-256:
`b39545cc3f523e042c00bce600067c8f4cb9c9dd6111e2c29f43e80b75b44ab9`.

Воспроизведение:

```bash
python3 scripts/check-terminal-pty.py --binary target/editor-offline/embedide-terminal --measure -v
```

По пять запусков отдельного процесса на файл. Измерение от `Popen` до получения
первого `keys>` через PTY, включая чтение файла, UTF-8/hash/index и первый draw.
Fixture записана заранее в `/tmp` (tmpfs); это тёплый запуск с прогретым binary
cache, а не чтение холодного диска. Affinity/governor не фиксировались,
пользовательская система продолжала работать.

| Размер файла | Min ms | Median ms | Max ms |
| ---: | ---: | ---: | ---: |
| 0 | 0,858 | 1,705 | 2,542 |
| 1 MiB | 4,847 | 5,872 | 8,815 |
| 10 MiB | 42,556 | 45,503 | 59,522 |

У ожидающего ввода процесса с пустым документом `VmRSS` = **1340 KiB**.
Прямое чтение `/proc/<child>/stat`: прирост CPU за 500 ms — **0 ticks** при
100 ticks/s. Это один idle sample с разрешением 10 ms; не доказательство
абсолютного нулевого CPU и не peak RSS больших файлов. Сравнения производительности
с прежней версией/другими редакторами не выполнялись.

Существующие release benches также реально выполнены этой итерацией:

| Engine bytes | Open µs | 1000 inserts µs | Delete µs | 1000 j/k µs | 100 undo/redo pairs µs | 1000 Insert chars µs |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 048 560 | 3 828 | 35 958 | 106 | 337 | 23 088 | 13 412 |
| 10 485 753 | 42 138 | 507 429 | 4 912 | 494 | 612 652 | 185 315 |

| Buffer bytes | Construct µs | Edit p50 ns | Edit p95 ns | Materialize µs |
| ---: | ---: | ---: | ---: | ---: |
| 1 048 576 | 1 | 48 922 | 126 916 | 607 |
| 10 485 760 | 1 | 59 616 | 110 576 | 6 175 |

Один workload run; методика и пределы этих benchmarks описаны в отчёте движка.
Профилирование и оптимизация line index не выполнялись. Для воспроизведения
без registry добавьте `--bench` в offline checker.

## Изменённые файлы

- `crates/embedide-terminal/{Cargo.toml,src/*,tests/*}` — новый содержательный
  frontend crate, standalone entry point, codec, renderer и сквозные тесты.
- `embedide-editor/src/document.rs`, `tests/engine.rs` — read-only indexed
  `line_range` и UTF-8/CRLF regression test; parser/executor/file/history не переписаны.
- Workspace/CLI manifests и `Cargo.lock`, CLI `main.rs`/`tests/cli.rs` — wiring
  `edit`, JSON/batch policy и три пока невыполненных CLI tests.
- `scripts/check-editor-offline.py`, новый `scripts/check-terminal-pty.py` —
  независимая сборка тех же sources, экспорт бинарника, PTY проверки и измерения.
- `.github/workflows/native-rust.yml`, README и native docs — актуальные проверки,
  запуск и ограничения. Старый Electron и пользовательские данные не изменялись.

## Следующий этап

Нужен доверенный cache с locked dependencies или разрешённый доступ crates.io.
После полного workspace check/test/Clippy выполнить три новых CLI tests.
Затем выбрать доступную поддерживаемую terminal library для raw-mode TUI и
заменить только транспорт/renderer, сохранив общий Editor. Нужны real-key input,
resize events, display width/графемы, mouse/Visual highlighting и проверки SSH.
Windows/FreeBSD не проверялись. Список оставшихся Vim-команд и файловых ограничений
остаётся в отчёте движка; GUI/LSP/debug в эту итерацию не добавлялись.
