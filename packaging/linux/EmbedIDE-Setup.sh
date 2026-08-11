#!/usr/bin/env bash
# Standalone EmbedIDE Linux setup: installs AppImage (or uses local one) + downloads compilers NOW.
# This is the installer path for AppImage/tar users (not first-launch of the IDE).
set -euo pipefail

VERSION="${EMBEDIDE_VERSION:-2.6.1}"
PREFIX="${EMBEDIDE_PREFIX:-$HOME/.local}"
APP_DIR="$PREFIX/share/embed-ide"
BIN_DIR="$PREFIX/bin"
DATA_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/embed-ide"
TOOLCHAIN_DEST="$DATA_DIR/toolchain"
RELEASE_BASE="${EMBEDIDE_RELEASE_BASE:-https://github.com/Kirpich-Space/EmbedIDE/releases/download/v${VERSION}}"
APPIMAGE_NAME="embed-ide-${VERSION}-linux-x86_64.AppImage"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "══════════════════════════════════════════════"
echo " EmbedIDE ${VERSION} — setup installer"
echo "══════════════════════════════════════════════"

mkdir -p "$APP_DIR" "$BIN_DIR" "$DATA_DIR"

# Prefer AppImage next to this script / cwd, else download
APPIMAGE_SRC=""
for cand in \
  "$SCRIPT_DIR/$APPIMAGE_NAME" \
  "$PWD/$APPIMAGE_NAME" \
  "$SCRIPT_DIR/../release/$APPIMAGE_NAME"
do
  if [[ -f "$cand" ]]; then
    APPIMAGE_SRC="$cand"
    break
  fi
done

APPIMAGE_DST="$APP_DIR/EmbedIDE.AppImage"
if [[ -n "$APPIMAGE_SRC" ]]; then
  echo "Using local AppImage: $APPIMAGE_SRC"
  cp -f "$APPIMAGE_SRC" "$APPIMAGE_DST"
else
  echo "Downloading AppImage…"
  curl -fL --progress-bar -o "$APPIMAGE_DST" "$RELEASE_BASE/$APPIMAGE_NAME"
fi
chmod a+x "$APPIMAGE_DST"

ln -sfn "$APPIMAGE_DST" "$BIN_DIR/embed-ide"

# Desktop entry
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/embed-ide.desktop" <<EOF
[Desktop Entry]
Name=EmbedIDE
Comment=Offline-first IDE for Cortex-M firmware
Exec=$APPIMAGE_DST --no-sandbox %U
Icon=EmbedIDE
Terminal=false
Type=Application
Categories=Development;IDE;
StartupWMClass=EmbedIDE
EOF

# Toolchain download DURING setup
TC_SCRIPT=""
for cand in \
  "$SCRIPT_DIR/install-toolchain.sh" \
  "$SCRIPT_DIR/../linux/install-toolchain.sh" \
  "$APP_DIR/install-toolchain.sh"
do
  if [[ -f "$cand" ]]; then
    TC_SCRIPT="$cand"
    break
  fi
done

if [[ -z "$TC_SCRIPT" ]]; then
  echo "Fetching install-toolchain.sh…"
  TC_SCRIPT="$APP_DIR/install-toolchain.sh"
  curl -fL -o "$TC_SCRIPT" \
    "https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/v${VERSION}/packaging/linux/install-toolchain.sh" \
    || curl -fL -o "$TC_SCRIPT" \
    "https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/packaging/linux/install-toolchain.sh"
fi
chmod a+x "$TC_SCRIPT"

echo ""
echo "Downloading compilers into $TOOLCHAIN_DEST …"
EMBEDIDE_FETCH_RUST=1 bash "$TC_SCRIPT" "$TOOLCHAIN_DEST"

echo ""
echo "Install complete."
echo "  Launch:  embed-ide   (or $APPIMAGE_DST)"
echo "  Toolchain: $TOOLCHAIN_DEST"
