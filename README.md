# EmbedIDE

**Offline-first engineering IDE for embedded flight computers**

Built by [Kirpich Space](https://github.com/Kirpich-Space). Cross-platform desktop IDE (Electron + React + CodeMirror) for ARM Cortex-M firmware, drivers, and OS kernels — Rust, C, C++, Assembly, Zig. No Arduino, ESP32, AVR, or Pico targets.

## Features

- Multi-language editor — Rust, C, C++, Assembly, Zig (CodeMirror 6)
- **67 target MCUs** — STM32, NXP, Nordic, Infineon, Renesas, Microchip (flight / industrial class)
- Project kinds: firmware, device drivers, OS / kernel (all supported languages)
- Build & flash via OpenOCD (board-aware profiles)
- Serial monitor
- Memory analyzer (flash/RAM from board profile)
- Themes (dark & light engineering palettes)
- UI languages: English, Russian, Chinese, Japanese, German, French
- File search across the project
- Optional AI assistants — **off by default**; Ollama, OpenAI, Claude (Anthropic), OpenRouter, Groq, DeepSeek, Qwen, Kimi, and other providers
- Works fully offline for edit / build / flash / serial (toolchains installed on the host)

## Supported boards (v1.1)

| Family | Examples |
|--------|----------|
| STM32F0 / F1 / F3 | F030, F072, F103, F107, F303, F334 |
| STM32F4 | F401…F479 (F407VG default) |
| STM32F7 / H5 / H7 | F722…F769, H563/H573, H723…H7B3 |
| STM32G0 / G4 | G071, G0B1, G431, G474, G491 |
| STM32L4 / L5 / U5 | L432…L4R5, L552, U575, U5A5 |
| STM32WB / WL / C0 | WB55, WLE5, WL55, C031 |
| NXP | LPC55S69, LPC54628, i.MX RT1062 / RT1176 |
| Nordic | nRF52832, nRF52840, nRF5340, nRF9160 |
| Infineon | XMC4700, PSoC 6 CY8C624 |
| Renesas | RA6M5, RA8D1 |
| Microchip | ATSAME54P20, ATSAME70Q21 |

## Requirements

- Node.js 18+
- npm
- Platform: Linux (primary), Windows, macOS

### Host toolchains (for build/flash)

- `arm-none-eabi-gcc` — C/C++/ASM
- `rustc` + `cargo` (+ rustup targets) — Rust
- `zig` — Zig firmware / drivers / OS
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
