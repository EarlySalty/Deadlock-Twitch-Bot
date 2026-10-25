import { record } from 'rrweb';
import { RecordingUploader, type ChunkSender } from './uploader';
import { isSocialMediaPath } from './recordingPolicy';

const FLUSH_INTERVAL_MS = 10_000;

export interface RecordingHandle {
  stop(flush: boolean): void;
}

function viewport() {
  return { viewport_width: window.innerWidth, viewport_height: window.innerHeight };
}

function startRrweb(uploader: RecordingUploader) {
  return record({
    emit: (event) => {
      if (isSocialMediaPath(window.location.pathname)) uploader.push(event);
    },
    maskAllInputs: true,
    maskInputOptions: { password: true, email: true, tel: true, text: true, textarea: true, search: true },
    maskTextSelector: '[data-recording-mask], input, textarea, [contenteditable="true"]',
    blockSelector: 'video, audio, canvas, iframe, [data-recording-block]',
    inlineStylesheet: true,
    recordCanvas: false,
    collectFonts: false,
    inlineImages: false,
    recordCrossOriginIframes: false,
    sampling: { mousemove: 100, scroll: 150, media: 800, input: 'last' },
  });
}

export function startRecording(send: ChunkSender): RecordingHandle {
  let stopRrweb: (() => void) | undefined;
  const uploader = new RecordingUploader(crypto.randomUUID(), send, viewport(), () => stopRrweb?.());
  stopRrweb = startRrweb(uploader);
  const timer = window.setInterval(() => void uploader.flush(false), FLUSH_INTERVAL_MS);
  const onPageHide = () => void uploader.flush(true);
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') void uploader.flush(true);
  };
  window.addEventListener('pagehide', onPageHide);
  document.addEventListener('visibilitychange', onVisibility);
  return {
    stop(flush: boolean) {
      window.clearInterval(timer);
      window.removeEventListener('pagehide', onPageHide);
      document.removeEventListener('visibilitychange', onVisibility);
      stopRrweb?.();
      stopRrweb = undefined;
      if (flush) void uploader.flush(true);
      else uploader.discard();
    },
  };
}
