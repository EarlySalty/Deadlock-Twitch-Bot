import type { UsageTotals } from '../api/socialAiTypes.ts';

export const GLOBAL_CAP_MICRO_USD = 2_000_000;

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

export function usdInput(value: number | null): string {
  return value === null ? '' : (value / 1_000_000).toFixed(6).replace('.', ',');
}

export function costTotal(totals: UsageTotals, key: 'estimated_micro_usd' | 'measured_micro_usd' | 'invoiced_micro_usd'): number | null {
  return totals.incomplete_attempts > 0 || totals.unknown_cost_attempts > 0 || totals.attempts === 0
    ? null : totals[key];
}

export function formatDate(value: string | null): string {
  if (!value) return 'Unbekannt';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? 'Unbekannt' : new Intl.DateTimeFormat('de-DE', {
    dateStyle: 'medium', timeStyle: 'short',
  }).format(date);
}
