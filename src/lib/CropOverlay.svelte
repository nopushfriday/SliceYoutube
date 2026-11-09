<script lang="ts">
  export type Crop = { left: number; right: number; top: number; bottom: number };

  let {
    crop = $bindable(),
    aspect,
    editing
  }: { crop: Crop; aspect: number; editing: boolean } = $props();

  // 플레이어 영역은 16:9이고 영상은 그 안에 비율대로 맞춰지므로(레터박스) 실제 영상 영역만 계산한다.
  const BOX = 16 / 9;
  const frame = $derived(
    aspect >= BOX
      ? { w: 100, h: (BOX / aspect) * 100 }
      : { w: (aspect / BOX) * 100, h: 100 }
  );
  const active = $derived(crop.left + crop.right + crop.top + crop.bottom > 0);
  // 남는 영역의 최소 비율(%). 서버 검증(95% 미만)보다 조금 더 보수적으로 둔다.
  const MIN = 5;

  type Mode = 'l' | 'r' | 't' | 'b' | 'tl' | 'tr' | 'bl' | 'br' | 'move';
  let el = $state<HTMLDivElement>();
  let drag: { mode: Mode; x: number; y: number; start: Crop } | null = null;

  const round = (v: number) => Math.round(v * 10) / 10;

  function down(e: PointerEvent, mode: Mode) {
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { mode, x: e.clientX, y: e.clientY, start: { ...crop } };
  }

  function move(e: PointerEvent) {
    if (!drag || !el) return;
    const r = el.getBoundingClientRect();
    const dx = ((e.clientX - drag.x) / r.width) * 100;
    const dy = ((e.clientY - drag.y) / r.height) * 100;
    const s = drag.start;
    const m = drag.mode;
    const n = { ...s };
    if (m === 'move') {
      const w = 100 - s.left - s.right;
      const h = 100 - s.top - s.bottom;
      n.left = Math.min(100 - w, Math.max(0, s.left + dx));
      n.right = 100 - w - n.left;
      n.top = Math.min(100 - h, Math.max(0, s.top + dy));
      n.bottom = 100 - h - n.top;
    } else {
      if (m.includes('l')) n.left = Math.min(100 - MIN - s.right, Math.max(0, s.left + dx));
      if (m.includes('r')) n.right = Math.min(100 - MIN - s.left, Math.max(0, s.right - dx));
      if (m.includes('t')) n.top = Math.min(100 - MIN - s.bottom, Math.max(0, s.top + dy));
      if (m.includes('b')) n.bottom = Math.min(100 - MIN - s.top, Math.max(0, s.bottom - dy));
    }
    crop = { left: round(n.left), right: round(n.right), top: round(n.top), bottom: round(n.bottom) };
  }

  function up() {
    drag = null;
  }

  const HANDLES: Mode[] = ['tl', 't', 'tr', 'r', 'br', 'b', 'bl', 'l'];
</script>

{#if active || editing}
  <div class="stage" class:editing>
    <div
      class="frame"
      bind:this={el}
      style="width:{frame.w}%;height:{frame.h}%"
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
      role="presentation"
    >
      <div class="dim" style="left:0;top:0;right:0;height:{crop.top}%"></div>
      <div class="dim" style="left:0;bottom:0;right:0;height:{crop.bottom}%"></div>
      <div class="dim" style="left:0;top:{crop.top}%;bottom:{crop.bottom}%;width:{crop.left}%"></div>
      <div class="dim" style="right:0;top:{crop.top}%;bottom:{crop.bottom}%;width:{crop.right}%"></div>
      <div
        class="box"
        style="left:{crop.left}%;right:{crop.right}%;top:{crop.top}%;bottom:{crop.bottom}%"
      >
        {#if editing}
          <div class="body" onpointerdown={(e) => down(e, 'move')} role="presentation"></div>
          {#each HANDLES as h}
            <div class="h {h}" onpointerdown={(e) => down(e, h)} role="presentation"></div>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .stage {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }
  .stage.editing {
    pointer-events: auto;
  }
  .frame {
    position: relative;
  }
  .dim {
    position: absolute;
    background: rgba(0, 0, 0, 0.62);
  }
  .box {
    position: absolute;
    outline: 1.5px solid #fff;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
  }
  .editing .box {
    /* 3분할 가이드 */
    background-image:
      linear-gradient(to right, transparent 33.2%, rgba(255, 255, 255, 0.35) 33.3%, transparent 33.5%, transparent 66.5%, rgba(255, 255, 255, 0.35) 66.6%, transparent 66.8%),
      linear-gradient(to bottom, transparent 33.2%, rgba(255, 255, 255, 0.35) 33.3%, transparent 33.5%, transparent 66.5%, rgba(255, 255, 255, 0.35) 66.6%, transparent 66.8%);
  }
  .body {
    position: absolute;
    inset: 0;
    cursor: move;
  }
  .h {
    position: absolute;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    background: #fff;
    border: 1.5px solid #2563eb;
    border-radius: 3px;
    box-sizing: border-box;
  }
  .tl { left: 0; top: 0; cursor: nwse-resize; }
  .t { left: 50%; top: 0; cursor: ns-resize; }
  .tr { left: 100%; top: 0; cursor: nesw-resize; }
  .r { left: 100%; top: 50%; cursor: ew-resize; }
  .br { left: 100%; top: 100%; cursor: nwse-resize; }
  .b { left: 50%; top: 100%; cursor: ns-resize; }
  .bl { left: 0; top: 100%; cursor: nesw-resize; }
  .l { left: 0; top: 50%; cursor: ew-resize; }
</style>
