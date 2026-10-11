export function formatMicroUsd(value: number | null | undefined): string {
  if (value == null || !Number.isSafeInteger(value) || value < 0) return 'Unbekannt';
  return new Intl.NumberFormat('de-DE', {
    style: 'currency', currency: 'USD', minimumFractionDigits: 4, maximumFractionDigits: 6,
  }).format(value / 1_000_000);
}

export function formatDuration(value: number | null | undefined): string {
  if (value == null || !Number.isSafeInteger(value) || value < 0) return 'Unbekannt';
  return `${new Intl.NumberFormat('de-DE', { maximumFractionDigits: 2 }).format(value / 1000)} s`;
}

export function formatCount(value: number | null | undefined): string {
  return value == null || !Number.isSafeInteger(value) || value < 0
    ? 'Unbekannt' : new Intl.NumberFormat('de-DE').format(value);
}

export function parseMicroUsd(value: string): number | null {
  const normalized = value.trim().replace(',', '.');
  if (!/^\d+(\.\d{1,6})?$/.test(normalized)) return null;
  const [whole, fraction = ''] = normalized.split('.');
  const amount = Number(whole) * 1_000_000 + Number(fraction.padEnd(6, '0'));
  return Number.isSafeInteger(amount) && amount >= 0 ? amount : null;
}
