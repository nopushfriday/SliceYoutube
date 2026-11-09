<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { downloadDir, join } from '@tauri-apps/api/path';
  import { open } from '@tauri-apps/plugin-dialog';
  import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';
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
    ['Q', '시작 지점 지정'],
    ['W', '끝 지점 지정'],
    ['Enter', '클립을 큐에 추가 (숫자패드 Enter 포함)'],
    ['Space', '재생 / 일시정지'],
    ['← →', '5초 이동 (Shift: 1초)'],
    [', .', '1프레임 이동'],
    ['\\', '구간 반복 켜기/끄기'],
    ['?', '이 도움말']
  ];

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
  let cropEditing = $state(false);
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
  let showSettings = $state(false);
  let showHelp = $state(false);

  // yt-dlp 자동 업데이트 상태
  type Yt = { version: string | null; updated: boolean; error: string | null };
  let ytBusy = $state(true);
  let yt = $state<Yt>({ version: null, updated: false, error: null });

  async function updateYtdlp() {
    ytBusy = true;
    try {
      yt = await invoke<Yt>('ytdlp_update');
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
    updateYtdlp(); // 앱을 열 때마다 yt-dlp 최신 버전 확인
    invoke<string | null>('hw_encoder')
      .then((e) => (hwEnc = e))
      .catch(() => (hwEnc = null));
    (async () => {
      outDir = saved.outDir ?? (await join(await downloadDir(), 'cut-dlp'));
    })();
    const un = listen<{ job: number; percent: number; phase: 'download' | 'encode' }>(
      'dl-progress',
      (e) => {
        const s = segs.find((x) => x.id === e.payload.job);
        if (s) {
          s.percent = e.payload.percent;
          s.phase = e.payload.phase;
        }
      }
    );
    return () => {
      un.then((f) => f());
    };
  });

  $effect(() => {
    localStorage.setItem('cut-dlp', JSON.stringify({ audioOnly, maxHeight, codec, hwAccel, outDir }));
  });

  async function load() {
    const u = url.trim();
    if (!u || loading) return;
    loading = true;
    error = '';
    try {
      info = await invoke<Info>('fetch_info', { url: u });
      sourceUrl = u;
      start = end = null;
      now = 0;
      crop = { ...NO_CROP };
      cropEditing = false;
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
      error = '클립보드를 읽을 수 없습니다. 주소를 직접 붙여넣어 주세요.';
      return;
    }
    if (url) load();
  }

  async function openOutDir() {
    try {
      await openPath(outDir);
    } catch (e) {
      error = `폴더를 열 수 없습니다 (아직 다운로드한 파일이 없으면 폴더가 없을 수 있습니다): ${e}`;
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

  async function runQueue() {
    if (running) return;
    running = true;
    stopQueue = false;
    for (const s of segs) {
      if (stopQueue) break;
      if (s.status !== 'pending' && s.status !== 'error' && s.status !== 'cancelled') continue;
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
            outDir
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
      onclick={updateYtdlp}
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
      {#if info}
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
          <CropOverlay bind:crop aspect={info.aspect} editing={cropEditing} />
        </div>
        <div class="time">
          <button class="icon-btn" onclick={() => player.toggle()} aria-label="재생/일시정지">
            <i class="ti {playing ? 'ti-player-pause' : 'ti-player-play'}"></i>
          </button>
          <b>{fmt(now, 2)}</b> / {fmt(info.duration, 0)}
          <span class="grow"></span>
          <button class="crop-btn" class:on={cropEditing} onclick={() => (cropEditing = !cropEditing)} title="영상 위에서 드래그해 잘라낼 범위를 지정합니다">
            <i class="ti ti-crop"></i> 자르기{cropped ? ' ●' : ''}
          </button>
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
          <p>
            영상을 불러온 뒤 재생하면서 <kbd>Q</kbd>로 시작, <kbd>W</kbd>로 끝 지점을 찍으세요.
            지정한 구간만 받아서 용량과 시간을 아낍니다.
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
              <p class="note">영상 위 '자르기' 버튼으로 드래그하거나, 숫자를 좌우로 끌어 조절(클릭하면 직접 입력)하세요. </p>
            </div>
            {#if codec === 'h264'}
              <label class="check" title="H.264로 인코딩할 때 GPU를 사용합니다. 지원하지 않거나 실패하면 CPU로 자동 전환됩니다.">
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
              {#if s.status === 'done' && s.file}
                <button class="mini" onclick={() => revealItemInDir(s.file!)}>폴더</button>
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
            <div>구간을 지정하고 <kbd>Enter</kbd>로 추가하세요</div>
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
        <button class="path" onclick={pickDir} title="클릭해서 저장 폴더 변경: {outDir}">
          <i class="ti ti-folder"></i><span>{outDir}</span>
        </button>
        <button class="mini" onclick={openOutDir} title="저장 폴더 열기" aria-label="저장 폴더 열기">
          <i class="ti ti-folder-open"></i>
        </button>
      </div>
    </aside>
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
        <p class="note">타임라인: 휠로 확대/축소, 핸들을 끌면 영상이 따라가고 Shift+드래그는 미세 조절입니다.</p>
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
  .crop-btn.on {
    background: var(--accent, #2563eb);
    color: #fff;
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
