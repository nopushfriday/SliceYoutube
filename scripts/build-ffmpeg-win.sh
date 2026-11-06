#!/usr/bin/env bash
# Windows 용 경량 LGPL ffmpeg 를 직접 빌드해 src-tauri/binaries 에 둔다.
# 보통은 래퍼를 쓴다:  pwsh scripts/build-ffmpeg-win.ps1 [-Version 9.0.2] [-Hw hw|sw]
# 직접 실행하려면 MSYS2 "MINGW64" 셸에서:  bash scripts/build-ffmpeg-win.sh [ffmpeg버전] [hw|sw]
#
# configure 옵션은 .github/workflows/build-ffmpeg.yml 과 같아야 한다. 한쪽을 고치면 다른 쪽도 고친다.
set -euo pipefail

VER="${1:-9.0.2}"
HW="${2:-hw}"

[ "${MSYSTEM:-}" = "MINGW64" ] || { echo "MSYS2 MINGW64 셸에서 실행하세요. (현재: ${MSYSTEM:-없음})"; exit 1; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/.ffmpeg-build"
OUT="$ROOT/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe"   # Tauri externalBin 은 <이름>-<타깃 트리플>.exe 를 요구한다 (패키징 때 ffmpeg.exe 로 설치됨)

PKGS=(
  base-devel git make diffutils
  mingw-w64-x86_64-toolchain mingw-w64-x86_64-pkgconf mingw-w64-x86_64-nasm mingw-w64-x86_64-zlib
  mingw-w64-x86_64-openh264 mingw-w64-x86_64-libvpx mingw-w64-x86_64-aom
  mingw-w64-x86_64-opus mingw-w64-x86_64-lame mingw-w64-x86_64-mpg123
  mingw-w64-x86_64-ffnvcodec-headers mingw-w64-x86_64-amf-headers
)
echo "== 의존 패키지 설치"
pacman -S --needed --noconfirm "${PKGS[@]}"

echo "== ffmpeg $VER 소스"
mkdir -p "$WORK"
cd "$WORK"
if [ ! -f "ffmpeg-$VER.tar.gz" ]; then
  curl -fsSL --retry 3 -o "ffmpeg-$VER.tar.gz" "https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n$VER.tar.gz" ||
    { curl -fsSL --retry 3 -o "ffmpeg-$VER.tar.xz" "https://ffmpeg.org/releases/ffmpeg-$VER.tar.xz"; }
fi
rm -rf src && mkdir src
tar -xf ffmpeg-"$VER".tar.* -C src --strip-components=1
cd src

echo "== configure ($HW)"
OPTS=(
  --arch=x86_64 --target-os=mingw32
  --disable-everything --disable-autodetect
  --disable-debug --disable-doc
  --disable-ffplay --disable-ffprobe
  --enable-static --disable-shared
  --pkg-config-flags=--static
  --extra-ldflags=-static
  "--extra-libs=-lmpg123 -lshlwapi -lstdc++"
  --enable-small --enable-lto
  --enable-zlib
  --enable-schannel
  --enable-protocol=file,pipe,http,https,tcp,tls,crypto,hls
  --enable-libopenh264 --enable-libvpx --enable-libaom --enable-libopus --enable-libmp3lame
  --enable-demuxer=mov,matroska,hls,mpegts,mp3,ogg,aac,flac,wav
  --enable-muxer=mp4,matroska,webm,mp3,ipod,adts,null
  --enable-parser=h264,vp9,av1,aac,opus,vorbis,mpegaudio,flac
  --enable-decoder=h264,vp9,libaom_av1,aac,opus,vorbis,mp3,flac,pcm_s16le,wrapped_avframe,rawvideo
  --enable-encoder=libopenh264,libvpx_vp9,libaom_av1,libopus,aac,libmp3lame
  --enable-bsf=aac_adtstoasc,extract_extradata,vp9_superframe,vp9_superframe_split,av1_frame_merge,av1_frame_split,h264_mp4toannexb,null
  --enable-filter=scale,aresample,format,aformat,null,anull,fps,trim,atrim,setpts,asetpts,volume
  --enable-indev=lavfi --enable-filter=testsrc2,sine
)
if [ "$HW" = "hw" ]; then
  OPTS+=(
    --enable-ffnvcodec --enable-nvenc --enable-amf --enable-d3d11va --enable-mediafoundation
    --enable-encoder=h264_nvenc,h264_amf,h264_mf
  )
fi
./configure "${OPTS[@]}" || { tail -n 50 ffbuild/config.log; exit 1; }

echo "== 빌드"
make -j"$(nproc)" ffmpeg.exe
strip ffmpeg.exe

echo "== 검증"
./ffmpeg.exe -hide_banner -version | tee ../version.txt
if grep -Eq -- '--enable-(gpl|nonfree|version3)' ../version.txt; then echo "GPL/nonfree 옵션 감지"; exit 1; fi
./ffmpeg.exe -hide_banner -loglevel error -y -f lavfi -i testsrc2=size=320x180:rate=30:duration=1 \
  -c:v libopenh264 -pix_fmt yuv420p smoke.mp4

mkdir -p "$(dirname "$OUT")"
cp ffmpeg.exe "$OUT"
echo "완료: $OUT"
