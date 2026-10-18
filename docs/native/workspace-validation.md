# Workspace: проверки и измерения — 2026-10-17

Результаты относятся к исходникам этого этапа; исторические замеры в старых
отчётах не заменяют эти проверки. Физического MCU/probe не использовали.

## Окружение и функциональные проверки

Void Linux musl, Linux 6.18.55_1, x86_64, Intel Core i5-3320M 2.60 GHz,
MemTotal 3 686 272 KiB, Rust/Cargo 1.99.0, LLVM 23.1.3. Host target
`x86_64-unknown-linux-musl`. Проверена именно musl-сборка, glibc/Windows/FreeBSD
локально не запускались. Remote CI изменён, но его выполнение ещё не подтверждено.

| Проверка | Результат |
| --- | --- |
| `cargo check --workspace --locked --offline` | passed |
| `cargo test --workspace --locked --offline` | 158 passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | passed |
| `cargo build --release --workspace --locked --offline` | passed, native musl |
| `check-workspace-pty.py --binary target/release/embedide` | 7/7 |
| `check-tui-pty.py --binary target/release/embedide-terminal` | 12/12 |
| `check-tui-pty.py --binary target/release/embedide --cli` | 12/12 |
| `check-terminal-pty.py --binary target/release/embedide-terminal` | 4/4 |
| `check-native-cli.py --binary target/release/embedide` | 12/12 |
| `check-editor-offline.py --terminal` | 107 passed, check/Clippy passed; TUI/core/CLI отключены |
| CLI build `examples/native/firmware-logic` | реальная Cargo `no_std` library build, offline |
| CLI build `examples/native/make-smoke` | реальная Make/GCC-сборка |
| CMake configure/build | executable отсутствует; только планы/аргументы протестированы |
| probe-rs/OpenOCD/GDB | executables отсутствуют; аппаратное выполнение не проверялось |

Новые Rust-проверки: 13 core service contracts, 4 Unicode regressions,
7 workspace contracts, 2 CLI shorthand regressions. Всего добавлено 26 к прежним 132.
PTY workspace проверяет explorer/file finder, несколько документов и сохранение,
редактирование во время фоновой сборки, structured Rust diagnostics и source jump,
resize до 2×2, фокус/диалоги, защиту всех dirty documents, отмену process group,
SIGTERM cleanup и полезный отказ flash без target.

Core integration tests используют временные проекты и executables с пробелами/Unicode,
не изменяют global PATH. Проверяются incremental compiler output, nonzero exit,
пропуск последующих steps, missing executable, cancel с шумным выводом и потомком,
Cargo JSON/GCC/Clang-style paths/CMake errors, layered config, ELF mismatch,
неоднозначный probe selector, symlink/overwrite protection и MI nested parsing.
Подставной GDB проверяет acknowledged initialization, Registers command и unexpected
termination. Это не реальный debugger на MCU. Serial-тест создаёт настоящую пару
Unix PTY, открывает slave через serialport, принимает binary/invalid UTF-8 bytes,
передаёт outgoing LF text и проверяет receive-only. Это не аппаратная проверка UART.

Новый real Cargo PTY-тест выявил ошибочный `json-render-diagnostics`, который Cargo
сам отображает текстом. Адаптер исправлен на `--message-format=json`; Problems теперь
получает compiler-message records. PTY также выявил слияние command/output строк;
Log исправлен. Эти сценарии остались в suite.

## Производительность

Release thin-LTO/strip, локальный тёплый `/tmp`, реальные PTY 80×24.
Нет измерения cold disk, SSH transport и рендеринга настоящим эмулятором терминала.
Ниже — воспроизводимые команды и результаты замера.

```sh
python3 -B scripts/check-workspace-pty.py --binary target/release/embedide --measure
python3 -B scripts/check-tui-pty.py --binary target/release/embedide-terminal --measure
cargo bench -p embedide-terminal --bench tui --locked --offline
```

| Старт | Медиана | Min–max | RSS |
| --- | --- | --- | --- |
| Workspace, 100 root dirs, 10000 файлов | 2.381 ms (10 запусков) | 1.873–5.064 ms | 2984 KiB |
| Минимальный редактор, пустой файл | 1.983 ms (5 запусков) | 1.350–3.458 ms | 1556 KiB |
| Минимальный редактор, 1 MiB | 7.659 ms (5 запусков) | 6.714–8.818 ms | не измерялся |
| Минимальный редактор, 10 MiB | 46.771 ms (5 запусков) | 43.825–49.194 ms | не измерялся |

