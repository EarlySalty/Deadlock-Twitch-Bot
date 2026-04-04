import { useState } from 'react';
import { motion } from 'framer-motion';
import {
  ArrowLeft,
  MessageCircle,
  Eye,
  Loader2,
  AlertCircle,
} from 'lucide-react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  ReferenceLine,
} from 'recharts';
import { useSessionDetail, useSessionEvents } from '@/hooks/useAnalytics';
import { SessionEventTimeline } from '@/components/SessionEventTimeline';
import { NoDataCard } from '@/components/cards/NoDataCard';
import { ViewerTimeline } from '@/pages/ViewerTimeline';
import { formatNumber, formatPercent, formatDuration, formatDateFull } from '@/utils/formatters';
import type { SessionEvent } from '@/types/analytics';

interface SessionDetailProps {
  sessionId: number;
  streamer: string;
  onBack: () => void;
}

// KPI tile
function KpiTile({
  label,
  value,
  color = 'text-white',
  sub,
}: {
  label: string;
  value: string;
  color?: string;
  sub?: string;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      className="bg-card rounded-xl border border-border p-4"
    >
      <p className="text-xs text-text-secondary mb-1">{label}</p>
      <p className={`text-xl font-bold ${color}`}>{value}</p>
      {sub && <p className="text-xs text-text-secondary mt-0.5">{sub}</p>}
    </motion.div>
  );
}

function retentionColor(pct: number): string {
  if (pct >= 60) return 'text-success';
  if (pct >= 40) return 'text-warning';
  return 'text-danger';
}

// Build event markers for the viewer chart
function buildEventMarkers(
  events: SessionEvent | undefined,
  sessionStart: string
): Array<{ minute: number; color: string; label: string }> {
  if (!events) return [];
  const startMs = new Date(sessionStart).getTime();
  const markers: Array<{ minute: number; color: string; label: string }> = [];

  let prevGame: string | null = null;
  let prevTitle: string | null = null;
  for (const cu of events.channel_updates) {
    const min = Math.round((new Date(cu.at).getTime() - startMs) / 60000);
    const gameChanged = prevGame !== null && cu.game !== prevGame;
    const titleChanged = prevTitle !== null && cu.title !== prevTitle;
    if (gameChanged) {
      markers.push({ minute: min, color: '#a78bfa', label: cu.game || 'Spielwechsel' });
    } else if (titleChanged) {
      markers.push({ minute: min, color: '#fbbf24', label: 'Titel' });
    }
    prevGame = cu.game;
    prevTitle = cu.title;
  }

  for (const raid of events.raids) {
    const min = Math.round((new Date(raid.at).getTime() - startMs) / 60000);
    markers.push({
      minute: min,
      color: raid.direction === 'incoming' ? '#4ade80' : '#60a5fa',
      label: `${raid.direction === 'incoming' ? 'Raid von' : 'Raid an'} ${raid.channel}`,
    });
  }

  return markers;
}

