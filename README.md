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

필요: Node.js, pnpm, Rust, ffmpeg(H.264/VP9/AV1 인코더 포함 빌드, PATH에 설치)

```powershell
pnpm install
pwsh scripts/fetch-binaries.ps1   # src-tauri/binaries 에 yt-dlp, ffmpeg 준비
pnpm tauri dev
```

## 알려진 사항

- YouTube 추출에 JS 런타임이 필요하며, 현재는 PC에 설치된 Node를 사용합니다. (배포 시 deno sidecar 번들 예정)
- sidecar 바이너리는 git에 포함하지 않습니다.
