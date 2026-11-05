# cut-dlp

YouTube 영상에서 **필요한 구간만** 받는 데스크톱 앱. (Tauri 2 + Svelte 5 + yt-dlp)

- 영상을 보면서 `Q`(시작) / `W`(끝)로 구간을 지정하고 큐에 담아 순서대로 다운로드
- 전체를 받지 않고 `yt-dlp --download-sections`로 지정 구간만 수신
- 코덱 선택(자동 / H.264 / VP9 / AV1). 영상이 해당 코덱을 제공하지 않으면 받은 뒤 인코딩
- 오디오만(mp3) 추출 지원

## 단축키

| 키 | 동작 |
|---|---|
| `Q` / `W` | 시작 / 끝 지점 지정 |
| `Space` | 재생 · 일시정지 |
| `←` `→` | 5초 이동 (`Shift` 1초) |
| `,` `.` | 1프레임 이동 |
| `\` | 구간 반복 |
| `Enter` | 큐에 추가 |

타임라인의 핸들을 끌면 영상 위치가 따라가며, `Shift`를 누르고 끌면 미세 조절됩니다.

## 개발 환경 준비

필요: Node.js, pnpm, Rust

```powershell
pnpm install
pwsh scripts/fetch-binaries.ps1 -FfmpegRepo <owner>/<repo>   # src-tauri/binaries 에 yt-dlp, deno, ffmpeg 준비
pnpm tauri dev
```

`-FfmpegRepo`를 생략하면 개발용으로 gyan.dev essentials(GPL, 약 100MB)를 받습니다. **배포 빌드에는 쓰지 마세요.**

## ffmpeg 빌드 (배포용, LGPL)

[`.github/workflows/build-ffmpeg.yml`](.github/workflows/build-ffmpeg.yml)이 이 앱에 필요한 기능만 켠 정적 ffmpeg를 빌드해 GitHub Release로 게시합니다.
Actions 탭에서 `build-ffmpeg`를 수동 실행(workflow_dispatch)하세요. 같은 버전·설정의 릴리스가 이미 있으면 다시 빌드하지 않으므로 한 번 만들어 두고 계속 재사용합니다.

- GPL/nonfree 없음 (libx264 미사용). H.264는 libopenh264, 하드웨어 인코더(NVENC/AMF/Media Foundation)는 옵션
- 소스 tarball과 BUILD-INFO.txt를 릴리스에 함께 게시 (LGPL 소스 제공)
- 설정을 바꾸면 워크플로의 `CONFIG_REV`를 올립니다.
- openh264는 소스 빌드를 정적 링크하므로 Cisco의 H.264 특허 사용료 보증이 적용되지 않습니다. 대규모 배포 시 검토가 필요합니다.

## Sidecar

| 바이너리 | 용도 |
|---|---|
| yt-dlp | 영상 정보 조회와 구간 다운로드. 앱을 열 때마다 최신 버전으로 자동 업데이트(앱 데이터 폴더의 복사본을 갱신) |
| deno | YouTube 추출에 필요한 JS 런타임 |
| ffmpeg | 구간 추출·병합, 코덱 변환 (H.264 openh264 / VP9 libvpx / AV1 libaom), mp3 추출 |

sidecar 바이너리는 git에 포함하지 않습니다.

## 알려진 사항

- AV1 인코딩(영상이 AV1을 제공하지 않을 때의 폴백)은 libaom을 쓰므로 H.264/VP9보다 느립니다.
