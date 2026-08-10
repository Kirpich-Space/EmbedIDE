## 2.0.1 — 2026-08-10

Patch update on the 2.0 line: AI API onboarding + performance.

### Added
- **Gemini** and **xAI (Grok)** cloud providers
- Clear Settings callouts: ChatGPT Plus / Claude Pro / Google AI Pro ≠ developer API keys
- Console “Get API key” links; reject Claude OAuth (`sk-ant-oat…`) and cookie/JWT pastes

### Changed
- OpenAI / Claude model presets refreshed (GPT-5.6, Claude Sonnet 5, …)
- **Performance:** renderer packages not double-packed into asar; settings disk writes debounced; cursor updates no longer re-render the whole IDE; diagnostics sync no longer runs on every keystroke; toolchain detection runs in parallel

## 2.0.0 — 2026-08-10

Major UX and editor release: richer settings, multi-provider AI that can edit project files, build diagnostics in the editor, CJK fonts, and cross-distro packaging.

### Added
- **Build error highlighting** in the editor (gcc/clang/rustc/zig diagnostics → underlines + lint gutter)
- **AI agent file edits** for both local (Ollama) and cloud providers; optional auto-apply (Settings → AI)
- Multi-provider AI: Ollama, OpenAI, Claude, OpenRouter, Groq, DeepSeek, Qwen, Kimi, Together, Mistral, Fireworks, Custom
- Fancy language / font / model selectors; Appearance settings (accent, UI scale, chrome toggles)
- Bundled **Noto Sans SC/JP** fonts (no tofu for zh/ja)
- Autocomplete keywords/snippets for C/C++/Rust/Zig/ASM; Zig language mode
- Packaging: AppImage, deb, rpm, pacman, tar.gz, Windows NSIS/portable targets; Void/Gentoo/Arch templates; macOS CI workflow
- Custom gray settings checkboxes (dark-theme friendly)

### Changed
- Settings expanded (editor behavior, appearance, AI auto-apply)
- Islands-style UI polish; theme accents and selection marks refined
- New Project / board pickers show clear selection marks
- Build output cleared on each new build so diagnostics stay fresh

### Fixed
- zh/ja square glyphs (missing CJK glyphs in UI font)
- White OS checkboxes / selection discs on dark themes
- White “Reset settings” button on dark themes
- Many toolchain/template/sandbox reliability fixes carried from 1.1.x

## 1.1.2 — 2026-08-10

### Added
- Installers / packages: AppImage, deb, rpm (Fedora), pacman (Arch), tar.gz (Void/Gentoo), Windows NSIS + portable
- Packaging templates: `packaging/arch`, `packaging/void`, `packaging/gentoo`
- GitHub Actions workflow for macOS DMG/ZIP (`dist:mac` on macOS runners)

### Fixed
- Settings toggles: custom gray checkboxes (no OS white cubes in dark theme)
- Selection marks / reset button styling for dark themes

## 1.1.1 — 2026-08-10

### Added
- Board catalog expanded to **67** Cortex-M targets (STM32 F0–H7/WB/WL/U5, NXP LPC/i.MX RT, Nordic nRF, Infineon, Renesas RA, Microchip SAM) — still no Arduino / ESP / AVR
- AI providers: OpenRouter, OpenAI, **Claude (Anthropic Messages API)**, Groq, DeepSeek, **Qwen (DashScope)**, **Kimi (Moonshot)**, Together, Mistral, Fireworks, Custom (+ Ollama)
- Project templates: Zig firmware; Drivers & OS/Kernel in C, C++, Rust, ASM, Zig
- Zig syntax highlighting, completions, toolchain detect (`zig version`)

### Changed
- Removed Python / Shell script project types (focus: firmware, drivers, OS)
- New Project dialog groups: Firmware / Device Drivers / OS & Kernel
- UI: Islands-style layout — floating panels, canvas gaps, glass rims, clearer surface hierarchy
- Tabs: animated accent indicators (editor + bottom tools)
- Empty editor: branded welcome with shortcut hints
- Status bar dims until hover; softer modal blur / spring motion
- Themes: dark themes use layered surfaces (canvas / chrome / editor) instead of flat #000; light theme refreshed (soft cool canvas, quieter borders/shadows)

### Fixed
- Linker `memLength`: boards with ≥1024 KB flash/RAM now get `1M`/`2M` instead of broken `1K`/`2K`
- Filesystem IPC path sandbox (blocks traversal / writes outside the project)
- Project name validation (no `../` path escape on create)
- Build no longer uses `shell: true`; rustup targets are allowlisted
- OpenOCD `program` path is quoted (spaces-safe)
- Serial connect waits for Python handshake and reports real failures
- Unsaved-changes dialog dismisses after Save / Don't Save
- Memory analyzer parses GNU `size` output (`text`/`data`/`bss`)
- Status bar cursor position tracks CodeMirror selection
- Menu/keyboard handlers no longer freeze on stale AI settings
- AI agents propose file writes for confirmation (no silent overwrite)
- API keys kept out of `localStorage` (userData settings only)
- File explorer expands `src` correctly; New File from a file uses sibling folder
- Delete/rename closes or remaps child editor tabs
- C/C++ templates include startup vector table; ASM handlers marked `.thumb_func`
- About box version matches `package.json`

### Changed
- Bottom panel tabs for Output / Serial / Memory / Peripherals
- Electron `sandbox: true` enabled

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
