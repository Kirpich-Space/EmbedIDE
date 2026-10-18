# Технический отчёт: первый этап EmbedIDE 2.0

Это исторический отчёт первоначального аудита. Актуальные результаты Rust
compilation/tests/benchmarks: [editor-engine.md](editor-engine.md).

Дата: 2026-10-15. Репозиторий: `/home/kirpich/project/EmbedIDE`.

## Обнаружено

Основной desktop — JavaScript/Electron 33, TypeScript/React 18,
CodeMirror 6, Vite 5; сборка npm и electron-builder. README описывает старую
версию, package.json имеет версию 2.6.2. Нативного Rust GUI в исходном дереве нет.

По исходникам доступны вкладки/редактирование/поиск, дерево проекта, темы,
локализация, генераторы firmware/driver/OS/host-script, board-aware build/flash,
batch OpenOCD/GDB, UART через Python/pyserial, необязательные AI providers.
Это перечень реализации в коде, а не заявление о проверке GUI или аппаратуры.

Полезные части: 19 генераторов в `electron/project.js`, 67 записей
`electron/boards.js`, диагностические примеры из `src/core/parseDiagnostics.ts`,
данные тем/переводов/шрифтов. Генераторы и каталог проверены на совместимость
с существующим CLI. Их корректность на реальных MCU не проверялась.

Основной долг: browser runtime; связанная логика UI/IPC; генераторы в одном
модуле примерно 1500 строк; отсутствие отдельного модального ядра и LSP;
GDB batch вместо интерактивной сессии; накопление строк вывода в памяти;
завершение непосредственного процесса не гарантирует завершение всего дерева.
Сопоставление diagnostics по basename допускает ошибки при одинаковых именах.
Serial преобразует поток в строки UTF-8 и не является готовым binary transport.

При старте `git status --short --branch` показал:

```text
## main...origin/main
 M .gitignore
?? Cargo.lock
?? Cargo.toml
?? crates/
```

Уже имелись три Rust crates, CLI `embedide`, исходные тесты и готовые ELF
бинарники в `target/debug`. Поэтому новый фундамент с нуля не создавался.
Имеющийся CLI предоставляет `init`, `open`, `build`, `doctor`, `toolchains`,
`targets`, `config`, JSON, build dry-run и offline Cargo. Команд flash/debug/
serial в нём пока нет.

Среда: Void Linux x86_64 musl 1.2.6; GCC 14.2.1, GNU Make 4.4.1,
Node 24.18.0, npm 11.16.0, Python 3.14.8, Fish, rust-analyzer.
Rust/Cargo/rustfmt/Clippy, CMake, Clang, Zig, GDB, OpenOCD и ARM GCC
в текущем PATH отсутствуют. Попытка обратиться к static.rust-lang.org через
curl закончилась `Could not resolve host`. Пакеты не устанавливались.

## Принятые решения и выполненная работа

Архитектура: небольшой Cargo workspace, frontend-independent project/core/editor,
общие сервисы CLI и будущего GUI; отдельные адаптеры протоколов и ОС по мере
реальной реализации. Подробности: [architecture.md](architecture.md).
Последовательность и критерии переходов: [migration.md](migration.md).

Добавлен `embedide-editor` без внешних зависимостей: UTF-8 Piece Table,
валидация байтовых диапазонов, replace/slice/chunks, потоковая запись в writer,
ограниченная по числу транзакций история undo/redo, saved revision.
Это реализация текстового ядра; Vim, file session и GUI ещё не подключены.
Исходники имеют 11 новых Rust contract tests, включая 1000 детерминированных
Unicode-редактирований со сравнением со String и восстановлением всех ревизий.
**Эти новые тесты пока не исполнялись: нет компилятора.**

Для GUI после сравнения Slint, GPUI, egui и Qt выбран eframe/egui как рабочий
кандидат следующего этапа. Решение, источники и обязательные проверки описаны
в архитектуре; GUI dependencies не добавлены. FreeBSD и работа на X230
не заявляются проверенными.

## Созданные и изменённые файлы

| Файлы | Содержание |
| --- | --- |
| `docs/native/architecture.md` | Решения, сравнение GUI/Rope/Piece Table, границы модулей, ограничения |
| `docs/native/migration.md` | Поэтапная миграция и критерии готовности |
| `docs/native/session-report.md` | Этот отчёт |
| `crates/embedide-editor/Cargo.toml` | Новый пакет и benchmark target |
| `crates/embedide-editor/src/lib.rs` | Текстовый буфер и явные ошибки |
| `crates/embedide-editor/tests/buffer.rs` | 11 тестов буфера |
| `crates/embedide-editor/benches/buffer.rs` | Детерминированный benchmark 1/10 MiB, 2000 вставок |
| `scripts/check-native-cli.py` | 12 black-box контрактов готового бинарника |
| `scripts/bench-native-cli.py` | Замер warm-cache запуска doctor с идентификацией бинарника |
| `.github/workflows/native-rust.yml` | Подготовлены Rust test/Clippy, editor fmt, Linux/Windows/musl jobs |
| `Cargo.toml`, `Cargo.lock` | Только регистрация нового локального пакета, без изменения зависимостей |
| `README.md` | Ссылки на миграцию, статус native и команды проверки |

