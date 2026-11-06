# slice-youtube

YouTube 영상에서 **필요한 구간만** 골라 받는 Windows · Linux 데스크톱 앱.
Tauri 2 · Svelte 5 · yt-dlp

## 주요 기능

- 영상을 보면서 시작/끝 지점을 지정하고, 여러 구간을 큐에 담아 순서대로 다운로드
- 전체를 받지 않고 `yt-dlp --download-sections`로 **지정 구간만** 수신
- 코덱 선택: 자동 / H.264 / VP9 / AV1 (영상이 해당 코덱을 제공하지 않으면 받은 뒤 인코딩)
- 오디오만(mp3) 추출

## 설치

[Releases](https://github.com/roro128/slice-youtube/releases)에서 받습니다.

- `slice-youtube_<버전>_x64-setup.exe` — 설치 파일
- `slice-youtube-<버전>-portable-x64.zip` — 압축만 풀면 실행되는 포터블 버전
- `slice-youtube_<버전>_amd64.AppImage` — Linux (x86_64)

## 사용법

1. 영상 URL을 입력해 불러옵니다.
2. 재생하면서 `Q`로 시작, `W`로 끝 지점을 지정합니다. (타임라인 핸들을 끌어도 됩니다)
3. `Enter`로 큐에 추가하고, 필요한 만큼 반복한 뒤 다운로드를 시작합니다.

### 단축키

| 키 | 동작 |
|---|---|
| `Q` / `W` | 시작 / 끝 지점 지정 |
| `Space` | 재생 · 일시정지 |
| `←` `→` | 5초 이동 (`Shift` 1초) |
| `,` `.` | 1프레임 이동 |
| `\` | 구간 반복 |
| `Enter` | 큐에 추가 |

타임라인 핸들을 끌면 영상 위치가 따라가며, `Shift`를 누르고 끌면 미세 조절됩니다.

### 알려진 사항

- AV1 인코딩(영상이 AV1을 제공하지 않을 때의 폴백)은 libaom을 쓰므로 H.264/VP9보다 느립니다.

## 빌드

필요: Node.js, pnpm, Rust

sidecar 바이너리는 git에 없으므로, 실행·빌드 전에 먼저 준비합니다.

```powershell
pnpm install
pwsh scripts/fetch-binaries.ps1      # yt-dlp, deno
pwsh scripts/build-ffmpeg-win.ps1    # ffmpeg 9.0.2 (Windows)
```

```bash
# Linux (Debian/Ubuntu 계열, sudo 필요)
pwsh scripts/fetch-binaries.ps1      # yt-dlp, deno
bash scripts/build-ffmpeg-linux.sh   # ffmpeg 9.0.2
```

Linux에서 Tauri를 빌드하려면 [시스템 의존 패키지](https://v2.tauri.app/start/prerequisites/#linux)도 필요합니다.

`-Version`, `-Hw sw`(하드웨어 인코더 제외)로 버전과 구성을 바꿀 수 있습니다.
configure 옵션은 [`build-ffmpeg.yml`](.github/workflows/build-ffmpeg.yml)과 같으니, 한쪽을 바꾸면 다른 쪽도 바꾸세요.

준비가 끝나면 실행하거나 빌드합니다.

```powershell
pnpm tauri dev
```

```powershell
pnpm tauri build
pwsh scripts/make-portable.ps1 -Version <버전>   # 포터블 zip (선택)
```

## 서드파티

앱은 아래 프로그램을 별도 실행 파일(sidecar)로 함께 배포하며, 링크하지 않고 프로세스로 실행합니다. 바이너리는 git에 포함하지 않습니다.

| 바이너리 | 라이선스 | 소스 |
|---|---|---|
| yt-dlp | Unlicense | [yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp) |
| deno | MIT | [denoland/deno](https://github.com/denoland/deno) |
| ffmpeg | LGPL 2.1+ | [FFmpeg/FFmpeg `n9.0.2`](https://github.com/FFmpeg/FFmpeg/tree/n9.0.2) |
