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
pwsh scripts/fetch-binaries.ps1   # src-tauri/binaries 에 yt-dlp, deno, ffmpeg(essentials) 준비
pnpm tauri dev
```

## Sidecar

| 바이너리 | 용도 |
|---|---|
| yt-dlp | 영상 정보 조회와 구간 다운로드. 앱을 열 때마다 최신 버전으로 자동 업데이트(앱 데이터 폴더의 복사본을 갱신) |
| deno | YouTube 추출에 필요한 JS 런타임 |
| ffmpeg (essentials) | 구간 추출·병합, 코덱 변환 (H.264 libx264 / VP9 libvpx / AV1 libaom) |

sidecar 바이너리는 git에 포함하지 않습니다.

## 알려진 사항

- AV1 인코딩(영상이 AV1을 제공하지 않을 때의 폴백)은 libaom을 쓰므로 H.264/VP9보다 느립니다.
