const berlin = new Intl.DateTimeFormat('sv-SE', {
  timeZone: 'Europe/Berlin', year: 'numeric', month: '2-digit', day: '2-digit',
  hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
});
export function berlinInput(iso: string): string {
  const at = new Date(iso);
  if (!Number.isFinite(at.getTime())) throw new Error('Ungültige Uhrzeit.');
  const parts = Object.fromEntries(berlin.formatToParts(at).map(p => [p.type, p.value]));
  return `${parts.year}-${parts.month}-${parts.day}T${parts.hour}:${parts.minute}`;
}
/** No silent DST correction and no dependence on the browser's timezone. */
export function fromBerlinInput(value: string, original?: string): string {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value)) throw new Error('Bitte Datum und Uhrzeit vollständig eintragen.');
  if (original && berlinInput(original) === value) return original;
  const nominal = Date.parse(`${value}:00Z`);
  if (!Number.isFinite(nominal)) throw new Error('Ungültiges Datum.');
  const matches: string[] = [];
  // All modern Berlin offsets are resolved through Intl, not a fixed UTC+1/2.
  for (let minutes = -180; minutes <= 180; minutes += 15) {
    const iso = new Date(nominal + minutes * 60_000).toISOString();
    if (berlinInput(iso) === value) matches.push(iso);
  }
  if (matches.length !== 1) throw new Error(matches.length === 0
    ? 'Diese Uhrzeit existiert in Berlin wegen der Zeitumstellung nicht. Bitte eine andere Uhrzeit wählen.'
    : 'Diese Uhrzeit kommt bei der Zeitumstellung zweimal vor. Bitte eine eindeutige Uhrzeit außerhalb der doppelten Stunde wählen.');
  return matches[0];
}
export function monthDays(month: string): { leading: number; days: string[] } {
  if (!/^\d{4}-\d{2}$/.test(month)) return { leading: 0, days: [] };
  const first = new Date(`${month}-01T12:00:00Z`);
  if (!Number.isFinite(first.getTime()) || first.toISOString().slice(0, 7) !== month) return { leading: 0, days: [] };
  const count = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
  return { leading: (first.getUTCDay() + 6) % 7, days: Array.from({ length: count }, (_, i) => `${month}-${String(i + 1).padStart(2, '0')}`) };
}
