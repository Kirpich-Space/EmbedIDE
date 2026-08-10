# EmbedIDE — installers by platform

Version **1.1.2** artifacts live in `release/` after `npm run dist:all`.

| Platform | Recommended package | Notes |
|----------|---------------------|-------|
| **Windows 10/11** | `embed-ide-*-windows-*-setup.exe` (NSIS) | Also: `*-portable.exe` |
| **macOS** | `embed-ide-*-macos-*.dmg` | Build on macOS / CI (`npm run dist:mac`). Not produced on Linux hosts. |
| **Fedora / RHEL / openSUSE** | `embed-ide-*-linux-*.rpm` | `sudo dnf install ./embed-ide-….rpm` |
| **Arch Linux / Manjaro** | `embed-ide-*-linux-*.pacman` or AppImage | Or `packaging/arch/PKGBUILD` + tar.gz |
| **Debian / Ubuntu** | `embed-ide-*-linux-*.deb` | `sudo apt install ./embed-ide-….deb` |
| **Void Linux** | AppImage or tar.gz | XBPS template: `packaging/void/` |
| **Gentoo** | AppImage or tar.gz | ebuild: `packaging/gentoo/embedide-1.1.2.ebuild` |
| **Any Linux (glibc)** | AppImage | `chmod +x` then run |

## Build commands

```bash
npm run dist:all      # Linux + Windows (from Linux/Arch)
npm run dist:linux    # AppImage deb rpm pacman tar.gz
npm run dist:win      # NSIS + portable
npm run dist:mac      # macOS only (Apple host or GitHub Actions macOS runner)
```

## Void

```bash
# quick
chmod +x embed-ide-*-linux-x64.AppImage && ./embed-ide-*-linux-x64.AppImage
# or package via void-packages using packaging/void/template (update checksum)
```

## Gentoo

```bash
# quick
chmod +x embed-ide-*-linux-x64.AppImage && ./embed-ide-*-linux-x64.AppImage
# or copy ebuild into an overlay and emerge
```
