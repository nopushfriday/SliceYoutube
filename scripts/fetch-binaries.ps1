# sidecar 바이너리(yt-dlp, ffmpeg)를 src-tauri/binaries 에 준비한다.
#  - yt-dlp: GitHub 최신 릴리스에서 다운로드
#  - ffmpeg: PATH 에 설치된 ffmpeg 를 복사 (H.264/VP9/AV1 인코더 포함 빌드 필요)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'src-tauri\binaries'
New-Item -ItemType Directory -Force $bin | Out-Null

$triple = ((rustc -vV | Select-String 'host:').ToString() -split ' ')[1]

$ytdlp = Join-Path $bin "yt-dlp-$triple.exe"
Write-Host "yt-dlp -> $ytdlp"
Invoke-WebRequest 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe' -OutFile $ytdlp

$ffmpeg = Join-Path $bin "ffmpeg-$triple.exe"
$src = (Get-Command ffmpeg -ErrorAction Stop).Source
Write-Host "ffmpeg <- $src"
Copy-Item $src $ffmpeg -Force

Write-Host 'done.'
