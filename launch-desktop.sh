#!/usr/bin/env bash
# Desktop launcher for EmbedIDE
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export DISPLAY="${DISPLAY:-:0}"

# --- Bundled compilers / debuggers / builders (always prefer these) ---
ARCH="$(uname -m)"
case "$ARCH" in
  aarch64|arm64) TC_PLATFORM="linux-arm64" ;;
  *) TC_PLATFORM="linux-x64" ;;
esac
TC_ROOT="$ROOT/vendor/toolchain/$TC_PLATFORM"
TC_BIN="$TC_ROOT/bin"

if [[ ! -x "$TC_BIN/arm-none-eabi-gcc" ]]; then
  echo "EmbedIDE: bundled toolchain missing — fetching…" >&2
  if command -v npm >/dev/null 2>&1; then
    (cd "$ROOT" && npm run toolchain:fetch) || true
  fi
fi

if [[ -d "$TC_BIN" ]]; then
  EXTRA_PATH="$TC_BIN"
  for d in \
    "$TC_ROOT/gcc/bin" \
    "$TC_ROOT/gcc/arm-none-eabi/bin" \
    "$TC_ROOT/openocd/bin" \
    "$TC_ROOT/zig/bin" \
    "$TC_ROOT/rust/cargo/bin"
  do
    [[ -d "$d" ]] && EXTRA_PATH="$EXTRA_PATH:$d"
  done
  export PATH="$EXTRA_PATH:$PATH"
  export EMBEDIDE_TOOLCHAIN="$TC_ROOT"
  if [[ -d "$TC_ROOT/openocd/share/openocd/scripts" ]]; then
    export OPENOCD_SCRIPTS="$TC_ROOT/openocd/share/openocd/scripts"
  fi
  if [[ -d "$TC_ROOT/rust/rustup" ]]; then
    export RUSTUP_HOME="$TC_ROOT/rust/rustup"
    export CARGO_HOME="$TC_ROOT/rust/cargo"
  fi
fi

ELECTRON_DIST="$ROOT/node_modules/electron/dist"
ELECTRON_BIN="$ELECTRON_DIST/electron"
if [[ ! -x "$ELECTRON_BIN" ]]; then
  if command -v npm >/dev/null 2>&1; then
    npm install-scripts approve electron >/dev/null 2>&1 || true
    (cd "$ROOT/node_modules/electron" && node install.js) || true
  fi
fi

if [[ ! -x "$ELECTRON_BIN" ]]; then
  zenity --error --text="EmbedIDE: Electron не установлен.\nЗапустите: cd $ROOT && npm install" 2>/dev/null \
    || notify-send "EmbedIDE" "Electron не установлен. Выполните npm install в $ROOT" 2>/dev/null \
    || echo "Electron missing in $ROOT" >&2
  exit 1
fi

# Rename process identity so the panel shows EmbedIDE (not Electron)
EMBEDIDE_BIN="$ELECTRON_DIST/EmbedIDE"
if [[ ! -e "$EMBEDIDE_BIN" ]]; then
  ln -sfn electron "$EMBEDIDE_BIN" 2>/dev/null || true
fi
if [[ -x "$EMBEDIDE_BIN" || -L "$EMBEDIDE_BIN" ]]; then
  ELECTRON_BIN="$EMBEDIDE_BIN"
fi

# Desktop icon for local launches (Icon=embedide in .desktop)
ICON_SRC="$ROOT/build/icons/512.png"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps"
if [[ -f "$ICON_SRC" ]]; then
  mkdir -p "$ICON_DIR" 2>/dev/null || true
  cp -f "$ICON_SRC" "$ICON_DIR/embedide.png" 2>/dev/null || true
  cp -f "$ICON_SRC" "$ICON_DIR/EmbedIDE.png" 2>/dev/null || true
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" >/dev/null 2>&1 || true
  fi
fi

if [[ ! -f "$ROOT/dist/index.html" ]]; then
  if command -v npx >/dev/null 2>&1; then
    npx --yes vite build
  fi
fi

# --class / --name force WM_CLASS so StartupWMClass matches
exec "$ELECTRON_BIN" "$ROOT" --no-sandbox --class=EmbedIDE --name=EmbedIDE "$@"
