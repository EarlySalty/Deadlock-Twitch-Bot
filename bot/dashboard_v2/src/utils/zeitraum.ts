export const MAX_ANALYTICS_DAYS = 3650;

export function clampDays(n: number): number {
  if (!Number.isFinite(n)) return 30;
  const whole = Math.trunc(n);
  if (whole < 7) return 7;
  if (whole > MAX_ANALYTICS_DAYS) return MAX_ANALYTICS_DAYS;
  return whole;
}

export function analyticsMonthsForDays(days: number): number {
  const averageMonthDays = 365.2425 / 12;
  return Math.min(120, Math.max(1, Math.ceil(clampDays(days) / averageMonthDays)));
}

export function parseDaysParam(raw: string | null): number {
  if (raw === null) return 30;
  const trimmed = raw.trim();
  if (!/^-?\d+$/.test(trimmed)) return 30;
  const parsed = Number.parseInt(trimmed, 10);
  if (!Number.isFinite(parsed)) return 30;
  return clampDays(parsed);
}

export function kalenderFenster(days: number): number {
  return Math.max(days, 30);
}

export function streamerAusUrlErlaubt(
  streamer: string,
  isDemoShell: boolean,
  allowedDemoProfiles: string[],
): boolean {
  if (!isDemoShell) return true;
  if (allowedDemoProfiles.length === 0) return true;
  return allowedDemoProfiles.includes(streamer);
}
