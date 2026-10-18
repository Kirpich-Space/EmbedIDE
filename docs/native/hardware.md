# Native hardware services: текущие возможности

Реальная прошивка, отладка MCU и физический UART в этой итерации **не проверялись**.
В окружении нет probe-rs, OpenOCD, GDB и подключённой тестовой платы. Проверены
планы команд, защитные отказы, подставной GDB/MI и настоящий OS serial I/O через PTY.
Это основа интеграции, не сертифицированная поддержка списка плат.

## Обнаружение инструментов и устройств

Discover Toolchains / CLI `toolchains --json` / `doctor --json` проверяют PATH
без запуска исполняемых файлов и сетевых запросов. Наличие executable не доказывает
его версию, работоспособность или совместимость с платой. Версии пока не собираются.
Ищутся rustc, cargo, GCC/G++, Clang/Clang++, arm-none-eabi-gcc/gdb, GDB,
gdb-multiarch, OpenOCD, probe-rs, CMake, Ninja, Make, Zig, Arduino CLI, pio,
esptool, rg, clangd, rust-analyzer. Последние два не означают наличие LSP-клиента.

Discover Probes явно запускает `probe-rs list` через тот же ограниченный process
manager; результат настоящий текст CLI. IDE не угадывает selector и совместимость.
Discover Serial Ports использует `serialport::available_ports()` в отдельном worker:
показывает actual device names и доступные USB/OS metadata. Пустой список остаётся
пустым. Без libudev (зависимость собрана с default-features=false) набор metadata
зависит от ОС/sysfs. Никакие права устройств/группы пользователя автоматически
не меняются. Tool paths задаются в project/local configuration; сами инструменты
нужно установить отдельно согласно документации их разработчиков.

## Цель и probe-rs flash

`native.targets` содержит только настроенные пользователем цели. Board display name,
compiler target triple, chip, debug probe и ELF artifact — разные поля. Select Target
выбирает запись, не подставляет произвольные chip/FQBN. Первая настроенная запись
активна при открытии; это не разрешение на прошивку. Никакие операции открытия,
сохранения и сборки не запускают flash.

Для Flash Firmware нужны:

1. Сохранённые документы, собранный ELF, завершённый task/build.
2. Закрытые serial/debug sessions.
3. Явные `chip`, `probe=VID:PID:serial`, `artifact` в выбранном target.
4. Установленный probe-rs и подключённый именно выбранный probe.
5. Просмотр конкретной команды и отдельное `y` в диалоге.

План проверяет обязательные поля, запрещает неоднозначный VID:PID без serial,
проверяет ELF magic и сопоставляет ARM/RISC-V e_machine с явно заданным triple.
Это sanity check архитектуры, не полная валидация платы, flash layout и chip revision.
Команда использует точные аргументы:

```text
probe-rs download --chip CHIP --probe VID:PID:SERIAL --verify /absolute/firmware.elf
```

Реальную проверку chip/probe и flash verification выполняет установленный backend.
Нет команды mass erase, unlock, fuse/security write или автоматического reset/run.
BIN/HEX/UF2 и другие flash backends пока не подключены. probe-rs может сам стирать
сектора, необходимые для явно подтверждённой записи: этот workflow действительно
изменяет firmware. Ненулевой exit сохраняется как ошибка; нулевой означает успех
CLI с запрошенной verification, не подтверждение поведения прошивки.

При подтверждении конфигурация и ELF повторно проверяются. Artifact не копируется
в неизменяемый snapshot и не закрепляется криптографическим хешем; внешний процесс
может изменить его между проверкой и чтением backend. Не меняйте artifact во время
операции. Это известное ограничение первого flash workflow. Внутри IDE параллельная
конфликтующая сборка/прошивка не запускается.

## Serial monitor

Пример локального override (файл `embedide.local.json`):

```json
{
  "serial": {
    "port": "/dev/ttyACM0",
    "baud": 115200,
    "data_bits": 8,
    "parity": "none",
    "stop_bits": 1,
    "flow_control": "none",
    "line_ending": "lf",
    "receive_only": false
  }
}
```

Путь — пример, а не обнаруженное устройство. Сначала Discover Serial Ports,
затем Select Serial Port / Baud (device и baud через пробел), Connect Serial Monitor.
Другие framing settings меняются файлом и Reload Settings до подключения.
Поддержаны 5–8 data bits, parity none/odd/even, 1/2 stop bits, flow control
none/software/hardware, endings none/lf/crlf и произвольный положительный baud,
если драйвер его принимает. По умолчанию `receive_only=true`; Send Serial Text
защищён и в сервисе, а не только UI. Outgoing text — до 4 KiB, очередь до 32 сообщений.