Оба idle сеанса: 0 CPU ticks / 500 ms при 100 Hz. SHA-256 измеренных binaries:

```text
embedide          4f9f1755256e3a8cdf629c0775829af7202d5fdbd140b989523eeafc08e4ccea
embedide-terminal b9eab711102e8b832a89bf6d8d30ac02a1f5fb77b996844e0545ac690b0aca0b
```

`file target/release/embedide` подтверждает stripped x86-64 ELF с dynamic musl
interpreter `/lib/ld-musl-x86_64.so.1`. Это не заявляется как static portable artifact.

Последний benchmark измеряет Event → engine → frame → diff, без PTY I/O и без
project panels. 1000 вставок в середину, 1000 horizontal moves, 100 undo/redo пар:

| Документ | Typing p50 / p95 | 1000 moves | 100 undo/redo |
| --- | --- | --- | --- |
| 1 048 560 bytes | 0.327 / 0.599 ms | 34.147 ms | 62.682 ms |
| 10 485 753 bytes | 0.922 / 3.477 ms | 61.544 ms | 249.148 ms |

Один прогон, без статистической гарантии. Typing здесь медленнее прежнего отчёта
(0.277/0.741 ms p50); графемная корректность добавляет работу, сравнение также зависит
от фоновой нагрузки и кэшей. Ускорение не заявляется. Для серьёзных выводов нужен
повторный изолированный baseline по одному и тому же commit/build/environment.

Старт workspace не запускает инструменты, USB scans или debugger. Дерево читает
один root directory; benchmark с 10000 файлами использует 100 root directories
по 100 файлов, поэтому не означает полную индексацию 10000 файлов за время старта.
Idle CPU измеряется ticks за 500 ms при CLK_TCK=100: нулевой результат означает
менее разрешения замера, не доказательство нулевого потребления.

Отклик во время build и resize покрыт PTY assertions, но отдельные распределения
latency, скорость вывода MB/s, RSS длительного serial/debug сеанса и stress test на
миллионах project entries пока не измерялись. Буферы ограничены: output queues,
2 MiB/2000 строк на log, 1000 problems, 4 MiB editor layout cache. Из-за таких лимитов
часть вывода может быть утрачена; output overflow показывается счётчиком, это не
полный журнал для аудита прошивки.

## Границы готовности и следующие проверки

Реализовано и проверено локально: project TUI, документы, фоновые процессы,
Cargo/Make builds, diagnostics navigation, grapheme editing, конфигурация,
Unix process cleanup, serial I/O на PTY и debugger protocol на mock.
Реализовано без hardware verification: probe-rs ELF workflow, serial device enumeration,
GDB/MI remote operations и OpenOCD lifecycle. CMake проверен без установленного CMake.
Zig сохранён из прежнего этапа, executable отсутствует.

Не реализовано: PlatformIO/Arduino/esptool adapters, FQBN/core management, остальные
flash backends, полноценный PTY shell, LSP, полная `.gitignore`/recursive fuzzy indexing,
syntax/bracket/current-line/search highlighting, session persistence, tool version/
compatibility probing, immutable flash snapshot, полный breakpoint/register UI.

Следующие acceptance steps:

1. Установить CMake выбранным пользователем способом; собрать cmake-smoke настоящим
   CMake+Make/Ninja, затем cross toolchain file с реальным ARM compiler.
2. Подключить заведомо совместимые MCU и probe; записать версии probe-rs/OpenOCD/GDB,
   выполнить list/info, проверить selector, ELF/chip mismatch и cancel, затем flash
   с отдельным подтверждением и физической проверкой поведения firmware.
3. Проверить UART на плате: framing, baud, permissions, unplug/reconnect, длительный
   поток, overflow counters, timestamps, send и export.
4. Проверить GDB remote: symbols, breakpoint hit, step/continue/interrupt, backtrace,
   registers, disconnected probe/occupied port/timeout, отсутствие running children.
5. Запустить Linux glibc, Windows, FreeBSD и SSH/реальные терминалы; снять повторные
   latency/RSS/CPU/throughput baselines, оптимизировать выборку видимых строк log.
