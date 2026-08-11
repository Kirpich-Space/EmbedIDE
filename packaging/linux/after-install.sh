#!/bin/bash
# electron-builder FPM after-install — downloads compilers DURING package installation.
set -e

# Keep default desktop/mime refresh behaviour
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database /usr/share/mime || true
fi

APP_ROOT="/opt/EmbedIDE"
# Older layouts / alternate product paths
for cand in /opt/EmbedIDE /opt/embed-ide /usr/lib/embed-ide; do
  if [[ -d "$cand/resources" ]] || [[ -x "$cand/embed-ide" ]] || [[ -x "$cand/EmbedIDE" ]]; then
    APP_ROOT="$cand"
    break
  fi
done

SCRIPT="$APP_ROOT/resources/install-toolchain.sh"
DEST="$APP_ROOT/resources/toolchain"
BOOTSTRAP="$APP_ROOT/resources/bootstrap"

echo ""
echo "EmbedIDE: downloading ARM GCC, OpenOCD, Zig, make, Rust (install-time)…"
echo "This may take several minutes and needs network access."
echo ""

if [[ ! -x "$SCRIPT" ]]; then
  # fpm may not mark executable; fix and retry
  if [[ -f "$SCRIPT" ]]; then
    chmod a+x "$SCRIPT" || true
  fi
fi

if [[ -x "$SCRIPT" ]] || [[ -f "$SCRIPT" ]]; then
  chmod a+x "$SCRIPT" 2>/dev/null || true
  export EMBEDIDE_TOOLCHAIN_DEST="$DEST"
  export EMBEDIDE_TOOLCHAIN_CACHE="${TMPDIR:-/tmp}/embedide-toolchain-cache"
  export EMBEDIDE_FETCH_RUST=1
  # Prefer bootstrap make shipped next to script
  if [[ -d "$BOOTSTRAP" ]]; then
    export PATH="$BOOTSTRAP/linux-x64:$BOOTSTRAP:$PATH"
  fi
  if ! bash "$SCRIPT" "$DEST"; then
    echo "WARNING: EmbedIDE toolchain download failed during install." >&2
    echo "You can retry later: sudo bash $SCRIPT $DEST" >&2
    echo "Or: Settings → Toolchain → Download / repair inside the IDE." >&2
    # Do not fail the whole package install — leave a marker
    mkdir -p "$DEST"
    echo '{"skipped":false,"pending":true,"source":"after-install-failed"}' > "$DEST/manifest.json" || true
  fi
else
  echo "WARNING: $SCRIPT not found — toolchain was not downloaded." >&2
fi

exit 0
