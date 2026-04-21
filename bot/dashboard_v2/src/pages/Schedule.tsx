import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { Clock, Calendar, Zap, TrendingUp, AlertCircle, Loader2, Star, Crown, Users, Play } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { fetchHourlyHeatmap, fetchWeekdayStats } from '@/api/analytics';
import type { HourlyHeatmapData, WeekdayStats, TimeRange } from '@/types/analytics';

interface ScheduleProps {
  streamer: string;
  days: TimeRange;
}

export function Schedule({ streamer, days }: ScheduleProps) {
  const { data: heatmapData, isLoading: loadingHeatmap } = useQuery<HourlyHeatmapData[]>({
    queryKey: ['hourlyHeatmap', streamer, days],
    queryFn: () => fetchHourlyHeatmap(streamer, days),
    enabled: true,
  });

  const { data: weeklyData, isLoading: loadingWeekly } = useQuery<WeekdayStats[]>({
    queryKey: ['weeklyStats', streamer, days],
    queryFn: () => fetchWeekdayStats(streamer, days),
    enabled: true,
  });

  // Find optimal times
  const analysis = useMemo(() => {
    if (!heatmapData || heatmapData.length === 0) return null;

    const sorted = [...heatmapData].sort((a, b) => b.avgViewers - a.avgViewers);
    const bestSlots = sorted.slice(0, 5);
    const worstSlots = sorted.slice(-5).reverse();

    // Group by weekday
    const byWeekday = new Map<number, HourlyHeatmapData[]>();
    heatmapData.forEach(d => {
      if (!byWeekday.has(d.weekday)) byWeekday.set(d.weekday, []);
      byWeekday.get(d.weekday)!.push(d);
    });

    // Find best time per day
    const bestPerDay: { weekday: number; hour: number; viewers: number }[] = [];
    byWeekday.forEach((slots, weekday) => {
      const best = slots.reduce((a, b) => a.avgViewers > b.avgViewers ? a : b);
      bestPerDay.push({ weekday, hour: best.hour, viewers: best.avgViewers });
    });

    return { bestSlots, worstSlots, bestPerDay };
  }, [heatmapData]);

  if (loadingHeatmap || loadingWeekly) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!heatmapData || heatmapData.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Keine Zeitplandaten verfügbar</p>
        <p className="text-text-secondary text-sm mt-2">Streame mehr, um Daten zu sammeln!</p>
      </div>
    );
  }

  const weekdayNames = ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'];
  const weekdayShort = ['So', 'Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa'];

  return (
    <div className="space-y-6">
      {/* Best Time Recommendations */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="bg-gradient-to-r from-primary/20 to-accent/20 rounded-xl border border-primary/30 p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Star className="w-6 h-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Optimale Streaming-Zeiten</h2>
        </div>

        {analysis && (
          <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
            {analysis.bestSlots.map((slot, i) => (
              <motion.div
                key={`${slot.weekday}-${slot.hour}`}
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ delay: i * 0.1 }}
                className={`p-4 rounded-lg ${i === 0 ? 'bg-gradient-to-br from-warning/20 to-warning/5 border border-warning/30' : 'bg-background/50'}`}
              >
                <div className="flex items-center justify-between mb-2">
                  <span className={`text-sm font-medium ${i === 0 ? 'text-warning' : 'text-text-secondary'}`}>
                    #{i + 1}
                  </span>
                  {i === 0 && <Star className="w-4 h-4 text-warning" />}
                </div>
                <div className="text-lg font-bold text-white">
                  {weekdayShort[slot.weekday]} {slot.hour}:00
                </div>
                <div className="text-sm text-text-secondary">
                  Ø {Math.round(slot.avgViewers)} Viewer
                </div>
              </motion.div>
            ))}
          </div>
        )}
      </motion.div>

      {weeklyData && weeklyData.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.15 }}
          className="bg-card rounded-xl border border-border p-6"
        >
          <div className="flex items-center gap-3 mb-6">
            <Calendar className="w-6 h-6 text-accent" />
            <h2 className="text-xl font-bold text-white">Wochentags-Analyse</h2>
          </div>
          <WeekdayCards data={weeklyData} />
        </motion.div>
      )}

      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2 }}
        className="bg-card rounded-xl border border-border p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Zap className="w-6 h-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Tipps zur Zeitplanung</h2>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {analysis && analysis.bestSlots[0] && (
            <InsightCard
              type="positive"
              title="Beste Streaming-Zeit"
              text={`${weekdayNames[analysis.bestSlots[0].weekday]} um ${analysis.bestSlots[0].hour}:00 Uhr bringt die meisten Viewer (Ø ${Math.round(analysis.bestSlots[0].avgViewers)}).`}
            />
          )}
          {analysis && analysis.worstSlots[0] && (
            <InsightCard
              type="warning"
              title="Zu vermeiden"
              text={`${weekdayNames[analysis.worstSlots[0].weekday]} um ${analysis.worstSlots[0].hour}:00 Uhr zeigt weniger Performance (Ø ${Math.round(analysis.worstSlots[0].avgViewers)} Viewer).`}
            />
          )}
          {weeklyData && generateScheduleInsights(weeklyData).map((insight, i) => (
            <InsightCard
              key={i}
              type={insight.priority === 'high' ? 'positive' : 'info'}
              title={insight.title}
              text={insight.text}
            />
          ))}
        </div>
      </motion.div>

      {/* Hourly Heatmap (7x24) */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.3 }}
        className="bg-card rounded-xl border border-border p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Clock className="w-6 h-6 text-primary" />
          <h2 className="text-xl font-bold text-white">Stunden-Heatmap</h2>
        </div>

        <div className="overflow-x-auto">
          <HeatmapGrid data={heatmapData} weekdayNames={weekdayShort} />
        </div>

        {/* Legend */}
        <div className="flex items-center justify-end gap-2 mt-4 text-xs text-text-secondary">
          <span>Weniger Viewer</span>
          <div className="flex gap-0.5">
            {[0.1, 0.3, 0.5, 0.7, 0.9].map(intensity => (
              <div
                key={intensity}
                className="w-4 h-4 rounded"
                style={{ backgroundColor: `rgba(124, 58, 237, ${intensity})` }}
              />
            ))}
          </div>
          <span>Mehr Viewer</span>
        </div>
      </motion.div>

      {/* Best Time Per Day */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.4 }}
        className="bg-card rounded-xl border border-border p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Calendar className="w-6 h-6 text-accent" />
          <h2 className="text-xl font-bold text-white">Beste Zeit pro Tag</h2>
        </div>

        {analysis && (
          <div className="grid grid-cols-1 md:grid-cols-7 gap-3">
            {analysis.bestPerDay
              .sort((a, b) => a.weekday - b.weekday)
              .map((day, i) => {
                const weekdayStats = weeklyData?.find(w => w.weekday === day.weekday);
                return (
                  <motion.div
                    key={day.weekday}
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: 0.3 + i * 0.05 }}
                    className="p-4 bg-background rounded-lg text-center"
                  >
                    <div className="text-sm text-text-secondary mb-2">{weekdayNames[day.weekday]}</div>
                    <div className="text-2xl font-bold text-white">{day.hour}:00</div>
                    <div className="text-xs text-primary mt-1">Ø {Math.round(day.viewers)} Viewer</div>
                    {weekdayStats && (
                      <div className="text-xs text-text-secondary mt-2">
                        {weekdayStats.streamCount} Streams
                      </div>
                    )}
                  </motion.div>
                );
              })}
          </div>
        )}
      </motion.div>
    </div>
  );
}

