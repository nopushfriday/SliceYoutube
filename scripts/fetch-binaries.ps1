# sidecar 바이너리(yt-dlp, deno, ffmpeg)를 src-tauri/binaries 에 준비한다.
#  - yt-dlp : GitHub 최신 릴리스 (앱이 실행 시 스스로 최신으로 업데이트하므로 번들본은 초기값)
#  - deno   : GitHub 최신 릴리스 (YouTube 추출용 JS 런타임)
#  - ffmpeg : -FfmpegRepo 를 주면 이 프로젝트의 build-ffmpeg 워크플로가 게시한 경량 LGPL 빌드를 받는다.
#             주지 않으면 gyan.dev essentials(GPL, 약 100MB)로 대체한다. (개발용이며 배포 금지)
#
# 사용 예:
#   pwsh scripts/fetch-binaries.ps1 -FfmpegRepo owner/cut-dlp
#   (비공개 저장소는 $env:GITHUB_TOKEN 을 설정)
param(
  [string]$FfmpegRepo = $env:CUTDLP_FFMPEG_REPO,
  [string]$FfmpegTag = ''   # 비우면 'ffmpeg-' 로 시작하는 가장 최근 릴리스
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root 'src-tauri\binaries'
New-Item -ItemType Directory -Force $bin | Out-Null
$triple = ((rustc -vV | Select-String 'host:').ToString() -split ' ')[1]
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("cut-dlp-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null

$headers = @{ 'User-Agent' = 'cut-dlp-fetch' }
if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }

try {
  Write-Host 'yt-dlp ...'
  Invoke-WebRequest 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe' `
    -OutFile (Join-Path $bin "yt-dlp-$triple.exe")

  Write-Host 'deno ...'
  $z = Join-Path $tmp 'deno.zip'
  Invoke-WebRequest 'https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip' -OutFile $z
  Expand-Archive $z (Join-Path $tmp 'deno') -Force
  Copy-Item (Join-Path $tmp 'deno\deno.exe') (Join-Path $bin "deno-$triple.exe") -Force

  $dest = Join-Path $bin "ffmpeg-$triple.exe"
  if ($FfmpegRepo) {
    Write-Host "ffmpeg (경량 LGPL 빌드) <- $FfmpegRepo ..."
    $rels = Invoke-RestMethod "https://api.github.com/repos/$FfmpegRepo/releases?per_page=30" -Headers $headers
    $rel = if ($FfmpegTag) { $rels | Where-Object tag_name -eq $FfmpegTag | Select-Object -First 1 }
           else { $rels | Where-Object { $_.tag_name -like 'ffmpeg-*' } | Select-Object -First 1 }
    if (-not $rel) { throw "ffmpeg 릴리스를 찾지 못했습니다. build-ffmpeg 워크플로를 먼저 실행하세요." }
    $asset = $rel.assets | Where-Object name -eq "ffmpeg-$triple.exe" | Select-Object -First 1
    if (-not $asset) { throw "릴리스 $($rel.tag_name) 에 ffmpeg-$triple.exe 가 없습니다." }
    Write-Host "  $($rel.tag_name)"
    $h = $headers.Clone(); $h['Accept'] = 'application/octet-stream'
    Invoke-WebRequest $asset.url -Headers $h -OutFile $dest
  }
  else {
    Write-Warning 'FfmpegRepo 가 없어 gyan.dev essentials(GPL)로 대체합니다. 개발용이며 배포하면 안 됩니다.'
    $z = Join-Path $tmp 'ffmpeg.zip'
    Invoke-WebRequest 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile $z
    Expand-Archive $z (Join-Path $tmp 'ffmpeg') -Force
    $ff = Get-ChildItem (Join-Path $tmp 'ffmpeg') -Recurse -Filter ffmpeg.exe | Select-Object -First 1
    Copy-Item $ff.FullName $dest -Force
  }
}
finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host 'done.'
