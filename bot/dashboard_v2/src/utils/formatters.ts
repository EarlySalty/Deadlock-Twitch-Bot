// Formatting utilities for the dashboard

export function formatNumber(n: number | null | undefined, decimals = 0, locale = 'de-DE'): string {
  if (n === null || n === undefined) return '-';
  return n.toLocaleString(locale, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

export function formatPercent(n: number | null | undefined, decimals = 1, locale?: string): string {
  if (n === null || n === undefined) return '-';
  return `${locale ? formatNumber(n, decimals, locale) : n.toFixed(decimals)}%`;
}

export function formatDuration(seconds: number): string {
  if (!seconds || seconds <= 0) return '0m';

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);

  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

export function formatDurationShort(seconds: number): string {
  if (!seconds || seconds <= 0) return '0m';

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);

  if (hours > 0) {
    return minutes > 0 ? `${hours}h${minutes}m` : `${hours}h`;
  }
  return `${minutes}m`;
}

export function formatHours(hours: number, locale?: string): string {
  if (hours < 1) {
    return `${Math.round(hours * 60)}m`;
  }
  if (hours < 100) {
    return `${locale ? formatNumber(hours, 1, locale) : hours.toFixed(1)}h`;
  }
  return `${locale ? formatNumber(Math.round(hours), 0, locale) : Math.round(hours)}h`;
}

export function formatDate(dateStr: string, locale = 'de-DE'): string {
  if (!dateStr) return '-';
  const date = new Date(dateStr);
  return date.toLocaleDateString(locale, {
    day: '2-digit',
    month: '2-digit',
    year: '2-digit',
  });
}

export function formatDateFull(dateStr: string, locale = 'de-DE'): string {
  if (!dateStr) return '-';
  const date = new Date(dateStr);
  return date.toLocaleDateString(locale, {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
  });
}

export function formatTime(timeStr: string): string {
  if (!timeStr) return '-';
  // Handle both "HH:MM:SS" and full datetime strings
  const parts = timeStr.split(':');
  if (parts.length >= 2) {
    return `${parts[0]}:${parts[1]}`;
  }
  return timeStr;
}

export function formatTrend(value: number, locale?: string): string {
  if (value > 0) return `+${(locale ? formatNumber(value, 1, locale) : value.toFixed(1))}%`;
  if (value < 0) return `${(locale ? formatNumber(value, 1, locale) : value.toFixed(1))}%`;
  return '0%';
}

export function formatCompact(n: number, locale?: string): string {
  if (locale) return new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(n);
  if (n >= 1_000_000) {
    return `${(n / 1_000_000).toFixed(1)}M`;
  }
  if (n >= 1_000) {
    return `${(n / 1_000).toFixed(1)}K`;
  }
  return n.toString();
}

export function getWeekdayLabel(weekday: number, locale?: string): string {
  if (locale && Number.isInteger(weekday) && weekday >= 0 && weekday <= 6) {
    return new Intl.DateTimeFormat(locale, { weekday: 'short', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, 0, 7 + weekday)));
  }
  const days = ['So', 'Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa'];
  return days[weekday] || '';
}

export function getWeekdayLabelFull(weekday: number, locale?: string): string {
  if (locale && Number.isInteger(weekday) && weekday >= 0 && weekday <= 6) {
    return new Intl.DateTimeFormat(locale, { weekday: 'long', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, 0, 7 + weekday)));
  }
  const days = ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'];
  return days[weekday] || '';
}

export function getMonthLabel(month: number, locale?: string): string {
  if (locale && Number.isInteger(month) && month >= 1 && month <= 12) {
    return new Intl.DateTimeFormat(locale, { month: 'short', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, month - 1, 1)));
  }
  const months = ['Jan', 'Feb', 'Mär', 'Apr', 'Mai', 'Jun', 'Jul', 'Aug', 'Sep', 'Okt', 'Nov', 'Dez'];
  return months[month - 1] || '';
}

export function getMonthLabelFull(month: number, locale?: string): string {
  if (locale && Number.isInteger(month) && month >= 1 && month <= 12) {
    return new Intl.DateTimeFormat(locale, { month: 'long', timeZone: 'UTC' }).format(new Date(Date.UTC(2024, month - 1, 1)));
  }
  const months = [
    'Januar', 'Februar', 'März', 'April', 'Mai', 'Juni',
    'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember'
  ];
  return months[month - 1] || '';
}

// Color utilities
export function getTrendColor(value: number): string {
  if (value > 0) return 'text-success';
  if (value < 0) return 'text-danger';
  return 'text-text-secondary';
}

// Vier Stufen, vier Farben. Die Marken-Palette kennt nur success/warning/danger,
// deshalb liegt die Orange-Stufe als Zwischenton zwischen warning und danger.
const SCORE_GOOD = '#00FF88'; // success
const SCORE_OK = '#E8A33D'; // warning
const SCORE_WEAK = '#D98A33'; // Zwischenton warning→danger
const SCORE_BAD = '#FF5A3C'; // danger

export function getScoreColor(score: number): string {
  if (score >= 80) return SCORE_GOOD;
  if (score >= 60) return SCORE_OK;
  if (score >= 40) return SCORE_WEAK;
  return SCORE_BAD;
}

export function getHeatmapColor(value: number, max: number): string {
  if (!max || !value) return 'rgba(0, 217, 255, 0.05)';
  const intensity = Math.min(value / max, 1);
  return `rgba(0, 217, 255, ${0.1 + intensity * 0.7})`;
}

export function getRetentionColor(retention: number): string {
  if (retention >= 70) return SCORE_GOOD;
  if (retention >= 50) return SCORE_OK;
  if (retention >= 30) return SCORE_WEAK;
  return SCORE_BAD;
}

export function getHoldTone(holdPct: number): 'good' | 'ok' | 'bad' {
  if (holdPct >= 60) return 'good';
  if (holdPct >= 40) return 'ok';
  return 'bad';
}

export function getHoldColor(holdPct: number): string {
  switch (getHoldTone(holdPct)) {
    case 'good':
      return SCORE_GOOD;
    case 'ok':
      return SCORE_OK;
    default:
      return SCORE_BAD;
  }
}
