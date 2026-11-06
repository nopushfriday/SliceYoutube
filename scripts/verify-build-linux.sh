#!/usr/bin/env bash
# tauri build 이후, Linux AppImage 가 올바른지 검증한다.
#  - AppImage 가 있는지, 안에 앱과 sidecar 3개가 있는지
#  - 번들된 ffmpeg 가 실행되고 GPL 이 아닌지, 필요한 인코더가 있는지
#   bash scripts/verify-build-linux.sh [버전]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${1:-$(node -p "require('$ROOT/src-tauri/tauri.conf.json').version")}"   # 버전은 tauri.conf.json 에서 읽는다

IMG="$(ls "$ROOT"/src-tauri/target/release/bundle/appimage/*.AppImage 2>/dev/null | head -n1 || true)"
[ -n "$IMG" ] || { echo "::error::AppImage 가 없습니다"; exit 1; }
echo "$IMG"

TMP="$(mktemp -d)"
cd "$TMP"
cp "$IMG" app.AppImage && chmod +x app.AppImage
./app.AppImage --appimage-extract >/dev/null   # FUSE 없이 풀어서 검사한다
BIN="$TMP/squashfs-root/usr/bin"

for f in slice-youtube yt-dlp ffmpeg deno; do
  [ -x "$BIN/$f" ] || { echo "::error::AppImage 에 $f 가 없습니다"; exit 1; }
done

V="$("$BIN/ffmpeg" -hide_banner -version)"
if echo "$V" | grep -Eq -- '--enable-(gpl|nonfree|version3)'; then echo "::error::ffmpeg 에 GPL/nonfree 옵션이 있습니다"; exit 1; fi
ENC="$("$BIN/ffmpeg" -hide_banner -encoders)"
for e in libopenh264 libvpx-vp9 libaom-av1 libopus libmp3lame; do
  echo "$ENC" | grep -q " $e " || { echo "::error::ffmpeg 인코더 없음: $e"; exit 1; }
done
"$BIN/yt-dlp" --version >/dev/null
"$BIN/deno" --version >/dev/null
echo "빌드 결과 검증 통과 ($VER)"
