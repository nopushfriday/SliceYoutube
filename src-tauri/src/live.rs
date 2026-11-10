//! 유튜브 라이브의 지난 구간 자르기.
//!
//! 방송 시작부터의 DASH 조각(`&sq=번호`)은 지난 것도 번호로 직접 받을 수 있다.
//! 지정한 구간을 덮는 조각만 받아 이어 붙인 뒤 ffmpeg로 잘라 낸다. (녹화처럼 데이터를 쌓아 두지 않는다)
//! 조각의 시각은 mp4의 tfdt(방송 시작 기준 초)이며 유튜브 플레이어의 재생 시간과 같은 좌표다.

use std::{collections::HashMap, io::Write as _};

use futures_util::StreamExt;

use super::*;

/// 조각 하나의 대략적인 길이(초). 실제 길이는 조금씩 달라 위치 탐색의 보폭으로만 쓴다.
const FRAGMENT_SECS: f64 = 5.0;
/// 동시에 받는 조각 수
const FETCH_PARALLEL: usize = 6;

/// 취소 요청된 라이브 작업. 조각을 받는 동안에는 종료할 프로세스가 없어 따로 표시한다.
#[derive(Default)]
pub struct LiveCancel(pub Mutex<std::collections::HashSet<u32>>);

/// 라이브에서 받을 영상·오디오 조각 스트림
struct LiveStreams {
    title: String,
    video: String,
    audio: String,
    vcodec: String,
    height: u32,
}

fn mp4_child(b: &[u8], from: usize, to: usize, name: &[u8; 4]) -> Option<(usize, usize)> {
    let mut o = from;
    while o + 8 <= to {
        let size = u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize;
        let (hdr, total) = match size {
            1 => (16, u64::from_be_bytes(b.get(o + 8..o + 16)?.try_into().ok()?) as usize),
            0 => (8, to - o),
            n => (8, n),
        };
        if total < hdr || o + total > to {
            return None;
        }
        if b.get(o + 4..o + 8)? == name {
            return Some((o + hdr, o + total));
        }
        o += total;
    }
    None
}

/// 조각이 시작하는 시각(방송 시작 기준 초). moov의 mdhd 시간 단위와 moof의 tfdt로 계산한다.
fn fragment_start_secs(b: &[u8]) -> Option<f64> {
    let n = b.len();
    let (s, e) = mp4_child(b, 0, n, b"moov")?;
    let (s, e) = mp4_child(b, s, e, b"trak")?;
    let (s, e) = mp4_child(b, s, e, b"mdia")?;
    let (h, _) = mp4_child(b, s, e, b"mdhd")?;
    let timescale = match *b.get(h)? {
        1 => u32::from_be_bytes(b.get(h + 20..h + 24)?.try_into().ok()?),
        _ => u32::from_be_bytes(b.get(h + 12..h + 16)?.try_into().ok()?),
    };
    let (s, e) = mp4_child(b, 0, n, b"moof")?;
    let (s, e) = mp4_child(b, s, e, b"traf")?;
    let (d, _) = mp4_child(b, s, e, b"tfdt")?;
    let base = match *b.get(d)? {
        1 => u64::from_be_bytes(b.get(d + 4..d + 12)?.try_into().ok()?),
        _ => u32::from_be_bytes(b.get(d + 4..d + 8)?.try_into().ok()?) as u64,
    };
    (timescale > 0).then(|| base as f64 / timescale as f64)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}

/// 방송의 최신 지점 (시각 초, 조각 번호). 조각 요청 응답 헤더에 들어 있다.
async fn live_head(client: &reqwest::Client, base: &str) -> Result<(f64, u64), String> {
    let r = client
        .head(format!("{base}&sq=0"))
        .header("User-Agent", PREVIEW_UA)
        .send()
        .await
        .map_err(|e| format!("방송 정보를 가져오지 못했습니다: {e}"))?;
    let num = |k: &str| r.headers().get(k).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    match (num("x-head-time-millis"), num("x-head-seqnum")) {
        (Some(ms), Some(seq)) if ms > 0 && seq > 0 => Ok((ms as f64 / 1000.0, seq)),
        _ => Err("방송의 현재 위치를 알 수 없습니다".into()),
    }
}

