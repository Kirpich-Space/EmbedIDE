# Полноэкранный редактор — 2026-10-16

> Историческое состояние предыдущего этапа. Актуальный проектный workspace,
> графемное редактирование и hardware services: [workspace.md](workspace.md).

## Сборка и запуск

```sh
cargo build --release --locked
./target/release/embedide edit example.rs
# Тот же frontend без основного CLI:
./target/release/embedide-terminal example.rs
```

Существующий файл открывается через FileBinding, отсутствующий путь создаёт
новый буфер. Родительский каталог должен существовать. Нужны терминальные
stdin/stdout и ANSI-совместимый терминал; `TERM=dumb` отвергается.
Нажмите `i`, введите текст, Esc, затем `:wq` и Enter. Повторный запуск читает
сохранённый файл. Интерактивные команды не требуют Enter, Ex/search требуют.

`--line` сохраняет прежний построчный режим; `--batch` — протокол для скриптов.
Они взаимоисключающие. JSON разрешён только для batch:
`printf 'iHello<Esc>\n:wq\n' | ./target/release/embedide --json edit --batch new.rs`.
Без default features standalone собирается с прежним line frontend.

## Архитектура и клавиши

`embedide-terminal/src/tui/` содержит event adapter, layout, row-diff renderer
и RAII terminal guard. `TuiEditor` передаёт Key существующему Vim parser/executor
через общую session; текст, курсор, регистры, история и файл остаются в Editor.
Общее ядро и его исходники не изменены. GUI сможет использовать тот же Editor
без terminal viewport и экранных координат.

Лёгкий crossterm 0.29 выбран для portable raw mode/events вместо собственного
unsafe termios. Unicode layout использует unicode-segmentation и unicode-width;
Unix signal flags — signal-hook. Async runtime и фоновый renderer не добавлены.

Работают `i/a`, Esc, `h/j/k/l`, arrows, `w/b/e`, `0/^/$`, `gg/G`, счётчики,
`d/c/y`, `dd/yy/ciw`, `p/P`, `u`, Ctrl+r, Visual, `/`, `?`, `n/N`,
`:w`, `:q`, `:q!`, `:wq`. Поиск буквальный с wrap; полноценной совместимости
с Vim нет. Неизвестные клавиши отменяют pending prefix и показывают сообщение.
Bracketed paste передаётся как текст, не исполняет Normal-команды.
Ctrl+C отменяет текущий ввод/режим без потери буфера; выход — через Ex.

## Отображение и terminal lifecycle

Есть номера строк, filename, dirty `*`, режим, координаты курсора, prompt и ошибки.
Viewport прокручивается вертикально и горизонтально; переноса длинных строк нет.
Tab равен 4 cells. Grapheme clusters не разрезаются при clipping; wide/combining
и emoji учитываются в layout. Неоднозначная ширина считается 1; реальный шрифт
и терминал могут отличаться. Управляющие и bidi-control символы экранируются.
Engine по-прежнему перемещает/удаляет Unicode scalars: внутри графемы экранный
курсор стоит в её первой cell. Полной графемной модели редактирования ещё нет.

Для строк длиннее 16 KiB действует streaming scalar fallback: non-ASCII
отображается escape-последовательностями. Обход до курсора линейный. Кэш layouts
имеет бюджет 4 MiB и сбрасывается при изменении текста. Видимая сетка ограничена
256 строками и 1024 колонками; последняя колонка резервируется. Это ограничения
frontend, не размера документа. Неизменённые строки не отправляются повторно;
identical frame вообще не пишет в терминал.

Raw mode, alternate screen, bracketed paste и cursor shape восстанавливаются
guard при выходе, recoverable error и Rust unwind. Unix SIGINT/SIGTERM/SIGHUP
запрашивают orderly exit без автосохранения; проверка флага каждые 250 ms.
SIGWINCH имеет резервный flag-путь: в этом окружении crossterm Resize event
не приходил. Timeout не перерисовывает экран. SIGKILL/SIGSTOP, abort и потеря
терминального канала не позволяют гарантировать очистку экрана.

## Сохранность файлов

