<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { downloadDir, join } from '@tauri-apps/api/path';
  import { open } from '@tauri-apps/plugin-dialog';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import Player from '$lib/Player.svelte';
  import Timeline from '$lib/Timeline.svelte';
  import { clamp, fmt } from '$lib/util';

  type Fmt = { codec: 'h264' | 'vp9' | 'av1'; height: number };
  type Info = { id: string; title: string; duration: number; channel: string; formats: Fmt[] };
  type Codec = 'auto' | 'h264' | 'vp9' | 'av1';
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

  let url = $state('');
  let loading = $state(false);
  let error = $state('');
  let info = $state<Info | null>(null);
  let sourceUrl = '';

  // svelte-ignore non_reactive_update
  let player: Player;
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

  let accurate = $state(true);
  let audioOnly = $state(false);
  let maxHeight = $state(0); // 0 = 최고 화질 자동
  let codec = $state<Codec>('auto');
  let outDir = $state('');

  const valid = $derived(start != null && end != null && end > start);
  const offered = $derived(
    info ? [...new Set(info.formats.map((f) => CODEC_LABEL[f.codec]))] : []
  );
  // 선택한 코덱을 영상이 (화질 상한 내에서) 제공하지 않으면 받은 뒤 인코딩한다
  const willEncode = $derived(
    !!info &&
      !audioOnly &&
      codec !== 'auto' &&
      !info.formats.some((f) => f.codec === codec && (maxHeight === 0 || f.height <= maxHeight))
  );

  onMount(() => {
    const saved = JSON.parse(localStorage.getItem('cut-dlp') ?? '{}');
    accurate = saved.accurate ?? true;
    audioOnly = saved.audioOnly ?? false;
    maxHeight = saved.maxHeight ?? 0;
    codec = saved.codec ?? 'auto';
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
    localStorage.setItem('cut-dlp', JSON.stringify({ accurate, audioOnly, maxHeight, codec, outDir }));
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
    } catch (e) {
      error = String(e);
      info = null;
    } finally {
      loading = false;
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
            accurate,
            audioOnly,
            maxHeight,
            codec,
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
    if (t.closest('input, select, textarea')) return;
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
  <header>
    <input
      class="url"
      placeholder="YouTube 주소를 붙여넣고 Enter"
      bind:value={url}
      onkeydown={(e) => e.key === 'Enter' && load()}
    />
    <button onclick={load} disabled={loading || !url.trim()}>{loading ? '불러오는 중…' : '불러오기'}</button>
  </header>
  {#if error}<p class="err">{error}</p>{/if}

  <div class="cols">
    <section class="left">
      {#if info}
        <h2 title={info.title}>{info.title}</h2>
        <Player bind:this={player} videoId={info.id} ontime={onTime} onstate={(p) => (playing = p)} />
        <div class="time">
          <b>{fmt(now, 2)}</b> / {fmt(info.duration, 0)}
          <span class="grow"></span>
          <button onclick={() => player.toggle()}>{playing ? '일시정지' : '재생'}</button>
        </div>
        <Timeline
          duration={info.duration}
          {now}
          {start}
          {end}
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
          <button class="s" onclick={() => setStart(player.time())}><kbd>Q</kbd> 시작 <b>{fmt(start, 2)}</b></button>
          <button class="e" onclick={() => setEnd(player.time())}><kbd>W</kbd> 끝 <b>{fmt(end, 2)}</b></button>
          <span class="dur">{valid ? `길이 ${fmt(end! - start!, 2)}` : ''}</span>
          <span class="grow"></span>
          <button onclick={preview} disabled={!valid}>구간 미리보기</button>
          <label><input type="checkbox" bind:checked={loop} /> 반복</label>
          <button class="primary" onclick={addSegment} disabled={!valid}>큐에 추가 (Enter)</button>
        </div>

        <p class="keys">
          <kbd>Q</kbd> 시작 · <kbd>W</kbd> 끝 · <kbd>Space</kbd> 재생 · <kbd>←</kbd><kbd>→</kbd> 5초
          (<kbd>Shift</kbd> 1초) · <kbd>,</kbd><kbd>.</kbd> 1프레임 · <kbd>\</kbd> 반복 ·
          <kbd>Enter</kbd> 큐 추가 · 핸들 드래그 시 영상 위치 동기화 (<kbd>Shift</kbd> 누르고 드래그하면 미세 조절)
        </p>
      {:else}
        <div class="hint">
          <h2>필요한 구간만 다운로드</h2>
          <p>YouTube 주소를 불러온 뒤, 재생하면서 <kbd>Q</kbd>로 시작, <kbd>W</kbd>로 끝 지점을 찍으세요.</p>
        </div>
      {/if}
    </section>

    <aside>
      <h3>다운로드 큐</h3>
      <div class="opts">
        <label>저장 폴더
          <div class="dir"><input readonly value={outDir} title={outDir} /><button onclick={pickDir}>…</button></div>
        </label>
        <label><input type="checkbox" bind:checked={audioOnly} /> 오디오만 (mp3)</label>
        {#if !audioOnly}
          <label>코덱
            <select bind:value={codec}>
              <option value="auto">자동 (제공되는 그대로)</option>
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
        <label title="끄면 키프레임 단위로 잘라 더 빠르지만 시작/끝이 몇 초 어긋날 수 있음">
          <input type="checkbox" bind:checked={accurate} /> 정확한 컷 (구간만 재인코딩)
        </label>
      </div>

      <ul>
        {#each segs as s (s.id)}
          <li class={s.status}>
            <div class="row">
              <span class="t">{fmt(s.start, 1)} → {fmt(s.end, 1)}</span>
              <span class="len">({fmt(s.end - s.start, 1)})</span>
              <span class="grow"></span>
              {#if s.status === 'done' && s.file}
                <button onclick={() => revealItemInDir(s.file!)}>폴더</button>
              {/if}
              <button onclick={() => remove(s.id)} title="삭제">✕</button>
            </div>
            <div class="name" title={s.title}>{s.title}</div>
            {#if s.status === 'running'}
              <div class="st">{s.phase === 'encode' ? '인코딩 중' : '다운로드 중'} {Math.round(s.percent)}%</div>
              <progress max="100" value={s.percent}></progress>
            {:else if s.status === 'error'}
              <div class="err">{s.error}</div>
            {:else}
              <div class="st">{{ pending: '대기', done: '완료', cancelled: '취소됨' }[s.status]}</div>
            {/if}
          </li>
        {:else}
          <li class="empty">구간을 지정하고 Enter로 추가하세요</li>
        {/each}
      </ul>

      {#if running}
        <button class="danger" onclick={cancelAll}>중지</button>
      {:else}
        <button
          class="primary"
          onclick={runQueue}
          disabled={!segs.some((s) => ['pending', 'error', 'cancelled'].includes(s.status))}
        >다운로드 시작</button>
      {/if}
    </aside>
  </div>
</main>

<style>
  :global(:root) {
    --bg: #f4f6fa;
    --card: #ffffff;
    --line: #e2e6ee;
    --text: #1c2333;
    --muted: #6b7690;
    --accent: #2563eb;
    --start: #2563eb;
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
    padding: 16px 20px;
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    gap: 8px;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 10px;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  .url {
    flex: 1;
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
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 350px;
    gap: 16px;
    flex: 1;
    min-height: 0;
  }
  .left {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-right: 2px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .time,
  .range {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .time {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .time b {
    color: var(--text);
    font-size: 16px;
  }
  .grow {
    flex: 1;
  }
  .range {
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 10px;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  .range .s {
    border-color: var(--start);
    background: rgba(37, 99, 235, 0.07);
  }
  .range .e {
    border-color: var(--end);
    background: rgba(234, 88, 12, 0.07);
  }
  .range b {
    font-variant-numeric: tabular-nums;
    margin-left: 4px;
  }
  .dur {
    color: var(--muted);
  }
  .keys {
    color: var(--muted);
    font-size: 12px;
    margin: 0;
  }
  .hint {
    margin: auto;
    text-align: center;
    color: var(--muted);
  }
  .hint h2 {
    color: var(--text);
    margin-bottom: 6px;
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
  aside {
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    box-shadow: 0 1px 3px rgba(20, 30, 60, 0.06);
  }
  h3 {
    margin: 0;
    font-size: 15px;
  }
  .opts {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--line);
  }
  .opts label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--muted);
  }
  .opts label:has(input[type='checkbox']) {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--text);
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
  .dir {
    display: flex;
    gap: 4px;
  }
  .dir input {
    flex: 1;
    min-width: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    background: #f8f9fc;
    border: 1px solid var(--line);
    border-left: 4px solid var(--accent);
    border-radius: 8px;
    padding: 8px 10px;
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
    background: none;
    border: 0;
    color: var(--muted);
    text-align: center;
    padding: 24px 0;
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
  .row button {
    padding: 1px 8px;
    font-size: 12px;
  }
  .len,
  .st {
    color: var(--muted);
    font-size: 12px;
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
    height: 6px;
    accent-color: var(--accent);
  }
  .err {
    color: var(--bad);
    font-size: 12px;
    word-break: break-all;
  }
</style>
