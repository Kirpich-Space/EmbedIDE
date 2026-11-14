#!/bin/sh
# Copy finished renders and subtitles from out/ into the repository's videos/ folder.
set -eu
cd "$(dirname "$0")"
V=0.1.0-alpha.1
dst=../../videos
for pair in video1_overview:overview video2_technical_demo:technical-demo video3_short:short; do
  src=${pair%%:*}; name=${pair##*:}
  cp "out/$src.mp4" "$dst/embedide-$name-$V.mp4"
  cp "out/${src}_ru.mp4" "$dst/embedide-$name-$V.ru.mp4"
  cp "out/$src.srt" "$dst/embedide-$name-$V.en.srt"
  cp "out/${src}_ru.srt" "$dst/embedide-$name-$V.ru.srt"
done
ls -l "$dst"
