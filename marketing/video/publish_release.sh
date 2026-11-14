#!/bin/sh
# Upload the rendered videos and subtitles to a GitHub release (needs `gh` logged in with write access).
#   sh publish_release.sh v0.1.0-alpha.1 0.1.0-alpha.1
set -eu
TAG="${1:?release tag, e.g. v0.1.0-alpha.1}"
VER="${2:?version for file names, e.g. 0.1.0-alpha.1}"
REPO="${REPO:-Kirpich-Space/EmbedIDE}"
cd "$(dirname "$0")/out"
stage=$(mktemp -d)
for pair in video1_overview:overview video2_technical_demo:technical-demo video3_short:short; do
  src="${pair%%:*}"; name="${pair##*:}"
  cp "$src.mp4" "$stage/embedide-$name-$VER.mp4";        cp "$src.srt" "$stage/embedide-$name-$VER.en.srt"
  cp "${src}_ru.mp4" "$stage/embedide-$name-$VER.ru.mp4"; cp "${src}_ru.srt" "$stage/embedide-$name-$VER.ru.srt"
done
gh release upload "$TAG" --repo "$REPO" --clobber "$stage"/*
rm -rf "$stage"
