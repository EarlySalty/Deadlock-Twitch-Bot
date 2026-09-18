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

const dayLabel = new Intl.DateTimeFormat('de-DE', {
  day: '2-digit', month: 'short', year: 'numeric', timeZone: 'UTC',
});
const monthLabel = new Intl.DateTimeFormat('de-DE', {
  month: 'long', year: 'numeric', timeZone: 'UTC',
});
const formatDay = (key: string) => dayLabel.format(new Date(`${key}T00:00:00Z`));

export function CalendarHeatmap({
  data,
  title = 'Stream-Aktivität',
  metric = 'hoursWatched',
  days = 365,
}: CalendarHeatmapProps) {
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
      ? `${monthLabel.format(new Date(`${cell.key}-01T00:00:00Z`))} (${formatDay(cell.startDate)} bis ${formatDay(cell.endDate)})`
      : formatDay(cell.startDate);
    return `${date} · ${formatHours(cell.hoursWatched)} Zuschauerzeit · ${formatNumber(cell.streamCount)} Streams`;
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
      <h3 className="text-lg font-bold text-white">{title}</h3>
      <p className="text-xs text-text-secondary mt-1 mb-4">
        {monthly ? 'Monatsansicht' : 'Tagesansicht'} · {formatDay(model.startDate)} bis {formatDay(model.endDate)}
      </p>

      <div ref={scrollRef} className="min-w-0 overflow-x-auto flex-1 pb-1" data-calendar-scroll="">
        {monthly ? (
          <div className="min-w-[400px]">
            <div className="grid gap-1 mb-2 text-xs text-text-secondary" style={{ gridTemplateColumns: yearColumns }}>
              <span>Jahr</span>
              {Array.from({ length: 12 }, (_, month) => (
                <span key={month} className="text-center">{getMonthLabel(month + 1)}</span>
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
                  {getMonthLabel(month)}
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

      {/* Details stay outside the scroll area, including on touch and keyboard. */}
      <p className="text-xs text-text-secondary min-h-10 pt-2" data-calendar-details="">
        {selected ? describe(selected) : `Für Details einen ${monthly ? 'Monat' : 'Tag'} auswählen.`}
      </p>
      <div className="flex flex-wrap items-center justify-between gap-3 mt-3 text-xs text-text-secondary">
        <span>{formatNumber(model.totalStreams)} Streams in den letzten {model.days} Tagen</span>
        <div className="flex items-center gap-2">
          <span>Weniger</span>
          <div className="flex gap-1">
            {[0, 0.25, 0.5, 0.75, 1].map(intensity => (
              <div
                key={intensity}
                className="w-3 h-3 rounded-sm"
                style={{ backgroundColor: getHeatmapColor(intensity, 1) }}
              />
            ))}
          </div>
          <span>Mehr</span>
        </div>
      </div>
    </Rise>
  );
}
