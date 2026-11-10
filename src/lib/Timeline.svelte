<script lang="ts">
  import { untrack } from 'svelte';
  import { clamp, fmt } from './util';

  type Clip = { start: number; end: number; status: string };

  let {
    duration,
    now,
    playing,
    start,
    end,
    clips,
    onseek,
    onstart,
    onend
  }: {
    duration: number;
    now: number;
    playing: boolean;
    start: number | null;
    end: number | null;
    clips: Clip[];
    onseek: (t: number) => void;
    onstart: (t: number) => void;
    onend: (t: number) => void;
  } = $props();

  // 상세 줄의 보이는 구간(줌 창)
  let wStart = $state(0);
  let wLen = $state(30);
  const wEnd = $derived(wStart + wLen);
  const minLen = $derived(Math.min(2, duration));

  let detail: HTMLDivElement;
  let over: HTMLDivElement;
  let drag = $state<'seek' | 'start' | 'end' | 'pan' | null>(null);
  let ovDrag = $state(false);

  function fit() {
    wLen = clamp(wLen, minLen, duration);
    wStart = clamp(wStart, 0, Math.max(0, duration - wLen));
  }
  function center(t: number) {
    wStart = t - wLen / 2;
    fit();
  }

  // 영상이 바뀌면 줌 창 초기화
  $effect(() => {
    wLen = Math.min(30, duration);
    wStart = 0;
  });

  // 재생 위치가 창 밖으로 나가면 창을 따라 이동 (재생 중엔 페이지 넘김, 정지 중엔 중앙 정렬)
  // (now가 바뀔 때만 동작해야 하므로 창 상태는 untrack: 사용자가 개요 줄로 창을 옮겨도 되돌아가지 않음)
  $effect(() => {
    const t = now;
    untrack(() => {
      if (drag || ovDrag) return;
      if (t >= wStart && t <= wStart + wLen) return;
      if (playing && t > wStart + wLen) wStart = t - wLen * 0.1;
      else center(t);
      fit();
    });
  });

  const dPct = (t: number) => ((t - wStart) / wLen) * 100;
  const oPct = (t: number) => (clamp(t / duration, 0, 1) * 100).toFixed(3);
  const visible = (a: number, b: number) => b >= wStart && a <= wEnd;

  const ticks = $derived.by(() => {
    const steps = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1200, 1800, 3600];
    const step = steps.find((s) => wLen / s <= 8) ?? 3600;
    const out: { t: number; label: string }[] = [];
    for (let t = Math.ceil(wStart / step) * step; t <= wEnd; t += step) {
      out.push({ t, label: fmt(t, step < 1 ? 2 : step < 60 && step % 1 !== 0 ? 1 : 0) });
    }
    return out;
  });

  function dTime(e: PointerEvent): number {
    const r = detail.getBoundingClientRect();
    return wStart + clamp((e.clientX - r.left) / r.width, 0, 1) * wLen;
  }

  // 핸들 드래그는 이동량 기준(Shift: 1/10 속도), 창 안으로 제한
  let cur = 0;
  let lastX = 0;

  function dDown(e: PointerEvent, kind: 'seek' | 'start' | 'end') {
    e.stopPropagation();
    // 트랙에서 Shift+드래그: 재생 위치는 두고 보이는 구간만 좌우로 이동
    if (kind === 'seek' && e.shiftKey) {
      drag = 'pan';
      lastX = e.clientX;
      detail.setPointerCapture(e.pointerId);
      return;
    }
    drag = kind;
    detail.setPointerCapture(e.pointerId);
    if (kind === 'seek') return onseek(dTime(e));
    cur = (kind === 'start' ? start : end) ?? dTime(e);
    lastX = e.clientX;
    kind === 'start' ? onstart(cur) : onend(cur);
  }

  function dMove(e: PointerEvent) {
    if (!drag) return;
    if (drag === 'seek') return onseek(dTime(e));
    const w = detail.getBoundingClientRect().width;
    if (drag === 'pan') {
      wStart -= ((e.clientX - lastX) / w) * wLen;
      lastX = e.clientX;
      return fit();
    }
    const dt = ((e.clientX - lastX) / w) * wLen * (e.shiftKey ? 0.1 : 1);
    lastX = e.clientX;
    cur = clamp(cur + dt, wStart, wEnd);
    drag === 'start' ? onstart(cur) : onend(cur);
  }

  function dUp() {
    drag = null;
  }

  // 휠로 줌 (커서 위치 고정)
  $effect(() => {
    const el = detail;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = el.getBoundingClientRect();
      // Shift+휠: 좌우 이동 (Shift를 누르면 브라우저가 세로 휠을 가로 휠(deltaX)로 바꿔 보낸다)
      if (e.shiftKey) {
        wStart += ((e.deltaX || e.deltaY) / r.width) * wLen;
        return fit();
      }
      const frac = clamp((e.clientX - r.left) / r.width, 0, 1);
      const tc = wStart + frac * wLen;
      wLen = clamp(wLen * (e.deltaY < 0 ? 0.8 : 1.25), minLen, duration);
      wStart = tc - frac * wLen;
      fit();
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  });

  // 개요 줄: 드래그하면 줌 창 이동, 단순 클릭이면 그 위치로 이동
  let ox = 0;
  let moved = false;
  function oTime(e: PointerEvent): number {
    const r = over.getBoundingClientRect();
    return clamp((e.clientX - r.left) / r.width, 0, 1) * duration;
  }
  function oDown(e: PointerEvent) {
    ovDrag = true;
    moved = false;
    ox = e.clientX;
    over.setPointerCapture(e.pointerId);
    center(oTime(e));
  }
  function oMove(e: PointerEvent) {
    if (!ovDrag) return;
    if (Math.abs(e.clientX - ox) > 3) moved = true;
    center(oTime(e));
  }
  function oUp(e: PointerEvent) {
    if (ovDrag && !moved) onseek(oTime(e));
    ovDrag = false;
  }

  const clipClass = (s: string) => (s === 'done' ? 'done' : s === 'error' ? 'error' : 'q');
