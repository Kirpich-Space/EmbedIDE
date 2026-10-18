# Итерация: исполнитель Vim-команд и файловая сессия

> Историческое состояние предыдущего этапа. Актуальный проектный workspace,
> графемное редактирование и hardware services: [workspace.md](workspace.md).

Отчёт стабилизации движка от 2026-10-15. Продолжение с проверенным терминальным
frontend и актуальным workspace status: [terminal-editor.md](terminal-editor.md).

Дата: 2026-10-15. Статус: editor crate скомпилирован на Linux musl,
82 теста прошли, Clippy чистый. Проверка всего workspace заблокирована registry-зависимостями.

## Проверено исходное состояние

Перед работой изучены Git status, Cargo workspace, все API TextBuffer/VimInput,
существующие buffer/group/parser tests, зависимости core и CLI. Предыдущие
изменения не удалялись. Входной status содержал изменённые `.gitignore`/README
и незакоммиченные Cargo/crates/docs/scripts. Старый Electron не изменён.

В текущей итерации найдены `/usr/bin/rustc`, Cargo 1.99.0, rustfmt 1.10.0
и Clippy 0.1.99 (дистрибутив Void). Host и единственный установленный target:
`x86_64-unknown-linux-musl`; sysroot `/usr`, rustup отсутствует. Установка
инструментов не выполнялась. Прежний отчёт об отсутствии compiler относился
к предыдущей итерации и больше не описывает это окружение.

`Cargo.lock` существует. Полный `cargo check --workspace --locked` завершился
ошибкой DNS для `index.crates.io`; кэша serde/clap нет. Проверены обычные локальные
каталоги Cargo, прежний `/tmp/embedide-cargo` из depfiles и archroot: подходящие
исходники зависимостей не найдены. Готовые artifacts не используются вместо
пересборки. Ошибка разрешения dependencies не является ошибкой Rust-кода,
но также не подтверждает компилируемость workspace.

## Изменения

| Файлы | Ответственность |
| --- | --- |
| `crates/embedide-editor/src/session.rs` | Публичный Editor, feed/save, rollback mode/prompt и отмена failed prefix |
| `src/document.rs` в editor crate | EditorState, Position/Selection/Register, dirty state, cursor/history metadata |
| `src/executor.rs` | Применение typed Action, единый путь замены диапазонов |
| `src/motion.rs` | Индекс строк, Unicode scalar motions, обратимые line-index deltas |
| `src/search.rs` | Потоковый literal KMP search, совпадения через границы pieces |
| `src/file.rs`, `src/file/platform.rs` | FileBinding, staged save, fingerprints, OS-specific identity/sync |
| `src/lib.rs` в editor crate | Borrowed range/character access; одинаковая замена теперь no-op |
| `src/vim.rs` | Добавлен `^`; Arc keymap для дешёвого clone parser state |
| `tests/engine.rs`, `tests/files.rs` | 37 editor integration tests (26 engine, 11 file) |
| `crates/embedide-core/tests/editor.rs` | Проверка общего core facade для редактора |
| `benches/engine.rs`, editor Cargo.toml | Workload открытия, вставки, удаления, движения, undo/redo |
| Core Cargo.toml/lib.rs, workspace Cargo.toml/Cargo.lock | Зависимость/re-export editor без новых registry packages |
| README и docs/native | Актуальный контракт и ограничения |

Стабилизация этой итерации:

- `motion.rs`/`executor.rs`: исправлены `dw`/`yw` у EOL, `cw` на пробелах,
  остановки `w` на пустых LF/CRLF строках. Для выбранных случаев результаты
  сопоставлены с установленным Vim 9.2 через disposable files (`-Nu NONE -i NONE`).
- `file.rs`/`file/platform.rs`: временные файлы Unix создаются с правами не шире
  `0600`; перед rename проверяется изменение mode/uid/gid/nlink/ctime.
