export const CATEGORY_HEARTBEAT_MAX_AGE_MS = 120_000;

export function collectorHeartbeatStale(
  heartbeatAt: string | null | undefined,
  nowMs: number,
  maxAgeMs = CATEGORY_HEARTBEAT_MAX_AGE_MS,
): boolean {
  if (!heartbeatAt || !Number.isFinite(nowMs)) {
    return true;
  }

  const heartbeatMs = new Date(heartbeatAt).getTime();
  if (!Number.isFinite(heartbeatMs)) {
    return true;
  }

  return nowMs - heartbeatMs > maxAgeMs;
}
