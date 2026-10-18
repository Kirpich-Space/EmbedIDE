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
echo "[make]"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
ensure_make() {
  if [[ -x "$BIN/make" ]] && "$BIN/make" --version >/dev/null 2>&1; then
    echo "  = make already present"
    return 0
  fi

  # 1) Portable bootstrap shipped in the package
  local cand
  for cand in \
    "$SCRIPT_DIR/bootstrap/linux-x64/make" \
    "$SCRIPT_DIR/bootstrap/make" \
    "$SCRIPT_DIR/../bootstrap/linux-x64/make" \
    "$SCRIPT_DIR/../bootstrap/make" \
    "${EMBEDIDE_BOOTSTRAP_MAKE:-}"
  do
    [[ -n "$cand" && -x "$cand" ]] || continue
    cp "$cand" "$BIN/make"
    chmod a+x "$BIN/make"
    if "$BIN/make" --version >/dev/null 2>&1; then
      echo "  ✓ make (bootstrap)"
      return 0
    fi
  done

  # 2) Host make (real binary, not our incomplete dest)
  if command -v make >/dev/null 2>&1; then
    local host
    host="$(command -v make)"
    if [[ "$(realpath "$host" 2>/dev/null || echo "$host")" != "$(realpath "$BIN/make" 2>/dev/null || echo "")" ]]; then
      cp "$host" "$BIN/make" 2>/dev/null || true
      chmod a+x "$BIN/make" 2>/dev/null || true
      if "$BIN/make" --version >/dev/null 2>&1; then
        echo "  ✓ make (host)"
        return 0
      fi
    fi
  fi

  # 3) Download GNU make and bootstrap-build with zig (already installed) or cc
  echo "  ↓ downloading GNU make 4.4.1 source…"
  local srcTar="$CACHE/make-4.4.1.tar.gz"
  local buildDir="$CACHE/make-4.4.1-build"
  if [[ ! -f "$srcTar" ]] || [[ "$(stat -c%s "$srcTar" 2>/dev/null || echo 0)" -lt 1000 ]]; then
    download "https://ftp.gnu.org/gnu/make/make-4.4.1.tar.gz" "$srcTar"
  fi
  rm -rf "$buildDir"
  mkdir -p "$buildDir"
  tar -xzf "$srcTar" -C "$buildDir" --strip-components=1

  local zig=""
  for cand in "$BIN/zig" "$DEST/zig/bin/zig"; do
    [[ -x "$cand" ]] && zig="$cand" && break
  done
  local cc="cc"
  if [[ -n "$zig" ]]; then
    cc="$zig cc"
    echo "  ✦ building make with zig cc…"
  elif command -v gcc >/dev/null 2>&1; then
    cc="gcc"
    echo "  ✦ building make with gcc…"
  elif command -v clang >/dev/null 2>&1; then
    cc="clang"
    echo "  ✦ building make with clang…"
  else
    echo "  ! no compiler to build make" >&2
    return 1
  fi

  (
    cd "$buildDir"
    # build.sh bootstraps a minimal make without requiring make
    CC="$cc" sh build.sh
    if [[ -x ./make ]]; then
      ./make -j"$(nproc 2>/dev/null || echo 2)" || true
    fi
  )
  if [[ -x "$buildDir/make" ]]; then
    cp "$buildDir/make" "$BIN/make"
    chmod a+x "$BIN/make"
    echo "  ✓ make (built 4.4.1)"
    return 0
  fi
  echo "  ! failed to install make" >&2
  return 1
}
ensure_make || true

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
    -t thumbv6m-none-eabi -t thumbv7m-none-eabi -t thumbv7em-none-eabi -t thumbv7em-none-eabihf -t thumbv8m.main-none-eabi -t thumbv8m.main-none-eabihf
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