- `tests/buffer.rs`: String-model oracle учитывает только реальные изменения,
  а не идентичные замены. Сам тест и все его проверки undo/redo сохранены.
- `tests/engine.rs`: добавлены 3 регрессионные проверки, одна включает 1500
  детерминированных последовательностей команд с проверками текста/курсора/истории.
- `tests/files.rs`, `src/file/tests.rs`: 8 новых файловых/staging проверок.
- `scripts/check-editor-offline.py`: воспроизводимая сборка тех же исходников,
  тестов и benches через временный Cargo manifest без registry dependencies.
- Все Rust-файлы workspace обработаны rustfmt; workflow теперь проверяет
  `cargo fmt --all -- --check` и `cargo check --workspace --locked`.

Парсер не переписан: все прежние типы и обработка клавиш используются
исполнителем. Mode и активный prompt существуют только в VimInput, без копии
в EditorState. В состоянии документа хранятся cursor, preferred scalar column,
Visual endpoints, unnamed register, search history и file binding.

Публичный API: `Editor::new/open/create/feed/replace/set_cursor/save`, read-only
`state()` и `input()`. Core экспортирует Editor/EditorState/EditorError/Effect
и `core::editor` module facade. CLI пока не имеет команды интерактивного или
скриптового редактирования; GUI отсутствует. Оба frontend могут использовать
этот же API, отдельного альтернативного редактора не создано.

## Исполняемые операции

- `h/j/k/l`, стрелки, `w/b/e`, `0/^/$`, `gg/G`, числовые motions.
- `i/a/I/A/o/O`, literal/IME Insert, Enter/Tab, Backspace/Delete, Escape.
- `d/c/y` с поддерживаемыми motions, `dd/yy/cc`, `ciw/diw/yiw`, operator counts.
- Символьный и построчный register, `p/P` и повтор paste.
- `v/V`, включительные endpoints, motions, удаление/копирование/изменение selection.
- `u`, Ctrl+r, поиск `/`/`?`, `n/N` с повторением и wrap.
- `:w`, `:q`, `:q!`, `:wq`, с реальной записью/проверкой dirty state.

Leader actions возвращаются frontend как Effect::Ide; Build/Debug процессы
редактор сам не запускает. Failed execution восстанавливает предыдущий mode/prompt
и отменяет pending count/operator. Parser errors остаются отдельными ошибками.

## Текст, Unicode и история

Byte offset всегда проверяется на UTF-8 boundary. Position содержит нулевые
line и scalar_column; tab/wide glyph считаются одним scalar. Это **не графемы
и не экранные колонки**. LF/CRLF обрабатываются как строки; CRLF не разделяется
Backspace/Delete. Standalone CR трактуется как символ, не отдельный EOL.
Terminal LF создаёт последнюю пустую логическую строку — это явная семантика API.

Enter/open-line используют первый обнаруженный EOL, paste не перекодируется.
Autoindent пока нет; Tab вставляет `\t`. Движения не копируют документ целиком.
Индекс строк обновляет только структуру starts, включая сдвиг последующих
offsets; это O(число последующих строк), а не логарифмический indexed tree.

Каждая мутация проходит через EditorState::replace: проверка диапазона/будущего
курсора, изменение TextBuffer, индекса и cursor history. Insert/change образуют
одну транзакцию; движение внутри Insert её разделяет. История восстанавливает
текст, cursor и preferred column. Line-index deltas применяются вперёд/назад,
undo/redo не сканируют весь текст заново. Смежные Insert deltas объединяются.
Пустые операции и идентичные замены не добавляют историю; новая ветвь сбрасывает redo.

История ограничена 128 транзакциями. Общего memory budget/compaction пока нет:
append-only store и число pieces могут расти. Повторные вставки ограничены
64 MiB на операцию, но это не общий лимит памяти редактора.

## Файловые гарантии и пределы

UTF-8 open сохраняет BOM/переводы строк; невалидная кодировка даёт ошибку.
Create связывает новый путь, не записывая файл до save. Parent canonicalize
фиксирует выбранный каталог, финальный symlink отвергается при open/save.

