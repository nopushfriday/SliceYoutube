#!/usr/bin/env bash
# Linux 용 경량 LGPL ffmpeg 를 직접 빌드해 src-tauri/binaries 에 둔다. (Debian/Ubuntu 계열)
#   bash scripts/build-ffmpeg-linux.sh [ffmpeg버전]
#
# Windows 용과 달리 TLS 는 gnutls(LGPL) 를 쓴다. 코덱(openh264/vpx/aom/opus/lame)은 정적 링크하고,
# glibc 와 gnutls(webkit2gtk 환경에는 항상 있음)만 시스템 것을 쓴다.
# 기능 목록은 build-ffmpeg-win.sh 와 같아야 한다. 한쪽을 고치면 다른 쪽도 고친다.
set -euo pipefail

VER="${1:-9.0.2}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/.ffmpeg-build"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
OUT="$ROOT/src-tauri/binaries/ffmpeg-$TRIPLE"   # Tauri externalBin 은 <이름>-<타깃 트리플> 을 요구한다

echo "== 의존 패키지 설치"
sudo apt-get update
sudo apt-get install -y build-essential nasm pkg-config curl xz-utils \
  zlib1g-dev libgnutls28-dev libopenh264-dev libvpx-dev libaom-dev libopus-dev libmp3lame-dev

echo "== ffmpeg $VER 소스"
mkdir -p "$WORK"
cd "$WORK"
if [ ! -f "ffmpeg-$VER.tar.gz" ]; then
  curl -fsSL --retry 3 -o "ffmpeg-$VER.tar.gz" "https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n$VER.tar.gz"
fi
rm -rf src-linux && mkdir src-linux
tar -xf "ffmpeg-$VER.tar.gz" -C src-linux --strip-components=1
cd src-linux

echo "== 코덱 정적 라이브러리 준비"
# 같은 이름의 .so 가 있으면 링커가 그쪽을 고르므로, .a 만 모은 폴더를 먼저 검색하게 한다.
LIBDIR="$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo x86_64-linux-gnu)"
STATIC="$WORK/static-libs"
rm -rf "$STATIC" && mkdir -p "$STATIC"
for l in openh264 vpx aom opus mp3lame; do
  A="/usr/lib/$LIBDIR/lib$l.a"
  [ -f "$A" ] || { echo "정적 라이브러리 없음: $A"; exit 1; }
  ln -s "$A" "$STATIC/"
done

echo "== configure"
./configure \
  --extra-ldflags="-L$STATIC" \
  --extra-libs="-lstdc++ -lm -lpthread" \
  --disable-everything --disable-autodetect \
  --disable-debug --disable-doc --disable-ffplay --disable-ffprobe \
  --enable-small --enable-lto --enable-zlib \
  --enable-gnutls \
  --enable-protocol=file,pipe,http,https,tcp,tls,crypto,hls \
  --enable-libopenh264 --enable-libvpx --enable-libaom --enable-libopus --enable-libmp3lame \
  --enable-demuxer=mov,matroska,hls,mpegts,mp3,ogg,aac,flac,wav \
  --enable-muxer=mp4,matroska,webm,mp3,ipod,adts,null \
  --enable-parser=h264,vp9,av1,aac,opus,vorbis,mpegaudio,flac \
  --enable-decoder=h264,vp9,libaom_av1,aac,opus,vorbis,mp3,flac,pcm_s16le,wrapped_avframe,rawvideo \
  --enable-encoder=libopenh264,libvpx_vp9,libaom_av1,libopus,aac,libmp3lame \
  --enable-bsf=aac_adtstoasc,extract_extradata,vp9_superframe,vp9_superframe_split,av1_frame_merge,av1_frame_split,h264_mp4toannexb,null \
  --enable-filter=scale,aresample,format,aformat,null,anull,fps,trim,atrim,setpts,asetpts,volume \
  --enable-indev=lavfi --enable-filter=testsrc2,sine \
  || { tail -n 50 ffbuild/config.log; exit 1; }

echo "== 빌드"
make -j"$(nproc)" ffmpeg
strip ffmpeg

echo "== 검증"
./ffmpeg -hide_banner -version | tee ../version-linux.txt
if grep -Eq -- '--enable-(gpl|nonfree|version3)' ../version-linux.txt; then echo "GPL/nonfree 옵션 감지"; exit 1; fi
./ffmpeg -hide_banner -loglevel error -y -f lavfi -i testsrc2=size=320x180:rate=30:duration=1 \
  -c:v libopenh264 -pix_fmt yuv420p smoke.mp4

mkdir -p "$(dirname "$OUT")"
cp ffmpeg "$OUT"
echo "완료: $OUT"