</script>

<div class="tl">
  <div class="cap"><span>전체</span><span>점선 박스 = 확대 구간 · 색 막대 = 큐에 담은 클립</span></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="over"
    bind:this={over}
    onpointerdown={oDown}
    onpointermove={oMove}
    onpointerup={oUp}
    onpointercancel={oUp}
  >
    {#each clips as c}
      <div
        class="oclip {clipClass(c.status)}"
        style:left="{oPct(c.start)}%"
        style:width="max(2px, {oPct(c.end)}% - {oPct(c.start)}%)"
      ></div>
    {/each}
    {#if start != null && end != null && end > start}
      <div
        class="oclip cur"
        style:left="{oPct(start)}%"
        style:width="max(2px, {oPct(end)}% - {oPct(start)}%)"
      ></div>
    {/if}
    <div class="win" style:left="{oPct(wStart)}%" style:width="max(6px, {oPct(wEnd)}% - {oPct(wStart)}%)"></div>
    <div class="ohead" style:left="{oPct(now)}%"></div>
  </div>

  <div class="cap">
    <span>상세 · {fmt(wLen, 0)} 구간</span><span>휠: 확대/축소 · Shift+드래그/휠: 좌우 이동</span>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="detail"
    bind:this={detail}
    onpointerdown={(e) => dDown(e, 'seek')}
    onpointermove={dMove}
    onpointerup={dUp}
    onpointercancel={dUp}
  >
    {#each ticks as k}
      <div class="tick" style:left="{dPct(k.t)}%"><span>{k.label}</span></div>
    {/each}
    {#each clips as c}
      {#if visible(c.start, c.end)}
        <div
          class="dclip {clipClass(c.status)}"
          style:left="{Math.max(0, dPct(c.start))}%"
          style:width="{Math.min(100, dPct(c.end)) - Math.max(0, dPct(c.start))}%"
        ></div>
      {/if}
    {/each}
    {#if start != null && end != null && end > start && visible(start, end)}
      <div
        class="sel"
        style:left="{Math.max(0, dPct(start))}%"
        style:width="{Math.min(100, dPct(end)) - Math.max(0, dPct(start))}%"
      ></div>
    {/if}
    {#if start != null && visible(start, start)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="handle s"
        style:left="{dPct(start)}%"
        title="시작 {fmt(start, 2)}"
        onpointerdown={(e) => dDown(e, 'start')}
      ></div>
    {/if}
    {#if end != null && visible(end, end)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="handle e"
        style:left="{dPct(end)}%"
        title="끝 {fmt(end, 2)}"
        onpointerdown={(e) => dDown(e, 'end')}
      ></div>
    {/if}
    <div class="head" style:left="{dPct(now)}%"></div>
  </div>
</div>

<style>
  .tl {
    user-select: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cap {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #6b7690;
  }
  .over,
  .detail {
    position: relative;
    background: #e9edf5;
    border: 1px solid #dfe4ee;
    border-radius: 6px;
    cursor: pointer;
    touch-action: none;
  }
  .over {
    height: 24px;
    overflow: hidden;
  }
  .detail {
    height: 52px;
    margin-top: 0;
    overflow: visible;
  }
  .oclip,
  .dclip {
    position: absolute;
    top: 0;
    bottom: 0;
  }
  .oclip.q,
  .dclip.q {
    background: rgba(37, 99, 235, 0.3);
  }
  .oclip.done,
  .dclip.done {
    background: rgba(22, 163, 74, 0.35);
  }
  .oclip.error,
  .dclip.error {
    background: rgba(220, 38, 38, 0.3);
  }
  .dclip {
    top: auto;
    height: 10px;
    bottom: 0;
  }
  .oclip.cur {
    background: rgba(37, 99, 235, 0.55);
  }
  .win {
    position: absolute;
    top: 0;
    bottom: 0;
    border: 1.5px dashed #1c2333;
    border-radius: 3px;
    box-sizing: border-box;
    pointer-events: none;
  }
  .ohead,
  .head {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: #1c2333;
    pointer-events: none;
  }
  .tick {
    position: absolute;
    top: 0;
    bottom: 0;
    border-left: 1px solid #d3d9e6;
    pointer-events: none;
  }
  .tick span {
    position: absolute;
    top: 2px;
    left: 4px;
    font-size: 10px;
    color: #6b7690;
    white-space: nowrap;
  }
  .sel {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgba(37, 99, 235, 0.2);
    border-top: 2px solid #2563eb;
    border-bottom: 2px solid #2563eb;
    pointer-events: none;
  }
  .handle {
    position: absolute;
    top: -4px;
    bottom: -4px;
    width: 10px;
    margin-left: -5px;
    background: #2563eb;
    border-radius: 3px;
    cursor: ew-resize;
    z-index: 2;
  }
  .handle.e {
    background: #ea580c;
  }
</style>
