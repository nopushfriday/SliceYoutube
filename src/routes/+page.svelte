<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { downloadDir, homeDir } from '@tauri-apps/api/path';
  import { open } from '@tauri-apps/plugin-dialog';
  import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';
  import Player from '$lib/Player.svelte';
  import StreamPlayer from '$lib/StreamPlayer.svelte';
  import Timeline from '$lib/Timeline.svelte';
  import ScrubNumber from '$lib/ScrubNumber.svelte';
  import CropOverlay, { type Crop } from '$lib/CropOverlay.svelte';
  import { clamp, fmt } from '$lib/util';

  type Fmt = { codec: 'h264' | 'vp9' | 'av1'; height: number };
  type Info = { id: string; title: string; duration: number; channel: string; aspect: number; embeddable: boolean; formats: Fmt[] };
  type Codec = 'h264' | 'vp9' | 'av1';
  const CODEC_LABEL: Record<string, string> = { h264: 'H.264', vp9: 'VP9', av1: 'AV1' };
  type Seg = {
    id: number;
    url: string;
    title: string;
    start: number;
    end: number;
    formats: Fmt[];
    status: 'pending' | 'running' | 'done' | 'error' | 'cancelled';
    phase: 'download' | 'encode';
    percent: number;
    file?: string;
    error?: string;
  };

  const SHORTCUTS: [string, string][] = [
    ['Q', '시작 지점'],
    ['W', '종료 지점'],
    ['Enter', '클립 추가'],
    ['Space', '재생 / 일시정지'],
    ['← →', '5초 이동 (Shift: 1초)'],
    [', .', '1프레임 이동'],
    ['\\', '구간 반복'],
    ['?', '단축키 도움말']
  ];

  // 앱 업데이트: 새 버전이 있으면 받아서 교체한 뒤 다시 시작한다
  let upd = $state<Update | null>(null);
  let updBusy = $state(false);
  let updPct = $state(0);

  let updChecking = $state(false);
  let updMsg = $state('');

  // force: 사용자가 직접 확인 (간격과 상관없이 확인하고 결과를 알려준다)
  async function checkUpdate(force = false) {
    const last = loadChecks();
    // 새 버전을 찾았는데 설치하지 않았다면 간격과 상관없이 다시 알려준다
    if (!force && !last.appFound && !isDue(last.app, CHECK_EVERY.app)) return;
    updChecking = true;
    updMsg = '';
    try {
      upd = await check();
      saveChecks({ app: Date.now(), appFound: !!upd });
      if (force && !upd) updMsg = '최신 버전입니다';
    } catch {
      // 자동 확인은 오프라인이거나 개발 실행이면 조용히 넘어간다
      if (force) updMsg = '확인 실패';
    } finally {
      updChecking = false;
      if (updMsg) setTimeout(() => (updMsg = ''), 4000);
    }
  }

  async function installUpdate() {
    if (!upd || updBusy) return;
    updBusy = true;
    updPct = 0;
    let total = 0;
    let got = 0;
    try {
      await upd.downloadAndInstall((ev) => {
        if (ev.event === 'Started') total = ev.data.contentLength ?? 0;
        else if (ev.event === 'Progress') {
          got += ev.data.chunkLength;
          updPct = total ? Math.round((got / total) * 100) : 0;
        }
      });
      await relaunch();
    } catch (e) {
      error = `업데이트에 실패했습니다: ${e}`;
      updBusy = false;
    }
  }

  // 탭: 자르기(구간 지정·화면 자르기·다운로드) / 다운로드(작업 없이 그대로 받기)
  let tab = $state<'slice' | 'download'>('slice');
  type DlItem = {
    id: number;
    url: string;
    status: 'pending' | 'running' | 'done' | 'error' | 'cancelled';
    phase: 'download' | 'encode';
    percent: number;
    file?: string;
    thumb?: string;
    title?: string;
    count?: number;
    error?: string;
  };
  let dlUrl = $state('');
  let dlItems = $state<DlItem[]>([]);
  let dlRunning = $state(false);
  let dlStop = false;
  let dlAudioOnly = $state(false);
  let dlMaxHeight = $state(0);
  let dlMode = $state<'merge' | 'original'>('merge');
  let dlAudio = $state<'aac' | 'mp3'>('aac');
  let dlPlaylist = $state(false);
  let dlKbps = $state(192);
  let dlCodec = $state<'h264' | 'vp9' | 'av1' | 'auto'>('h264');
  let dlAuto = $state(true);
  // 컷 작업 id(1부터)와 겹치지 않게 큰 값에서 시작한다 (취소·진행률이 id로 구분됨)
  let dlNextId = 1_000_000;
  const dlPending = $derived(
    dlItems.filter((s) => ['pending', 'error', 'cancelled'].includes(s.status)).length
  );
  // 유튜브 주소는 바로 썸네일을 알 수 있다 (그 외 사이트는 다운로드가 시작되면 yt-dlp가 알려준다)
  function ytThumb(u: string) {
    const m = u.match(/(?:youtu\.be\/|[?&]v=|\/shorts\/|\/embed\/|\/live\/)([\w-]{11})/);
    return m ? `https://i.ytimg.com/vi/${m[1]}/mqdefault.jpg` : undefined;
  }
  async function openFile(path?: string) {
    if (!path) return;
    try {
      await openPath(path);
    } catch (e) {
      error = `파일을 열 수 없습니다: ${e}`;
    }
  }
  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

  function addDownload() {
    const u = dlUrl.trim();
    if (!u) return;
    dlItems.push({ id: dlNextId++, url: u, thumb: ytThumb(u), status: 'pending', phase: 'download', percent: 0 });
    dlUrl = '';
    if (dlAuto) runDownloads();
  }

  async function runDownloads() {
    if (dlRunning || ytBusy) return;
    dlRunning = true;
    dlStop = false;
    // 실행 중에 추가된 항목도 이어서 처리한다
    for (const s of dlItems) {
      if (dlStop) break;
      if (!['pending', 'error', 'cancelled'].includes(s.status)) continue;
      s.status = 'running';
      s.phase = 'download';
      s.percent = 0;
      s.error = undefined;
      try {
        const files = await invoke<string[]>('download_plain', {
          req: {
            job: s.id,
            url: s.url,
            audioOnly: dlAudioOnly,
            maxHeight: dlMaxHeight,
            mode: dlMode,
            audio: dlAudio,
            audioKbps: dlKbps,
            playlist: dlPlaylist,
            codec: dlCodec,
            hwAccel,
            outDir
          }
        });
        s.file = files[0];
        s.count = files.length;
        s.status = 'done';
        s.percent = 100;
      } catch (e) {
        if (String(e) === 'cancelled') s.status = 'cancelled';
        else {
          s.status = 'error';
          s.error = String(e);
        }
      }
    }
    dlRunning = false;
  }

  function cancelDownloads() {
    dlStop = true;
    for (const s of dlItems) if (s.status === 'running') invoke('cancel', { job: s.id });
  }

  function removeDownload(id: number) {
    if (dlItems.find((x) => x.id === id)?.status === 'running') invoke('cancel', { job: id });
    dlItems = dlItems.filter((x) => x.id !== id);
  }

  let url = $state('');
  let loading = $state(false);
  let error = $state('');
  let info = $state<Info | null>(null);
  let sourceUrl = $state('');

  // svelte-ignore non_reactive_update
  let player: Player | StreamPlayer;
  let now = $state(0);
  let playing = $state(false);
  let start = $state<number | null>(null);
  let end = $state<number | null>(null);
  let loop = $state(false);
  let previewOnce = false;

  let segs = $state<Seg[]>([]);
  let nextId = 1;
  let running = $state(false);
  let stopQueue = false;

  let audioOnly = $state(false);
  let maxHeight = $state(0); // 0 = 최고 화질 자동
  let codec = $state<Codec>('h264');
  let hwAccel = $state(true);
  const NO_CROP: Crop = { left: 0, right: 0, top: 0, bottom: 0 };
  let crop = $state<Crop>({ ...NO_CROP });
  // 임베드가 막힌 영상은 스트림 재생으로 대체
  let embedBlocked = $state(false);
  const cropped = $derived(crop.left + crop.right + crop.top + crop.bottom > 0);
  const CROP_SIDES: [keyof Crop, string][] = [
    ['left', '왼쪽'],
    ['top', '위쪽'],
    ['right', '오른쪽'],
    ['bottom', '아래쪽']
  ];

  // 숫자 입력값을 0~(남는 영역 5% 이상) 범위로 보정한다
  function setCrop(side: keyof Crop, raw: number) {
    const pair = { left: 'right', right: 'left', top: 'bottom', bottom: 'top' } as const;
    const v = Number.isFinite(raw) ? raw : 0;
    crop[side] = Math.round(clamp(v, 0, 95 - crop[pair[side]]) * 10) / 10;
  }
  // 감지된 하드웨어 H.264 인코더: undefined = 확인 중, null = 없음(CPU 인코딩)
  let hwEnc = $state<string | null | undefined>(undefined);
  let outDir = $state('');
  // 사용자 폴더 안의 경로는 사용자 이름이 드러나지 않게 홈 폴더 기준으로 줄여 표시한다 (다른 드라이브는 전체 경로)
  let home = $state('');
  function shortPath(p: string): string {
    const prefix = home.replace(/[\\/]+$/, '');
    if (!prefix || p.length <= prefix.length) return p;
    const head = p.slice(0, prefix.length);
    const sep = p[prefix.length];
    return head.toLowerCase() === prefix.toLowerCase() && (sep === '\\' || sep === '/') ? p.slice(prefix.length + 1) : p;
  }
  let showSettings = $state(false);
  let showHelp = $state(false);

  // yt-dlp 자동 업데이트 상태
  type Yt = { version: string | null; updated: boolean; error: string | null };
  let ytBusy = $state(true);
  let yt = $state<Yt>({ version: null, updated: false, error: null });

  // 업데이트 확인은 외부(GitHub) 요청이라 일정 간격으로만 한다. 사용자가 직접 누르면 항상 확인한다.
  const CHECK_EVERY = { app: 6, ytdlp: 12 }; // 시간
  type Checks = { app?: number; appFound?: boolean; ytdlp?: number };
  const loadChecks = (): Checks => JSON.parse(localStorage.getItem('cut-dlp-checks') ?? '{}');
  function saveChecks(patch: Checks) {
    localStorage.setItem('cut-dlp-checks', JSON.stringify({ ...loadChecks(), ...patch }));
  }
  const isDue = (last: number | undefined, hours: number) => !last || Date.now() - last >= hours * 3_600_000;

  async function updateYtdlp(force = true) {
    const check = force || isDue(loadChecks().ytdlp, CHECK_EVERY.ytdlp);
    ytBusy = true;
    try {
      yt = await invoke<Yt>('ytdlp_update', { check });
      if (check && !yt.error) saveChecks({ ytdlp: Date.now() });
    } catch (e) {
      yt = { version: yt.version, updated: false, error: String(e) };
    } finally {
      ytBusy = false;
    }
  }

  const valid = $derived(start != null && end != null && end > start);
  const offered = $derived(
    info ? [...new Set(info.formats.map((f) => CODEC_LABEL[f.codec]))] : []
  );
  // 선택한 코덱을 영상이 (화질 상한 내에서) 제공하지 않으면 받은 뒤 인코딩한다
  const willEncode = $derived(
    !!info &&
      !audioOnly &&
      !info.formats.some((f) => f.codec === codec && (maxHeight === 0 || f.height <= maxHeight))
  );
  const pendingCount = $derived(
    segs.filter((s) => ['pending', 'error', 'cancelled'].includes(s.status)).length
  );
  const summary = $derived(
    audioOnly
      ? 'MP3 오디오'
      : [
          maxHeight === 0 ? '최고 화질' : `${maxHeight}p 이하`,
          CODEC_LABEL[codec],
          '재인코딩',
          ...(cropped ? ['자르기'] : [])
        ].join(' · ')
  );

  onMount(() => {
    const saved = JSON.parse(localStorage.getItem('cut-dlp') ?? '{}');
    audioOnly = saved.audioOnly ?? false;
    maxHeight = saved.maxHeight ?? 0;
    codec = saved.codec && saved.codec !== 'auto' ? saved.codec : 'h264';
    hwAccel = saved.hwAccel ?? true;
    dlAudioOnly = saved.dlAudioOnly ?? false;
    dlMaxHeight = saved.dlMaxHeight ?? 0;
    dlMode = saved.dlMode ?? 'merge';
    dlAudio = saved.dlAudio ?? 'aac';
    dlCodec = saved.dlCodec ?? 'h264';
    dlKbps = saved.dlKbps ?? 192;
    dlPlaylist = saved.dlPlaylist ?? false;
    dlAuto = saved.dlAuto ?? true;
    checkUpdate();
    updateYtdlp(false); // yt-dlp 준비와 현재 버전 표시 (업데이트 확인은 간격이 지났을 때만)
    invoke<string | null>('hw_encoder')
      .then((e) => (hwEnc = e))
      .catch(() => (hwEnc = null));
    homeDir()
      .then((h) => (home = h))
      .catch(() => {});
    (async () => {
      outDir = saved.outDir ?? (await downloadDir());
    })();
    const unInfo = listen<{ job: number; thumbnail: string | null; title: string | null }>('dl-info', (e) => {
      const d = dlItems.find((x) => x.id === e.payload.job);
      if (!d) return;
      d.thumb ??= e.payload.thumbnail ?? undefined;
      if (e.payload.title) d.title = e.payload.title;
    });
    const un = listen<{ job: number; percent: number; phase: 'download' | 'encode' }>(
      'dl-progress',
      (e) => {
        const s = segs.find((x) => x.id === e.payload.job);
        if (s) {
          s.percent = e.payload.percent;
          s.phase = e.payload.phase;
        }
        const d = dlItems.find((x) => x.id === e.payload.job);
        if (d) {
          d.percent = e.payload.percent;
          d.phase = e.payload.phase;
        }
      }
    );
    return () => {
      un.then((f) => f());
      unInfo.then((f) => f());
    };
  });

  $effect(() => {
    localStorage.setItem('cut-dlp', JSON.stringify({ audioOnly, maxHeight, codec, hwAccel, outDir, dlAudioOnly, dlMaxHeight, dlMode, dlAudio, dlCodec, dlKbps, dlPlaylist, dlAuto }));
  });

  async function load() {
    const u = url.trim();
    if (!u || loading) return;
    loading = true;
    error = '';
    try {
      info = await invoke<Info>('fetch_info', { url: u });
      // 다른 영상이면 이전 영상의 받아 둔 구간을 비운다
      if (u !== sourceUrl) invoke('clear_section_cache').catch(() => {});
      sourceUrl = u;
      start = end = null;
      now = 0;
      crop = { ...NO_CROP };
      embedBlocked = !info.embeddable;
      playing = false;
    } catch (e) {
      error = String(e);
      info = null;
    } finally {
      loading = false;
    }
  }

  async function pasteAndLoad() {
    try {
      url = (await navigator.clipboard.readText()).trim();
    } catch {
      error = '클립보드를 읽을 수 없습니다. 직접 붙여넣어 주세요.';
      return;
    }
    if (url) load();
  }

  async function openOutDir() {
    try {
      await openPath(outDir);
    } catch (e) {
      error = `폴더를 열 수 없습니다 (다운로드한 파일이 없으면 폴더가 없음): ${e}`;
    }
  }

  async function pickDir() {
    const d = await open({ directory: true, defaultPath: outDir });
    if (typeof d === 'string') outDir = d;
  }

  function onTime(t: number) {
    now = t;
    if (end != null && start != null && playing) {
      if (loop && t >= end) player.seek(start);
      else if (previewOnce && t >= end) {
        previewOnce = false;
        player.pause();
      }
    }
  }

  function setStart(t: number) {
    start = Math.round(t * 100) / 100;
    if (end != null && end <= start) end = null;
  }
  function setEnd(t: number) {
    end = Math.round(t * 100) / 100;
    if (start != null && start >= end) start = null;
  }

  // 핸들을 끌면 영상 위치도 따라가게 한다 (재생 중이면 일시정지, seek는 쓰로틀 + 마지막 위치 보정)
  let lastSeek = 0;
  let seekTimer: ReturnType<typeof setTimeout> | undefined;
  function scrub(t: number) {
    if (playing) player.pause();
    previewOnce = false;
    const n = performance.now();
    clearTimeout(seekTimer);
    if (n - lastSeek > 60) {
      lastSeek = n;
      player.seek(t);
    } else {
      seekTimer = setTimeout(() => player.seek(t), 80);
    }
  }

  function addSegment() {
    if (!info || start == null || end == null || end <= start) return;
    segs.push({
      id: nextId++,
      url: sourceUrl,
      title: info.title,
      start,
      end,
      formats: info.formats,
      status: 'pending',
      phase: 'download',
      percent: 0
    });
    start = end = null;
  }

  function preview() {
    if (start == null || end == null) return;
    previewOnce = true;
    player.seek(start);
    player.play();
  }

  const waiting = (s: Seg) => s.status === 'pending' || s.status === 'error' || s.status === 'cancelled';

  // 같은 영상에서 서로 겹치는 대기 클립들의 합친 범위. 첫 클립이 이 범위를 한 번에 받고 나머지는 그 안에서 자른다.
  function overlapRange(s: Seg): { fetchStart: number; fetchEnd: number } {
    const same = segs.filter((x) => x.url === s.url && waiting(x)).sort((a, b) => a.start - b.start);
    let lo = s.start;
    let hi = s.end;
    for (let grew = true; grew; ) {
      grew = false;
      for (const x of same) {
        if (x.start <= hi && x.end >= lo && (x.start < lo || x.end > hi)) {
          lo = Math.min(lo, x.start);
          hi = Math.max(hi, x.end);
          grew = true;
        }
      }
    }
    return { fetchStart: lo, fetchEnd: hi };
  }

  async function runQueue() {
    if (running) return;
    running = true;
    stopQueue = false;
    for (const s of segs) {
      if (stopQueue) break;
      if (!waiting(s)) continue;
      s.status = 'running';
      s.phase = 'download';
      s.percent = 0;
      s.error = undefined;
      try {
        s.file = await invoke<string>('download', {
          req: {
            job: s.id,
            url: s.url,
            start: s.start,
            end: s.end,
            accurate: true,
            audioOnly,
            maxHeight,
            codec,
            hwAccel,
            crop: $state.snapshot(crop),
            formats: s.formats,
            outDir,
            ...overlapRange(s)
          }
        });
        s.status = 'done';
        s.percent = 100;
      } catch (e) {
        if (String(e) === 'cancelled') s.status = 'cancelled';
        else {
          s.status = 'error';
          s.error = String(e);
        }
      }
    }
    running = false;
    // 대기 중인 클립이 더 없으면 받아 둔 원본 구간은 바로 지워 저장 공간을 확보한다
    if (!stopQueue && !segs.some((x) => x.status === 'pending')) invoke('clear_section_cache').catch(() => {});
  }

  function cancelAll() {
    stopQueue = true;
    for (const s of segs) if (s.status === 'running') invoke('cancel', { job: s.id });
  }

  function remove(id: number) {
    const s = segs.find((x) => x.id === id);
    if (s?.status === 'running') invoke('cancel', { job: id });
    segs = segs.filter((x) => x.id !== id);
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (e.key === 'Escape') {
      showHelp = false;
      return;
    }
    if (t.closest('input, select, textarea')) return;
    if (tab !== 'slice') return;
    if (e.key === '?') {
      showHelp = !showHelp;
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || !info) return;
    const step = e.shiftKey ? 1 : 5;
    switch (e.code) {
      case 'KeyQ':
        setStart(player.time());
        break;
      case 'KeyW':
        setEnd(player.time());
        break;
      case 'Space':
        player.toggle();
        break;
      case 'ArrowLeft':
        player.seek(clamp(player.time() - step, 0, info.duration));
        break;
      case 'ArrowRight':
        player.seek(clamp(player.time() + step, 0, info.duration));
        break;
      case 'Comma':
        player.seek(clamp(player.time() - 1 / 30, 0, info.duration));
        break;
      case 'Period':
        player.seek(clamp(player.time() + 1 / 30, 0, info.duration));
        break;
      case 'Backslash':
        loop = !loop;
        break;
      case 'Enter':
      case 'NumpadEnter':
        if (t.closest('button')) return;
        addSegment();
        break;
      default:
        return;
    }
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} />