async fn fetch_fragment(client: &reqwest::Client, base: &str, sq: u64) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..3u64 {
        match client.get(format!("{base}&sq={sq}")).header("User-Agent", PREVIEW_UA).send().await {
            Ok(r) if r.status().is_success() => match r.bytes().await {
                Ok(b) => return Ok(b.to_vec()),
                Err(e) => last = e.to_string(),
            },
            Ok(r) => last = format!("HTTP {}", r.status().as_u16()),
            Err(e) => last = e.to_string(),
        }
        tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt + 1))).await;
    }
    Err(format!("방송 구간을 받지 못했습니다 (조각 {sq}, {last}). 보관 기간이 지난 구간일 수 있습니다"))
}

/// `target`(초)을 포함하는 조각 번호와 그 조각의 시작 시각을 찾는다.
/// 평균 길이로 번호를 짐작한 뒤, 실제 시각을 읽어 가며 보정한다.
async fn locate(
    client: &reqwest::Client,
    audio: &str,
    target: f64,
    head: (f64, u64),
) -> Result<(u64, f64), String> {
    let mut seen: HashMap<u64, f64> = HashMap::new();
    let mut sq = (target * head.1 as f64 / head.0).floor().max(0.0) as u64;
    for _ in 0..16 {
        let t0 = match seen.get(&sq) {
            Some(t) => *t,
            None => {
                let b = fetch_fragment(client, audio, sq).await?;
                let t = fragment_start_secs(&b).ok_or("조각의 시각을 읽지 못했습니다")?;
                seen.insert(sq, t);
                t
            }
        };
        if t0 > target {
            let step = ((t0 - target) / FRAGMENT_SECS).ceil().max(1.0) as u64;
            sq = sq.saturating_sub(step);
            continue;
        }
        // t0 <= target: 다음 조각이 아직 target보다 앞이면 한 칸 더 간다
        let next = sq + 1;
        let nt = match seen.get(&next) {
            Some(t) => Some(*t),
            None if target - t0 >= FRAGMENT_SECS * 0.9 => {
                let b = fetch_fragment(client, audio, next).await?;
                let t = fragment_start_secs(&b);
                if let Some(t) = t {
                    seen.insert(next, t);
                }
                t
            }
            None => None,
        };
        match nt {
            Some(t) if t <= target => sq = next,
            _ => return Ok((sq, t0)),
        }
    }
    Err("구간의 위치를 찾지 못했습니다".into())
}

