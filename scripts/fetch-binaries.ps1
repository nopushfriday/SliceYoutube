# sidecar 바이너리(yt-dlp, deno, ffmpeg)를 src-tauri/binaries 에 준비한다.
#  - yt-dlp : GitHub 최신 릴리스 (앱이 실행 시 스스로 최신으로 업데이트하므로 번들본은 초기값)
#  - deno   : GitHub 최신 릴리스 (YouTube 추출용 JS 런타임)
#  - ffmpeg : gyan.dev essentials 빌드 (H.264 / VP9 / AV1(libaom) 인코더 포함)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'src-tauri\binaries'
New-Item -ItemType Directory -Force $bin | Out-Null
$triple = ((rustc -vV | Select-String 'host:').ToString() -split ' ')[1]
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("cut-dlp-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

try {
  Write-Host 'yt-dlp ...'
  Invoke-WebRequest 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe' `
    -OutFile (Join-Path $bin "yt-dlp-$triple.exe")

  Write-Host 'deno ...'
  $z = Join-Path $tmp 'deno.zip'
  Invoke-WebRequest 'https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip' -OutFile $z
  Expand-Archive $z (Join-Path $tmp 'deno') -Force
  Copy-Item (Join-Path $tmp 'deno\deno.exe') (Join-Path $bin "deno-$triple.exe") -Force

  Write-Host 'ffmpeg (essentials) ...'
  $z = Join-Path $tmp 'ffmpeg.zip'
  Invoke-WebRequest 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile $z
  Expand-Archive $z (Join-Path $tmp 'ffmpeg') -Force
  $ff = Get-ChildItem (Join-Path $tmp 'ffmpeg') -Recurse -Filter ffmpeg.exe | Select-Object -First 1
  Copy-Item $ff.FullName (Join-Path $bin "ffmpeg-$triple.exe") -Force
}
finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host 'done.'
