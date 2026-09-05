import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { Rise } from '../../motion/Rise';
import type { CalendarHeatmapData } from '@/types/analytics';
import { getHeatmapColor, formatHours, getMonthLabel } from '@/utils/formatters';

interface CalendarHeatmapProps {
  data: CalendarHeatmapData[];
  title?: string;
  metric?: 'hoursWatched' | 'streamCount';
  days?: number;
}

export function CalendarHeatmap({
  data,
  title = 'Stream-Aktivität',
  metric = 'hoursWatched',
  days = 365,
}: CalendarHeatmapProps) {
  const { weeks, maxValue, dataMap, monthLabels } = useMemo(() => {
    const map = new Map<string, CalendarHeatmapData>();
    let max = 0;

    data.forEach(d => {
      map.set(d.date, d);
      const value = metric === 'hoursWatched' ? d.hoursWatched : d.streamCount;
      if (value > max) max = value;
    });

    const weeks: Date[][] = [];
    const today = new Date();
    const startDate = new Date(today);
    startDate.setDate(startDate.getDate() - (Math.max(days, 1) - 1));

    while (startDate.getDay() !== 0) {
      startDate.setDate(startDate.getDate() - 1);
    }

    let currentWeek: Date[] = [];
    const endDate = new Date(today);

    for (let d = new Date(startDate); d <= endDate; d.setDate(d.getDate() + 1)) {
      currentWeek.push(new Date(d));
      if (currentWeek.length === 7) {
        weeks.push(currentWeek);
        currentWeek = [];
      }
    }
    if (currentWeek.length > 0) {
      weeks.push(currentWeek);
    }

    const labels: { month: number; weekIndex: number }[] = [];
    let lastMonth = -1;
    weeks.forEach((week, weekIndex) => {
      const firstOfWeek = week[0];
      if (firstOfWeek.getMonth() !== lastMonth) {
        labels.push({ month: firstOfWeek.getMonth() + 1, weekIndex });
        lastMonth = firstOfWeek.getMonth();
      }
    });

    const breiteLabels = labels.filter((label, i) => {
      const naechsterIndex = i + 1 < labels.length ? labels[i + 1].weekIndex : weeks.length;
      return naechsterIndex - label.weekIndex >= 3;
    });

    return { weeks, maxValue: max, dataMap: map, monthLabels: breiteLabels };
  }, [data, metric, days]);

  const formatDateKey = (date: Date): string => {
    return date.toISOString().split('T')[0];
  };

  return (
    <Rise
      className="bg-card rounded-xl border border-border p-5 h-full flex flex-col"
    >
      <h3 className="text-lg font-bold text-white mb-4">{title}</h3>

      <div className="flex-1">
        <div
          className="mb-1 grid"
          style={{
            gridTemplateColumns: `repeat(${weeks.length}, minmax(0, 1fr))`,
            maxWidth: `${weeks.length * 26}px`,
          }}
        >
          {monthLabels.map(({ month, weekIndex }) => (
            <div
              key={weekIndex}
              className="text-xs text-text-secondary"
              style={{ gridColumn: weekIndex + 1 }}
            >
              {getMonthLabel(month)}
            </div>
          ))}
        </div>

        <div
          className="grid gap-1"
          style={{
            gridTemplateColumns: `repeat(${weeks.length}, minmax(0, 1fr))`,
            gridTemplateRows: 'repeat(7, auto)',
            gridAutoFlow: 'column',
            maxWidth: `${weeks.length * 26}px`,
            justifyContent: 'start',
          }}
        >
          {weeks.flatMap((week, weekIndex) =>
            week.map((date, dayIndex) => {
              const dateKey = formatDateKey(date);
              const cellData = dataMap.get(dateKey);
              const value = cellData
                ? metric === 'hoursWatched'
                  ? cellData.hoursWatched
                  : cellData.streamCount
                : 0;

              return (
                <motion.div
                  key={`${weekIndex}-${dayIndex}`}
                  initial={{ opacity: 0, scale: 0.8 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{ delay: Math.min(weekIndex * 0.01, 0.24) }}
                  className="rounded-sm relative group cursor-pointer"
                  style={{
                    aspectRatio: '1',
                    maxWidth: '22px',
                    backgroundColor: getHeatmapColor(value, maxValue),
                  }}
                >
                  <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 opacity-0 group-hover:opacity-100 transition-opacity z-10 pointer-events-none">
                    <div className="bg-card border border-border rounded px-2 py-1 text-xs whitespace-nowrap shadow-xl">
                      <div className="text-white font-medium">
                        {date.toLocaleDateString('de-DE', { day: '2-digit', month: 'short', year: 'numeric' })}
                      </div>
                      {cellData ? (
                        <>
                          <div className="text-text-secondary">
                            {formatHours(cellData.hoursWatched)} watched
                          </div>
                          <div className="text-text-secondary">
                            {cellData.streamCount} Streams
                          </div>
                        </>
                      ) : (
                        <div className="text-text-secondary">Kein Stream</div>
                      )}
                    </div>
                  </div>
                </motion.div>
              );
            }),
          )}
        </div>
      </div>

      <div className="flex items-center justify-between mt-4 text-xs text-text-secondary">
        <div>
          {data.length > 0 && (
            <span>
              {data.reduce((sum, d) => sum + d.streamCount, 0)} Streams in {days} Tagen
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <span>Weniger</span>
          <div className="flex gap-1">
            {[0.1, 0.3, 0.5, 0.7, 0.9].map(intensity => (
              <div
                key={intensity}
                className="w-3 h-3 rounded-sm"
                style={{ backgroundColor: `rgba(0, 217, 255, ${intensity})` }}
              />
            ))}
          </div>
          <span>Mehr</span>
        </div>
      </div>
    </Rise>
  );
}
