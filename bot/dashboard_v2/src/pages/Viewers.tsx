import { useState, useMemo, useCallback } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  AlertCircle,
  Loader2,
  Search,
  ChevronDown,
  ChevronUp,
  UserSearch,
  Users,
  Share2,
  Lock,
  AlertTriangle,
  TrendingUp,
  TrendingDown,
  Minus,
  Clock,
  MessageSquare,
} from 'lucide-react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import { useViewerDirectory, useViewerDetail, useViewerSegments } from '@/hooks/useAnalytics';
import type {
  RawChatStatus,
  TimeRange,
  ViewerSortField,
  ViewerFilterType,
  ViewerEntry,
  SegmentData,
} from '@/types/analytics';

interface ViewersProps {
  streamer: string | null;
  days: TimeRange;
}

const SEGMENT_CONFIG: Record<string, { label: string; color: string; bgClass: string }> = {
  dedicated: { label: 'Dedicated', color: '#22c55e', bgClass: 'bg-success/10 text-success border-success/20' },
  regular: { label: 'Regular', color: '#3b82f6', bgClass: 'bg-primary/10 text-primary border-primary/20' },
  casual: { label: 'Casual', color: '#f59e0b', bgClass: 'bg-warning/10 text-warning border-warning/20' },
  lurker: { label: 'Lurker', color: '#8b5cf6', bgClass: 'bg-accent/10 text-accent border-accent/20' },
  new: { label: 'Neu', color: '#06b6d4', bgClass: 'bg-cyan-500/10 text-cyan-400 border-cyan-500/20' },
};

const FILTER_OPTIONS: { value: ViewerFilterType; label: string }[] = [
  { value: 'all', label: 'Alle' },
  { value: 'active', label: 'Aktiv' },
  { value: 'lurker', label: 'Lurker' },
  { value: 'exclusive', label: 'Exklusiv' },
  { value: 'shared', label: 'Shared' },
  { value: 'new', label: 'Neu' },
  { value: 'churned', label: 'Churned' },
];

const PERSONALITY_LABELS: Record<string, string> = {
  'Command': 'Befehlsgeber',
  'Greeting': 'Willkommener',
  'Question': 'Fragesteller',
  'Reaction': 'Reactor',
  'Game-Related': 'Game-Diskutierer',
  'Engagement': 'Engagierter',
  'Other': 'Allrounder',
};

const PERSONALITY_COLORS = ['#3b82f6', '#22c55e', '#f59e0b', '#8b5cf6', '#ef4444', '#06b6d4', '#6b7280'];

function formatNumber(n: number): string {
  return n.toLocaleString('de-DE');
}

function CategoryBadge({ category }: { category: string }) {
  const config = SEGMENT_CONFIG[category] || SEGMENT_CONFIG.casual;
  return (
    <span className={`px-2 py-0.5 rounded-full text-xs font-semibold border ${config.bgClass}`}>
      {config.label}
    </span>
  );
}

function RawChatGapNotice({
  status,
  note,
  compact = false,
}: {
  status?: RawChatStatus;
  note?: string | null;
  compact?: boolean;
}) {
  const message = note || status?.note || null;
  if (!message && !status?.suspectedIngestionIssue && status?.available !== false) {
    return null;
  }

  return (
    <div
      className={`rounded-xl border ${
        status?.suspectedIngestionIssue
          ? 'border-warning/30 bg-warning/10 text-warning'
          : 'border-white/10 bg-white/[0.04] text-text-secondary'
      } ${compact ? 'px-3 py-2 text-xs' : 'px-4 py-3 text-sm'}`}
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className={`${compact ? 'mt-0.5 h-3.5 w-3.5' : 'mt-0.5 h-4 w-4'} shrink-0`} />
        <div>
          <p className="font-medium text-white">
            {status?.suspectedIngestionIssue ? 'Roh-Chat-Lücke im Zeitraum' : 'Keine Roh-Chat-Nachrichten im Zeitraum'}
          </p>
          <p className="mt-1 leading-5">{message || 'Viewer-Daten bleiben sichtbar, message-basierte Details sind eingeschränkt.'}</p>
        </div>
      </div>
    </div>
  );
}

