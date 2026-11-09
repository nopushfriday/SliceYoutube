<script lang="ts">
  import { onDestroy } from 'svelte';

  let {
    videoId,
    ontime,
    onstate,
    onerror
  }: {
    videoId: string;
    ontime?: (t: number) => void;
    onstate?: (playing: boolean) => void;
    onerror?: () => void;
  } = $props();

  let host: HTMLDivElement;
  let player: any = null;
  let ready = false;
  let timer: ReturnType<typeof setInterval> | undefined;

  function loadApi(): Promise<void> {
    const w = window as any;
    if (w.YT?.Player) return Promise.resolve();
    return new Promise((resolve) => {
      const prev = w.onYouTubeIframeAPIReady;
      w.onYouTubeIframeAPIReady = () => {
        prev?.();
        resolve();
      };
      if (!document.querySelector('script[data-yt-api]')) {
        const s = document.createElement('script');
        s.src = 'https://www.youtube.com/iframe_api';
        s.dataset.ytApi = '1';
        document.head.append(s);
      }
    });
  }

  async function mount(id: string) {
    await loadApi();
    clearInterval(timer);
    ready = false;
    player = null;

    // 직접 iframe을 만들어 referrer 정책을 지정 (Tauri 웹뷰에서 임베드 오류 방지)
    const iframe = document.createElement('iframe');
    const q = new URLSearchParams({
      enablejsapi: '1',
      origin: location.origin,
      controls: '0', // 자체 UI 사용 (키보드 포커스가 iframe으로 넘어가지 않도록)
      disablekb: '1',
      rel: '0',
      modestbranding: '1',
      playsinline: '1',
      iv_load_policy: '3'
    });
    iframe.src = `https://www.youtube.com/embed/${id}?${q}`;
    iframe.referrerPolicy = 'strict-origin-when-cross-origin';
    iframe.allow = 'autoplay; encrypted-media';
    iframe.style.cssText = 'width:100%;height:100%;border:0;';
    host.replaceChildren(iframe);

    const YT = (window as any).YT;
    player = new YT.Player(iframe, {
      events: {
        onReady: () => {
          ready = true;
        },
        onStateChange: (e: any) => onstate?.(e.data === 1),
        // 101/150: 소유자가 임베드를 금지한 영상
        onError: (e: any) => {
          if (e.data === 101 || e.data === 150 || e.data === 153) onerror?.();
        }
      }
    });
    timer = setInterval(() => {
      if (ready) ontime?.(player.getCurrentTime());
    }, 80);
  }

  $effect(() => {
    mount(videoId);
  });

  onDestroy(() => clearInterval(timer));

  export function seek(t: number) {
    if (!ready) return;
    player.seekTo(Math.max(0, t), true);
    ontime?.(Math.max(0, t));
  }
  export function time(): number {
    return ready ? player.getCurrentTime() : 0;
  }
  export function isPlaying(): boolean {
    return ready && player.getPlayerState() === 1;
  }
  export function play() {
    if (ready) player.playVideo();
  }
  export function pause() {
    if (ready) player.pauseVideo();
  }
  export function toggle() {
    isPlaying() ? pause() : play();
  }
</script>

<div class="wrap">
  <div class="host" bind:this={host}></div>
  <!-- 클릭 시 재생/일시정지. iframe이 포커스를 가져가 단축키가 막히는 것을 방지 -->
  <button class="shield" aria-label="재생/일시정지" onclick={toggle}></button>
</div>

<style>
  .wrap {
    position: relative;
    aspect-ratio: 16 / 9;
    background: #000;
    border-radius: 8px;
    overflow: hidden;
  }
  .host,
  .shield {
    position: absolute;
    inset: 0;
  }
  .shield {
    background: transparent;
    border: 0;
    cursor: pointer;
  }
</style>
