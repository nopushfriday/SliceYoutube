# 포터블 zip 을 만든다. (tauri build 이후 실행)
#  zip 안: slice-youtube.exe + sidecar(yt-dlp, ffmpeg, deno) + README/라이선스 안내
#  사용 예: pwsh scripts/make-portable.ps1 -Version 0.1.0
param(
  [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$rel = Join-Path $root 'src-tauri\target\release'
$name = "slice-youtube-$Version-portable-x64"
$out = Join-Path $root 'dist'
$stage = Join-Path $out $name

if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

foreach ($f in 'slice-youtube.exe', 'yt-dlp.exe', 'ffmpeg.exe', 'deno.exe') {
  $src = Join-Path $rel $f
  if (-not (Test-Path $src)) { throw "$f 를 찾을 수 없습니다: $src  (먼저 pnpm tauri build 를 실행하세요)" }
  Copy-Item $src $stage
}

@"
slice-youtube $Version (portable)

실행: slice-youtube.exe
- 같은 폴더의 yt-dlp.exe / ffmpeg.exe / deno.exe 를 사용합니다. 이동할 때는 폴더째 옮기세요.
- yt-dlp 는 실행할 때마다 최신 버전으로 자동 업데이트됩니다 (%APPDATA%\com.slice-youtube.app\bin).
- Windows 10/11 의 WebView2 런타임이 필요합니다 (Windows 11 에는 기본 포함).

구성 요소와 라이선스
- yt-dlp  : Unlicense            https://github.com/yt-dlp/yt-dlp
- deno    : MIT                  https://github.com/denoland/deno
- ffmpeg  : LGPL (정적 빌드)      https://ffmpeg.org
            소스와 빌드 정보는 이 프로젝트 릴리스의 ffmpeg-* 항목에 있습니다.
"@ | Set-Content -Path (Join-Path $stage 'README.txt') -Encoding UTF8

$zip = Join-Path $out "$name.zip"
if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -LiteralPath $stage -Recurse -Force
Write-Host "created $zip"
