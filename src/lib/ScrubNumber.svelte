<script lang="ts">
  import { t } from './i18n.svelte';
  // 프리미어 프로식 수치 조절: 숫자를 좌우로 드래그하면 값이 바뀌고, 클릭하면 직접 입력한다.
  let {
    value,
    onchange,
    unit = '%'
  }: { value: number; onchange: (v: number) => void; unit?: string } = $props();

  let editing = $state(false);
  let input = $state<HTMLInputElement>();
  let drag: { x: number; v: number; moved: boolean } | null = null;

  function down(e: PointerEvent) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { x: e.clientX, v: value, moved: false };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) < 3) return;
    drag.moved = true;
    onchange(drag.v + dx * (e.shiftKey ? 0.02 : 0.15));
  }

  function up() {
    const wasClick = drag && !drag.moved;
    drag = null;
    if (wasClick) {
      editing = true;
      queueMicrotask(() => input?.select());
    }
  }

  function commit() {
    editing = false;
    const v = input?.valueAsNumber;
    if (v !== undefined && Number.isFinite(v)) onchange(v);
  }
</script>

{#if editing}
  <input
    bind:this={input}
    type="number"
    step="0.1"
    {value}
    onblur={commit}
    onkeydown={(e) => {
      if (e.key === 'Enter') commit();
      else if (e.key === 'Escape') editing = false;
    }}
  />
{:else}
  <span
    class="num"
    title={t('scrub.tip')}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={() => (drag = null)}
    role="presentation">{value.toFixed(1)}</span
  >
{/if}
<span class="unit">{unit}</span>

<style>
  .num {
    display: inline-block;
    min-width: 44px;
    text-align: right;
    color: #2563eb;
    cursor: ew-resize;
    user-select: none;
    touch-action: none;
    padding: 2px 4px;
    border-radius: 4px;
  }
  .num:hover {
    background: rgba(37, 99, 235, 0.1);
  }
  input {
    width: 64px;
    text-align: right;
  }
</style>