Прежний файловый слой не ослаблен: временный файл, sync, baseline fingerprint,
readonly/symlink/hardlink проверки. `:q` отвергает dirty buffer; `:q!` явно
отбрасывает его. Ошибка `:w/:wq` сохраняет dirty state, историю и открытый буфер;
после ошибки Ex нажмите Esc для новой команды. Внешняя правка блокирует запись.
Произвольный shell через Ex не исполняется. TOCTOU-окно и ACL/xattr preservation
не устранены; абсолютная atomic/power-loss гарантия не заявляется.
Подробности: [editor-engine.md](editor-engine.md).

## Подтверждённые проверки

Rust/Cargo 1.99.0, Void Linux musl, target `x86_64-unknown-linux-musl`.
Прежний DNS/registry blocker снят разрешённым Cargo-запуском вне sandbox;
зависимости получены с проверкой TLS/checksums. Системные DNS/proxy не менялись.
Cargo.lock обновлён Cargo для optional terminal dependencies.

- `cargo check --workspace --locked` и release build — успешно.
- `cargo test --workspace --locked`: **132 passed, 0 failed** (83 editor,
  13 CLI, 3 core/facade, 33 terminal, включая 13 новых TUI tests).
- `cargo fmt --all -- --check` и workspace all-targets Clippy `-D warnings` — успешно.
- `check-tui-pty.py`: **12/12** standalone и **12/12** main CLI;
  `check-terminal-pty.py`: **4/4** прежних line tests.
- `check-native-cli.py`: **12/12** contracts; offline editor/line checker:
  **103 passed**, TUI намеренно отключён.

PTY проверяют immediate input, Unicode edit/save/reopen, регистры, search,
undo/redo, CRLF, resize, dirty quit, readonly/external writes, bracketed paste,
invalid Ex/open, SIGINT/SIGTERM/SIGHUP. Termios сравнивается до/после, проверяются
show-cursor/leave-alt/disable-paste sequences. Новые проверки включены в CI;
удалённый CI, Windows, FreeBSD и реальная SSH-сессия здесь не запускались.

## Измерения

ThinkPad X230, i5-3320M, 3 686 272 KiB RAM, Linux 6.18.55_1, Rust 1.99.0;
release thin-LTO/strip. Warm tmpfs, PTY 80×24, 5 запусков на размер; время
от Popen до первого mode marker. Это не холодный диск и не latency реального SSH.

| Размер | Медиана запуска | Min–max |
| --- | --- | --- |
| Пустой | 1.83 ms | 1.13–2.64 ms |
| 1 MiB | 5.78 ms | 4.67–8.79 ms |
| 10 MiB | 42.97 ms | 42.21–44.65 ms |

Empty-session RSS: **1548 KiB**; за 500 ms idle — 0 CPU ticks при 100 Hz.
Это разрешение измерения, не доказательство нулевого CPU. Воспроизведение:
`python3 -B scripts/check-tui-pty.py --binary target/release/embedide-terminal --measure`.
SHA-256 измеренного executable:
`8280cbfc208ffec1a2c329bbc1286237beb18d2b8caaf891e48b59b8cabae077`.

`cargo bench -p embedide-terminal --bench tui --locked --offline` измеряет
Event → engine → frame → diff, без реального terminal I/O. ASCII строки по
17 bytes, 1000 вставок в середину, 1000 horizontal moves, 100 undo/redo пар:

| Документ | Вставка p50 / p95 | 1000 moves | 100 undo/redo |
| --- | --- | --- | --- |
| ~1 MiB | 0.277 / 0.472 ms | 56.3 ms | 65.4 ms |
| ~10 MiB | 0.741 / 2.380 ms | 48.3 ms | 204.0 ms |

Это один прогон, без сравнения с прежней реализацией; ускорение не заявляется.
Исторические editor benchmarks сохранены в прежнем отчёте.

## Следующий этап

Проверить реальные терминалы/SSH и Windows CI, затем согласовать графемные
движения общего engine с display mapping. GUI/LSP/debugger в эту итерацию не
начинались; длинные строки и память длительных сессий требуют отдельных замеров.
