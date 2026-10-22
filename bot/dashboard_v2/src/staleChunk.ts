const RELOAD_KEY = 'dashboard-stale-chunk-reload-at';
const RELOAD_COOLDOWN_MS = 30_000;

const CHUNK_ERROR_PATTERNS = [
  'Failed to fetch dynamically imported module',
  'error loading dynamically imported module',
  'Importing a module script failed',
  'Unable to preload CSS',
];

export function isStaleChunkError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error ?? '');
  return CHUNK_ERROR_PATTERNS.some(pattern => message.includes(pattern));
}

export function reloadOnceForStaleChunk(storage: Storage = window.sessionStorage, now = Date.now()): boolean {
  try {
    const last = Number(storage.getItem(RELOAD_KEY) ?? 0);
    if (now - last < RELOAD_COOLDOWN_MS) return false;
    storage.setItem(RELOAD_KEY, String(now));
  } catch {
    return false;
  }
  window.location.reload();
  return true;
}

export function installStaleChunkReload(target: Window = window): void {
  target.addEventListener('vite:preloadError', event => {
    if (reloadOnceForStaleChunk()) event.preventDefault();
  });
}
