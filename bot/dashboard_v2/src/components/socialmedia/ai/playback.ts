import type { AiTimeRange } from './api';
import { sourceTimeMs } from './timeline';

export function attachSyncedPlayback(out: HTMLVideoElement, original: HTMLVideoElement, kept: AiTimeRange[]) {
  const wasMuted = original.muted;
  const previousRate = original.playbackRate;
  const rate = () => { original.playbackRate = out.playbackRate; };
  original.muted = true;
  rate();
  const align = (force: boolean) => {
    const target = sourceTimeMs(kept, out.currentTime * 1000);
    if (target === null) return;
    if (force || Math.abs(original.currentTime - target / 1000) > 0.25) original.currentTime = target / 1000;
  };
  const play = () => { align(true); void original.play().catch(() => undefined); };
  const pause = () => original.pause();
  const seek = () => align(true);
  const tick = () => align(false);
  align(true);
  if (!out.paused) play();
  out.addEventListener('play', play);
  out.addEventListener('pause', pause);
  out.addEventListener('seeked', seek);
  out.addEventListener('timeupdate', tick);
  out.addEventListener('ratechange', rate);
  original.addEventListener('loadedmetadata', seek);
  return () => {
    out.removeEventListener('play', play);
    out.removeEventListener('pause', pause);
    out.removeEventListener('seeked', seek);
    out.removeEventListener('timeupdate', tick);
    out.removeEventListener('ratechange', rate);
    original.removeEventListener('loadedmetadata', seek);
    original.pause();
    original.muted = wasMuted;
    original.playbackRate = previousRate;
  };
}
