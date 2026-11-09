<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  // 임베드가 막힌 영상용 대체 플레이어. Player.svelte와 같은 공개 메서드를 제공한다.
  let {
    url,
    ontime,
    onstate
  }: {
    url: string;
    ontime?: (t: number) => void;
    onstate?: (playing: boolean) => void;
  } = $props();

  let video = $state<HTMLVideoElement>();
  let src = $state('');
  let error = $state('');
  let loading = $state(true);

  $effect(() => {
    const u = url;
    loading = true;
    error = '';
    src = '';
    invoke<string>('stream_url', { url: u })
      .then((s) => (src = s))
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  export function seek(t: number) {
    if (!video) return;
    video.currentTime = Math.max(0, t);
    ontime?.(video.currentTime);
  }
  export function time(): number {
    return video?.currentTime ?? 0;
  }
  export function isPlaying(): boolean {
    return !!video && !video.paused;
  }
  export function play() {
    video?.play().catch(() => {});
  }
  export function pause() {
    video?.pause();
  }
  export function toggle() {
    isPlaying() ? pause() : play();
  }
</script>

<div class="wrap">
  {#if src}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={video}
      {src}
      preload="auto"
      playsinline
      ontimeupdate={() => ontime?.(video!.currentTime)}
      onplay={() => onstate?.(true)}
      onpause={() => onstate?.(false)}
      onended={() => onstate?.(false)}
      onerror={() =>
        (error = `이 영상은 미리보기를 재생할 수 없습니다 (code ${video?.error?.code} net=${video?.networkState} h264=${video?.canPlayType('video/mp4; codecs="avc1.42001E, mp4a.40.2"') || 'no'} ${video?.error?.message ?? ''})`)}
    ></video>
    <button class="shield" aria-label="재생/일시정지" onclick={toggle}></button>
  {/if}
  {#if loading}
    <div class="msg">미리보기 불러오는 중…</div>
  {:else if error}
    <div class="msg">{error}</div>
  {/if}
  <div class="badge">임베드 불가 영상 · 스트림 미리보기</div>
</div>

<style>
  .wrap {
    position: relative;
    aspect-ratio: 16 / 9;
    background: #000;
    border-radius: 8px;
    overflow: hidden;
  }
  video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .shield {
    position: absolute;
    inset: 0;
    background: transparent;
    border: 0;
    cursor: pointer;
  }
  .msg {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #cbd5e1;
    font-size: 14px;
  }
  .badge {
    position: absolute;
    left: 8px;
    top: 8px;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.55);
    color: #e2e8f0;
    font-size: 11px;
    pointer-events: none;
  }
</style>