Все существовавшие `crates/*` файлы и `.gitignore` сохранили исходные SHA-256.
Из входных Rust manifests изменились только workspace members и локальная запись
editor в lockfile. Контрольные суммы registry dependencies не менялись.
Старая реализация, лицензия и пользовательские проекты не изменялись.
Commit, push и публикация не выполнялись.

## Проверки и результаты

| Проверка | Результат |
| --- | --- |
| Новый `check-native-cli.py` против имеющегося бинарника | **12/12 passed**, без пропусков |
| Реальная C-сборка Make/GCC и запуск результата | Passed; результат `native-ok` |
| Реальная ошибка GCC | Passed; CLI и JSON вернули код 2 |
| Совместимость шаблонов | Все 19 открыты CLI; байты всех файлов сохранены |
| Сравнение каталогов MCU | Все 67 записей совпали с JS source целиком |
| Сохранённый core test binary | **2/2 passed** |
| Сохранённый CLI integration test binary | **9/9 passed** |
| rust-analyzer parse новых `.rs` | 0 syntax error nodes в library/tests/benchmark |
| Python AST новых скриптов, TOML/lockfile | Синтаксис корректен; все 4 пакета присутствуют в lockfile |
| `node --check` существующих Electron modules | Все 10 прошли |
| `git diff --check` | Passed |
| `cargo test`, `cargo fmt`, `cargo clippy` | **Не выполнены**, `cargo: command not found` |
| `npm run typecheck` | Не прошёл: отсутствуют node_modules React/CodeMirror/types; старый GUI не пересобран |
| Новый GitHub workflow | Записан, не запускался; локального YAML parser нет |

Сохранённые бинарники не пересобирались и не доказывают компилируемость текущих
исходников. Проверенный `target/debug/embedide` — статический musl ELF,
16 540 568 байт, debug artifact:

```text
SHA-256: 5e7e8951d88f67302aa09c079e63e5c3e4ff4cf3b87585c8a56676f33a846126
```

Замер этого бинарника: 30 запусков `doctor --json` после одного warm-up,
включая создание процесса и PATH inventory, Linux 6.18.55_1 x86_64 musl:
median **3.674 ms**, p95 **6.067 ms**, min 2.839 ms, max 7.473 ms.
Это baseline конкретного окружения с тёплым файловым кэшем, **не холодный запуск**,
не сравнение версий и не подтверждение требований X230. RSS не измерялся.
Benchmark редактора подготовлен, но не выполнялся.

## Воспроизведение

Без компилятора, для уже имеющегося бинарника:

```bash
python3 scripts/check-native-cli.py --binary target/debug/embedide
python3 scripts/bench-native-cli.py --binary target/debug/embedide --samples 30
```

С Rust toolchain и кешем зависимостей:

```bash
cargo test --workspace --locked --offline
cargo fmt -p embedide-editor -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build -p embedide-cli --locked --offline
python3 scripts/check-native-cli.py --binary target/debug/embedide
cargo bench -p embedide-editor --bench buffer --locked --offline
```

В Windows исполняемый файл — `target/debug/embedide.exe`; Python запускается
доступной в окружении командой `python`/`python3`. В CI зависимости загружаются
при первичной сборке; offline-флаги выше требуют предварительно заполненного cache.
Полный `cargo fmt --all -- --check` остаётся отдельной задачей: входной
`crates/embedide-cli/tests/cli.rs` имеет formatting debt и в этой сессии сохранён.

## Оставшиеся ограничения и следующий этап

Сначала требуется компиляция новых исходников и выполнение Rust tests/Clippy/fmt
в доступном toolchain. Пока этого нет, D нельзя считать принятым. Затем нужны
измерения Piece Table против Rope, memory budget/compaction, индекс строк,
безопасная file session и Vim parser. После этого — cancellable build worker
и первое нативное окно. Hardware integration, полноценный debugger, UART,
LSP/Tree-sitter, Windows/FreeBSD validation и оптимизация остаются следующими
этапами. Объём миграции существенно больше смены UI: переносятся runtime,
редактор, генераторы, протоколы, управление процессами и упаковка.