<main>
  <nav class="tabs">
    <button class:on={tab === 'slice'} onclick={() => (tab = 'slice')}><i class="ti ti-scissors"></i> 자르기</button>
    <button class:on={tab === 'download'} onclick={() => (tab = 'download')}><i class="ti ti-download"></i> 다운로드</button>
    <span class="grow"></span>
    {#if upd}
      <button class="upd" onclick={installUpdate} disabled={updBusy || running || dlRunning} title="작업 종료 후 업데이트 가능">
        <i class="ti {updBusy ? 'ti-loader-2 spin' : 'ti-arrow-up-circle'}"></i>
        {updBusy ? `업데이트 중 ${updPct}%` : `v${upd.version} 업데이트`}
      </button>
    {:else}
      <button class="check" onclick={() => checkUpdate(true)} disabled={updChecking} title="앱 업데이트 확인">
        <i class="ti {updChecking ? 'ti-loader-2 spin' : 'ti-refresh'}"></i>
        {updChecking ? '확인 중…' : updMsg || '업데이트 확인'}
      </button>
    {/if}
  </nav>

  <!-- 탭을 바꿔도 플레이어와 지정한 구간이 유지되도록 숨기기만 한다 -->
  <div class="pane" class:off={tab !== 'slice'}>
  <header class="bar">
    <i class="ti ti-link icon"></i>
    <input
      class="url"
      placeholder="YouTube 주소를 붙여넣고 Enter"
      bind:value={url}
      onkeydown={(e) => e.key === 'Enter' && load()}
    />
    {#if info}<span class="meta">{info.channel} · {fmt(info.duration, 0)}</span>{/if}
    <button onclick={load} disabled={loading || ytBusy || !url.trim()}>{loading ? '불러오는 중…' : '불러오기'}</button>
    <button
      class="yt"
      class:bad={!!yt.error}
      onclick={() => updateYtdlp(true)}
      disabled={ytBusy || running}
      title={yt.error ? `${yt.error}\n눌러서 다시 확인` : '눌러서 yt-dlp 업데이트 확인'}
    >
      <i class="ti {ytBusy ? 'ti-loader-2 spin' : yt.error ? 'ti-alert-triangle' : 'ti-circle-check'}"></i>
      {#if ytBusy}yt-dlp 확인 중…{:else if yt.error}yt-dlp {yt.version ?? ''} · 확인 실패{:else}yt-dlp {yt.version}{yt.updated ? ' (업데이트됨)' : ''}{/if}
    </button>
    <button class="icon-btn" onclick={() => (showHelp = true)} title="단축키 (?)" aria-label="단축키">
      <i class="ti ti-keyboard"></i>
    </button>
  </header>
  {#if error}<p class="err">{error}</p>{/if}

  <div class="cols">
    <section class="left">
      {#if loading}
        <div class="empty-main" role="status">
          <div class="loader"><i class="ti ti-scissors"></i></div>
          <h2>영상 불러오는 중<span class="dots"></span></h2>
        </div>
      {:else if info}
        <h2 title={info.title}>{info.title}</h2>
        <div class="stagebox">
          {#if embedBlocked}
            <StreamPlayer bind:this={player} url={sourceUrl} ontime={onTime} onstate={(p) => (playing = p)} />
          {:else}
            <Player
              bind:this={player}
              videoId={info.id}
              ontime={onTime}
              onstate={(p) => (playing = p)}
              onerror={() => (embedBlocked = true)}
            />
          {/if}
          <CropOverlay bind:crop aspect={info.aspect} ontap={() => player.toggle()} />
        </div>
        <div class="time">
          <button class="icon-btn" onclick={() => player.toggle()} aria-label="재생/일시정지">
            <i class="ti {playing ? 'ti-player-pause' : 'ti-player-play'}"></i>
          </button>
          <b>{fmt(now, 2)}</b> / {fmt(info.duration, 0)}
          <span class="grow"></span>
          <label class="loop"><input type="checkbox" bind:checked={loop} /> 구간 반복 <kbd>\</kbd></label>
        </div>

        <Timeline
          duration={info.duration}
          {now}
          {playing}
          {start}
          {end}
          clips={segs}
          onseek={(t) => player.seek(t)}
          onstart={(t) => {
            setStart(t);
            scrub(t);
          }}
          onend={(t) => {
            setEnd(t);
            scrub(t);
          }}
        />

        <div class="range">
          <button class="s" onclick={() => setStart(player.time())}>
            <kbd>Q</kbd> 시작 <b>{fmt(start, 2)}</b>
          </button>
          <i class="ti ti-arrow-right muted"></i>
          <button class="e" onclick={() => setEnd(player.time())}>
            <kbd>W</kbd> 끝 <b>{fmt(end, 2)}</b>
          </button>
          <span class="dur">{valid ? `${fmt(end! - start!, 1)}초` : ''}</span>
          <span class="grow"></span>
          <button onclick={preview} disabled={!valid}>
            <i class="ti ti-player-play"></i> 미리보기
          </button>
          <button class="add" onclick={addSegment} disabled={!valid}>
            클립 추가 <kbd>Enter</kbd>
          </button>
        </div>
      {:else}
        <div class="empty-main">
          <i class="ti ti-scissors big"></i>
          <h2>필요한 구간만 다운로드</h2>
          <p class="keys">
            <span><kbd>Q</kbd> 시작 지점</span>
            <span><kbd>W</kbd> 종료 지점</span>
          </p>
          <button class="primary" onclick={pasteAndLoad}>
            <i class="ti ti-clipboard"></i> 클립보드의 주소 불러오기
          </button>
        </div>
      {/if}
    </section>

    <aside>
      <div class="ahead">
        <h3>클립 ({segs.length})</h3>
      </div>

      <button class="summary" onclick={() => (showSettings = !showSettings)} aria-expanded={showSettings}>
        <i class="ti ti-adjustments"></i>
        <span class="grow">{summary}</span>
        <i class="ti {showSettings ? 'ti-chevron-up' : 'ti-chevron-down'}"></i>
      </button>

      {#if showSettings}
        <div class="opts">
          <label class="check"><input type="checkbox" bind:checked={audioOnly} /> 오디오만 (mp3)</label>
          {#if !audioOnly}
            <label>코덱
              <select bind:value={codec}>
                <option value="h264">H.264 (호환성 최고)</option>
                <option value="vp9">VP9</option>
                <option value="av1">AV1</option>
              </select>
            </label>
            {#if info}
              <p class="note">
                이 영상 제공 코덱: {offered.join(', ') || '확인 불가'}
                {#if willEncode}
                  <br /><b>{CODEC_LABEL[codec]} 미제공 → 받은 뒤 인코딩합니다 (시간이 더 걸립니다)</b>
                {/if}
              </p>
            {/if}
            <label>최대 화질
              <select bind:value={maxHeight}>
                <option value={0}>최고 화질 (자동)</option>
                {#each [360, 480, 720, 1080, 1440, 2160] as h}<option value={h}>{h}p 이하</option>{/each}
              </select>
            </label>
          {/if}
          {#if !audioOnly}
            <div class="crop">
              <div class="crop-head">
                <span>화면 자르기</span>
                <button class="mini" onclick={() => (crop = { ...NO_CROP })} disabled={!cropped}>초기화</button>
              </div>
              {#each CROP_SIDES as [side, label]}
                <div class="crop-row">
                  <span>{label}</span>
                  <span><ScrubNumber value={crop[side]} onchange={(v) => setCrop(side, v)} /></span>
                </div>
              {/each}
              <p class="note">영상 위 드래그 또는 숫자로 조절</p>
            </div>
            {#if codec === 'h264'}
              <label class="check" title="GPU로 인코딩 (실패 시 CPU)">
                <input type="checkbox" bind:checked={hwAccel} disabled={hwEnc === null} /> 하드웨어 인코딩 (H.264)
              </label>
            {/if}
          {/if}
        </div>
      {/if}

      <ul>
        {#each segs as s (s.id)}
          <li class={s.status}>
            <div class="row">
              <span class="t">{fmt(s.start, 1)} – {fmt(s.end, 1)}</span>
              <span class="len">{fmt(s.end - s.start, 1)}초</span>
              <span class="grow"></span>
              <button class="mini" onclick={() => navigator.clipboard.writeText(s.url)} title="주소 복사" aria-label="주소 복사">
                  <i class="ti ti-copy"></i>
                </button>
                {#if s.status === 'done' && s.file}
                <button class="mini" onclick={() => revealItemInDir(s.file!)} title="폴더에서 보기" aria-label="폴더에서 보기">
                    <i class="ti ti-folder"></i>
                  </button>
              {/if}
              <button class="mini" onclick={() => remove(s.id)} title="삭제" aria-label="삭제">
                <i class="ti ti-x"></i>
              </button>
            </div>
            <div class="name" title={s.title}>{s.title}</div>
            {#if s.status === 'running'}
              <div class="st run">{s.phase === 'encode' ? '인코딩 중' : '다운로드 중'} {Math.round(s.percent)}%</div>
              <progress max="100" value={s.percent}></progress>
            {:else if s.status === 'error'}
              <div class="err">{s.error}</div>
            {:else}
              <div class="st {s.status}">{{ pending: '대기', done: '완료', cancelled: '취소됨' }[s.status]}</div>
            {/if}
          </li>
        {:else}
          <li class="empty">
            <i class="ti ti-playlist-add big"></i>
            <div>구간 지정 후 <kbd>Enter</kbd>로 추가</div>
          </li>
        {/each}
      </ul>

      {#if running}
        <button class="danger" onclick={cancelAll}>중지</button>
      {:else}
        <button class="primary" onclick={runQueue} disabled={pendingCount === 0 || ytBusy}>
          {pendingCount > 0 ? `클립 ${pendingCount}개 다운로드` : '다운로드'}
        </button>
      {/if}
      <div class="folder">
        <button class="path" onclick={pickDir} title="저장 폴더 변경: {shortPath(outDir)}">
          <i class="ti ti-folder"></i><span>{shortPath(outDir)}</span>
        </button>
        <button class="mini" onclick={openOutDir} title="저장 폴더 열기" aria-label="저장 폴더 열기">
          <i class="ti ti-folder-open"></i>
        </button>
      </div>
    </aside>
  </div>
  </div>

  <div class="pane" class:off={tab !== 'download'}>
    <header class="bar">
      <i class="ti ti-link icon"></i>
      <input
        class="url"
        placeholder="주소를 붙여넣고 Enter"
        bind:value={dlUrl}
        onkeydown={(e) => e.key === 'Enter' && addDownload()}
      />
      <button onclick={addDownload} disabled={!dlUrl.trim()}>큐에 추가</button>
    </header>

    <div class="cols">
      <section class="left">
        <div class="opts dlopts">
          <h3>다운로드 옵션</h3>
          <label class="check"><input type="checkbox" bind:checked={dlAuto} /> 큐에 추가하면 바로 다운로드</label>
          <label title="재생목록 주소의 다운로드 범위">재생목록
            <select bind:value={dlPlaylist}>
              <option value={false}>첫 번째 영상만</option>
              <option value={true}>전체 다운로드</option>
            </select>
          </label>
          <label class="check"><input type="checkbox" bind:checked={dlAudioOnly} /> 오디오만</label>
          <label title="AAC/MP3가 아닌 오디오는 선택한 형식으로 변환">오디오 형식
            <select bind:value={dlAudio}>
              <option value="aac">AAC</option>
              <option value="mp3">MP3</option>
            </select>
          </label>
          <label title="오디오 인코딩 비트레이트 (재인코딩하지 않으면 적용 안 됨)">오디오 비트레이트
            <select bind:value={dlKbps}>
              {#each [128, 192, 256, 320] as k}<option value={k}>{k} kbps</option>{/each}
            </select>
          </label>
          {#if !dlAudioOnly}
            <label title="합치기: 영상·오디오를 따로 받아 mp4로 병합 · 원본: 단일 파일 그대로">다운로드 방식
              <select bind:value={dlMode}>
                <option value="merge">영상+오디오 합치기 (최고 화질)</option>
                <option value="original">원본 (합치지 않은 단일 파일)</option>
              </select>
            </label>
            <label title="해당 코덱이 없으면 받은 뒤 인코딩 · 원본 유지: VP9·AV1·H.264 외에만 H.264로 변환">영상 코덱
              <select bind:value={dlCodec}>
                <option value="h264">H.264 (호환성 최고)</option>
                <option value="vp9">VP9</option>
                <option value="av1">AV1</option>
                <option value="auto">원본 유지 (VP9·AV1·H.264 외에만 H.264)</option>
              </select>
            </label>
            <label>최대 화질
              <select bind:value={dlMaxHeight}>
                <option value={0}>최고 화질 (자동)</option>
                {#each [360, 480, 720, 1080, 1440, 2160] as h}<option value={h}>{h}p 이하</option>{/each}
              </select>
            </label>
          {/if}
        </div>
      </section>

      <aside>
        <div class="ahead"><h3>목록 ({dlItems.length})</h3></div>
        <ul>
          {#each dlItems as s (s.id)}
            <li class={s.status}>
              <div class="it">
                <button class="thumb" disabled={s.status !== 'done' || !s.file} onclick={() => openFile(s.file)} title="파일 열기" aria-label="파일 열기">
                  {#if s.thumb}<img src={s.thumb} alt="" />{:else}<i class="ti ti-video"></i>{/if}
                </button>
                <div class="body">
                  <div class="row">
                    <span class="grow"></span>
                    <button class="mini" onclick={() => navigator.clipboard.writeText(s.url)} title="주소 복사" aria-label="주소 복사">
                      <i class="ti ti-copy"></i>
                    </button>
                    {#if s.status === 'done' && s.file}
                      <button class="mini" onclick={() => revealItemInDir(s.file!)} title="폴더에서 보기" aria-label="폴더에서 보기">
                        <i class="ti ti-folder"></i>
                      </button>
                    {/if}
                    <button class="mini" onclick={() => removeDownload(s.id)} title="삭제" aria-label="삭제">
                      <i class="ti ti-x"></i>
                    </button>
                  </div>
                  <button class="name namebtn" disabled={s.status !== 'done' || !s.file} onclick={() => openFile(s.file)} title={s.file ? shortPath(s.file) : (s.title ?? s.url)}>
                    {s.file ? baseName(s.file) : (s.title ?? s.url)}
                  </button>
                  {#if s.status === 'running'}
                    <div class="st run">{s.phase === 'encode' ? '변환 중' : '다운로드 중'} {Math.round(s.percent)}%</div>
                    <progress max="100" value={s.percent}></progress>
                  {:else if s.status === 'error'}
                    <div class="err">{s.error}</div>
                  {:else}
                    <div class="st {s.status}">{{ pending: '대기', done: '완료', cancelled: '취소됨' }[s.status]}{s.status === 'done' && (s.count ?? 0) > 1 ? ` · ${s.count}개` : ''}</div>
                  {/if}
                </div>
              </div>
            </li>
          {:else}
            <li class="empty">
              <i class="ti ti-playlist-add big"></i>
              <div>주소를 추가하세요</div>
            </li>
          {/each}
        </ul>

        {#if dlRunning}
          <button class="danger" onclick={cancelDownloads}>중지</button>
        {:else}
          <button class="primary" onclick={runDownloads} disabled={dlPending === 0 || ytBusy}>
            {dlPending > 0 ? `${dlPending}개 다운로드` : '다운로드'}
          </button>
        {/if}
        <div class="folder">
          <button class="path" onclick={pickDir} title="저장 폴더 변경: {shortPath(outDir)}">
            <i class="ti ti-folder"></i><span>{shortPath(outDir)}</span>
          </button>
          <button class="mini" onclick={openOutDir} title="저장 폴더 열기" aria-label="저장 폴더 열기">
            <i class="ti ti-folder-open"></i>
          </button>
        </div>
      </aside>
    </div>
  </div>

  {#if showHelp}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (showHelp = false)}>
      <div class="help" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-label="단축키">
        <h3>단축키</h3>
        <table>
          <tbody>
            {#each SHORTCUTS as [k, d]}
              <tr><td><kbd>{k}</kbd></td><td>{d}</td></tr>
            {/each}
          </tbody>
        </table>
        <p class="note">타임라인 — 휠: 확대/축소 · Shift+드래그/휠: 좌우 이동</p>
        <button onclick={() => (showHelp = false)}>닫기 <kbd>Esc</kbd></button>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(:root) {
    --bg: #f4f6fa;
    --card: #ffffff;
    --line: #e2e6ee;
    --text: #1c2333;
    --muted: #6b7690;
    --accent: #2563eb;
    --end: #ea580c;
    --ok: #16a34a;
    --bad: #dc2626;
  }
  :global(body) {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font: 14px/1.45 'Segoe UI', 'Malgun Gothic', system-ui, sans-serif;
  }
  :global(button),
  :global(input),
  :global(select) {
    font: inherit;
    color: inherit;
  }
  main {
    padding: 14px 18px;
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .tabs {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .tabs button {
    padding: 4px 12px;
    font-size: 13px;
    color: var(--muted);
  }
  .tabs button.on {
    background: var(--text);
    border-color: var(--text);
    color: #fff;
  }
  .tabs button.upd {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .pane {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    min-height: 0;
  }
  .pane.off {
    display: none;
  }
  .it {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .thumb {
    width: 72px;
    height: 41px;
    flex: none;
    padding: 0;
    border-radius: 6px;
    overflow: hidden;
    background: #e9edf5;
    color: var(--muted);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .namebtn {
    display: block;
    max-width: 100%;
    border: 0;
    background: none;
    padding: 0;
    justify-content: flex-start;
    text-align: left;
  }
  .namebtn:disabled {
    opacity: 1;
    cursor: default;
  }
  .namebtn:hover:not(:disabled) {
    background: none;
    text-decoration: underline;
    color: var(--text);
  }
  .dlopts {
    border: 0;
    max-height: none;
    overflow: visible;
    max-width: 360px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 8px 10px;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  .bar .icon {
    color: var(--muted);
    font-size: 17px;
  }
  .bar .url {
    flex: 1;
    border: 0;
    box-shadow: none;
    padding-left: 2px;
  }
  button.yt {
    font-size: 12px;
    padding: 4px 9px;
    color: var(--muted);
    white-space: nowrap;
  }
  button.yt.bad {
    color: var(--bad);
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .meta {
    color: var(--muted);
    white-space: nowrap;
    font-size: 13px;
  }
  input:not([type='checkbox']),
  select {
    background: #fff;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 8px 11px;
    outline: none;
  }
  input:not([type='checkbox']):focus,
  select:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.15);
  }
  .bar .url:focus {
    box-shadow: none;
  }
  input[type='checkbox'] {
    accent-color: var(--accent);
  }
  button {
    background: #fff;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 7px 14px;
    cursor: pointer;
    transition: background 0.12s;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }
  button:hover:not(:disabled) {
    background: #eef1f7;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  button.primary:hover:not(:disabled) {
    background: #1d4fd0;
  }
  button.danger {
    background: var(--bad);
    border-color: var(--bad);
    color: #fff;
    font-weight: 600;
  }
  .icon-btn {
    padding: 6px 9px;
    font-size: 16px;
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 330px;
    gap: 14px;
    flex: 1;
    min-height: 0;
  }
  .left {
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-right: 2px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .time {
    display: flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .time b {
    color: var(--text);
    font-size: 15px;
  }
  .loop {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 13px;
  }
  .grow {
    flex: 1;
  }
  .range {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 9px 10px;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  .range .s {
    border-color: var(--accent);
    background: rgba(37, 99, 235, 0.07);
  }
  .range .e {
    border-color: var(--end);
    background: rgba(234, 88, 12, 0.07);
  }
  .range .add {
    border-color: var(--text);
    font-weight: 600;
  }
  .range b {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .muted,
  .dur {
    color: var(--muted);
  }
  kbd {
    background: #fff;
    border: 1px solid #cbd2e0;
    border-bottom-width: 2px;
    border-radius: 5px;
    padding: 0 6px;
    font: 12px 'Cascadia Mono', monospace;
    color: var(--text);
  }
  .empty-main {
    margin: auto;
    max-width: 380px;
    text-align: center;
    color: var(--muted);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .empty-main h2 {
    color: var(--text);
    font-size: 19px;
  }
  .empty-main p {
    margin: 0 0 6px;
  }
  .empty-main .keys {
    display: flex;
    gap: 16px;
  }
  .loader {
    position: relative;
    width: 72px;
    height: 72px;
    display: grid;
    place-items: center;
    font-size: 26px;
    color: var(--accent);
    margin-bottom: 6px;
  }
  .loader::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 3px solid var(--line);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  .loader i {
    animation: snip 1.2s ease-in-out infinite;
  }
  @keyframes snip {
    0%,
    100% {
      transform: rotate(-18deg);
    }
    50% {
      transform: rotate(18deg);
    }
  }
  .dots::after {
    content: '';
    animation: dots 1.4s steps(4, end) infinite;
  }
  @keyframes dots {
    0% {
      content: '';
    }
    25% {
      content: '.';
    }
    50% {
      content: '..';
    }
    75% {
      content: '...';
    }
  }
  .big {
    font-size: 34px;
    color: #a4afc4;
  }
  aside {
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  h3 {
    margin: 0;
    font-size: 15px;
  }
  .summary {
    background: #f4f6fa;
    font-size: 12.5px;
    color: var(--muted);
    justify-content: flex-start;
    padding: 6px 10px;
  }
  .opts {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
    padding: 4px 2px 10px;
    max-height: 48vh;
    overflow-y: auto;
    flex-shrink: 0;
    border-bottom: 1px solid var(--line);
  }
  .opts label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--muted);
  }
  .opts label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }
  .stagebox {
    position: relative;
  }
  .crop {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .crop-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--muted);
  }
  .crop-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--muted);
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }
  .note b {
    color: var(--end);
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    border: 1px solid var(--line);
    border-left: 3px solid var(--accent);
    padding: 7px 10px;
  }
  li.done {
    border-left-color: var(--ok);
  }
  li.error {
    border-left-color: var(--bad);
  }
  li.cancelled {
    border-left-color: var(--muted);
  }
  li.empty {
    border: 0;
    color: var(--muted);
    text-align: center;
    padding: 28px 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-variant-numeric: tabular-nums;
  }
  .row .t {
    font-weight: 600;
  }
  .mini {
    padding: 1px 7px;
    font-size: 12px;
  }
  .len,
  .st {
    color: var(--muted);
    font-size: 12px;
  }
  .st.done {
    color: var(--ok);
  }
  .st.run {
    color: var(--accent);
  }
  .name {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  progress {
    width: 100%;
    height: 5px;
    accent-color: var(--accent);
  }
  .err {
    color: var(--bad);
    font-size: 12px;
    word-break: break-all;
    margin: 0;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
    padding: 2px 4px;
  }
  .path {
    flex: 1;
    min-width: 0;
    justify-content: flex-start;
    font-size: 12px;
    color: var(--muted);
    border: 0;
    background: none;
    padding: 2px 4px;
  }
  .path span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(20, 30, 60, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
  .help {
    background: var(--card);
    border-radius: 12px;
    padding: 18px 22px;
    width: 340px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 12px 40px rgba(20, 30, 60, 0.25);
  }
  .help table {
    border-collapse: collapse;
  }
  .help td {
    padding: 4px 0;
  }
  .help td:first-child {
    width: 90px;
  }
</style>
