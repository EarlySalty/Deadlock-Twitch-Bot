import type { CalendarHeatmapData } from '../types/analytics';
import { MAX_ANALYTICS_DAYS } from './zeitraum';

export type CalendarMetric = 'hoursWatched' | 'streamCount';

export interface CalendarCell {
  key: string;
  startDate: string;
  endDate: string;
  streamCount: number;
  hoursWatched: number;
}

interface MonthLabel {
  month: number;
  weekIndex: number;
  weekSpan: number;
}

export interface CalendarHeatmapModel {
  days: number;
  resolution: 'day' | 'month';
  startDate: string;
  endDate: string;
  totalStreams: number;
  maxValue: number;
  weeks: (CalendarCell | null)[][];
  monthLabels: MonthLabel[];
  years: { year: number; months: (CalendarCell | null)[] }[];
}

const DAY_MS = 86_400_000;
const dateKey = (date: Date) => date.toISOString().slice(0, 10);

// The API returns date-only keys. Use UTC calendar arithmetic throughout so
// browser time zones and daylight-saving changes cannot move or repeat a day.
export function buildCalendarHeatmap(
  data: CalendarHeatmapData[],
  requestedDays: number,
  metric: CalendarMetric,
  now = new Date(),
): CalendarHeatmapModel {
  const days = Number.isFinite(requestedDays)
    ? Math.min(MAX_ANALYTICS_DAYS, Math.max(1, Math.trunc(requestedDays)))
    : 365;
  const end = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const start = new Date(end.getTime() - (days - 1) * DAY_MS);
  const startDate = dateKey(start);
  const endDate = dateKey(end);
  const resolution = days > 366 ? 'month' : 'day';
  const values = new Map<string, { streamCount: number; hoursWatched: number }>();
  let totalStreams = 0;

  for (const item of data) {
    if (item.date < startDate || item.date > endDate) continue;
    const key = resolution === 'month' ? item.date.slice(0, 7) : item.date;
    const previous = values.get(key);
    values.set(key, {
      streamCount: (previous?.streamCount ?? 0) + item.streamCount,
      hoursWatched: (previous?.hoursWatched ?? 0) + item.hoursWatched,
    });
    totalStreams += item.streamCount;
  }

  let maxValue = 0;
  const cell = (key: string, first: string, last = first): CalendarCell => {
    const value = values.get(key) ?? { streamCount: 0, hoursWatched: 0 };
    maxValue = Math.max(maxValue, value[metric]);
    return { key, startDate: first, endDate: last, ...value };
  };

  const weeks: CalendarHeatmapModel['weeks'] = [];
  const years: CalendarHeatmapModel['years'] = [];
  const monthLabels: MonthLabel[] = [];

  if (resolution === 'month') {
    // At most 11 year rows / 132 cells for the supported ten-year window,
    // instead of thousands of animated days squeezed into 522 columns.
    for (let year = end.getUTCFullYear(); year >= start.getUTCFullYear(); year--) {
      const months = Array.from({ length: 12 }, (_, month) => {
        const first = dateKey(new Date(Date.UTC(year, month, 1)));
        const last = dateKey(new Date(Date.UTC(year, month + 1, 0)));
        if (last < startDate || first > endDate) return null;
        return cell(first.slice(0, 7), first < startDate ? startDate : first, last > endDate ? endDate : last);
      });
      years.push({ year, months });
    }
  } else {
    const firstSunday = start.getTime() - start.getUTCDay() * DAY_MS;
    const count = Math.ceil((start.getUTCDay() + days) / 7);
    const labels: { month: number; weekIndex: number }[] = [];
    let lastMonth = '';

    for (let weekIndex = 0; weekIndex < count; weekIndex++) {
      const week = Array.from({ length: 7 }, (_, dayIndex) => {
        const date = new Date(firstSunday + (weekIndex * 7 + dayIndex) * DAY_MS);
        const key = dateKey(date);
        return key < startDate || key > endDate ? null : cell(key, key);
      });
      weeks.push(week);
      const first = week.find(day => day !== null);
      if (first && first.key.slice(0, 7) !== lastMonth) {
        labels.push({ month: Number(first.key.slice(5, 7)), weekIndex });
        lastMonth = first.key.slice(0, 7);
      }
    }
    for (let index = 0; index < labels.length; index++) {
      const label = labels[index];
      const weekSpan = (labels[index + 1]?.weekIndex ?? count) - label.weekIndex;
      if (weekSpan >= 3) monthLabels.push({ ...label, weekSpan });
    }
  }

  return { days, resolution, startDate, endDate, totalStreams, maxValue, weeks, monthLabels, years };
}
