# sidecar 바이너리(yt-dlp, deno)를 src-tauri/binaries 에 준비한다.
#  - yt-dlp : GitHub 최신 릴리스 (앱이 실행 시 스스로 최신으로 업데이트하므로 번들본은 초기값)
#  - deno   : GitHub 최신 릴리스 (YouTube 추출용 JS 런타임)
#  Windows, Linux(x86_64), macOS(Apple Silicon) 를 지원한다. (실행 중인 OS 의 바이너리를 받는다)
#  ffmpeg 는 받지 않는다. scripts/build-ffmpeg-win.ps1 (Linux 는 build-ffmpeg-linux.sh)로 직접 빌드한다.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'src-tauri\binaries'
New-Item -ItemType Directory -Force $bin | Out-Null
$triple = ((rustc -vV | Select-String 'host:').ToString() -split ' ')[1]
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("cut-dlp-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

$isWin = $IsWindows -or $env:OS -eq 'Windows_NT'
$isMac = (-not $isWin) -and $IsMacOS
$ext = if ($isWin) { '.exe' } else { '' }
$ytAsset = if ($isWin) { 'yt-dlp.exe' } elseif ($isMac) { 'yt-dlp_macos' } else { 'yt-dlp_linux' }
$denoAsset = if ($isWin) { 'deno-x86_64-pc-windows-msvc.zip' } elseif ($isMac) { "deno-$triple.zip" } else { 'deno-x86_64-unknown-linux-gnu.zip' }

try {
  Write-Host 'yt-dlp ...'
  $ytDest = Join-Path $bin "yt-dlp-$triple$ext"
  Invoke-WebRequest "https://github.com/yt-dlp/yt-dlp/releases/latest/download/$ytAsset" -OutFile $ytDest

  Write-Host 'deno ...'
  $z = Join-Path $tmp 'deno.zip'
  Invoke-WebRequest "https://github.com/denoland/deno/releases/latest/download/$denoAsset" -OutFile $z
  Expand-Archive $z (Join-Path $tmp 'deno') -Force
  $denoDest = Join-Path $bin "deno-$triple$ext"
  Copy-Item (Join-Path $tmp "deno\deno$ext") $denoDest -Force

  if (-not $isWin) { chmod +x $ytDest $denoDest }}
finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host 'done.'
