<script lang="ts">
  import { clamp, fmt } from './util';

  let {
    duration,
    now,
    start,
    end,
    onseek,
    onstart,
    onend
  }: {
    duration: number;
    now: number;
    start: number | null;
    end: number | null;
    onseek: (t: number) => void;
    onstart: (t: number) => void;
    onend: (t: number) => void;
  } = $props();

  let track: HTMLDivElement;
  let drag: 'seek' | 'start' | 'end' | null = null;

  const pct = (t: number) => `${(clamp(t / duration, 0, 1) * 100).toFixed(3)}%`;

  function timeAt(e: PointerEvent): number {
    const r = track.getBoundingClientRect();
    return clamp((e.clientX - r.left) / r.width, 0, 1) * duration;
  }

  // 핸들 드래그: 상대 이동량으로 계산 (Shift를 누르면 1/10 속도로 미세 조절)
  let cur = 0;
  let lastX = 0;

  function down(e: PointerEvent, kind: 'seek' | 'start' | 'end') {
    e.stopPropagation();
    drag = kind;
    track.setPointerCapture(e.pointerId);
    if (kind === 'seek') {
      move(e);
    } else {
      cur = (kind === 'start' ? start : end) ?? timeAt(e);
      lastX = e.clientX;
      kind === 'start' ? onstart(cur) : onend(cur);
    }
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    if (drag === 'seek') return onseek(timeAt(e));
    const w = track.getBoundingClientRect().width;
    const dt = ((e.clientX - lastX) / w) * duration * (e.shiftKey ? 0.1 : 1);
    lastX = e.clientX;
    cur = clamp(cur + dt, 0, duration);
    drag === 'start' ? onstart(cur) : onend(cur);
  }

  function up() {
    drag = null;
  }
</script>

<div class="tl">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="track"
    bind:this={track}
    onpointerdown={(e) => down(e, 'seek')}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
  >
    {#if start != null && end != null && end > start}
      <div class="sel" style:left={pct(start)} style:width="calc({pct(end)} - {pct(start)})"></div>
    {/if}
    {#if start != null}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="handle s"
        style:left={pct(start)}
        title="시작 {fmt(start)}"
        onpointerdown={(e) => down(e, 'start')}
      ></div>
    {/if}
    {#if end != null}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="handle e"
        style:left={pct(end)}
        title="끝 {fmt(end)}"
        onpointerdown={(e) => down(e, 'end')}
      ></div>
    {/if}
    <div class="head" style:left={pct(now)}></div>
  </div>
  <div class="ticks"><span>0:00</span><span>{fmt(duration, 0)}</span></div>
</div>

<style>
  .tl {
    user-select: none;
  }
  .track {
    position: relative;
    height: 38px;
    background: #e4e8f1;
    border-radius: 6px;
    cursor: pointer;
    touch-action: none;
  }
  .sel {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgba(37, 99, 235, 0.22);
    border-top: 2px solid #2563eb;
    border-bottom: 2px solid #2563eb;
  }
  .handle {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 10px;
    margin-left: -5px;
    background: #2563eb;
    border-radius: 3px;
    cursor: ew-resize;
  }
  .handle.e {
    background: #ea580c;
  }
  .head {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: #1c2333;
    pointer-events: none;
  }
  .ticks {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #6b7690;
    margin-top: 3px;
  }
</style>