export function SessionDetail({ sessionId, streamer: _streamer, onBack }: SessionDetailProps) {
  const [activeTab, setActiveTab] = useState<'overview' | 'events' | 'viewer-timeline'>('overview');
  const { data: detail, isLoading: loadingDetail, error: detailError } = useSessionDetail(sessionId);
  const { data: events, isLoading: loadingEvents } = useSessionEvents(sessionId);

  if (loadingDetail) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (detailError || !detail) {
    return (
      <div className="space-y-4">
        <button
          onClick={onBack}
          className="flex items-center gap-2 text-text-secondary hover:text-white transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          Zuruck
        </button>
        <div className="flex flex-col items-center justify-center h-64">
          <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
          <p className="text-text-secondary text-lg">Session nicht gefunden</p>
        </div>
      </div>
    );
  }

  // detail fields from the API — cast through unknown to access camelCase keys
  const d = detail as unknown as Record<string, unknown>;
  const title = String(d.title || d.stream_title || '');
  const startedAt = String(d.startedAt || d.started_at || '');
  const duration = Number(d.duration || d.duration_seconds || 0);
  const startViewers = Number(d.startViewers || d.start_viewers || 0);
  const peakViewers = Number(d.peakViewers || d.peak_viewers || 0);
  const endViewers = Number(d.endViewers || d.end_viewers || 0);
  const avgViewers = Number(d.avgViewers || d.avg_viewers || 0);
  const retention5m = Number(d.retention5m || 0);
  const retention10m = Number(d.retention10m || 0);
  const retention20m = Number(d.retention20m || 0);
  const uniqueChatters = Number(d.uniqueChatters || d.unique_chatters || 0);
  const firstTimeChatters = Number(d.firstTimeChatters || d.first_time_chatters || 0);
  const followerDelta = Number(d.followersEnd || 0) - Number(d.followersStart || 0);
  const timeline = (d.timeline as Array<{ minute: number; viewers: number }>) || [];
  const chatters = (d.chatters as Array<{ login: string; messages: number }>) || [];

  const startDate = startedAt ? new Date(startedAt) : null;
  const dateLabel = startDate ? formatDateFull(startedAt) : '-';
  const timeLabel = startDate
    ? startDate.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' })
    : '';
  const sessionDurationMin = Math.max(0, Math.round(duration / 60));

  const eventMarkers = buildEventMarkers(events, startedAt);

  return (
    <div className="space-y-6">
      {/* Header */}
      <motion.div
        initial={{ opacity: 0, y: -10 }}
        animate={{ opacity: 1, y: 0 }}
        className="flex items-start gap-4"
      >
        <button
          onClick={onBack}
          className="mt-1 p-2 rounded-lg hover:bg-white/10 transition-colors text-text-secondary hover:text-white"
        >
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div className="flex-1 min-w-0">
          <h2 className="text-xl font-bold text-white truncate">
            {title || 'Stream Session'}
          </h2>
          <p className="text-sm text-text-secondary mt-0.5">
            {dateLabel} um {timeLabel} &middot; {formatDuration(duration)}
          </p>
        </div>
      </motion.div>

      {/* KPI Row */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-3">
        <KpiTile
          label="Start Viewer"
          value={formatNumber(startViewers)}
          color="text-white"
        />
        <KpiTile
          label="Peak Viewer"
          value={formatNumber(peakViewers)}
          color="text-accent"
        />
        <KpiTile
          label="End Viewer"
          value={formatNumber(endViewers)}
          color="text-white"
        />
        <KpiTile
          label="Ø Viewer"
          value={formatNumber(avgViewers)}
          color="text-primary"
        />
        <KpiTile
          label="Follower"
          value={followerDelta >= 0 ? `+${formatNumber(followerDelta)}` : formatNumber(followerDelta)}
          color={followerDelta >= 0 ? 'text-success' : 'text-danger'}
        />
      </div>

      {/* Retention + Chatters */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-3">
        <KpiTile
          label="5m Retention"
          value={formatPercent(retention5m)}
          color={retentionColor(retention5m)}
        />
        <KpiTile
          label="10m Retention"
          value={formatPercent(retention10m)}
          color={retentionColor(retention10m)}
        />
        <KpiTile
          label="20m Retention"
          value={formatPercent(retention20m)}
          color={retentionColor(retention20m)}
        />
        <KpiTile
          label="Unique Chatters"
          value={formatNumber(uniqueChatters)}
          color="text-white"
        />
        <KpiTile
          label="Erstchatter"
          value={formatNumber(firstTimeChatters)}
          color="text-accent"
        />
      </div>

      <div className="flex flex-wrap gap-2">
        {[
          ['overview', 'Overview'],
          ['events', 'Events & Chat'],
          ['viewer-timeline', 'Viewer-Timeline'],
        ].map(([tabId, label]) => {
          const isActive = activeTab === tabId;
          return (
            <button
              key={tabId}
              onClick={() => setActiveTab(tabId as 'overview' | 'events' | 'viewer-timeline')}
              className={`rounded-xl px-4 py-2 text-sm font-medium transition ${
                isActive
                  ? 'bg-primary text-white shadow-lg shadow-primary/20'
                  : 'border border-white/10 bg-card text-text-secondary hover:text-white'
              }`}
            >
              {label}
            </button>
          );
        })}
      </div>

      {activeTab === 'overview' && (
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="bg-card rounded-xl border border-border p-5"
        >
          <h3 className="text-white font-semibold mb-4 flex items-center gap-2">
            <Eye className="w-5 h-5 text-primary" />
            Viewer-Verlauf
          </h3>
          {timeline.length === 0 ? (
            <NoDataCard message="Keine Timeline-Daten" submessage="" />
          ) : (
            <ResponsiveContainer width="100%" height={300}>
              <LineChart data={timeline}>
                <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.06)" />
                <XAxis
                  dataKey="minute"
                  tick={{ fill: '#888', fontSize: 12 }}
                  tickFormatter={(v: number) => `${v}m`}
                />
                <YAxis tick={{ fill: '#888', fontSize: 12 }} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1e1e2e',
                    border: '1px solid rgba(255,255,255,0.1)',
                    borderRadius: '8px',
                    color: '#fff',
                  }}
                  labelFormatter={(v) => `Minute ${v}`}
                  formatter={(v) => [formatNumber(Number(v)), 'Viewer']}
                />
                <Line
                  type="monotone"
                  dataKey="viewers"
                  stroke="#7c3aed"
                  strokeWidth={2}
                  dot={false}
                  activeDot={{ r: 4 }}
                />
                {eventMarkers.map((m, i) => (
                  <ReferenceLine
                    key={`ev-${i}`}
                    x={m.minute}
                    stroke={m.color}
                    strokeDasharray="4 4"
                    strokeWidth={1.5}
                    label={{
                      value: m.label,
                      position: 'top',
                      fill: m.color,
                      fontSize: 10,
                    }}
                  />
                ))}
              </LineChart>
            </ResponsiveContainer>
          )}
        </motion.div>
      )}

      {activeTab === 'events' && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <SessionEventTimeline
            events={events}
            sessionStart={startedAt}
            loading={loadingEvents}
          />

          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.15 }}
            className="bg-card rounded-xl border border-border p-5"
          >
            <h3 className="text-white font-semibold mb-4 flex items-center gap-2">
              <MessageCircle className="w-5 h-5 text-accent" />
              Top Chatters
            </h3>
            {chatters.length === 0 ? (
              <NoDataCard message="Keine Chatter-Daten" submessage="" />
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-text-secondary text-xs uppercase tracking-wider border-b border-border">
                      <th className="text-left px-3 py-2">Chatter</th>
                      <th className="text-right px-3 py-2">Nachrichten</th>
                    </tr>
                  </thead>
                  <tbody>
                    {chatters.map((c, i) => (
                      <motion.tr
                        key={c.login}
                        initial={{ opacity: 0, x: -5 }}
                        animate={{ opacity: 1, x: 0 }}
                        transition={{ delay: i * 0.03 }}
                        className="border-b border-border/50 hover:bg-white/5 transition"
                      >
                        <td className="px-3 py-2 text-white">{c.login}</td>
                        <td className="px-3 py-2 text-right text-text-secondary">
                          {formatNumber(c.messages)}
                        </td>
                      </motion.tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </motion.div>
        </div>
      )}

      {activeTab === 'viewer-timeline' && (
        <ViewerTimeline
          sessionId={sessionId}
          streamer={_streamer}
          sessionStart={startedAt}
          sessionDurationMin={sessionDurationMin}
          onBack={onBack}
        />
      )}
    </div>
  );
}

export default SessionDetail;
