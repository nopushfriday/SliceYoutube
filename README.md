# SliceYoutube

**English** · [한국어](README.ko.md)

A desktop app for Windows, Linux and macOS that downloads **only the part of a YouTube video you need**.
Tauri 2 · Svelte 5 · yt-dlp

![SliceYoutube — a 7.5 s to 12.6 s range of "Me at the zoo" set and queued as a clip](docs/screenshot-slice.jpg)

*Watch the video, set the range with `Q` / `W`, press `Enter` to queue it, and only that range is downloaded.*

## Features

- Set start and end points while watching, queue several ranges, and download them in order
- Fetches **only the selected range** with `yt-dlp --download-sections` instead of the whole video
- Codec choice: Auto / H.264 / VP9 / AV1 (if the video doesn't offer the codec, it is re-encoded after download)
- Audio-only (mp3) extraction
- 14 UI languages, picked automatically from your system language and changeable in the app

## Install

Download from [Releases](https://github.com/nopushfriday/slice-youtube/releases).

- `slice-youtube_<version>_x64-setup.exe` — installer
- `slice-youtube-<version>-portable-x64.zip` — portable build, just unzip and run
- `slice-youtube_<version>_amd64.AppImage` — Linux (x86_64)
- `slice-youtube_<version>_aarch64.dmg` — macOS (Apple Silicon)

> The macOS build is distributed without Apple developer signing/notarization, so the first launch shows an "unidentified developer" warning. Click "Open Anyway" in System Settings → Privacy & Security, or run `xattr -dr com.apple.quarantine /Applications/slice-youtube.app` in a terminal.

## Usage

1. Enter a video URL and load it.
2. While playing, press `Q` to set the start and `W` to set the end. (You can also drag the timeline handles.)
3. Press `Enter` to add it to the queue, repeat as needed, then start the download.

### Shortcuts

| Key | Action |
|---|---|
| `Q` / `W` | Set start / end point |
| `Space` | Play / pause |
| `←` `→` | Seek 5 s (`Shift`: 1 s) |
| `,` `.` | Step one frame |
| `\` | Loop the range |
| `Enter` | Add to queue |

Dragging a timeline handle moves the video position with it; hold `Shift` while dragging for fine adjustment.

### Known issues

- AV1 encoding (the fallback when a video doesn't offer AV1) uses libaom, so it is slower than H.264/VP9.

## Build

Requires: Node.js, pnpm, Rust

### Windows (PowerShell)

```powershell
pnpm install
pwsh scripts/fetch-binaries.ps1      # fetch yt-dlp, deno
pwsh scripts/build-ffmpeg-win.ps1    # build ffmpeg 9.0.2
pnpm tauri build                     # installer (NSIS)
pwsh scripts/make-portable.ps1       # portable zip
pwsh scripts/verify-build.ps1        # verify the output (optional)
```

- Installer: `src-tauri/target/release/bundle/nsis/slice-youtube_<version>_x64-setup.exe`
- Portable zip: `dist/slice-youtube-<version>-portable-x64.zip`

### Linux (Debian/Ubuntu)

```bash
pnpm install
pwsh scripts/fetch-binaries.ps1      # fetch yt-dlp, deno
bash scripts/build-ffmpeg-linux.sh   # build ffmpeg 9.0.2 (needs sudo)
pnpm tauri build                     # AppImage
bash scripts/verify-build-linux.sh   # verify the output (optional)
```

- AppImage: `src-tauri/target/release/bundle/appimage/slice-youtube_<version>_amd64.AppImage`

### macOS (Apple Silicon, macOS 14+)

```bash
pnpm install
pwsh scripts/fetch-binaries.ps1      # fetch yt-dlp, deno (needs PowerShell 7: brew install powershell)
bash scripts/build-ffmpeg-macos.sh   # build ffmpeg 9.0.2 (needs Homebrew)
pnpm tauri build                     # .app / .dmg
bash scripts/verify-build-macos.sh   # verify the output (optional)
```

- DMG: `src-tauri/target/release/bundle/dmg/slice-youtube_<version>_aarch64.dmg`

### Notes

- The version follows `version` in `src-tauri/tauri.conf.json`. When releasing, bump `version` in `package.json` and `src-tauri/Cargo.toml` too.
- The sidecar binaries (yt-dlp, deno, ffmpeg) are not in git, so the steps above are needed before `tauri dev` / `tauri build`. For development only, use `pnpm tauri dev` instead of `pnpm tauri build`.
- The ffmpeg scripts accept `-Version 9.0.2` and `-Hw sw` (no hardware encoders, Windows) to change the version and configuration. The configure options match [`build-ffmpeg.yml`](.github/workflows/build-ffmpeg.yml), so change both together.
- Linux also needs Tauri's [system dependencies](https://v2.tauri.app/start/prerequisites/#linux).

## Third-party

The app ships the programs below as separate executables (sidecars) and runs them as processes rather than linking them. The binaries are not included in git.

| Binary | License | Source |
|---|---|---|
| yt-dlp | Unlicense | [yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp) |
| deno | MIT | [denoland/deno](https://github.com/denoland/deno) |
| ffmpeg | LGPL 2.1+ | [FFmpeg/FFmpeg `n9.0.2`](https://github.com/FFmpeg/FFmpeg/tree/n9.0.2) |
