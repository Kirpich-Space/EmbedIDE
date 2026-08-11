#!/usr/bin/env bash
# EmbedIDE toolchain installer — run by package postinst / setup scripts (NOT by the IDE on first launch).
# Usage:
#   EMBEDIDE_TOOLCHAIN_DEST=/opt/EmbedIDE/resources/toolchain ./install-toolchain.sh
#   ./install-toolchain.sh /path/to/toolchain
set -euo pipefail

DEST="${1:-${EMBEDIDE_TOOLCHAIN_DEST:-}}"
if [[ -z "${DEST}" ]]; then
  if [[ -n "${HOME:-}" ]]; then
    DEST="${XDG_CONFIG_HOME:-$HOME/.config}/embed-ide/toolchain"
  else
    DEST="/opt/EmbedIDE/resources/toolchain"
  fi
fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) PLATFORM="linux-x64" ;;
  aarch64|arm64) PLATFORM="linux-arm64" ;;
  *)
    echo "EmbedIDE: unsupported arch: $ARCH" >&2
    exit 1
    ;;
esac

INCLUDE_RUST="${EMBEDIDE_FETCH_RUST:-1}"
CACHE="${EMBEDIDE_TOOLCHAIN_CACHE:-${DEST}/../toolchain-cache}"
BIN="$DEST/bin"

mkdir -p "$DEST" "$BIN" "$CACHE"

download() {
  local url="$1" out="$2"
  echo "  ↓ $(basename "$url")"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --progress-bar -o "$out" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$out" "$url"
  else
    echo "EmbedIDE: need curl or wget" >&2
    exit 1
  fi
}

extract_tar() {
  local archive="$1" dest="$2" strip="${3:-1}"
  mkdir -p "$dest"
  tar -xaf "$archive" -C "$dest" --strip-components="$strip"
}

link_bins() {
  local pkgdir="$1"
  local src="$pkgdir/bin"
  [[ -d "$src" ]] || return 0
  local f
  for f in "$src"/*; do
    [[ -e "$f" ]] || continue
    local name
    name="$(basename "$f")"
    local dst="$BIN/$name"
    rm -f "$dst"
    ln -s "$(realpath --relative-to="$BIN" "$f" 2>/dev/null || python3 -c "import os.path; print(os.path.relpath('$f','$BIN'))")" "$dst" 2>/dev/null \
      || ln -s "$f" "$dst"
    chmod a+x "$dst" 2>/dev/null || true
  done
}

echo "══════════════════════════════════════════════"
echo " EmbedIDE — installing compilers & builders"
echo " Destination: $DEST"
echo "══════════════════════════════════════════════"

declare -A URLS STRIP RENAME
if [[ "$PLATFORM" == "linux-x64" ]]; then
  URLS[gcc]="https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-x64.tar.gz"
  URLS[openocd]="https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-x64.tar.gz"
  URLS[zig]="https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz"
elif [[ "$PLATFORM" == "linux-arm64" ]]; then
  URLS[gcc]="https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-arm64.tar.gz"
  URLS[openocd]="https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-arm64.tar.gz"
  URLS[zig]="https://ziglang.org/download/0.13.0/zig-linux-aarch64-0.13.0.tar.xz"
fi
STRIP[gcc]=1
STRIP[openocd]=1
STRIP[zig]=1
RENAME[zig]=1

for name in gcc openocd zig; do
  url="${URLS[$name]}"
  file="$CACHE/$(basename "$url")"
  echo ""
  echo "[$name]"
  if [[ ! -f "$file" ]] || [[ "$(stat -c%s "$file" 2>/dev/null || echo 0)" -lt 1000 ]]; then
    download "$url" "$file"
  else
    echo "  = cache hit $(basename "$file")"
  fi
  unpack="$DEST/$name"
  rm -rf "$unpack"
  mkdir -p "$unpack"
  echo "  ✦ extract"
  extract_tar "$file" "$unpack" "${STRIP[$name]}"
  if [[ -n "${RENAME[$name]:-}" ]]; then
    mkdir -p "$unpack/bin"
    if [[ -f "$unpack/zig" ]]; then
      cp "$unpack/zig" "$unpack/bin/zig"
      chmod a+x "$unpack/bin/zig"
    fi
  fi
  link_bins "$unpack"
done

echo ""
echo "[host tools]"
if command -v make >/dev/null 2>&1; then
  cp "$(command -v make)" "$BIN/make"
  chmod a+x "$BIN/make"
  echo "  ✓ make"
elif [[ -x "$(dirname "$0")/bootstrap/linux-x64/make" ]]; then
  cp "$(dirname "$0")/bootstrap/linux-x64/make" "$BIN/make"
  chmod a+x "$BIN/make"
  echo "  ✓ make (bootstrap)"
elif [[ -x "$(dirname "$0")/../bootstrap/linux-x64/make" ]]; then
  cp "$(dirname "$0")/../bootstrap/linux-x64/make" "$BIN/make"
  chmod a+x "$BIN/make"
  echo "  ✓ make (bootstrap)"
else
  echo "  ! make not found — install make from your distro if builds fail"
fi
if command -v python3 >/dev/null 2>&1; then
  ln -sf "$(command -v python3)" "$BIN/python3" 2>/dev/null || true
fi

if [[ "$INCLUDE_RUST" == "1" ]]; then
  echo ""
  echo "[rust]"
  export RUSTUP_HOME="$DEST/rust/rustup"
  export CARGO_HOME="$DEST/rust/cargo"
  mkdir -p "$RUSTUP_HOME" "$CARGO_HOME"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable \
    -t thumbv6m-none-eabi -t thumbv7m-none-eabi -t thumbv7em-none-eabi -t thumbv7em-none-eabihf
  for tool in cargo rustc rustup; do
    if [[ -e "$CARGO_HOME/bin/$tool" ]]; then
      rm -f "$BIN/$tool"
      ln -s "$(realpath --relative-to="$BIN" "$CARGO_HOME/bin/$tool" 2>/dev/null || echo "$CARGO_HOME/bin/$tool")" "$BIN/$tool" 2>/dev/null \
        || ln -s "$CARGO_HOME/bin/$tool" "$BIN/$tool"
    fi
  done
  echo "  ✓ rustc/cargo"
fi

cat > "$DEST/manifest.json" <<EOF
{
  "platform": "$PLATFORM",
  "fetchedAt": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "source": "package-installer",
  "rust": $([ "$INCLUDE_RUST" = 1 ] && echo true || echo false)
}
EOF

echo ""
echo "Done. Toolchain ready at: $DEST"
echo -n "Tools: "
ls "$BIN" 2>/dev/null | tr '\n' ' ' || true
echo ""
