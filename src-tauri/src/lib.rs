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
    /// 가로/세로 비율 (자르기 미리보기용). 알 수 없으면 16/9
    aspect: f64,
    /// 유튜브 임베드 플레이어로 재생 가능한지. false면 스트림 재생으로 대체한다
    embeddable: bool,
    formats: Vec<FormatInfo>,
}

/// 영상 가장자리를 잘라낼 비율(%). 해상도를 몰라도 되도록 비율로 받는다.
#[derive(Deserialize, Default, Clone, Copy)]
struct Crop {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

impl Crop {
    fn is_active(&self) -> bool {
        self.left > 0.0 || self.right > 0.0 || self.top > 0.0 || self.bottom > 0.0
    }

    fn validate(&self) -> Result<(), String> {
        let sides = [self.left, self.right, self.top, self.bottom];
        if sides.iter().any(|v| !v.is_finite() || !(0.0..100.0).contains(v)) {
            return Err("자르기 값은 0~100% 사이여야 합니다".into());
        }
        if self.left + self.right >= 95.0 || self.top + self.bottom >= 95.0 {
            return Err("남는 영상 영역이 너무 작습니다 (좌+우, 상+하는 각각 95% 미만)".into());
        }
        Ok(())
    }

    /// ffmpeg crop 필터. yuv420p/H.264는 짝수 크기만 허용하므로 모든 값을 짝수로 맞춘다.
    fn filter(&self) -> Option<String> {
        if !self.is_active() {
            return None;
        }
        let f = |v: f64| format!("{:.5}", v / 100.0);
        let w = f(100.0 - self.left - self.right);
        let h = f(100.0 - self.top - self.bottom);
        Some(format!(
            "crop=trunc(iw*{w}/2)*2:trunc(ih*{h}/2)*2:trunc(iw*{}/2)*2:trunc(ih*{}/2)*2",
            f(self.left),
            f(self.top)
        ))
    }
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
    /// 하드웨어 인코딩 사용(지원되는 경우에만). 미지원/실패 시 CPU로 자동 폴백
    hw_accel: bool,
    /// "auto" | "h264" | "vp9" | "av1"
    codec: String,
    /// 상하좌우 자르기(%). 하나라도 있으면 구간을 재인코딩한다 (오디오만일 때는 무시)
    #[serde(default)]
    crop: Crop,
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
    let deno = dir.join(format!("deno{}", std::env::consts::EXE_SUFFIX));
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
    Ok(dir.join(format!("yt-dlp{}", std::env::consts::EXE_SUFFIX)))
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
    let bundled = bundled_dir()?.join(format!("yt-dlp{}", std::env::consts::EXE_SUFFIX));
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

/// 번들 ffmpeg에서 쓸 수 있는 H.264 인코더를 고른다.
/// libx264(GPL 빌드, 개발용) > libopenh264(배포용 LGPL 빌드) > h264_mf(Windows Media Foundation)
async fn h264_encoder(app: &AppHandle) -> String {
    let text = match app.shell().sidecar("ffmpeg") {
        Ok(c) => c
            .args(["-hide_banner", "-encoders"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default(),
        Err(_) => String::new(),
    };
    ["libx264", "libopenh264", "h264_mf"]
        .into_iter()
        .find(|n| text.contains(&format!(" {n} ")))
        .unwrap_or("libx264")
        .to_string()
}

/// 해상도에 맞는 목표 비트레이트.
/// libopenh264/h264_mf는 CRF가 없고 비트레이트 기준이며 기본값(200kbps)이 매우 낮아 반드시 지정해야 한다.
fn target_bitrate(height: u32) -> &'static str {
    match height {
        0..=360 => "1M",
        361..=480 => "2M",
        481..=720 => "4M",
        721..=1080 => "8M",
        1081..=1440 => "14M",
        _ => "28M",
    }
}

/// 실제로 받게 될 영상의 높이(포맷 목록과 화질 상한으로 추정)
fn selected_height(formats: &[FormatInfo], cap: u32, codec: Option<&str>) -> u32 {
    formats
        .iter()
        .filter(|f| codec.is_none_or(|c| f.codec == c) && (cap == 0 || f.height <= cap))
        .map(|f| f.height)
        .max()
        .unwrap_or(if cap == 0 { 1080 } else { cap })
}

/// 하드웨어 H.264 인코더 후보 (우선순위 순). QSV는 별도 라이브러리가 필요해 Media Foundation이 대신한다.
/// (macOS 는 VideoToolbox 하나만 쓴다)
const HW_CANDIDATES: [&str; 4] = ["h264_nvenc", "h264_amf", "h264_mf", "h264_videotoolbox"];

/// 하드웨어 인코더 인자 (일정 품질 기준: 화면이 복잡하면 비트레이트가 자동으로 늘어난다)
/// 일정 품질 모드를 지원하지 않는 드라이버는 probe_hw의 시험 인코딩에서 걸러져 CPU(CRF)로 넘어간다.
fn hw_h264_args(enc: &str) -> Vec<String> {
    let v: Vec<&str> = match enc {
        "h264_nvenc" => vec![
            "-c:v", enc, "-preset", "p5", "-rc", "vbr", "-cq", "20", "-b:v", "0", "-pix_fmt", "yuv420p",
        ],
        "h264_amf" => vec![
            "-c:v", enc, "-quality", "quality", "-rc", "cqp", "-qp_i", "20", "-qp_p", "22", "-pix_fmt",
            "yuv420p",
        ],
        // -allow_sw 0: 하드웨어 인코더를 쓸 수 없으면 실패시켜 소프트웨어로 대체되게 한다
        "h264_videotoolbox" => {
            vec!["-c:v", enc, "-allow_sw", "0", "-q:v", "65", "-pix_fmt", "yuv420p"]
        }
        // hw_encoding=1: 하드웨어 변환기가 없으면 실패시켜 소프트웨어로 대체되게 한다
        _ => vec!["-c:v", enc, "-hw_encoding", "1", "-rate_control", "quality", "-quality", "80"],
    };
    v.into_iter().map(String::from).collect()
}

/// 실제로 하드웨어 인코딩이 되는지 짧은 테스트 인코딩으로 확인한다.
/// (컴파일되어 있어도 GPU/드라이버가 없으면 실패하므로 인코더 목록만으로는 알 수 없다)
async fn probe_hw(app: &AppHandle) -> Option<String> {
    for enc in HW_CANDIDATES {
        let mut args: Vec<String> = [
            "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
            "testsrc2=size=640x360:rate=1:duration=1", "-frames:v", "1",
        ]
        .map(String::from)
        .to_vec();
        args.extend(hw_h264_args(enc));
        args.extend(["-f", "null", "-"].map(String::from));
        let Ok(cmd) = app.shell().sidecar("ffmpeg") else {
            return None;
        };
        if let Ok(o) = cmd.args(args).output().await {
            if o.status.success() {
                return Some(enc.to_string());
            }
        }
    }
    None
}

/// 감지된 하드웨어 인코더 (앱 실행 중 한 번만 검사해 재사용)
#[derive(Default)]
struct HwCache(Mutex<Option<Option<String>>>);

async fn detect_hw(app: &AppHandle, cache: &HwCache) -> Option<String> {
    if let Some(v) = cache.0.lock().unwrap().clone() {
        return v;
    }
    let found = probe_hw(app).await;
    *cache.0.lock().unwrap() = Some(found.clone());
    found
}

/// UI 표시용: 감지된 하드웨어 인코더 이름 (없으면 None)
#[tauri::command]
async fn hw_encoder(app: AppHandle, cache: State<'_, HwCache>) -> Result<Option<String>, String> {
    Ok(detect_hw(&app, &cache).await)
}

/// 다운로드한 파일을 지정 코덱으로 인코딩하는 ffmpeg 인자
/// `hw`가 Some이면 H.264를 해당 하드웨어 인코더로 인코딩한다.
fn encode_args(
    codec: &str,
    input: &str,
    output: &str,
    h264_enc: &str,
    bitrate: &str,
    hw: Option<&str>,
    vf: Option<&str>,
) -> Vec<String> {
    if let (Some(enc), "h264") = (hw, codec) {
        let mut a: Vec<String> = ["-hide_banner", "-y", "-i", input].map(String::from).to_vec();
        if let Some(vf) = vf {
            a.extend(["-vf".to_string(), vf.to_string()]);
        }
        a.extend(hw_h264_args(enc));
        a.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", output].map(String::from));
        return a;
    }
    let mut a: Vec<&str> = vec!["-hide_banner", "-y", "-i", input];
    if let Some(vf) = vf {
        a.extend(["-vf", vf]);
    }
    match codec {
        "h264" if h264_enc == "libx264" => a.extend([
            "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a",
            "aac", "-b:a", "192k",
        ]),
        "h264" => a.extend([
            "-c:v", h264_enc, "-b:v", bitrate, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a",
            "192k",
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

/// 미리보기 스트림을 중계하는 로컬 프록시 상태
#[derive(Default)]
struct StreamProxy {
    port: tokio::sync::Mutex<Option<u16>>,
    upstream: std::sync::Arc<Mutex<String>>,
}

const PREVIEW_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// googlevideo는 브라우저 형태의 User-Agent가 없으면 403을 돌려주고, 웹뷰는 이를 직접 설정할 수 없다.
/// 그래서 127.0.0.1에서 요청(Range 포함)을 받아 헤더를 붙여 원본으로 전달한다.
async fn serve_proxy(listener: tokio::net::TcpListener, upstream: std::sync::Arc<Mutex<String>>) {
    use futures_util::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let client = reqwest::Client::new();
    loop {
        let Ok((mut sock, _)) = listener.accept().await else { continue };
        let client = client.clone();
        let upstream = upstream.clone();
        tauri::async_runtime::spawn(async move {
            let mut buf = vec![0u8; 8192];
            let Ok(n) = sock.read(&mut buf).await else { return };
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let range = head.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("range").then(|| v.trim().to_string())
            });
            // 직후 첫 요청이 간헐적으로 403이 되는 경우가 있어 짧게 재시도한다
            let mut result = None;
            for _ in 0..3 {
                let url = upstream.lock().unwrap().clone();
                let mut rq = client.get(&url).header("User-Agent", PREVIEW_UA);
                if let Some(r) = &range {
                    rq = rq.header("Range", r);
                }
                let r = rq.send().await;
                let retry = matches!(&r, Ok(x) if x.status().as_u16() == 403);
                result = Some(r);
                if !retry {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            }
            let Some(Ok(resp)) = result else {
                let _ = sock
                    .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await;
                return;
            };
            let mut out = format!(
                "HTTP/1.1 {} {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n",
                resp.status().as_u16(),
                resp.status().canonical_reason().unwrap_or("")
            );
            for h in ["content-type", "content-length", "content-range", "accept-ranges"] {
                if let Some(v) = resp.headers().get(h).and_then(|v| v.to_str().ok()) {
                    out.push_str(&format!("{h}: {v}\r\n"));
                }
            }
            out.push_str("\r\n");
            if sock.write_all(out.as_bytes()).await.is_err() {
                return;
            }
            let mut body = resp.bytes_stream();
            while let Some(Ok(chunk)) = body.next().await {
                // 플레이어가 시크하며 연결을 닫으면 쓰기가 실패하므로 여기서 멈춘다
                if sock.write_all(&chunk).await.is_err() {
                    return;
                }
            }
        });
    }
}

/// 임베드가 안 되는 영상의 미리보기용 스트림 주소 (낮은 화질, 소리가 합쳐진 포맷 우선).
/// 로컬 프록시 주소를 돌려주며, <video>가 재생하는 구간만 HTTP Range로 받으므로
/// 긴 영상도 전체를 내려받지 않는다.
#[tauri::command]
async fn stream_url(
    app: AppHandle,
    proxy: State<'_, StreamProxy>,
    url: String,
) -> Result<String, String> {
    let mut args = base_args()?;
    args.extend(
        [
            "-g",
            "--no-playlist",
            "--no-warnings",
            "-f",
            "18/b[height<=480][acodec!=none][protocol=https]/bv*[height<=480][protocol=https]/b[height<=480]",
        ]
        .map(String::from),
    );
    args.push(url);
    let out = ytdlp_cmd(&app)
        .await?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let direct = match text.lines().map(str::trim).find(|l| l.starts_with("http")) {
        Some(u) if out.status.success() => u.to_string(),
        _ => return Err("미리보기 스트림 주소를 가져오지 못했습니다".into()),
    };
    *proxy.upstream.lock().unwrap() = direct;

    let mut port = proxy.port.lock().await;
    let p = match *port {
        Some(p) => p,
        None => {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| e.to_string())?;
            let p = listener.local_addr().map_err(|e| e.to_string())?.port();
            tauri::async_runtime::spawn(serve_proxy(listener, proxy.upstream.clone()));
            *port = Some(p);
            p
        }
    };
    // 영상이 바뀌면 브라우저 캐시를 쓰지 않도록 매번 다른 쿼리를 붙인다
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Ok(format!("http://127.0.0.1:{p}/stream?{nonce}"))
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
    // HLS(m3u8) 포맷은 제외한다: 구간 수신이 비효율적이고 재인코딩 시 코덱이 바뀌는 문제가 있었다.
    // (https가 하나도 없는 사이트면 전체 포맷을 사용)
    let collect = |https_only: bool| {
        let mut out: Vec<FormatInfo> = Vec::new();
        for f in v["formats"].as_array().into_iter().flatten() {
            if https_only && f["protocol"].as_str() != Some("https") {
                continue;
            }
            let Some(codec) = f["vcodec"].as_str().and_then(norm_codec) else {
                continue;
            };
            let height = f["height"].as_u64().unwrap_or(0) as u32;
            if !out.iter().any(|x| x.codec == codec && x.height == height) {
                out.push(FormatInfo { codec: codec.to_string(), height });
            }
        }
        out
    };
    let mut formats = collect(true);
    if formats.is_empty() {
        formats = collect(false);
    }

    let aspect = v["formats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f["width"].as_f64()?, f["height"].as_f64()?)))
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .or_else(|| Some((v["width"].as_f64()?, v["height"].as_f64()?)))
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .map_or(16.0 / 9.0, |(w, h)| w / h);

    Ok(VideoInfo {
        aspect,
        embeddable: v["extractor_key"].as_str() == Some("Youtube")
            && v["playable_in_embed"].as_bool() != Some(false),
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

/// 실제로 받게 될 영상 스트림의 코덱을 정한다. (포맷 정보가 없으면 None)
/// 같은 해상도라면 정확한 컷(재인코딩)일 때는 빠른 H.264를, 아니면 압축 효율이 좋은 코덱을 우선한다.
fn pick_src_codec(formats: &[FormatInfo], cap: u32, accurate: bool) -> Option<&'static str> {
    let best = formats
        .iter()
        .filter(|f| cap == 0 || f.height <= cap)
        .map(|f| f.height)
        .max()?;
    let order: [&'static str; 3] = if accurate {
        ["h264", "vp9", "av1"]
    } else {
        ["av1", "vp9", "h264"]
    };
    order
        .into_iter()
        .find(|c| formats.iter().any(|f| f.codec == *c && f.height == best))
}

/// 정확한 컷(재인코딩)에서 받는 스트림 코덱에 맞춰 인코더를 명시한다.
/// 지정하지 않으면 ffmpeg가 출력 컨테이너(mp4)의 기본값인 H.264로 인코딩해 버린다.
fn cut_encoder_args(
    src_codec: Option<&str>,
    cut_hw: Option<&str>,
    h264_enc: &str,
    bitrate: &str,
    vf: Option<&str>,
) -> Vec<String> {
    let mut v: Vec<String> = match (src_codec, cut_hw) {
        (Some("h264"), Some(hw)) => hw_h264_args(hw),
        (Some("h264"), None) if h264_enc == "libx264" => {
            ["-c:v", "libx264", "-preset", "fast", "-crf", "18", "-pix_fmt", "yuv420p"]
                .map(String::from)
                .to_vec()
        }
        (Some("h264"), None) => ["-c:v", h264_enc, "-b:v", bitrate, "-pix_fmt", "yuv420p"]
            .map(String::from)
            .to_vec(),
        (Some("vp9"), _) => ["-c:v", "libvpx-vp9", "-crf", "30", "-b:v", "0", "-row-mt", "1", "-cpu-used", "4"]
            .map(String::from)
            .to_vec(),
        (Some("av1"), _) => {
            ["-c:v", "libaom-av1", "-crf", "30", "-b:v", "0", "-cpu-used", "8", "-row-mt", "1"]
                .map(String::from)
                .to_vec()
        }
        _ => vec!["-b:v".to_string(), bitrate.to_string()],
    };
    if let Some(vf) = vf {
        v.extend(["-vf".to_string(), vf.to_string()]);
    }
    v
}

/// yt-dlp 다운로드 인자를 만든다. `cut_hw`가 Some이면 정확한 컷의 재인코딩을 해당 하드웨어 인코더로 한다.
fn build_dl_args(
    req: &DownloadReq,
    dl_dir: &str,
    src_codec: Option<&str>,
    h264_enc: &str,
    bitrate: &str,
    cut_hw: Option<&str>,
    vf: Option<&str>,
) -> Result<Vec<String>, String> {
    let cap = req.max_height;
    let mut args = base_args()?;
    args.extend(
        ["--newline", "--no-playlist", "--windows-filenames", "--no-mtime"].map(String::from),
    );
    args.push("--download-sections".into());
    args.push(format!("*{}-{}", req.start, req.end));
    if req.accurate {
        args.push("--force-keyframes-at-cuts".into());
        if !req.audio_only {
            // 정확한 컷은 구간을 재인코딩한다. CRF가 없는 인코더(openh264 등)의 기본 비트레이트가
            // 매우 낮으므로 출력 비트레이트를 명시한다. (copy 모드에서는 쓰이지 않음)
            let out_args = cut_encoder_args(src_codec, cut_hw, h264_enc, bitrate, vf).join(" ");
            args.push("--downloader-args".into());
            args.push(format!("ffmpeg_o:{out_args}"));
        }
    }
    if req.audio_only {
        args.extend(["-x", "--audio-format", "mp3", "-f", "ba/b"].map(String::from));
    } else {
        // 높이 상한(있다면) 내에서 해상도 → fps 순으로 최고 화질. 분리 스트림(bv+ba)이라 구간만 수신
        let sort = if cap == 0 { "res,fps".to_string() } else { format!("res:{cap},fps") };
        let hf = if cap == 0 { String::new() } else { format!("[height<={cap}]") };
        // 직접 https 스트림(DASH)을 우선하고, 없을 때만 그 외(HLS 등)로 대체한다.
        let selector = match src_codec {
            Some(c) => {
                let p = codec_prefix(c);
                format!(
                    "bv*[vcodec^={p}]{hf}[protocol=https]+ba[protocol=https]/bv*[vcodec^={p}]{hf}+ba"
                )
            }
            // 포맷 정보가 없는 사이트
            None => format!("bv*{hf}[protocol=https]+ba[protocol=https]/bv*{hf}+ba/b{hf}"),
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
    Ok(args)
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
    hw_cache: State<'_, HwCache>,
    mut req: DownloadReq,
) -> Result<String, String> {
    if req.end <= req.start {
        return Err("끝 지점이 시작 지점보다 뒤여야 합니다".into());
    }
    req.crop.validate()?;
    // 자르기는 영상을 다시 인코딩해야 하므로 영상이 있으면 정확한 컷(재인코딩) 경로를 쓴다.
    let vf = if req.audio_only { None } else { req.crop.filter() };
    if vf.is_some() {
        req.accurate = true;
    }
    let vf = vf.as_deref();
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

    // 받을 스트림의 코덱: 지정 코덱을 그대로 받는 경우는 그 코덱, 그 외(자동/인코딩 폴백)는 직접 고른다.
    let src_codec: Option<&str> = if req.audio_only {
        None
    } else if want_codec && native {
        Some(want)
    } else {
        pick_src_codec(&req.formats, cap, req.accurate)
    };
    // 받을 영상 높이로 목표 비트레이트를 정한다. (인코더 폴백과 정확한 컷에서 사용)
    let bitrate = target_bitrate(selected_height(&req.formats, cap, src_codec));
    let h264_enc = h264_encoder(&app).await;

    // 하드웨어 인코더 감지(앱 실행 중 한 번). 지원하지 않으면 None → CPU 인코딩
    let hw = if req.hw_accel && !req.audio_only {
        detect_hw(&app, &hw_cache).await
    } else {
        None
    };
    // 정확한 컷의 재인코딩을 하드웨어로 하려면 받는 스트림이 H.264여야 한다.
    let cut_hw = if req.accurate && src_codec == Some("h264") { hw.as_deref() } else { None };

    let mut r = {
        let args = build_dl_args(&req, &dl_dir, src_codec, &h264_enc, bitrate, cut_hw, vf)?;
        let cmd = ytdlp_cmd(&app).await?.args(args);
        run_job(&app, &jobs, req.job, cmd, span, "download").await?
    };
    // 하드웨어 인코더가 실패하면(드라이버 오류, 세션 제한 등) CPU 인코딩으로 자동 재시도
    if cut_hw.is_some() && !r.cancelled && r.code != Some(0) {
        let args = build_dl_args(&req, &dl_dir, src_codec, &h264_enc, bitrate, None, vf)?;
        let cmd = ytdlp_cmd(&app).await?.args(args);
        r = run_job(&app, &jobs, req.job, cmd, span, "download").await?;
    }
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
    let encode = |hw: Option<&str>| -> Result<Command, String> {
        Ok(app
            .shell()
            .sidecar("ffmpeg")
            .map_err(|e| e.to_string())?
            .args(encode_args(want, &downloaded, &final_path, &h264_enc, bitrate, hw, vf)))
    };
    // H.264 변환은 하드웨어 인코더가 있으면 먼저 시도하고, 실패하면 CPU 인코딩으로 자동 재시도
    let hw_try = if want == "h264" { hw.as_deref() } else { None };
    let mut e = run_job(&app, &jobs, req.job, encode(hw_try)?, span, "encode").await?;
    if hw_try.is_some() && !e.cancelled && e.code != Some(0) {
        let _ = app.emit("dl-progress", Progress { job: req.job, percent: 0.0, phase: "encode" });
        e = run_job(&app, &jobs, req.job, encode(None)?, span, "encode").await?;
    }
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
        .manage(HwCache::default())
        .manage(StreamProxy::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            fetch_info,
            stream_url,
            download,
            cancel,
            ytdlp_update,
            hw_encoder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_codec_names() {
        assert_eq!(norm_codec("avc1.640028"), Some("h264"));
        assert_eq!(norm_codec("vp09.00.40.08"), Some("vp9"));
        assert_eq!(norm_codec("av01.0.08M.08"), Some("av1"));
        assert_eq!(norm_codec("none"), None);
    }

    #[test]
    fn codec_prefix_matches_normalized_names() {
        for codec in ["h264", "vp9", "av1"] {
            let prefix = codec_prefix(codec);
            assert_eq!(norm_codec(prefix), Some(codec));
        }
    }

    #[test]
    fn crop_filter_uses_even_dimensions() {
        assert_eq!(Crop::default().filter(), None);
        let c = Crop { left: 10.0, right: 10.0, top: 0.0, bottom: 5.0 };
        assert_eq!(
            c.filter().unwrap(),
            "crop=trunc(iw*0.80000/2)*2:trunc(ih*0.95000/2)*2:trunc(iw*0.10000/2)*2:trunc(ih*0.00000/2)*2"
        );
        assert!(c.validate().is_ok());
        assert!(Crop { left: 60.0, right: 40.0, ..Default::default() }.validate().is_err());
        assert!(Crop { top: -1.0, ..Default::default() }.validate().is_err());
    }

    #[test]
    fn formats_time_tag_without_colons() {
        assert_eq!(time_tag(90.5), "1m30s");
        assert_eq!(time_tag(3725.0), "1h02m05s");
        assert_eq!(time_tag(-3.0), "0m00s");
    }
}