Receive worker имеет 30 ms timeout, очередь до 128×4 KiB. Pane хранит до 2000 строк,
2 MiB, до 8192 bytes в строке. Потерянные при переполнении очереди bytes показываются
счётчиком; старые строки вытесняются. Pause сохраняет отдельный ограниченный snapshot,
но чтение порта продолжается. После Resume виден доступный хвост, а не бесконечный архив.
Есть опциональные timestamps на chunk, clear, ручная прокрутка/возврат в конец,
export в новый workspace-relative файл без перезаписи. Clipboard интеграции пока нет.

Сервис сохраняет raw bytes, TUI декодирует для показа через from_utf8_lossy и экранирует
управляющие символы. Невалидный UTF-8 не вызывает panic. Граница read chunks может
разделить валидный символ и дать replacement glyph; экспорт — отображённый текст,
не побайтовая запись serial-трафика. Binary/hex view и streaming UTF-8 decoder —
следующий этап. Reconnect ручной; ошибки доступа/отключения отображаются без chmod
и без изменения групп пользователя. Disconnect закрывает worker/port.

## GDB/MI и OpenOCD

Пример `native.debug` или локального `debug`:

```json
{
  "debug": {
    "gdb": "arm-none-eabi-gdb",
    "openocd": "openocd",
    "configs": ["interface/cmsis-dap.cfg", "target/rp2040.cfg"],
    "remote": "127.0.0.1:3333",
    "elf": "target/thumbv6m-none-eabi/debug/blink"
  }
}
```

Эти cfg и ELF должны соответствовать вашему оборудованию. Можно поставить
`openocd=null`, `configs=[]` и подключиться к явно указанному уже работающему серверу.
Для собственного OpenOCD допускается loopback-адрес; занятый порт отклоняется,
startup failure отображается, ожидание ограничено 8 секундами. Настройки OpenOCD
являются исполняемым Tcl-кодом; перед Start Debugging IDE показывает их и требует `y`.
Выбор startup commands, transport, SWD speed, probe serial и прочих параметров пока
делается самими доверенными OpenOCD cfg, отдельного board wizard нет.

GDB запускается с `--nx --nh --quiet --interpreter=mi2`. До загрузки ELF отключается
auto-load; затем включается mi-async, загружаются symbols и выполняется
`-target-select remote`. Каждый стартовый запрос получает числовой token;
ошибка или timeout завершает owned processes. Connected показывается только после
ответа GDB, а не нажатия кнопки. Никакого `load`/flash/reset при старте не выполняется.

Реальные MI-команды, доступные из палитры:

- Continue / Interrupt: `-exec-continue`, `-exec-interrupt --all`.
- Step / Next: `-exec-step`, `-exec-next`.
- Breakpoint at Current Line: `-break-insert "file:line"`.
- Remove Breakpoint: `-break-delete ID` (числовой ID вводится явно).
- Backtrace: `-stack-list-frames`.
- Registers: `-data-list-register-values x`.
- Stop: остановка GDB и принадлежащего IDE OpenOCD, освобождение pipes/workers.

Parser обрабатывает result/async records, nested tuples/lists, escaped C strings,
числовые tokens; ограничивает запись 64 KiB и глубину 32. Панель показывает status,
последний breakpoint response, structured stack и numeric register values, console
и ошибки. На `*stopped` fullname/file + line открывается в общем редакторе. Нет
полной таблицы breakpoints, mapping имён регистров, watch expressions, memory view,
DAP, multi-core selection или произвольного GDB CLI input. Стартовые ответы
проверяются последовательно, последующие ошибки команд показываются в статусе.

stdout/stderr и MI обрабатываются в worker; вывод ограничен. Потеря входных MI данных
или переполнение очереди structured records останавливает сеанс, чтобы не продолжать
с неверным состоянием; финальный error имеет отдельный слот. Console stream records
пока остаются исходным MI-текстом, а не полноценным terminal emulator.

Процессы на Unix имеют собственную process group: отмена и Drop убивают группу и
reap прямого ребёнка. На Windows предусмотрен taskkill /T /F и прямой kill/wait;
этот путь не проверен в текущем окружении. FreeBSD и физическое оборудование также
требуют отдельных acceptance tests. Проектные команды используют аргументы, не shell.

Протоколы и команды следует сверять с установленной версией инструментов:
[probe-rs CLI](https://probe.rs/docs/tools/probe-rs/),
[GDB/MI specification](https://sourceware.org/gdb/current/onlinedocs/gdb.html/GDB_002fMI.html),
[OpenOCD guide](https://openocd.org/doc/html/General-Commands.html).
PlatformIO/Arduino CLI/esptool сейчас только обнаруживаются в PATH; их build/upload
adapters, FQBN/core management и board discovery не реализованы.
