import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { AlertCircle, Loader2, MessageSquare, Search, Users } from 'lucide-react';
import { useViewerTimeline } from '@/hooks/useAnalytics';
import { SEGMENT_CONFIG } from '@/pages/Viewers';
import { formatDateFull, formatDuration, formatNumber } from '@/utils/formatters';

interface ViewerTimelineProps {
  sessionId: number;
  streamer: string;
  sessionStart: string;
  sessionDurationMin: number;
  onBack?: () => void;
}

const SEGMENT_OPTIONS = [
  { value: 'all', label: 'Alle Segmente' },
  { value: 'dedicated', label: 'Dedicated' },
  { value: 'regular', label: 'Regular' },
  { value: 'casual', label: 'Casual' },
  { value: 'lurker', label: 'Lurker' },
  { value: 'new', label: 'Neu' },
] as const;

function formatPresenceMinutes(minutes: number): string {
  const safeMinutes = Math.max(0, Math.round(minutes));
  const hours = Math.floor(safeMinutes / 60);
  const restMinutes = safeMinutes % 60;
  if (hours <= 0) {
    return `${restMinutes}m`;
  }
  return `${hours}h ${restMinutes}m`;
}

export function ViewerTimeline({
  sessionId,
  streamer,
  sessionStart,
  sessionDurationMin,
}: ViewerTimelineProps) {
  const [minPresentMin, setMinPresentMin] = useState(5);
  const [segment, setSegment] = useState('all');
  const [search, setSearch] = useState('');
  const [limit, setLimit] = useState(200);

  useEffect(() => {
    setLimit(200);
  }, [sessionId, minPresentMin, segment, search]);

  const { data, isLoading, error } = useViewerTimeline(streamer, sessionId, {
    minPresentMin,
    segment,
    search,
    limit,
  });

  const chartDurationMin = Math.max(
    1,
    data?.session_duration_min ?? sessionDurationMin ?? 1
  );
  const viewers = data?.viewers ?? [];
  const totalTracked = data?.total_unique_tracked ?? 0;
  const subtitleDate = sessionStart ? formatDateFull(sessionStart) : '-';

  const stats = useMemo(() => {
    const totalMinutes = viewers.reduce((sum, viewer) => sum + viewer.total_present_min, 0);
    return {
      listed: viewers.length,
      avgMinutes: viewers.length > 0 ? Math.round(totalMinutes / viewers.length) : 0,
    };
  }, [viewers]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-card rounded-xl border border-border p-6 flex items-center gap-3 text-danger">
        <AlertCircle className="w-5 h-5 shrink-0" />
        <span>{(error as Error).message || 'Viewer-Timeline konnte nicht geladen werden.'}</span>
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <h3 className="text-white font-semibold text-lg">Viewer-Timeline</h3>
            <p className="text-sm text-text-secondary mt-1">
              {subtitleDate} · {formatDuration(chartDurationMin * 60)} · {formatNumber(totalTracked)} getrackte Viewer
            </p>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-2">
              <div className="text-xs text-text-secondary">Gelistet</div>
              <div className="text-lg font-semibold text-white">{formatNumber(stats.listed)}</div>
            </div>
            <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-2">
              <div className="text-xs text-text-secondary">Ø Presence</div>
              <div className="text-lg font-semibold text-white">{formatPresenceMinutes(stats.avgMinutes)}</div>
            </div>
            <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-2">
              <div className="text-xs text-text-secondary">Min. Filter</div>
              <div className="text-lg font-semibold text-white">{formatPresenceMinutes(minPresentMin)}</div>
            </div>
            <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-2">
              <div className="text-xs text-text-secondary">Session</div>
              <div className="text-lg font-semibold text-white">#{sessionId}</div>
            </div>
          </div>
        </div>
      </motion.div>

      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        className="bg-card rounded-xl border border-border p-5 space-y-4"
      >
        <div className="grid grid-cols-1 gap-3 lg:grid-cols-[220px_180px_minmax(0,1fr)_120px]">
          <label className="space-y-2">
            <span className="text-xs uppercase tracking-wide text-text-secondary">Min. Presence</span>
            <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-3">
              <input
                type="range"
                min={0}
                max={chartDurationMin}
                step={1}
                value={minPresentMin}
                onChange={(event) => setMinPresentMin(Number(event.target.value))}
                className="w-full accent-primary"
              />
              <input
                type="number"
                min={0}
                max={chartDurationMin}
                value={minPresentMin}
                onChange={(event) => setMinPresentMin(Math.max(0, Math.min(chartDurationMin, Number(event.target.value) || 0)))}
                className="mt-3 w-full rounded-lg border border-white/10 bg-black/20 px-3 py-2 text-sm text-white outline-none"
              />
            </div>
          </label>

          <label className="space-y-2">
            <span className="text-xs uppercase tracking-wide text-text-secondary">Segment</span>
            <select
              value={segment}
              onChange={(event) => setSegment(event.target.value)}
              className="rounded-xl border border-white/10 bg-background/40 px-3 py-3 text-sm text-white outline-none"
            >
              {SEGMENT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>

          <label className="space-y-2">
            <span className="text-xs uppercase tracking-wide text-text-secondary">Suche</span>
            <div className="flex items-center gap-2 rounded-xl border border-white/10 bg-background/40 px-3 py-3">
              <Search className="w-4 h-4 text-text-secondary" />
              <input
                type="text"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder="Viewer-Login suchen"
                className="w-full bg-transparent text-sm text-white outline-none placeholder:text-text-secondary"
              />
            </div>
          </label>

          <div className="rounded-xl border border-white/10 bg-background/40 px-3 py-3 flex flex-col justify-between">
            <div className="text-xs uppercase tracking-wide text-text-secondary">Sichtbar</div>
            <div className="text-lg font-semibold text-white">{formatNumber(viewers.length)}</div>
            <div className="text-xs text-text-secondary">
              von {formatNumber(totalTracked)} Treffern
            </div>
          </div>
        </div>

        {viewers.length === 0 ? (
          <div className="rounded-xl border border-dashed border-white/10 bg-background/30 px-6 py-12 text-center">
            <Users className="w-10 h-10 text-text-secondary mx-auto mb-3" />
            <p className="text-white font-medium">Keine Viewer im aktuellen Filter</p>
            <p className="text-sm text-text-secondary mt-1">
              Passe Presence-Minuten, Segment oder Suche an.
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="grid grid-cols-[minmax(220px,280px)_minmax(0,1fr)] gap-4 px-2 text-xs uppercase tracking-wide text-text-secondary">
              <div>Viewer</div>
              <div className="flex justify-between">
                <span>Timeline</span>
                <span>{formatPresenceMinutes(chartDurationMin)}</span>
              </div>
            </div>

            <div className="space-y-2">
              {viewers.map((viewer, index) => {
                const segmentConfig = viewer.segment
                  ? SEGMENT_CONFIG[viewer.segment]
                  : null;

                return (
                  <motion.div
                    key={viewer.login}
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: Math.min(index * 0.01, 0.2) }}
                    className="group relative grid grid-cols-[minmax(220px,280px)_minmax(0,1fr)] gap-4 rounded-xl border border-white/8 bg-background/35 p-3"
                  >
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="truncate font-medium text-white">{viewer.login}</span>
                        {segmentConfig && (
                          <span
                            className={`px-2 py-0.5 rounded-full text-[11px] font-semibold border ${segmentConfig.bgClass}`}
                          >
                            {segmentConfig.label}
                          </span>
                        )}
                      </div>
                      <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-text-secondary">
                        <span className="flex items-center gap-1">
                          <MessageSquare className="w-3.5 h-3.5" />
                          {formatNumber(viewer.chat_messages)} Messages
                        </span>
                        <span>{formatPresenceMinutes(viewer.total_present_min)} online</span>
                      </div>
                    </div>

                    <div className="relative">
                      <div className="relative h-12 rounded-lg border border-white/10 bg-black/20 overflow-hidden">
                        <div className="absolute inset-0 bg-[linear-gradient(to_right,rgba(255,255,255,0.07)_1px,transparent_1px)] bg-[size:10%_100%]" />
                        {viewer.spans.map((span, spanIndex) => {
                          const leftPct = (span.start_min / chartDurationMin) * 100;
                          const widthPct = Math.max(
                            ((span.end_min - span.start_min) / chartDurationMin) * 100,
                            0.8
                          );
                          return (
                            <div
                              key={`${viewer.login}-${spanIndex}`}
                              className="absolute top-2 bottom-2 rounded-md shadow-[0_0_0_1px_rgba(255,255,255,0.12)]"
                              style={{
                                left: `${leftPct}%`,
                                width: `${widthPct}%`,
                                backgroundColor: segmentConfig?.color || '#94a3b8',
                              }}
                            />
                          );
                        })}
                      </div>

                      <div className="pointer-events-none absolute right-2 top-2 z-10 hidden min-w-[220px] rounded-xl border border-white/10 bg-slate-950/95 px-3 py-2 text-xs text-white shadow-2xl group-hover:block">
                        <div className="font-semibold">{viewer.login}</div>
                        <div className="mt-1 text-text-secondary">
                          Presence: {formatPresenceMinutes(viewer.total_present_min)}
                        </div>
                        <div className="text-text-secondary">
                          Segment: {segmentConfig?.label || 'Unbekannt'}
                        </div>
                        <div className="text-text-secondary">
                          Messages: {formatNumber(viewer.chat_messages)}
                        </div>
                      </div>
                    </div>
                  </motion.div>
                );
              })}
            </div>

            {viewers.length < totalTracked && (
              <div className="flex justify-center pt-2">
                <button
                  onClick={() => setLimit((current) => current + 200)}
                  className="rounded-xl border border-white/10 bg-background/50 px-4 py-2 text-sm font-medium text-white transition hover:bg-background/80"
                >
                  Mehr laden
                </button>
              </div>
            )}
          </div>
        )}
      </motion.div>
    </div>
  );
}

export default ViewerTimeline;