Save проверяет baseline (content FNV fingerprint, length, modified time,
Unix device/inode), пишет unique create_new temporary в тот же каталог,
на Unix начинает с приватных прав не шире `0600` (umask может сузить),
копирует permission bits исходника и вызывает sync_all. Новые файлы остаются
с приватными правами. Затем повторяет проверку
baseline и protection metadata и коммитит: rename для существующего файла, exclusive hard_link
для нового. При неподдерживаемом hard_link новый файл не перезаписывается
небезопасным fallback. После коммита выполняется Unix directory sync;
результат доступен в SaveReport. Dirty state сбрасывается только после успеха.

Это не файловая блокировка и не compare-and-swap: остаётся check-to-rename race.
FNV — детектор обычных изменений, не криптографическая гарантия. Универсальная
atomic/power-loss устойчивость не заявляется. Unix mode bits копируются,
owner/group mismatch и multiply-linked files приводят к отказу записи.
ACL/xattr не сохраняются: replacement может потерять эти атрибуты исходника; Windows identity не проверяется по file ID, directory
sync там не заявлен. Windows/FreeBSD поведения ещё требуют настоящих runner tests.

Закрытие изменённого буфера требует `:w`, `:wq` или явного `:q!`. После failed
writequit сессия остаётся открытой, данные в памяти не объявляются сохранёнными.
У untitled буфера без path `:w` даёт NoPath; Save As пока не реализован.

## Тесты и фактические проверки

Реально выполнено **82 теста редактора, 82 passed, 0 failed/ignored**:

| Набор | Тестов |
| --- | ---: |
| file staging unit tests | 4 |
| buffer | 11 |
| edit_groups | 7 |
| engine key traces/model invariants | 26 |
| file save/reload/failures | 11 |
| Vim parser | 23 |

Проверены Unicode, CRLF, курсор/preferred column, движения/границы/пустые строки,
operators/counts/register types, Visual в обе стороны, change+Insert transaction,
undo/redo, новая redo-ветка, literal/wrapped/cross-piece search, 100 000 строк,
String-model replacements, BOM/CRLF reload, savepoints, dirty-close,
external change/deletion/collision, invalid encoding/path, readonly/missing parents,
Unix modes/symlink/hard-link поведение. Проверка смены inode сохраняет прежние
байты и mtime, чтобы подтвердить проверку identity. Проверка staging искусственно
возвращает I/O error после частичной записи и проверяет cleanup; SIGKILL и
отключение питания не тестировались. Permission-denial fixture запускалась
под непривилегированным пользователем (UID 1000). Все 15 файловых/staging
тестов дополнительно прошли на ext4 (`TMPDIR` во временном каталоге внутри
`target/`), помимо основного прогона на tmpfs. Это повтор части тех же 82
тестов, а не ещё 15 новых tests. Для выбора FS fixtures используют `TMPDIR`.

| Проверка | Фактический результат |
| --- | --- |
| `cargo check --workspace --locked` | Blocked: нет DNS crates.io/кэша serde |
| `cargo test --workspace --locked` (`CARGO_NET_OFFLINE=true`) | Blocked: no matching package serde |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (offline env) | Blocked тем же отсутствующим serde |
| `cargo fmt --all -- --check` | Passed после форматирования всего workspace |
| `python3 scripts/check-editor-offline.py --bench` | Passed: реальная Cargo compilation, 82 tests, Clippy all-targets `-D warnings`, оба release benches |
| Изолированный editor `cargo check --target x86_64-unknown-linux-musl --locked --offline` | Passed |

Offline checker не копирует и не подменяет исходники: пути `lib`, `test`, `bench`
временного manifest указывают на файлы этого репозитория. Package metadata,
Rust lints и release profile берутся из workspace. Создаётся отдельный lockfile
без внешних dependencies, основной `Cargo.lock` не изменяется. Это **не проверка
core/project/CLI** и не замена `cargo test --workspace`. Python 3.11+ нужен только
для этого проверочного скрипта, редактор от Python не зависит.

