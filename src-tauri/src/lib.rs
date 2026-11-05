use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::{
    ShellExt,
    process::{Command, CommandChild, CommandEvent},
};

/// 실행 중인 작업(취소용). job 하나당 현재 실행 중인 프로세스 1개
#[derive(Default)]
struct Jobs(Mutex<HashMap<u32, CommandChild>>);

/// 영상이 제공하는 비디오 포맷(코덱, 높이)
#[derive(Serialize, Deserialize, Clone)]
struct FormatInfo {
    /// "h264" | "vp9" | "av1"
    codec: String,
    height: u32,
}

#[derive(Serialize)]
struct VideoInfo {
    id: String,
    title: String,
    duration: f64,
    channel: String,
    formats: Vec<FormatInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadReq {
    job: u32,
    url: String,
    start: f64,
    end: f64,
    /// true: 컷 지점을 프레임 단위로 정확히(구간만 재인코딩), false: 키프레임 단위(빠름)
    accurate: bool,
    audio_only: bool,
    /// 0 = 제한 없음(최고 화질)
    max_height: u32,
    /// "auto" | "h264" | "vp9" | "av1"
    codec: String,
    formats: Vec<FormatInfo>,
    out_dir: String,
}

#[derive(Serialize, Clone)]
struct Progress {
    job: u32,
    percent: f64,
    /// "download" | "encode"
    phase: &'static str,
}

/// 번들된 sidecar들이 있는 폴더 (실행 파일과 같은 폴더)
fn bundled_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "실행 파일 경로를 찾을 수 없습니다".to_string())
}

/// yt-dlp 공통 인자: 번들된 ffmpeg 위치와 JS 런타임(deno 우선) 지정
fn base_args() -> Result<Vec<String>, String> {
    let dir = bundled_dir()?;
    let mut a = vec![
        "--ffmpeg-location".to_string(),
        dir.to_string_lossy().into_owned(),
    ];
    let deno = dir.join("deno.exe");
    a.push("--js-runtimes".into());
    a.push(if deno.exists() {
        format!("deno:{}", deno.display())
    } else {
        "node".to_string()
    });
    Ok(a)
}

/// 자동 업데이트가 가능한 yt-dlp 복사본 위치 (설치 폴더는 쓰기 불가일 수 있어 앱 데이터에 둔다)
fn ytdlp_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("yt-dlp.exe"))
}

async fn version_of(app: &AppHandle, path: &Path) -> Option<String> {
    let out = app
        .shell()
        .command(path)
        .args(["--version"])
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 갱신 가능한 복사본을 보장한다. 없거나 번들본이 더 최신이면 번들본으로 (다시) 복사한다.
async fn ensure_ytdlp(app: &AppHandle) -> Result<PathBuf, String> {
    let path = ytdlp_path(app)?;
    let bundled = bundled_dir()?.join("yt-dlp.exe");
    let need_copy = if !path.exists() {
        true
    } else {
        match (version_of(app, &bundled).await, version_of(app, &path).await) {
            (Some(b), Some(c)) => b > c, // 버전이 날짜 형식(2026.10.26)이라 문자열 비교로 충분
            (_, None) => true,
            _ => false,
        }
    };
    if need_copy {
        std::fs::copy(&bundled, &path).map_err(|e| format!("yt-dlp 준비 실패: {e}"))?;
    }
    Ok(path)
}

#[derive(Serialize)]
struct YtStatus {
    version: Option<String>,
    /// 이번 확인에서 새 버전으로 바뀌었는가
    updated: bool,
    /// 업데이트 확인 실패 사유 (오프라인 등). 이전 버전은 계속 사용 가능
    error: Option<String>,
}

/// yt-dlp를 최신 버전으로 업데이트한다. 앱 시작 시와 사용자가 요청할 때 호출된다.
#[tauri::command]
async fn ytdlp_update(app: AppHandle) -> Result<YtStatus, String> {
    let path = ensure_ytdlp(&app).await?;
    let before = version_of(&app, &path).await;
    let res = app.shell().command(&path).args(["-U"]).output().await;
    let after = version_of(&app, &path).await;
    let error = match res {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("업데이트를 확인하지 못했습니다")
                .trim()
                .to_string(),
        ),
        Err(e) => Some(e.to_string()),
    };
    let updated = matches!((&before, &after), (Some(b), Some(a)) if a != b);
    Ok(YtStatus { version: after, updated, error })
}

/// yt-dlp 실행 명령 (업데이트 가능한 복사본 사용)
async fn ytdlp_cmd(app: &AppHandle) -> Result<Command, String> {
    Ok(app.shell().command(ensure_ytdlp(app).await?))
}

/// yt-dlp의 vcodec 문자열을 h264/vp9/av1로 정규화
fn norm_codec(vcodec: &str) -> Option<&'static str> {
    if vcodec.starts_with("avc1") || vcodec.starts_with("h264") {
        Some("h264")
    } else if vcodec.starts_with("vp09") || vcodec.starts_with("vp9") {
        Some("vp9")
    } else if vcodec.starts_with("av01") || vcodec.starts_with("av1") {
        Some("av1")
    } else {
        None
    }
}

