const BERLIN_TZ = 'Europe/Berlin';
const LOCAL_INPUT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/;

function tzOffsetMs(instant: Date, timeZone: string): number {
  const local = new Date(instant.toLocaleString('en-US', { timeZone }));
  const utc = new Date(instant.toLocaleString('en-US', { timeZone: 'UTC' }));
  return local.getTime() - utc.getTime();
}

export function utcIsoToBerlinLocalInput(iso: string | null | undefined): string {
  if (!iso) return '';
  const instant = new Date(iso);
  if (Number.isNaN(instant.getTime())) return '';
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: BERLIN_TZ,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).formatToParts(instant);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? '';
  let hour = get('hour');
  if (hour === '24') hour = '00';
  return `${get('year')}-${get('month')}-${get('day')}T${hour}:${get('minute')}`;
}

export function berlinLocalInputToUtcIso(local: string): string | null {
  if (!local) return '';
  const m = LOCAL_INPUT.exec(local);
  if (!m) return null;
  const [, y, mo, d, h, mi] = m.map(Number);
  const asUtc = Date.UTC(y, mo - 1, d, h, mi);
  let offset = tzOffsetMs(new Date(asUtc), BERLIN_TZ);
  let utcMs = asUtc - offset;
  offset = tzOffsetMs(new Date(utcMs), BERLIN_TZ);
  utcMs = asUtc - offset;
  return new Date(utcMs).toISOString();
}

export function berlinNowLocalInput(): string {
  return utcIsoToBerlinLocalInput(new Date().toISOString());
}
