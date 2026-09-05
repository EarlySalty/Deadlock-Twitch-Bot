export function clampDays(n: number): number {
  if (!Number.isFinite(n)) return 30;
  const whole = Math.trunc(n);
  if (whole < 7) return 7;
  if (whole > 365) return 365;
  return whole;
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
