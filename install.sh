#!/bin/sh
# Install the latest EmbedIDE (`embed`) pre-release for Linux x86_64 into ~/.local/bin.
# Downloads the static (musl) build, which runs on any Linux distribution, and verifies its
# SHA-256 checksum. Nothing is installed system-wide and nothing runs as root.
#
#   sh install.sh                 # latest release
#   sh install.sh v0.1.0-alpha.1  # a specific tag
#   PREFIX=/opt/embed sh install.sh   # binary goes to $PREFIX/bin
set -eu

REPO="Kirpich-Space/EmbedIDE"
TAG="${1:-}"
PREFIX="${PREFIX:-$HOME/.local}"
BASE="${EMBED_BASE_URL:-https://github.com/$REPO/releases}"

fail() { echo "install.sh: $*" >&2; exit 1; }

case "$(uname -s)" in
  Linux) ;;
  FreeBSD) fail "no prebuilt FreeBSD binary yet. Build from source: pkg install rust gmake git && cargo install --path crates/embedide-cli --locked --root ~/.local" ;;
  *) fail "unsupported system $(uname -s): EmbedIDE supports Linux and FreeBSD" ;;
esac
[ "$(uname -m)" = "x86_64" ] || fail "only x86_64 binaries are published (this is $(uname -m)); build from source with cargo"

if command -v curl >/dev/null 2>&1; then
  get() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  get() { wget -q "$1" -O "$2"; }
else
  fail "curl or wget is required"
fi
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

if [ -z "$TAG" ]; then
  # `latest` ignores pre-releases, so resolve the newest tag from the releases page.
  get "https://api.github.com/repos/$REPO/releases" "${TMPDIR:-/tmp}/embed-releases.$$" 2>/dev/null \
    || fail "cannot list releases (pass a tag: sh install.sh v0.1.0-alpha.1)"
  TAG="$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "${TMPDIR:-/tmp}/embed-releases.$$" | head -n 1)"
  rm -f "${TMPDIR:-/tmp}/embed-releases.$$"
  [ -n "$TAG" ] || fail "no release found"
fi
VERSION="${TAG#v}"
NAME="embed-$VERSION-x86_64-unknown-linux-musl"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
echo "Downloading $NAME.tar.gz ($TAG)..."
get "$BASE/download/$TAG/$NAME.tar.gz" "$WORK/$NAME.tar.gz" || fail "download failed"
get "$BASE/download/$TAG/SHA256SUMS" "$WORK/SHA256SUMS" || fail "checksum download failed"

( cd "$WORK" && grep " $NAME.tar.gz\$" SHA256SUMS | sha256sum -c - >/dev/null ) \
  || fail "checksum mismatch: not installing"
echo "Checksum OK."

tar -xzf "$WORK/$NAME.tar.gz" -C "$WORK"
mkdir -p "$PREFIX/bin"
install -m 0755 "$WORK/$NAME/embed" "$PREFIX/bin/embed"
echo "Installed $("$PREFIX/bin/embed" --version) to $PREFIX/bin/embed"
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) echo "Note: $PREFIX/bin is not on your PATH (fish: fish_add_path $PREFIX/bin; bash/zsh: export PATH=\"$PREFIX/bin:\$PATH\")." ;;
esac
echo "Next: embed doctor   |   embed   |   docs: https://github.com/$REPO/tree/main/docs"
