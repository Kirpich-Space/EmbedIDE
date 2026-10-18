# EmbedIDE — installers by platform

Compilers (ARM GCC, OpenOCD, Zig, Rust) are downloaded **during installation**, not when you first open the IDE.

| Platform | Package | Notes |
|---|---|---|
| Debian/Ubuntu | `embed-ide-*-linux-amd64.deb` | `dpkg -i` / `apt install` runs postinst download |
| Arch/Manjaro | `embed-ide-*-linux-*.pacman` | `pacman -U` runs after-install download |
| Generic Linux | `EmbedIDE-Setup.sh` + AppImage | Setup script installs AppImage **and** downloads toolchain |
| AppImage alone | portable | Prefer Setup.sh; or Settings → Toolchain repair |
| Windows | NSIS setup | Downloads toolchain during installer |
| tar.gz | extract + `resources/install-toolchain.sh` | Run the script once after extract |

Network is required at install time (~1–3 GB once). Offline afterwards.