/// yt-dlp 포맷 필터에 쓰는 vcodec 접두사
fn codec_prefix(codec: &str) -> &'static str {
    match codec {
        "h264" => "avc1",
        "vp9" => "vp09",
        _ => "av01",
    }
}

/// 파일명에 쓸 시간 표기 (콜론 불가): 90.5 -> "1m30s"
fn time_tag(sec: f64) -> String {
    let t = sec.max(0.0) as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else {
        format!("{m}m{s:02}s")
    }
}

/// ffmpeg의 `time=HH:MM:SS.xx` 또는 yt-dlp의 `[download] 42.0%`에서 진행률(%) 추출
fn parse_progress(s: &str, span: f64) -> Option<f64> {
    if let Some(i) = s.rfind("time=") {
        let t: String = s[i + 5..].chars().take_while(|c| !c.is_whitespace()).collect();
        let mut secs = 0.0;
        for part in t.split(':') {
            secs = secs * 60.0 + part.parse::<f64>().ok()?;
        }
        return Some((secs / span * 100.0).clamp(0.0, 100.0));
    }
    if let Some(i) = s.find("[download]") {
        if let Some(j) = s[i..].find('%') {
            return s[i + 10..i + j].trim().parse::<f64>().ok();
        }
    }
    None
}

/// 다운로드한 파일을 지정 코덱으로 인코딩하는 ffmpeg 인자
fn encode_args(codec: &str, input: &str, output: &str) -> Vec<String> {
    let mut a: Vec<&str> = vec!["-hide_banner", "-y", "-i", input];
    match codec {
        "h264" => a.extend([
            "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a",
            "aac", "-b:a", "192k",
        ]),
        "vp9" => a.extend([
            "-c:v", "libvpx-vp9", "-crf", "30", "-b:v", "0", "-row-mt", "1", "-c:a", "libopus",
        ]),
        // essentials 빌드에는 SVT-AV1이 없어 libaom 사용 (-cpu-used로 속도 확보)
        _ => a.extend([
            "-c:v", "libaom-av1", "-crf", "30", "-b:v", "0", "-cpu-used", "6", "-row-mt", "1",
            "-c:a", "libopus",
        ]),
    }
    a.extend(["-movflags", "+faststart", output]);
    a.into_iter().map(String::from).collect()
}

struct Outcome {
    code: Option<i32>,
    file: Option<String>,
    last_error: String,
    cancelled: bool,
}

/// sidecar 프로세스를 실행하고 종료될 때까지 진행률 이벤트를 내보낸다.
async fn run_job(
    app: &AppHandle,
    jobs: &State<'_, Jobs>,
    job: u32,
    cmd: Command,
    span: f64,
    phase: &'static str,
) -> Result<Outcome, String> {
    let (mut rx, child) = cmd.spawn().map_err(|e| e.to_string())?;
    jobs.0.lock().unwrap().insert(job, child);

    let mut file: Option<String> = None;
    let mut last_error = String::new();
    let mut code: Option<i32> = None;

    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                let s = String::from_utf8_lossy(&b).to_string();
                if let Some(p) = s.trim().strip_prefix("CUTDLP_FILE ") {
                    file = Some(p.trim().to_string());
                    continue;
                }
                if s.contains("ERROR") || s.contains("Error") {
                    last_error = s.trim().to_string();
                }
                if let Some(percent) = parse_progress(&s, span) {
                    let _ = app.emit("dl-progress", Progress { job, percent, phase });
                }
            }
            CommandEvent::Terminated(p) => {
                code = p.code;
                break;
            }
            CommandEvent::Error(e) => last_error = e,
            _ => {}
        }
    }

    // cancel()에서 이미 제거했다면 사용자가 취소한 것
    let cancelled = jobs.0.lock().unwrap().remove(&job).is_none();
    Ok(Outcome { code, file, last_error, cancelled })
}

#[tauri::command]
async fn fetch_info(app: AppHandle, url: String) -> Result<VideoInfo, String> {
    let mut args = base_args()?;
    args.extend(["-J", "--no-playlist", "--no-warnings"].map(String::from));
    args.push(url);

    let out = ytdlp_cmd(&app)
        .await?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .unwrap_or("영상 정보를 가져오지 못했습니다");
        return Err(line.trim().to_string());
    }

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;

    // 제공되는 (코덱, 높이) 조합 수집
    let mut formats: Vec<FormatInfo> = Vec::new();
    for f in v["formats"].as_array().into_iter().flatten() {
        let Some(codec) = f["vcodec"].as_str().and_then(norm_codec) else {
            continue;
        };
        let height = f["height"].as_u64().unwrap_or(0) as u32;
        if !formats.iter().any(|x| x.codec == codec && x.height == height) {
            formats.push(FormatInfo { codec: codec.to_string(), height });
        }
    }

    Ok(VideoInfo {
        id: v["id"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        duration: v["duration"].as_f64().unwrap_or(0.0),
        channel: v["channel"]
            .as_str()
            .or(v["uploader"].as_str())
            .unwrap_or_default()
            .to_string(),
        formats,
    })
}