/// 방송 시작부터의 조각 포맷 정보를 yt-dlp로 받는다.
async fn live_json(app: &AppHandle, url: &str) -> Result<serde_json::Value, String> {
    let mut args = base_args()?;
    args.extend(["-J", "--no-playlist", "--live-from-start", "--no-warnings", "--"].map(String::from));
    args.push(url.to_string());
    let out = ytdlp_cmd(app).await?.args(args).output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("방송 정보를 가져오지 못했습니다");
        return Err(line.trim().to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

fn is_fragment_format(f: &serde_json::Value) -> bool {
    f["protocol"].as_str() == Some("http_dash_segments_generator") && f["url"].as_str().is_some()
}

/// mp4로 이어 붙일 수 있는 조각 포맷만 쓴다. (webm 조각은 단순히 이어 붙일 수 없다)
fn mp4_video_formats(v: &serde_json::Value) -> Vec<&serde_json::Value> {
    v["formats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| {
            is_fragment_format(f)
                && f["ext"].as_str() == Some("mp4")
                && f["vcodec"].as_str().is_some_and(|c| c != "none" && norm_codec(c).is_some())
                && f["acodec"].as_str().is_none_or(|c| c == "none")
        })
        .collect()
}

/// 라이브 영상의 정보. 길이는 방송 시작부터 지금까지(초)이며 플레이어의 재생 시간과 같은 좌표다.
pub async fn live_info(v: &serde_json::Value, embeddable: bool) -> Result<VideoInfo, String> {
    let videos = mp4_video_formats(v);
    let audio = v["formats"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| is_fragment_format(f) && f["vcodec"].as_str() == Some("none"))
        .and_then(|f| f["url"].as_str())
        .ok_or("이 방송은 지난 구간을 받을 수 없습니다")?;
    if videos.is_empty() {
        return Err("이 방송은 지난 구간을 받을 수 없습니다".into());
    }
    let (head_time, _) = live_head(&http_client(), audio).await?;

    let mut formats: Vec<FormatInfo> = Vec::new();
    for f in &videos {
        let codec = f["vcodec"].as_str().and_then(norm_codec).unwrap_or("h264");
        let height = f["height"].as_u64().unwrap_or(0) as u32;
        if !formats.iter().any(|x| x.codec == codec && x.height == height) {
            formats.push(FormatInfo { codec: codec.to_string(), height });
        }
    }
    let aspect = videos
        .iter()
        .filter_map(|f| Some((f["width"].as_f64()?, f["height"].as_f64()?)))
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(16.0 / 9.0, |(w, h)| w / h);
    Ok(VideoInfo {
        id: v["id"].as_str().unwrap_or_default().to_string(),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        duration: head_time,
        channel: v["channel"].as_str().or(v["uploader"].as_str()).unwrap_or_default().to_string(),
        aspect,
        embeddable,
        live: true,
        formats,
    })
}

fn pick_streams(v: &serde_json::Value, cap: u32, want: &str) -> Result<LiveStreams, String> {
    let videos: Vec<_> = mp4_video_formats(v)
        .into_iter()
        .filter(|f| cap == 0 || f["height"].as_u64().unwrap_or(0) as u32 <= cap)
        .collect();
    let rank = |f: &serde_json::Value| (f["height"].as_u64().unwrap_or(0), f["fps"].as_f64().unwrap_or(0.0) as u64);
    let best = |codec: Option<&str>| {
        videos
            .iter()
            .filter(|f| codec.is_none_or(|c| f["vcodec"].as_str().and_then(norm_codec) == Some(c)))
            .max_by_key(|f| rank(f))
            .copied()
    };
    // 원하는 코덱이 있으면 그것을, 없으면 호환성이 좋은 H.264를 우선한다 (이후 필요하면 변환)
    let video = best(Some(want)).or_else(|| best(Some("h264"))).or_else(|| best(None)).ok_or("받을 수 있는 화질이 없습니다")?;
    let audio = v["formats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| is_fragment_format(f) && f["vcodec"].as_str() == Some("none") && f["ext"].as_str() == Some("m4a"))
        .max_by(|a, b| a["abr"].as_f64().unwrap_or(0.0).total_cmp(&b["abr"].as_f64().unwrap_or(0.0)))
        .ok_or("받을 수 있는 오디오가 없습니다")?;
    Ok(LiveStreams {
        title: v["title"].as_str().unwrap_or("live").to_string(),
        video: video["url"].as_str().unwrap_or_default().to_string(),
        audio: audio["url"].as_str().unwrap_or_default().to_string(),
        vcodec: video["vcodec"].as_str().unwrap_or_default().to_string(),
        height: video["height"].as_u64().unwrap_or(0) as u32,
    })
}

fn check_cancel(app: &AppHandle, job: u32) -> Result<(), String> {
    if app.state::<LiveCancel>().0.lock().unwrap().remove(&job) {
        return Err("cancelled".into());
    }
    Ok(())
}

/// 지정 구간(방송 시작 기준 초)의 조각만 받아 이어 붙이고 잘라 저장한다. 저장된 파일 경로를 돌려준다.
pub async fn download_live(
    app: &AppHandle,
    jobs: &State<'_, Jobs>,
    hw_cache: &State<'_, HwCache>,
    req: &DownloadReq,
    vf: Option<&str>,
    native: bool,
) -> Result<String, String> {
    app.state::<LiveCancel>().0.lock().unwrap().remove(&req.job);
    let resolved = req.resolved.as_ref().ok_or("주소를 해석하지 못했습니다")?;
    let want = req.codec.as_str();
    let want_codec = matches!(want, "h264" | "vp9" | "av1") && !req.audio_only;

    let v = live_json(app, &resolved.url).await?;
    let streams = pick_streams(&v, req.max_height, if want_codec { want } else { "h264" })?;
    let client = http_client();
    let head = live_head(&client, &streams.audio).await?;
    if req.end > head.0 - 2.0 * FRAGMENT_SECS {
        return Err("방송 중인 최신 구간은 잠시 뒤에 받을 수 있습니다. 끝 지점을 앞으로 옮겨 주세요".into());
    }
    check_cancel(app, req.job)?;

    let (sq_a, t_a) = locate(&client, &streams.audio, req.start, head).await?;
    check_cancel(app, req.job)?;
    let (sq_b, _) = locate(&client, &streams.audio, req.end, head).await?;
    let total = (sq_b - sq_a + 1) as usize * if req.audio_only { 1 } else { 2 };

    std::fs::create_dir_all(&req.out_dir).map_err(|e| e.to_string())?;
    let out_dir = req.out_dir.trim_end_matches(['/', '\\']).to_string();
    let work = format!("{out_dir}/.cutdlp-tmp-{}", req.job);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = run_live(app, jobs, hw_cache, req, vf, native, &streams, &client, (sq_a, sq_b, total), req.start - t_a, &work, &out_dir).await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_live(
    app: &AppHandle,
    jobs: &State<'_, Jobs>,
    hw_cache: &State<'_, HwCache>,
    req: &DownloadReq,
    vf: Option<&str>,
    native: bool,
    streams: &LiveStreams,
    client: &reqwest::Client,
    (sq_a, sq_b, total): (u64, u64, usize),
    offset: f64,
    work: &str,
    out_dir: &str,
) -> Result<String, String> {
    // 조각 받기: 영상·오디오를 번호 순서대로 이어 붙여 파일 하나씩 만든다
    let mut jobs_list: Vec<(bool, u64)> = Vec::new();
    for sq in sq_a..=sq_b {
        if !req.audio_only {
            jobs_list.push((true, sq));
        }
        jobs_list.push((false, sq));
    }
    let video_path = format!("{work}/video.mp4");
    let audio_path = format!("{work}/audio.m4a");
    let mut vfile = if req.audio_only { None } else { Some(std::fs::File::create(&video_path).map_err(|e| e.to_string())?) };
    let mut afile = std::fs::File::create(&audio_path).map_err(|e| e.to_string())?;
    let mut stream = futures_util::stream::iter(jobs_list.into_iter().map(|(is_video, sq)| {
        let base = if is_video { streams.video.clone() } else { streams.audio.clone() };
        let client = client.clone();
        async move { fetch_fragment(&client, &base, sq).await.map(|b| (is_video, b)) }
    }))
    .buffered(FETCH_PARALLEL);
    let mut done = 0usize;
    while let Some(item) = stream.next().await {
        check_cancel(app, req.job)?;
        let (is_video, bytes) = item?;
        let file = if is_video { vfile.as_mut().ok_or("영상 조각이 예상과 다릅니다")? } else { &mut afile };
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        done += 1;
        let _ = app.emit(
            "dl-progress",
            Progress { job: req.job, percent: done as f64 / total as f64 * 100.0, phase: "download" },
        );
    }
    drop((vfile, afile));
    drop(stream);

    // 자르기: 조각은 5초 안팎이라 앞쪽 여분(offset)을 건너뛰고 필요한 길이만 남긴다
    let dur = req.end - req.start;
    let title = template_literal(&streams.title);
    let tag = format!("{} [{}-{}]", title, time_tag(req.start), time_tag(req.end));
    let ext = if req.audio_only { "mp3" } else { "mp4" };
    let out_path = format!("{work}/{tag}.{ext}");
    let (off, len) = (format!("{offset:.3}"), format!("{dur:.3}"));
    let mut head = vec!["-hide_banner".to_string(), "-y".into()];
    let inputs = |path: &str| ["-ss", &off, "-t", &len, "-i", path].map(String::from);
    if req.audio_only {
        head.extend(inputs(&audio_path));
        head.extend(["-vn", "-c:a", "libmp3lame", "-q:a", "2"].map(String::from));
        head.push(out_path.clone());
        let o = app.shell().sidecar("ffmpeg").map_err(|e| e.to_string())?.args(head).output().await.map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err("오디오 변환에 실패했습니다".into());
        }
        return move_unique(&out_path, out_dir);
    }
    head.extend(inputs(&video_path));
    head.extend(inputs(&audio_path));
    head.extend(["-map", "0:v:0", "-map", "1:a:0"].map(String::from));

    let reencode = req.accurate || vf.is_some() || !native;
    if !reencode {
        // 키프레임 단위 컷: 다시 인코딩하지 않아 빠르다
        head.extend(["-c", "copy", "-movflags", "+faststart"].map(String::from));
        head.push(out_path.clone());
        let o = app.shell().sidecar("ffmpeg").map_err(|e| e.to_string())?.args(head).output().await.map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err("영상 자르기에 실패했습니다".into());
        }
        return move_unique(&out_path, out_dir);
    }

    let want = req.codec.as_str();
    let target = if matches!(want, "h264" | "vp9" | "av1") { want } else { norm_codec(&streams.vcodec).unwrap_or("h264") };
    let h264_enc = h264_encoder(app).await;
    let bitrate = target_bitrate(streams.height);
    let hw = if req.hw_accel && target == "h264" { detect_hw(app, hw_cache).await } else { None };
    let build = |hw: Option<&str>| -> Vec<String> {
        let mut a = head.clone();
        a.extend(cut_encoder_args(Some(target), hw, &h264_enc, bitrate, vf));
        let audio = if target == "h264" { ["-c:a", "aac", "-b:a", "192k"] } else { ["-c:a", "libopus", "-b:a", "160k"] };
        a.extend(audio.map(String::from));
        a.extend(["-movflags", "+faststart"].map(String::from));
        a.push(out_path.clone());
        a
    };
    let run = |hw: Option<&str>| -> Result<Command, String> {
        Ok(app.shell().sidecar("ffmpeg").map_err(|e| e.to_string())?.args(build(hw)))
    };
    let _ = app.emit("dl-progress", Progress { job: req.job, percent: 0.0, phase: "encode" });
    let mut e = run_job(app, jobs, req.job, run(hw.as_deref())?, dur.max(0.1), "encode").await?;
    if hw.is_some() && !e.cancelled && e.code != Some(0) {
        e = run_job(app, jobs, req.job, run(None)?, dur.max(0.1), "encode").await?;
    }
    if e.cancelled {
        return Err("cancelled".into());
    }
    if e.code != Some(0) {
        return Err(if e.last_error.is_empty() { "인코딩에 실패했습니다".into() } else { e.last_error });
    }
    move_unique(&out_path, out_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend(name);
        v.extend(payload);
        v
    }

    #[test]
    fn reads_fragment_start_from_tfdt() {
        let mut mdhd = vec![0u8; 4]; // version 0, flags
        mdhd.extend([0u8; 8]); // creation, modification
        mdhd.extend(90_000u32.to_be_bytes()); // timescale
        mdhd.extend([0u8; 8]);
        let moov = boxed(b"moov", &boxed(b"trak", &boxed(b"mdia", &boxed(b"mdhd", &mdhd))));
        let mut tfdt = vec![0u8; 4];
        tfdt.extend(450_000u32.to_be_bytes()); // 5초
        let moof = boxed(b"moof", &boxed(b"traf", &boxed(b"tfdt", &tfdt)));
        let mut file = boxed(b"ftyp", b"isom");
        file.extend(moov);
        file.extend(boxed(b"emsg", b"x"));
        file.extend(moof);
        assert_eq!(fragment_start_secs(&file), Some(5.0));
        assert_eq!(fragment_start_secs(&file[..20]), None);
    }
}
