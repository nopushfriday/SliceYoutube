# Windows 용 ffmpeg 빌드. build-ffmpeg-win.sh 를 MSYS2 MINGW64 환경에서 실행하는 래퍼이며,
# MSYS2 가 없으면 winget 으로 설치한다.
#   pwsh scripts/build-ffmpeg-win.ps1 [-Version 9.0.2] [-Hw hw|sw]
# MSYS2 를 C:\msys64 가 아닌 곳에 설치했다면 $env:MSYS2_ROOT 를 설정한다.
param(
  [string]$Version = '9.0.2',
  [ValidateSet('hw', 'sw')][string]$Hw = 'hw'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$msys = if ($env:MSYS2_ROOT) { $env:MSYS2_ROOT } else { 'C:\msys64' }
$bash = Join-Path $msys 'usr\bin\bash.exe'
if (-not (Test-Path $bash)) {
  Write-Host 'MSYS2 가 없어 winget 으로 설치합니다 ...'
  winget install --id MSYS2.MSYS2 -e --accept-package-agreements --accept-source-agreements
  if (-not (Test-Path $bash)) { throw "MSYS2 를 찾지 못했습니다: $bash (설치 경로가 다르면 `$env:MSYS2_ROOT 를 설정하세요)" }
}

$env:MSYSTEM = 'MINGW64'
$env:CHERE_INVOKING = '1'   # 로그인 셸이 현재 폴더를 유지하게 함
Push-Location $root
try {
  & $bash -l scripts/build-ffmpeg-win.sh $Version $Hw
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg 빌드 실패 (exit $LASTEXITCODE)" }
}
finally { Pop-Location }
