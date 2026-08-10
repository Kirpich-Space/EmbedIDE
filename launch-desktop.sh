#!/usr/bin/env bash
# Desktop launcher for EmbedIDE
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export DISPLAY="${DISPLAY:-:0}"

ELECTRON_BIN="$ROOT/node_modules/electron/dist/electron"
if [[ ! -x "$ELECTRON_BIN" ]]; then
  # Try to recover electron binary
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

# Ensure UI build exists
if [[ ! -f "$ROOT/dist/index.html" ]]; then
  if command -v npx >/dev/null 2>&1; then
    npx --yes vite build
  fi
fi

exec "$ELECTRON_BIN" "$ROOT" --no-sandbox "$@"
