import { useLanguage } from '../../context/LanguageContext';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Rise } from '../../motion/Rise';
import type { CalendarHeatmapData } from '@/types/analytics';
import { getHeatmapColor, formatHours, formatNumber, getMonthLabel } from '@/utils/formatters';
import { buildCalendarHeatmap, type CalendarCell, type CalendarMetric } from '@/utils/calendarHeatmap';

interface CalendarHeatmapProps {
  data: CalendarHeatmapData[];
  title?: string;
  metric?: CalendarMetric;
  days?: number;
}

export function CalendarHeatmap({
  data,
  title = 'Stream-Aktivität',
  metric = 'hoursWatched',
  days = 365,
}: CalendarHeatmapProps) {
  const { t, locale } = useLanguage();
  const dayLabel = new Intl.DateTimeFormat(locale, { day: '2-digit', month: 'short', year: 'numeric', timeZone: 'UTC' });
  const monthLabel = new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric', timeZone: 'UTC' });
  const formatDay = (key: string) => dayLabel.format(new Date(`${key}T00:00:00Z`));
  const today = new Date().toISOString().slice(0, 10);
  const model = useMemo(
    () => buildCalendarHeatmap(data, days, metric, new Date(`${today}T00:00:00Z`)),
    [data, days, metric, today],
  );
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const monthly = model.resolution === 'month';
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Start at the recent end; refetches must not reset a user's scroll position.
    const scroll = scrollRef.current;
    if (scroll) scroll.scrollLeft = scroll.scrollWidth;
  }, [model.days, model.endDate, monthly]);
  const cells = monthly ? model.years.flatMap(year => year.months) : model.weeks.flat();
  const selected = cells.find(cell => cell?.key === selectedKey);
  const columns = `repeat(${model.weeks.length}, minmax(12px, 1fr))`;
  const yearColumns = '2.75rem repeat(12, minmax(24px, 1fr))';

  const describe = (cell: CalendarCell) => {
    const date = monthly
      ? t('{month} ({start} bis {end})', { month: monthLabel.format(new Date(`${cell.key}-01T00:00:00Z`)), start: formatDay(cell.startDate), end: formatDay(cell.endDate) })
      : formatDay(cell.startDate);
    return t('{date} · {hours} Zuschauerzeit · {count} Streams', { date, hours: formatHours(cell.hoursWatched, locale), count: formatNumber(cell.streamCount, 0, locale) });
  };

  const renderCell = (cell: CalendarCell | null, index: number) => {
    if (!cell) return <span key={`empty-${index}`} aria-hidden="true" />;
    const label = describe(cell);
    return (
      <button
        key={cell.key}
        type="button"
        data-calendar-cell={cell.key}
        data-stream-count={cell.streamCount}
        className="block w-full min-w-0 rounded-sm cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-accent hover:ring-1 hover:ring-white/50"
        style={{ backgroundColor: getHeatmapColor(cell[metric], model.maxValue) }}
        aria-label={label}
        title={label}
        onMouseEnter={() => setSelectedKey(cell.key)}
        onFocus={() => setSelectedKey(cell.key)}
        onClick={() => setSelectedKey(cell.key)}
        onKeyDown={event => { if (event.key === 'Escape') setSelectedKey(null); }}
      />
    );
  };

  return (
    <Rise
      data-calendar-heatmap=""
      data-resolution={model.resolution}
      className="bg-card rounded-xl border border-border p-5 h-full min-w-0 flex flex-col"
    >
      <h3 className="text-lg font-bold text-white">{t(title)}</h3>
      <p className="text-xs text-text-secondary mt-1 mb-4">
        {t(monthly ? 'Monatsansicht · {start} bis {end}' : 'Tagesansicht · {start} bis {end}', { start: formatDay(model.startDate), end: formatDay(model.endDate) })}
      </p>

      <div ref={scrollRef} className="min-w-0 overflow-x-auto shrink-0 pb-1" data-calendar-scroll="">
        {monthly ? (
          <div className="min-w-[400px]">
            <div className="grid gap-1 mb-2 text-xs text-text-secondary" style={{ gridTemplateColumns: yearColumns }}>
              <span>{t("Jahr")}</span>
              {Array.from({ length: 12 }, (_, month) => (
                <span key={month} className="text-center">{getMonthLabel(month + 1, locale)}</span>
              ))}
            </div>
            <div className="space-y-1">
              {model.years.map(({ year, months }) => (
                <div
                  key={year}
                  data-calendar-year={year}
                  className="grid gap-1 h-6"
                  style={{ gridTemplateColumns: yearColumns }}
                >
                  <span className="text-xs text-text-secondary self-center tabular-nums">{year}</span>
                  {months.map(renderCell)}
                </div>
              ))}
            </div>
          </div>
        ) : (
          <div style={{ minWidth: model.weeks.length * 12 + (model.weeks.length - 1) * 4 }}>
            <div className="mb-1 grid gap-1 h-4" style={{ gridTemplateColumns: columns }}>
              {model.monthLabels.map(({ month, weekIndex, weekSpan }) => (
                <div
                  key={weekIndex}
                  className="text-xs text-text-secondary whitespace-nowrap"
                  style={{ gridColumn: `${weekIndex + 1} / span ${weekSpan}` }}
                >
                  {getMonthLabel(month, locale)}
                </div>
              ))}
            </div>
            <div
              className="grid gap-1"
              style={{
                gridTemplateColumns: columns,
                gridTemplateRows: 'repeat(7, 1.75rem)',
                gridAutoFlow: 'column',
              }}
            >
              {model.weeks.flat().map(renderCell)}
            </div>
          </div>
        )}
      </div>


      <p className="text-xs text-text-secondary min-h-10 pt-2" data-calendar-details="">
        {selected ? describe(selected) : t(monthly ? 'Für Details einen Monat auswählen.' : 'Für Details einen Tag auswählen.')}
      </p>
      <div className="flex flex-wrap items-center justify-between gap-3 mt-3 text-xs text-text-secondary">
        <span>{t('{count} Streams in den letzten {days} Tagen', { count: formatNumber(model.totalStreams, 0, locale), days: model.days })}</span>
        <div className="flex items-center gap-2">
          <span>{t("Weniger")}</span>
          <div className="flex gap-1">
            {[0, 0.25, 0.5, 0.75, 1].map(intensity => (
              <div
                key={intensity}
                className="w-3 h-3 rounded-sm"
                style={{ backgroundColor: getHeatmapColor(intensity, 1) }}
              />
            ))}
          </div>
          <span>{t("Mehr")}</span>
        </div>
      </div>
    </Rise>
  );
}