interface HeatmapGridProps {
  data: HourlyHeatmapData[];
  weekdayNames: string[];
}

function HeatmapGrid({ data, weekdayNames }: HeatmapGridProps) {
  const maxViewers = Math.max(...data.map(d => d.avgViewers), 1);

  // Create lookup map
  const dataMap = new Map<string, HourlyHeatmapData>();
  data.forEach(d => dataMap.set(`${d.weekday}-${d.hour}`, d));

  const hours = Array.from({ length: 24 }, (_, i) => i);

  return (
    <div className="min-w-[700px]">
      {/* Hour labels */}
      <div className="flex ml-12 mb-2">
        {hours.filter(h => h % 3 === 0).map(hour => (
          <div
            key={hour}
            className="text-xs text-text-secondary"
            style={{ width: `${100 / 8}%` }}
          >
            {hour}:00
          </div>
        ))}
      </div>

      {/* Grid */}
      {Array.from({ length: 7 }, (_, weekday) => (
        <div key={weekday} className="flex items-center mb-1">
          <div className="w-12 text-xs text-text-secondary">{weekdayNames[weekday]}</div>
          <div className="flex flex-1 gap-0.5">
            {hours.map(hour => {
              const cell = dataMap.get(`${weekday}-${hour}`);
              const intensity = cell ? cell.avgViewers / maxViewers : 0;
              const hasData = cell && cell.streamCount > 0;

              return (
                <motion.div
                  key={hour}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ delay: (weekday * 24 + hour) * 0.002 }}
                  className="flex-1 h-8 rounded-sm relative group cursor-pointer"
                  style={{
                    backgroundColor: hasData
                      ? `rgba(124, 58, 237, ${0.1 + intensity * 0.9})`
                      : 'rgba(55, 65, 81, 0.3)',
                  }}
                >
                  {/* Tooltip */}
                  <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 opacity-0 group-hover:opacity-100 transition-opacity z-20 pointer-events-none">
                    <div className="bg-card border border-border rounded px-2 py-1 text-xs whitespace-nowrap shadow-xl">
                      <div className="font-medium text-white">
                        {weekdayNames[weekday]} {hour}:00
                      </div>
                      {cell ? (
                        <>
                          <div className="text-text-secondary">
                            Ø {Math.round(cell.avgViewers)} Viewer
                          </div>
                          <div className="text-text-secondary">
                            {cell.streamCount} Streams
                          </div>
                        </>
                      ) : (
                        <div className="text-text-secondary">Keine Daten</div>
                      )}
                    </div>
                  </div>
                </motion.div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

interface InsightCardProps {
  type: 'positive' | 'warning' | 'info';
  title: string;
  text: string;
}

function InsightCard({ type, title, text }: InsightCardProps) {
  const styles = {
    positive: {
      bg: 'bg-success/10',
      border: 'border-success/20',
      icon: <TrendingUp className="w-5 h-5 text-success" />,
    },
    warning: {
      bg: 'bg-warning/10',
      border: 'border-warning/20',
      icon: <AlertCircle className="w-5 h-5 text-warning" />,
    },
    info: {
      bg: 'bg-primary/10',
      border: 'border-primary/20',
      icon: <Zap className="w-5 h-5 text-primary" />,
    },
  };

  const style = styles[type];

  return (
    <div className={`p-4 rounded-lg ${style.bg} border ${style.border}`}>
      <div className="flex items-center gap-2 mb-2">
        {style.icon}
        <span className="font-medium text-white">{title}</span>
      </div>
      <p className="text-sm text-text-secondary">{text}</p>
    </div>
  );
}

function WeekdayCards({ data }: { data: WeekdayStats[] }) {
  const maxViewers = Math.max(...data.map(d => d.avgViewers), 1);
  const bestDay = data.reduce((a, b) => a.avgViewers > b.avgViewers ? a : b);

  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-3">
      {data.map((day, i) => {
        const viewerPct = (day.avgViewers / maxViewers) * 100;
        const isBest = day.weekdayLabel === bestDay.weekdayLabel;
        const hasStreams = day.streamCount > 0;

        return (
          <motion.div
            key={day.weekdayLabel}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.05 * i }}
            className={`relative p-4 rounded-xl border transition-all ${
              isBest
                ? 'bg-gradient-to-b from-accent/20 to-card border-accent/40 ring-1 ring-accent/20'
                : hasStreams
                ? 'bg-background border-border hover:border-border-hover'
                : 'bg-background/50 border-border/50 opacity-60'
            }`}
          >
            {isBest && (
              <div className="absolute -top-2.5 left-1/2 -translate-x-1/2">
                <div className="bg-accent/20 border border-accent/30 rounded-full p-1">
                  <Crown className="w-3 h-3 text-accent" />
                </div>
              </div>
            )}
            <div className={`text-center text-sm font-semibold mb-3 ${isBest ? 'text-accent' : 'text-text-secondary'}`}>
              {day.weekdayLabel}
            </div>
            <div className="h-24 flex items-end justify-center mb-3">
              <motion.div
                initial={{ height: 0 }}
                animate={{ height: `${Math.max(hasStreams ? 8 : 0, viewerPct)}%` }}
                transition={{ delay: 0.2 + i * 0.05, duration: 0.5, ease: 'easeOut' }}
                className={`w-8 rounded-t-lg ${
                  isBest
                    ? 'bg-gradient-to-t from-accent/60 to-accent'
                    : hasStreams
                    ? 'bg-gradient-to-t from-primary/40 to-primary/70'
                    : 'bg-border/30'
                }`}
              />
            </div>
            <div className="text-center">
              <div className={`text-lg font-bold ${isBest ? 'text-white' : hasStreams ? 'text-white' : 'text-text-secondary'}`}>
                {hasStreams ? Math.round(day.avgViewers) : '-'}
              </div>
              <div className="text-[10px] text-text-secondary uppercase tracking-wider">
                <Users className="w-3 h-3 inline mr-0.5 -mt-0.5" />
                Ø Viewer
              </div>
            </div>
            <div className="mt-3 pt-3 border-t border-border/50 space-y-1.5">
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1"><Play className="w-3 h-3" />Streams</span>
                <span className="text-white font-medium">{day.streamCount}</span>
              </div>
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1"><Clock className="w-3 h-3" />Ø Dauer</span>
                <span className="text-white font-medium">{day.avgHours > 0 ? `${day.avgHours.toFixed(1)}h` : '-'}</span>
              </div>
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1"><TrendingUp className="w-3 h-3" />Peak</span>
                <span className="text-white font-medium">{day.avgPeak > 0 ? Math.round(day.avgPeak) : '-'}</span>
              </div>
            </div>
          </motion.div>
        );
      })}
    </div>
  );
}

function generateScheduleInsights(data: WeekdayStats[]): Array<{ priority: string; title: string; text: string }> {
  const insights = [];
  const sorted = [...data].sort((a, b) => b.avgViewers - a.avgViewers);
  const best = sorted[0];
  const underperforming = data.filter(d => d.streamCount > 0 && d.avgViewers < sorted[Math.floor(sorted.length / 2)]?.avgViewers);

  insights.push({
    priority: 'high',
    title: `Fokus auf ${best.weekdayLabel}`,
    text: `${best.weekdayLabel} zeigt die besten Viewer-Zahlen (Ø ${Math.round(best.avgViewers)}). Plane wichtige Content-Events an diesem Tag.`,
  });

  if (underperforming.length > 0) {
    insights.push({
      priority: 'medium',
      title: 'Optimierungspotential',
      text: `${underperforming.map(d => d.weekdayLabel).join(', ')} haben unterdurchschnittliche Performance. Experimentiere mit anderen Zeiten.`,
    });
  }

  const noStreams = data.filter(d => d.streamCount === 0);
  if (noStreams.length > 0) {
    insights.push({
      priority: 'medium',
      title: 'Ungenutzte Tage',
      text: `Keine Streams an ${noStreams.map(d => d.weekdayLabel).join(', ')}. Teste diese Slots!`,
    });
  }

  return insights;
}

export default Schedule;
