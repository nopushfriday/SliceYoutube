/** 초 -> "H:MM:SS.t" / "M:SS.t" */
export function fmt(sec: number | null | undefined, decimals = 1): string {
  if (sec == null || !isFinite(sec)) return '--:--';
  const t = Math.max(0, sec);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const ss = s.toFixed(decimals).padStart(decimals ? 3 + decimals : 2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}
