export interface CategoryTrendInput {
  at: string;
  streams: number | null;
  viewers: number | null;
  polls?: number;
}

export function languageName(code: string): string {
  if (code === 'und') return 'Unbekannt / nicht sicher erkannt';
  try { return new Intl.DisplayNames(['de'], { type: 'language' }).of(code) ?? code; } catch { return code; }
}

/** Keep every hourly bucket. Never average averages or turn gaps into zero. */
export function buildCategoryTrend(input: CategoryTrendInput[]) {
  const sorted = input
    .map(row => ({ ...row, timestamp: Date.parse(row.at) }))
    .filter(row => Number.isFinite(row.timestamp))
    .sort((a, b) => a.timestamp - b.timestamp);
  const result: Array<{ timestamp: number; streams: number | null; viewers: number | null; polls: number }> = [];
  let previous: number | undefined;
  for (const row of sorted) {
    if (previous !== undefined && row.timestamp - previous > 3_600_000) {
      result.push({ timestamp: previous + 3_600_000, streams: null, viewers: null, polls: 0 });
    }
    result.push({ timestamp: row.timestamp,
      streams: row.streams !== null && Number.isFinite(row.streams) ? row.streams : null,
      viewers: row.viewers !== null && Number.isFinite(row.viewers) ? row.viewers : null,
      polls: row.polls ?? 0 });
    previous = row.timestamp;
  }
  return result;
}

export function categoryArchiveLabel(confirmed: boolean | undefined): string {
  return confirmed === true
    ? 'Keine automatische Löschung. Rohdaten und Stundenaggregate bleiben erhalten.'
    : 'Aufbewahrungsstatus noch nicht bestätigt.';
}

export function categoryUtcLabel(value: number | string): string {
  const time = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(time)) return 'Unbekannter Zeitpunkt';
  return new Date(time).toLocaleString('de-DE', { timeZone: 'UTC', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
}
