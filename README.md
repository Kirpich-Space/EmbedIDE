# EmbedIDE

**Offline-first engineering IDE for embedded flight computers**

Built by [Kirpich Space](https://github.com/Kirpich-Space). Cross-platform desktop IDE (Electron + React + CodeMirror) for ARM Cortex-M firmware — Rust, C, C++, and Assembly. No Arduino, ESP32, or AVR targets.

## Features

- Multi-language editor — Rust, C, C++ (CodeMirror 6)
- **20 target boards** — STM32 F4 / F7 / H7 / G4 / L4 / U5 flight-class MCUs
- Build & flash via OpenOCD (board-aware profiles)
- Serial monitor
- Memory analyzer (flash/RAM from board profile)
- Themes (dark & light engineering palettes)
- UI languages: English, Russian, Chinese, Japanese, German, French
- File search across the project
- Optional AI assistants — **off by default**; Local (Ollama) or Cloud when enabled
- Works fully offline for edit / build / flash / serial (toolchains installed on the host)

## Supported boards (v1.1)

| Family | Boards |
|--------|--------|
| STM32F4 | F401RE, F405RG, F407VG, F411CE, F412ZG, F429ZI, F446RE, F469NI |
| STM32F7 | F722ZE, F746NG, F767ZI |
| STM32H7 | H743ZI, H750VB, H753ZI, H7A3ZI |
| STM32G4 | G431CB, G474RE |
| STM32L4 | L476RG, L4R5ZI |
| STM32U5 | U575ZI |

## Requirements

- Node.js 18+
- npm
- Platform: Linux (primary), Windows, macOS

### Host toolchains (for build/flash)

- `arm-none-eabi-gcc` — C/C++/ASM
- `rustc` + `cargo` (+ rustup targets) — Rust
- `openocd` — flash / debug probe
- `make`

## Quick Start

```bash
git clone https://github.com/Kirpich-Space/EmbedIDE.git
cd EmbedIDE
npm install
npm run dev          # development
npm run launch       # production build + run
```

## Build Distribution

```bash
npm run dist:linux   # AppImage + deb
npm run dist:win     # NSIS
npm run dist:mac     # DMG
```

Artifacts are written to `release/`.

## AI (optional)

Settings → AI:

- **Disabled** (default) — no panel, no network calls
- **Local** — OpenAI-compatible endpoint, default `http://127.0.0.1:11434/v1` (Ollama)
- **Cloud** — HTTPS endpoint + API key

Core IDE features never require AI or internet.

## Project layout

```
EmbedIDE/
├── electron/          # Main process (boards, project, toolchain, serial)
├── src/               # React UI
├── public/fonts/      # Offline UI/editor fonts
├── build/icons        # App icons
└── package.json
```

## License

MIT — see [LICENSE](LICENSE).
