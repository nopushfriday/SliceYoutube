# tauri build + make-portable 이후, Windows 배포 파일이 올바른지 검증한다.
#  - 설치 파일(NSIS)과 포터블 zip 이 있는지
#  - 포터블 zip 안에 앱과 sidecar 3개가 있는지
#  - 번들된 ffmpeg 가 실행되고 GPL 이 아닌지, 필요한 인코더가 있는지
#  사용 예: pwsh scripts/verify-build.ps1 -Version 0.1.0
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fail = @()

$setup = Get-ChildItem (Join-Path $root 'src-tauri\target\release\bundle\nsis') -Filter '*-setup.exe' -ErrorAction SilentlyContinue
if (-not $setup) { $fail += '설치 파일(NSIS)이 없습니다' }

$zip = Join-Path $root "dist\slice-youtube-$Version-portable-x64.zip"
if (-not (Test-Path $zip)) { $fail += "포터블 zip 이 없습니다: $zip" }
else {
  $dir = Join-Path ([IO.Path]::GetTempPath()) ("verify-" + [guid]::NewGuid().ToString('N'))
  Expand-Archive $zip $dir -Force
  $app = Get-ChildItem $dir -Recurse -Filter 'slice-youtube.exe' | Select-Object -First 1
  if (-not $app) { $fail += 'zip 에 slice-youtube.exe 가 없습니다' }
  else {
    foreach ($f in 'yt-dlp.exe', 'ffmpeg.exe', 'deno.exe') {
      if (-not (Test-Path (Join-Path $app.DirectoryName $f))) { $fail += "zip 에 $f 가 없습니다" }
    }
    $ff = Join-Path $app.DirectoryName 'ffmpeg.exe'
    if (Test-Path $ff) {
      $ver = & $ff -hide_banner -version 2>&1 | Out-String
      if ($LASTEXITCODE -ne 0) { $fail += 'ffmpeg.exe 실행 실패' }
      if ($ver -match '--enable-(gpl|nonfree|version3)') { $fail += 'ffmpeg 에 GPL/nonfree 옵션이 있습니다' }
      $enc = & $ff -hide_banner -encoders 2>&1 | Out-String
      foreach ($e in 'libopenh264', 'libvpx-vp9', 'libaom-av1', 'libopus', 'libmp3lame') {
        if ($enc -notmatch " $e ") { $fail += "ffmpeg 인코더 없음: $e" }
      }
    }
    foreach ($f in 'yt-dlp.exe', 'deno.exe') {
      $p = Join-Path $app.DirectoryName $f
      if (Test-Path $p) {
        & $p --version *> $null
        if ($LASTEXITCODE -ne 0) { $fail += "$f 실행 실패" }
      }
    }
  }
  Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}

if ($fail) { $fail | ForEach-Object { Write-Host "::error::$_" }; throw '빌드 결과 검증 실패' }
Write-Host '빌드 결과 검증 통과'