function SegmentCard({ name, data }: { name: string; data: SegmentData }) {
  const config = SEGMENT_CONFIG[name] || SEGMENT_CONFIG.casual;
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className="bg-card rounded-xl border border-border p-4 flex flex-col"
    >
      <div className="flex items-center justify-between mb-2">
        <span className={`px-2 py-0.5 rounded-full text-xs font-bold border ${config.bgClass}`}>
          {config.label}
        </span>
        <span className="text-xs text-text-secondary">{data.pct}%</span>
      </div>
      <div className="text-2xl font-bold text-white mb-1">{formatNumber(data.count)}</div>
      <div className="text-xs text-text-secondary space-y-0.5">
        <div>Ø {data.avgSessions.toFixed(1)} Sessions</div>
        <div>Ø {data.avgMessages.toFixed(1)} Messages</div>
      </div>
      {/* Mini progress bar */}
      <div className="mt-2 h-1.5 bg-background rounded-full overflow-hidden">
        <div
          className="h-full rounded-full transition-all"
          style={{ width: `${data.pct}%`, backgroundColor: config.color }}
        />
      </div>
    </motion.div>
  );
}

function ViewerRow({
  viewer,
  isExpanded,
  onToggle,
  streamer,
  days,
}: {
  viewer: ViewerEntry;
  isExpanded: boolean;
  onToggle: () => void;
  streamer: string;
  days: TimeRange;
}) {
  return (
    <>
      <tr
        className="border-b border-border/50 hover:bg-background/50 cursor-pointer transition-colors"
        onClick={onToggle}
      >
        <td className="py-3 px-4">
          <div className="flex items-center gap-2">
            <span className="text-white font-medium">{viewer.login}</span>
          </div>
          {viewer.presenceOnlyInWindow && (
            <div className="mt-1 flex items-center gap-1 text-xs text-warning">
              <AlertTriangle className="h-3.5 w-3.5" />
              <span>Nur Presence-Daten im gewählten Zeitraum</span>
            </div>
          )}
        </td>
        <td className="py-3 px-4 text-text-secondary">{formatNumber(viewer.totalSessions)}</td>
        <td className="py-3 px-4 text-text-secondary">{formatNumber(viewer.totalMessages)}</td>
        <td className="py-3 px-4 text-text-secondary">
          {viewer.daysSinceLastSeen === 0
            ? 'Heute'
            : viewer.daysSinceLastSeen === 1
              ? 'Gestern'
              : `vor ${viewer.daysSinceLastSeen}d`}
        </td>
        <td className="py-3 px-4 text-text-secondary">
          {viewer.otherChannels > 0 ? (
            <span className="flex items-center gap-1">
              <Share2 className="w-3 h-3" />
              {viewer.otherChannels}
            </span>
          ) : (
            <span className="flex items-center gap-1 text-success">
              <Lock className="w-3 h-3" />
              Exklusiv
            </span>
          )}
        </td>
        <td className="py-3 px-4">
          <CategoryBadge category={viewer.category} />
        </td>
        <td className="py-3 px-2">
          {isExpanded ? (
            <ChevronUp className="w-4 h-4 text-text-secondary" />
          ) : (
            <ChevronDown className="w-4 h-4 text-text-secondary" />
          )}
        </td>
      </tr>
      <AnimatePresence>
        {isExpanded && (
          <tr>
            <td colSpan={7} className="p-0">
              <ViewerExpandedRow login={viewer.login} streamer={streamer} days={days} />
            </td>
          </tr>
        )}
      </AnimatePresence>
    </>
  );
}

