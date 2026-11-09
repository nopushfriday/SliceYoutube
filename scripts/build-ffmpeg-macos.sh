#!/usr/bin/env bash
# macOS (Apple Silicon) 용 경량 LGPL ffmpeg 를 직접 빌드해 src-tauri/binaries 에 둔다. (Homebrew 필요)
#   bash scripts/build-ffmpeg-macos.sh [ffmpeg버전]
#
# Linux 와 같은 기능 목록을 쓴다. 한쪽을 고치면 다른 쪽도 고친다.
#  - TLS 는 SecureTransport(시스템 내장), 하드웨어 인코더는 VideoToolbox.
#  - 코덱 라이브러리는 정적(.a)으로 링크해 배포 앱이 Homebrew 에 의존하지 않게 한다.
#  - 빌드 러너의 macOS 버전이 최소 지원 버전을 정한다 (CI 는 macos-15 러너에서 MACOSX_DEPLOYMENT_TARGET=14.0 으로 빌드).
set -euo pipefail

VER="${1:-9.0.2}"

[ "$(uname -s)" = "Darwin" ] || { echo "macOS 에서만 빌드합니다."; exit 1; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/.ffmpeg-build"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"            # 예: aarch64-apple-darwin
OUT="$ROOT/src-tauri/binaries/ffmpeg-$TRIPLE"             # Tauri externalBin 은 <이름>-<타깃 트리플> 을 요구한다
JOBS="$(sysctl -n hw.ncpu)"

echo "== 의존 패키지 설치"
brew install nasm pkg-config libvpx aom opus lame

# Homebrew 의 openh264 는 .dylib 만 있어 정적 라이브러리가 없다. 소스(BSD)로 직접 빌드한다.
OH_VER="${OPENH264_VERSION:-2.6.0}"
OH="$WORK/openh264-install"
if [ ! -f "$OH/lib/libopenh264.a" ]; then
  echo "== openh264 $OH_VER 소스 빌드 (정적)"
  mkdir -p "$WORK" && cd "$WORK"
  curl -fsSL --retry 3 -o "openh264-$OH_VER.tar.gz" "https://github.com/cisco/openh264/archive/refs/tags/v$OH_VER.tar.gz"
  rm -rf openh264-src && mkdir openh264-src
  tar -xf "openh264-$OH_VER.tar.gz" -C openh264-src --strip-components=1
  (cd openh264-src && make -j"$JOBS" OS=darwin ARCH=arm64 PREFIX="$OH" install-static)
  cd "$ROOT"
fi
export PKG_CONFIG_PATH="$OH/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

echo "== 정적 라이브러리 확인 (.a 만 모은 폴더를 먼저 검색하게 해 .dylib 링크를 막는다)"
STATIC="$WORK/static-libs"
rm -rf "$STATIC" && mkdir -p "$STATIC"
for pair in openh264:openh264 vpx:libvpx aom:aom opus:opus mp3lame:lame; do
  name="${pair%%:*}"; formula="${pair##*:}"
  if [ "$name" = openh264 ]; then A="$OH/lib/libopenh264.a"; else A="$(brew --prefix "$formula")/lib/lib$name.a"; fi
  [ -f "$A" ] || { echo "정적 라이브러리 없음: $A (직접 빌드가 필요합니다)"; exit 1; }
  ln -s "$A" "$STATIC/"
done

echo "== ffmpeg $VER 소스"
mkdir -p "$WORK"
cd "$WORK"
if [ ! -f "ffmpeg-$VER.tar.gz" ]; then
  curl -fsSL --retry 3 -o "ffmpeg-$VER.tar.gz" "https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n$VER.tar.gz"
fi
rm -rf src-macos && mkdir src-macos
tar -xf "ffmpeg-$VER.tar.gz" -C src-macos --strip-components=1
cd src-macos

echo "== configure"
./configure \
  --cc=clang \
  --extra-cflags="-I$OH/include -I$(brew --prefix)/include" \
  --extra-ldflags="-L$STATIC -L$(brew --prefix)/lib" \
  --extra-libs="-lc++ -lm -lpthread" \
  --disable-everything --disable-autodetect \
  --disable-debug --disable-doc --disable-ffplay --disable-ffprobe \
  --enable-small --enable-lto --enable-zlib \
  --enable-securetransport \
  --enable-protocol=file,pipe,http,https,tcp,tls,crypto,hls \
  --enable-videotoolbox \
  --enable-libopenh264 --enable-libvpx --enable-libaom --enable-libopus --enable-libmp3lame \
  --enable-demuxer=mov,matroska,hls,mpegts,mp3,ogg,aac,flac,wav \
  --enable-muxer=mp4,matroska,webm,mp3,ipod,adts,null \
  --enable-parser=h264,vp9,av1,aac,opus,vorbis,mpegaudio,flac \
  --enable-decoder=h264,vp9,libaom_av1,aac,opus,vorbis,mp3,flac,pcm_s16le,wrapped_avframe,rawvideo \
  --enable-encoder=libopenh264,libvpx_vp9,libaom_av1,libopus,aac,libmp3lame,h264_videotoolbox \
  --enable-bsf=aac_adtstoasc,extract_extradata,vp9_superframe,vp9_superframe_split,av1_frame_merge,av1_frame_split,h264_mp4toannexb,null \
  --enable-filter=scale,aresample,format,aformat,null,anull,fps,trim,atrim,setpts,asetpts,volume \
  --enable-indev=lavfi --enable-filter=testsrc2,sine \
  || { tail -n 50 ffbuild/config.log; exit 1; }

echo "== 빌드"
make -j"$JOBS" ffmpeg
strip ffmpeg
# strip 으로 바뀐 서명을 다시 맞춘다 (arm64 에서는 서명이 깨진 바이너리가 실행되지 않는다)
codesign --force -s - ffmpeg

echo "== 검증"
./ffmpeg -hide_banner -version | tee ../version-macos.txt
if grep -Eq -- '--enable-(gpl|nonfree|version3)' ../version-macos.txt; then echo "GPL/nonfree 옵션 감지"; exit 1; fi
# 시스템 라이브러리(/usr/lib, /System) 외에 Homebrew 등 외부 경로에 의존하면 안 된다
if otool -L ffmpeg | tail -n +2 | grep -vE '^\s+(/usr/lib/|/System/Library/)'; then
  echo "외부 dylib 에 의존합니다 (정적 링크 필요)"; exit 1
fi
./ffmpeg -hide_banner -loglevel error -y -f lavfi -i testsrc2=size=320x180:rate=30:duration=1 \
  -c:v libopenh264 -pix_fmt yuv420p smoke.mp4
./ffmpeg -hide_banner -loglevel error -y -f lavfi -i testsrc2=size=320x180:rate=30:duration=1 \
  -c:v h264_videotoolbox -b:v 1M -pix_fmt yuv420p smoke_hw.mp4 || echo "참고: VideoToolbox 인코딩 실패 (앱은 소프트웨어로 대체합니다)"

mkdir -p "$(dirname "$OUT")"
cp ffmpeg "$OUT"
echo "완료: $OUT"