До появления кэша можно воспроизвести:

```bash
cargo fmt --all -- --check
python3 scripts/check-editor-offline.py --bench
```

После предоставления dependency cache или рабочего доступа к crates.io:

```bash
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked --target x86_64-unknown-linux-musl
cargo bench -p embedide-editor --bench engine --bench buffer --locked
```

## Измерения 2026-10-15

Оборудование: ThinkPad product `2325CB6`, Intel Core i5-3320M 2.60 GHz
(2 cores / 4 threads), MemTotal 3 686 272 KiB (~3,52 GiB), Void Linux musl,
Linux 6.18.55_1, rustc 1.99.0/LLVM 23.1.3. Release с thin LTO и strip,
без CPU affinity; рабочая пользовательская система. Fixture находится в
`/tmp` на tmpfs, а не на диске. Открытие после записи тёплое, включает read,
UTF-8 validation, fingerprint и индекс строк. Это не cold-storage benchmark
и не измерение RSS редактора/GUI.

Один финальный прогон после исправлений, время в микросекундах:

| Bytes | Open | 1000 inserts | Delete | 1000 j/k | 100 undo+redo pairs | Insert typing ×1000 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 048 560 | 3 752 | 40 158 | 93 | 336 | 25 687 | 11 207 |
| 10 485 753 | 36 843 | 443 800 | 1 815 | 521 | 661 968 | 419 564 |

Engine fixture состоит из 17-byte ASCII строк. Inserts — 1000 независимых
замен в середине текущего документа (по одному `x`), delete — диапазон
`100..10000`, движения — чередующиеся `j/k`, history — 100 отмен/повторов
последнего удаления. Числа в таблице — суммарное время workload, не каждой
операции. Последний столбец — отдельный свежий документ: `i`, 1000 отдельных
`Key::Char('x')` подряд в середине, Escape. Его open/set_cursor не входят
в typing time; дополнительно проверяется, что одно undo возвращает исходный текст.

Buffer bench (2000 разнесённых однобайтовых вставок):

| Bytes | Construct µs | Edit p50 ns | Edit p95 ns | Materialize µs |
| ---: | ---: | ---: | ---: | ---: |
| 1 048 576 | 0 | 55 286 | 161 144 | 664 |
| 10 485 760 | 1 | 56 615 | 136 102 | 7 939 |

Construct передаёт готовую String во владение и исключает её allocation;
значение 0 отражает разрешение таймера. p50/p95 описывают разные операции
одного прогона с растущим числом pieces, а не распределение повторных запусков.
Сравнения с прежней версией/Rope, cold-cache и peak RSS пока нет. Значительный
рост цены Insert/undo на 10 MiB согласуется с линейными сдвигами line starts;
крупная оптимизация в этой итерации не выполнялась.

## Совместимость и следующий конкретный этап

Это Vim-подобная модель, не полная совместимость с Vim. Кроме особенностей
Unicode scalar и terminal LF, `b`/`e` ещё не моделируют пустые строки как Vim;
полного набора special operator-motion range rules нет. `dgg`, Visual Block,
regex, named registers/clipboard, macros, counted Insert, Save As и undo tree
ещё не реализованы. ACL/xattr preservation, handle-based race protection,
Windows/FreeBSD и реальные disk-failure/power-loss проверки остаются открытыми.

Сначала нужен dependency cache из `Cargo.lock` либо доступ к crates.io: выполнить
полный workspace check/test/Clippy и устранить возможные межмодульные ошибки.
Затем подключить **существующий** `core::Editor` к минимальной CLI-команде,
проверяя open → keys → save → reopen отдельным black-box тестом. CLI/GUI
интеграция в этой итерации не расширялась, поскольку условие успешной проверки
всего workspace ещё не выполнено. GUI/LSP/debug не начинались.