function ViewerExpandedRow({
  login,
  streamer,
  days,
}: {
  login: string;
  streamer: string;
  days: TimeRange;
}) {
  const { data, isLoading } = useViewerDetail(streamer, login, days);

  if (isLoading) {
    return (
      <motion.div
        initial={{ opacity: 0, height: 0 }}
        animate={{ opacity: 1, height: 'auto' }}
        exit={{ opacity: 0, height: 0 }}
        className="bg-background/30 px-6 py-4 flex items-center justify-center"
      >
        <Loader2 className="w-5 h-5 animate-spin text-primary" />
      </motion.div>
    );
  }

  if (!data) return null;

  const trendIcon =
    data.chatPatterns.messagesTrend === 'increasing' ? (
      <TrendingUp className="w-4 h-4 text-success" />
    ) : data.chatPatterns.messagesTrend === 'decreasing' ? (
      <TrendingDown className="w-4 h-4 text-error" />
    ) : (
      <Minus className="w-4 h-4 text-text-secondary" />
    );

  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: 'auto' }}
      exit={{ opacity: 0, height: 0 }}
      className="bg-background/30 border-t border-border/30 px-6 py-4"
    >
      {(data.overview.presenceOnlyInWindow || data.overview.messageGapNote) && (
        <div className="mb-4">
          <RawChatGapNotice
            status={data.rawChatStatus}
            note={data.overview.messageGapNote}
            compact
          />
        </div>
      )}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {/* Activity Timeline */}
        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Clock className="w-4 h-4 text-primary" />
            Aktivität (letzte 90 Tage)
          </h4>
          {data.activityTimeline.length > 0 ? (
            <div className="h-[120px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={data.activityTimeline.slice(-30)}>
                  <XAxis dataKey="date" hide />
                  <YAxis hide />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: '#1f2937',
                      border: '1px solid rgba(194,221,240,0.25)',
                      borderRadius: '8px',
                      fontSize: '12px',
                    }}
                    labelStyle={{ color: '#fff' }}
                  />
                  <Bar dataKey="messages" fill="var(--color-primary)" radius={[2, 2, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <p className="text-xs text-text-secondary">Keine Aktivitätsdaten</p>
          )}
        </div>

        {/* Cross-Channel Presence */}
        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Share2 className="w-4 h-4 text-accent" />
            Andere Channels
          </h4>
          {data.crossChannelPresence.length > 0 ? (
            <div className="space-y-2 max-h-[120px] overflow-y-auto">
              {data.crossChannelPresence.slice(0, 5).map(cc => (
                <div key={cc.streamer} className="flex items-center justify-between text-xs">
                  <span className="text-white font-medium truncate">{cc.streamer}</span>
                  <span className="text-text-secondary">{cc.sessions} Sessions</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-secondary">Exklusiver Viewer</p>
          )}
        </div>

        {/* Chat Patterns */}
        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <MessageSquare className="w-4 h-4 text-warning" />
            Chat-Muster
          </h4>
          <div className="space-y-2 text-xs">
            <div className="flex justify-between">
              <span className="text-text-secondary">Ø Messages/Session</span>
              <span className="text-white font-medium">{data.chatPatterns.avgMessagesPerSession}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-secondary">Aktivster Tag</span>
              <span className="text-white font-medium">{data.chatPatterns.mostActiveDay}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-secondary">Peak-Stunden</span>
              <span className="text-white font-medium">
                {data.chatPatterns.peakHours.map(h => `${h}:00`).join(', ')}
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-text-secondary">Trend</span>
              <span className="flex items-center gap-1">{trendIcon}</span>
            </div>
          </div>
        </div>
      </div>

      {/* Personality Panel */}
      {data.personality && (
        <div className="mt-4 bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Users className="w-4 h-4 text-success" />
            Personality
            <span className="px-2 py-0.5 rounded-full text-xs font-bold bg-success/10 text-success border border-success/20">
              {PERSONALITY_LABELS[data.personality.primary] || data.personality.primary}
            </span>
          </h4>
          <div className="flex items-center gap-4">
            <div className="h-[100px] w-[100px] shrink-0">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={Object.entries(data.personality.distribution).map(([k, v]) => ({
                      name: k,
                      value: v,
                    }))}
                    innerRadius={25}
                    outerRadius={42}
                    dataKey="value"
                    stroke="none"
                  >
                    {Object.keys(data.personality.distribution).map((key, idx) => (
                      <Cell key={key} fill={PERSONALITY_COLORS[idx % PERSONALITY_COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip
                    contentStyle={{
                      backgroundColor: '#1f2937',
                      border: '1px solid rgba(194,221,240,0.25)',
                      borderRadius: '8px',
                      fontSize: '12px',
                    }}
                  />
                </PieChart>
              </ResponsiveContainer>
            </div>
            <div className="flex flex-wrap gap-2 text-xs">
              {Object.entries(data.personality.distribution)
                .sort(([, a], [, b]) => b - a)
                .map(([key, val]) => {
                  const total = Object.values(data.personality!.distribution).reduce((s, v) => s + v, 0);
                  return (
                    <span key={key} className="flex items-center gap-1 text-text-secondary">
                      <span
                        className="w-2 h-2 rounded-sm"
                        style={{ backgroundColor: PERSONALITY_COLORS[Object.keys(data.personality!.distribution).indexOf(key) % PERSONALITY_COLORS.length] }}
                      />
                      {key}: {total > 0 ? Math.round(val / total * 100) : 0}%
                    </span>
                  );
                })}
            </div>
          </div>
        </div>
      )}
    </motion.div>
  );
}

export function Viewers({ streamer, days }: ViewersProps) {
  const [sort, setSort] = useState<ViewerSortField>('sessions');
  const [order, setOrder] = useState<'asc' | 'desc'>('desc');
  const [filter, setFilter] = useState<ViewerFilterType>('all');
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [page, setPage] = useState(1);
  const [expandedViewer, setExpandedViewer] = useState<string | null>(null);

  // Debounce search
  const handleSearchChange = useCallback((value: string) => {
    setSearch(value);
    setPage(1);
    const timeout = setTimeout(() => setDebouncedSearch(value), 300);
    return () => clearTimeout(timeout);
  }, []);

  const {
    data: directory,
    isLoading: loadingDirectory,
    isError: directoryError,
    refetch: refetchDirectory,
  } = useViewerDirectory(streamer, days, sort, order, filter, debouncedSearch, page);

  const {
    data: segments,
    isLoading: loadingSegments,
  } = useViewerSegments(streamer, days);

  const isLoading = loadingDirectory || loadingSegments;

  const handleSort = (field: ViewerSortField) => {
    if (sort === field) {
      setOrder(o => (o === 'desc' ? 'asc' : 'desc'));
    } else {
      setSort(field);
      setOrder('desc');
    }
    setPage(1);
  };

  const handleFilterChange = (f: ViewerFilterType) => {
    setFilter(f);
    setPage(1);
  };

  // Donut chart data for exclusive vs shared
  const donutData = useMemo(() => {
    if (!directory?.summary) return [];
    return [
      { name: 'Exklusiv', value: directory.summary.exclusiveViewers, color: '#22c55e' },
      { name: 'Shared', value: directory.summary.sharedViewers, color: '#8b5cf6' },
    ];
  }, [directory?.summary]);

  const totalPages = directory ? Math.ceil(directory.total / directory.perPage) : 1;

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Wähle einen Streamer aus</p>
      </div>
    );
  }

  if (isLoading && !directory) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (directoryError) {
    return (
      <div className="bg-error/10 border border-error/30 rounded-lg p-4 text-error">
        <div className="flex items-center gap-2 font-semibold mb-2">
          <AlertCircle className="w-5 h-5" />
          <span>Fehler beim Laden der Viewer-Daten</span>
        </div>
        <button
          onClick={() => refetchDirectory()}
          className="px-3 py-1.5 rounded-md bg-error/20 text-error text-sm font-semibold hover:bg-error/30"
        >
          Erneut laden
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {directory?.rawChatStatus && (
        <RawChatGapNotice status={directory.rawChatStatus} />
      )}

      {/* ── Segment Cards ── */}
      {segments && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="grid grid-cols-2 md:grid-cols-5 gap-3"
        >
          {['dedicated', 'regular', 'casual', 'lurker', 'new'].map(seg => (
            <SegmentCard
              key={seg}
              name={seg}
              data={segments.segments[seg] || { count: 0, pct: 0, avgMessages: 0, avgSessions: 0 }}

            />
          ))}
        </motion.div>
      )}

      {/* ── Insights Row ── */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.1 }}
        className="grid grid-cols-1 md:grid-cols-3 gap-4"
      >
        {/* Churn Risk — "Vermisst"-Liste */}
        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-error/20 flex items-center justify-center">
              <AlertTriangle className="w-5 h-5 text-error" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">Vermisste Chatter</h3>
              <p className="text-xs text-text-secondary">Aktive Viewer die fehlen</p>
            </div>
          </div>
          {segments ? (
            <div className="space-y-2">
              <div className="flex items-baseline gap-2 mb-2">
                <span className="text-2xl font-bold text-error">{segments.churnRisk.atRisk}</span>
                <span className="text-sm text-text-secondary">vermisst</span>
                <span className="text-text-secondary mx-1">·</span>
                <span className="text-sm text-text-secondary">{segments.churnRisk.recentlyChurned} abgewandert</span>
              </div>
              {segments.churnRisk.atRiskViewers.length > 0 ? (
                <div className="space-y-1.5 max-h-[140px] overflow-y-auto">
                  {segments.churnRisk.atRiskViewers.slice(0, 8).map(v => (
                    <div key={v.login} className="flex items-center justify-between text-xs">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="text-white font-medium truncate">{v.login}</span>
                        <span className="text-text-secondary shrink-0">vor {v.daysSinceLastSeen}d</span>
                      </div>
                      <div className="text-text-secondary truncate ml-2 text-right">
                        {v.recentlySeenAt && v.recentlySeenAt.length > 0
                          ? <span className="text-accent">bei {v.recentlySeenAt.slice(0, 2).join(', ')}</span>
                          : <span className="text-text-secondary/50">offline</span>}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-success">Keine vermissten Chatter</p>
              )}
            </div>
          ) : (
            <Loader2 className="w-5 h-5 animate-spin text-text-secondary" />
          )}
        </div>

        {/* Top Shared Channels */}
        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-accent/20 flex items-center justify-center">
              <Share2 className="w-5 h-5 text-accent" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">Top Shared Channels</h3>
              <p className="text-xs text-text-secondary">
                Ø {segments?.crossChannelStats.avgOtherChannels ?? directory?.summary.avgOtherChannels ?? 0} andere Channels
              </p>
            </div>
          </div>
          {segments?.crossChannelStats.topSharedChannels && segments.crossChannelStats.topSharedChannels.length > 0 ? (
            <div className="space-y-1.5">
              {segments.crossChannelStats.topSharedChannels.slice(0, 5).map(ch => (
                <div key={ch.streamer} className="flex items-center justify-between text-xs">
                  <span className="text-white font-medium truncate">{ch.streamer}</span>
                  <span className="text-text-secondary">{formatNumber(ch.sharedCount)} gemeinsam</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-secondary">Keine Shared-Channel-Daten</p>
          )}
        </div>

        {/* Exclusive vs Shared Donut */}
        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-success/20 flex items-center justify-center">
              <Users className="w-5 h-5 text-success" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">Exklusiv vs Shared</h3>
              <p className="text-xs text-text-secondary">
                {segments?.crossChannelStats.exclusiveViewersPct ?? 0}% exklusiv
              </p>
            </div>
          </div>
          {donutData.length > 0 ? (
            <div className="h-[100px] flex items-center justify-center">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={donutData}
                    innerRadius={30}
                    outerRadius={45}
                    dataKey="value"
                    stroke="none"
                  >
                    {donutData.map((entry, idx) => (
                      <Cell key={idx} fill={entry.color} />
                    ))}
                  </Pie>
                  <Tooltip
                    contentStyle={{
                      backgroundColor: '#1f2937',
                      border: '1px solid rgba(194,221,240,0.25)',
                      borderRadius: '8px',
                      fontSize: '12px',
                    }}
                    formatter={(value) => formatNumber(Number(value))}
                  />
                </PieChart>
              </ResponsiveContainer>
            </div>
          ) : null}
        </div>
      </motion.div>

      {/* ── Viewer Directory ── */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-primary/20 flex items-center justify-center">
              <UserSearch className="w-5 h-5 text-primary" />
            </div>
            <div>
              <h3 className="text-lg font-bold text-white">Viewer-Verzeichnis</h3>
              <p className="text-sm text-text-secondary">
                {formatNumber(directory?.total || 0)} Viewer
                {directory?.summary ? ` · ${formatNumber(directory.summary.totalViewers)} gesamt` : ''}
                {` · Fenster ${days} Tage`}
              </p>
            </div>
          </div>
        </div>

        {/* Search + Filter */}
        <div className="flex flex-col sm:flex-row gap-3 mb-4">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
            <input
              type="text"
              placeholder="Viewer suchen..."
              value={search}
              onChange={e => handleSearchChange(e.target.value)}
              className="w-full pl-9 pr-3 py-2 rounded-lg bg-background border border-border text-white text-sm placeholder:text-text-secondary focus:outline-none focus:border-primary/50"
            />
          </div>
          <div className="flex gap-1.5 flex-wrap">
            {FILTER_OPTIONS.map(opt => (
              <button
                key={opt.value}
                onClick={() => handleFilterChange(opt.value)}
                className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-colors ${
                  filter === opt.value
                    ? 'bg-primary/20 text-primary border border-primary/30'
                    : 'bg-background text-text-secondary border border-border hover:text-white'
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>

        {/* Table */}
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-text-secondary text-left">
                <SortHeader label="Login" field="first_seen" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label="Sessions" field="sessions" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label="Messages" field="messages" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label="Letzte Aktivität" field="last_seen" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label="Andere Ch." field="other_channels" currentSort={sort} order={order} onSort={handleSort} />
                <th className="py-2 px-4 font-semibold text-xs uppercase tracking-wide">Kategorie</th>
                <th className="py-2 px-2 w-8"></th>
              </tr>
            </thead>
            <tbody>
              {directory?.viewers.map(viewer => (
                <ViewerRow
                  key={viewer.login}
                  viewer={viewer}
                  isExpanded={expandedViewer === viewer.login}
                  onToggle={() =>
                    setExpandedViewer(prev => (prev === viewer.login ? null : viewer.login))
                  }
                  streamer={streamer}
                  days={days}
                />
              ))}
              {directory?.viewers.length === 0 && (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-text-secondary">
                    Keine Viewer gefunden
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {/* Pagination */}
        {totalPages > 1 && (
          <div className="flex items-center justify-between mt-4 pt-4 border-t border-border">
            <button
              onClick={() => setPage(p => Math.max(1, p - 1))}
              disabled={page <= 1}
              className="px-3 py-1.5 rounded-lg text-sm font-semibold bg-background border border-border text-text-secondary hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Zurück
            </button>
            <span className="text-sm text-text-secondary">
              Seite {page} von {totalPages}
            </span>
            <button
              onClick={() => setPage(p => Math.min(totalPages, p + 1))}
              disabled={page >= totalPages}
              className="px-3 py-1.5 rounded-lg text-sm font-semibold bg-background border border-border text-text-secondary hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Weiter
            </button>
          </div>
        )}
      </motion.div>
    </div>
  );
}

function SortHeader({
  label,
  field,
  currentSort,
  order,
  onSort,
}: {
  label: string;
  field: ViewerSortField;
  currentSort: ViewerSortField;
  order: 'asc' | 'desc';
  onSort: (f: ViewerSortField) => void;
}) {
  const isActive = currentSort === field;
  return (
    <th
      className="py-2 px-4 font-semibold text-xs uppercase tracking-wide cursor-pointer hover:text-white transition-colors"
      onClick={() => onSort(field)}
    >
      <span className="flex items-center gap-1">
        {label}
        {isActive && (
          order === 'desc'
            ? <ChevronDown className="w-3 h-3" />
            : <ChevronUp className="w-3 h-3" />
        )}
      </span>
    </th>
  );
}
