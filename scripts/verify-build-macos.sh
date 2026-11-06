#!/usr/bin/env bash
# tauri build 이후, macOS .app/.dmg 가 올바른지 검증한다.
#  - .app 안에 앱과 sidecar 3개(+deno)가 있는지, 서명이 유효한지
#  - 번들된 ffmpeg 가 GPL 이 아니고, 외부 dylib 에 의존하지 않으며, 필요한 인코더가 있는지
#   bash scripts/verify-build-macos.sh [버전]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${1:-$(node -p "require('$ROOT/src-tauri/tauri.conf.json').version")}"

APP="$(ls -d "$ROOT"/src-tauri/target/release/bundle/macos/*.app 2>/dev/null | head -n1 || true)"
[ -n "$APP" ] || { echo "::error::.app 이 없습니다"; exit 1; }
DMG="$(ls "$ROOT"/src-tauri/target/release/bundle/dmg/*.dmg 2>/dev/null | head -n1 || true)"
[ -n "$DMG" ] || { echo "::error::.dmg 가 없습니다"; exit 1; }
echo "$APP"

BIN="$APP/Contents/MacOS"
for f in slice-youtube yt-dlp ffmpeg deno; do
  [ -x "$BIN/$f" ] || { echo "::error::.app 에 $f 가 없습니다"; exit 1; }
done

codesign --verify --deep --strict "$APP" || { echo "::error::서명 검증 실패"; exit 1; }

V="$("$BIN/ffmpeg" -hide_banner -version)"
if echo "$V" | grep -Eq -- '--enable-(gpl|nonfree|version3)'; then echo "::error::ffmpeg 에 GPL/nonfree 옵션이 있습니다"; exit 1; fi
if otool -L "$BIN/ffmpeg" | tail -n +2 | grep -vE '^\s+(/usr/lib/|/System/Library/)'; then
  echo "::error::ffmpeg 가 외부 dylib 에 의존합니다"; exit 1
fi
ENC="$("$BIN/ffmpeg" -hide_banner -encoders)"
for e in libopenh264 libvpx-vp9 libaom-av1 libopus libmp3lame; do
  echo "$ENC" | grep -q " $e " || { echo "::error::ffmpeg 인코더 없음: $e"; exit 1; }
done
"$BIN/yt-dlp" --version >/dev/null
"$BIN/deno" --version >/dev/null
echo "빌드 결과 검증 통과 ($VER)"