/// 지정 구간만 다운로드한다. (--download-sections: ffmpeg가 HTTP Range로 해당 구간만 요청)
///
/// 코덱 지정 시:
///  - 영상이 해당 코덱(높이 상한 이내)을 제공하면 그 포맷을 그대로 받는다.
///  - 제공하지 않으면 최고 화질로 구간만 받은 뒤, 받은 파일을 지정 코덱으로 인코딩한다.
///
/// 성공 시 저장된 파일 경로를 돌려준다.
#[tauri::command]
async fn download(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    req: DownloadReq,
) -> Result<String, String> {
    if req.end <= req.start {
        return Err("끝 지점이 시작 지점보다 뒤여야 합니다".into());
    }
    let span = (req.end - req.start).max(0.1);
    let cap = req.max_height;
    let want = req.codec.as_str();
    let want_codec = matches!(want, "h264" | "vp9" | "av1") && !req.audio_only;
    let native = !want_codec
        || req
            .formats
            .iter()
            .any(|f| f.codec == want && (cap == 0 || f.height <= cap));
    let reencode = !native;

    std::fs::create_dir_all(&req.out_dir).map_err(|e| e.to_string())?;
    let out_dir = req.out_dir.trim_end_matches(['/', '\\']).to_string();
    // 인코딩이 필요하면 임시 폴더에 받은 뒤 최종 폴더로 인코딩한다.
    let tmp_dir = format!("{out_dir}/.cutdlp-tmp-{}", req.job);
    let dl_dir = if reencode { tmp_dir.clone() } else { out_dir.clone() };
    let cleanup = |tmp: &str| {
        let _ = std::fs::remove_dir_all(tmp);
    };

    let mut args = base_args()?;
    args.extend(
        ["--newline", "--no-playlist", "--windows-filenames", "--no-mtime"].map(String::from),
    );
    args.push("--download-sections".into());
    args.push(format!("*{}-{}", req.start, req.end));
    if req.accurate {
        args.push("--force-keyframes-at-cuts".into());
    }
    if req.audio_only {
        args.extend(["-x", "--audio-format", "mp3", "-f", "ba/b"].map(String::from));
    } else {
        // 높이 상한(있다면) 내에서 해상도 → fps 순으로 최고 화질. 분리 스트림(bv+ba)이라 구간만 수신
        let sort = if cap == 0 { "res,fps".to_string() } else { format!("res:{cap},fps") };
        let hf = if cap == 0 { String::new() } else { format!("[height<={cap}]") };
        let selector = if want_codec && native {
            format!("bv*[vcodec^={}]{hf}+ba", codec_prefix(want))
        } else {
            // 자동(제공되는 최고 화질) 또는 인코딩 폴백용 원본
            format!("bv*{hf}+ba/b{hf}")
        };
        args.extend([
            "-S".to_string(),
            sort,
            "-f".into(),
            selector,
            "--merge-output-format".into(),
            "mp4".into(),
        ]);
    }
    args.extend([
        "-o".into(),
        format!(
            "{dl_dir}/%(title).120B [{}-{}].%(ext)s",
            time_tag(req.start),
            time_tag(req.end)
        ),
    ]);
    args.extend(["--print".into(), "after_move:CUTDLP_FILE %(filepath)s".into()]);
    args.push(req.url.clone());

    let cmd = ytdlp_cmd(&app).await?.args(args);
    let r = run_job(&app, &jobs, req.job, cmd, span, "download").await?;
    if r.cancelled {
        cleanup(&tmp_dir);
        return Err("cancelled".into());
    }
    if r.code != Some(0) {
        cleanup(&tmp_dir);
        return Err(if r.last_error.is_empty() {
            "다운로드에 실패했습니다".into()
        } else {
            r.last_error
        });
    }
    let Some(downloaded) = r.file else {
        return Ok(req.out_dir);
    };
    if !reencode {
        return Ok(downloaded);
    }

    // 폴백: 받은 파일을 지정 코덱으로 인코딩
    let name = Path::new(&downloaded)
        .file_name()
        .ok_or("파일 이름을 확인할 수 없습니다")?
        .to_string_lossy()
        .into_owned();
    let final_path = format!("{out_dir}/{name}");
    let _ = app.emit("dl-progress", Progress { job: req.job, percent: 0.0, phase: "encode" });
    let ffmpeg = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(encode_args(want, &downloaded, &final_path));
    let e = run_job(&app, &jobs, req.job, ffmpeg, span, "encode").await?;
    cleanup(&tmp_dir);
    if e.cancelled {
        let _ = std::fs::remove_file(&final_path);
        return Err("cancelled".into());
    }
    if e.code != Some(0) {
        let _ = std::fs::remove_file(&final_path);
        return Err(if e.last_error.is_empty() {
            "인코딩에 실패했습니다".into()
        } else {
            e.last_error
        });
    }
    Ok(final_path)
}

#[tauri::command]
fn cancel(jobs: State<'_, Jobs>, job: u32) {
    if let Some(child) = jobs.0.lock().unwrap().remove(&job) {
        let _ = child.kill();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Jobs::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            fetch_info,
            download,
            cancel,
            ytdlp_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
