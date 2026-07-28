# Changelog

## 1.1.0 — 2026-07-28

### Added
- Board catalog with **20** flight/avionics STM32 targets (F4, F7, H7, G4, L4, U5)
- Project metadata file `embedide.json` (type, boardId)
- Board picker in New Project dialog (search + family groups)
- AI settings: disabled by default; Local (Ollama-compatible) and Cloud modes
- Bundled offline fonts (IBM Plex Sans, JetBrains Mono)

### Changed
- Templates (C/C++/ASM/Rust) parameterized by selected board (CPU flags, memory map, OpenOCD target)
- Flash uses board profile from `embedide.json`
- Memory analyzer totals follow selected board flash/RAM
- Peripheral panel shows board-aware peripheral blocks
- Status bar shows active board
- Toolbar branding: EmbedIDE / Kirpich Space
- CSP tightened for offline-first (loopback + optional HTTPS for cloud AI)
- CI runs Vite build before electron-builder; releases are non-prerelease

### Fixed
- Rust bare-metal template no longer depends on broken `embedded-hal` imports
- ELF detection uses Cargo package name / Makefile `TARGET`
- README clone URL and release packaging metadata

## 1.0.0

- Initial public release: editor, build/flash, serial, themes, i18n
