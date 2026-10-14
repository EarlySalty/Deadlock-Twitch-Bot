import { useState, useMemo, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Rise } from '../motion/Rise';
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
import {
  CHAT_LUECKE_TITEL,
  CHAT_LUECKE_TEXT,
  CHAT_KEINE_TITEL,
  CHAT_KEINE_TEXT,
} from '@/pages/chatAnalyticsShared';
import type {
  RawChatStatus,
  TimeRange,
  ViewerSortField,
  ViewerFilterType,
  ViewerEntry,
  SegmentData,
} from '@/types/analytics';
import { useLanguage, useT } from '@/context/LanguageContext';

interface ViewersProps {
  streamer: string | null;
  days: TimeRange;
}

// eslint-disable-next-line react-refresh/only-export-components
export const SEGMENT_CONFIG: Record<string, { label: string; color: string; bgClass: string }> = {
  dedicated: { label: 'Dedicated', color: 'var(--color-primary)', bgClass: 'bg-primary/10 text-primary border-primary/20' },
  regular: { label: 'Regular', color: 'var(--color-success)', bgClass: 'bg-success/10 text-success border-success/20' },
  casual: { label: 'Casual', color: 'var(--color-warning)', bgClass: 'bg-warning/10 text-warning border-warning/20' },
  lurker: { label: 'Lurker', color: 'var(--color-secondary)', bgClass: 'bg-secondary/10 text-secondary border-secondary/20' },
  new: { label: 'Neu', color: 'var(--color-accent)', bgClass: 'bg-accent/10 text-accent border-accent/20' },
};

function filterOptions(t: ReturnType<typeof useT>): { value: ViewerFilterType; label: string }[] {
  return [
    { value: 'all', label: t('Alle') },
    { value: 'active', label: t('Aktiv') },
    { value: 'lurker', label: t('Lurker') },
    { value: 'exclusive', label: t('Exklusiv') },
    { value: 'shared', label: t('Shared') },
    { value: 'new', label: t('Neu') },
    { value: 'churned', label: t('Churned') },
  ];
}

function personalityLabels(t: ReturnType<typeof useT>): Record<string, string> {
  return {
    'Command': t('Befehlsgeber'),
    'Greeting': t('Willkommener'),
    'Question': t('Fragesteller'),
    'Reaction': t('Reactor'),
    'Hype': t('Hype-Leader'),
    'Game-Related': t('Game-Diskutierer'),
    'Feedback': t('Kritiker/Lobender'),
    'Technical': t('Technik-Check'),
    'Social': t('Socializer'),
    'Other': t('Allrounder'),
  };
}

const PERSONALITY_COLORS = [
  '#C5A059',
  '#00D9FF',
  '#FF5A3C',
  '#00C46A',
  '#B5A488',
  'var(--color-warning)',
  '#C5A059',
  '#00D9FF',
  '#FF5A3C',
  '#00C46A',
];

function formatNumber(n: number, locale: string): string {
  return n.toLocaleString(locale);
}

function CategoryBadge({ category }: { category: string }) {
  const t = useT();
  const config = SEGMENT_CONFIG[category] || SEGMENT_CONFIG.casual;
  return (
    <span className={`px-2 py-0.5 rounded-full text-xs font-semibold border ${config.bgClass}`}>
      {t(config.label)}
    </span>
  );
}

function chatLueckeSichtbar(status?: RawChatStatus): boolean {
  return Boolean(status?.suspectedIngestionIssue) || status?.available === false;
}

function RawChatGapNotice({
  status,
  compact = false,
}: {
  status?: RawChatStatus;
  compact?: boolean;
}) {
  const t = useT();
  const warnung = Boolean(status?.suspectedIngestionIssue);
  if (!chatLueckeSichtbar(status)) {
    return null;
  }

  return (
    <div
      className={`rounded-xl border ${
        warnung
          ? 'border-warning/30 bg-warning/10 text-warning'
          : 'border-white/10 bg-white/[0.04] text-text-secondary'
      } ${compact ? 'px-3 py-2 text-xs' : 'px-4 py-3 text-sm'}`}
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className={`${compact ? 'mt-0.5 h-3.5 w-3.5' : 'mt-0.5 h-4 w-4'} shrink-0`} />
        <div>
          <p className="font-medium text-white">
            {warnung ? t(CHAT_LUECKE_TITEL) : t(CHAT_KEINE_TITEL)}
          </p>
          <p className="mt-1 leading-5">
            {warnung ? t(CHAT_LUECKE_TEXT) : t(CHAT_KEINE_TEXT)}
          </p>
        </div>
      </div>
    </div>
  );
}

function SegmentCard({ name, data }: { name: string; data: SegmentData }) {
  const { t, locale } = useLanguage();
  const config = SEGMENT_CONFIG[name] || SEGMENT_CONFIG.casual;
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className="bg-card rounded-xl border border-border p-4 flex flex-col"
    >
      <div className="flex items-center justify-between mb-2">
        <span className={`px-2 py-0.5 rounded-full text-xs font-bold border ${config.bgClass}`}>
          {t(config.label)}
        </span>
        <span className="text-xs text-text-secondary">{data.pct}%</span>
      </div>
      <div className="text-2xl font-bold text-white mb-1">{formatNumber(data.count, locale)}</div>
      <div className="text-xs text-text-secondary space-y-0.5">
        <div>{t('Ø {count} Sessions', { count: data.avgSessions.toFixed(1) })}</div>
        <div>{t('Ø {count} Messages', { count: data.avgMessages.toFixed(1) })}</div>
      </div>
      <div className="mt-2 h-1.5 bg-background rounded-full overflow-hidden">
        <div
          className="h-full rounded-full transition-[width]"
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
  const { t, locale } = useLanguage();
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
              <span>{t('Nur Presence-Daten im gewählten Zeitraum')}</span>
            </div>
          )}
        </td>
        <td className="py-3 px-4 text-text-secondary">{formatNumber(viewer.totalSessions, locale)}</td>
        <td className="py-3 px-4 text-text-secondary">{formatNumber(viewer.totalMessages, locale)}</td>
        <td className="py-3 px-4 text-text-secondary">
          {viewer.daysSinceLastSeen === 0
            ? t('Heute')
            : viewer.daysSinceLastSeen === 1
              ? t('Gestern')
              : t('vor {days}d', { days: viewer.daysSinceLastSeen })}
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
              {t('Exklusiv')}
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
  const t = useT();
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
      {chatLueckeSichtbar(data.rawChatStatus) && (
        <div className="mb-4">
          <RawChatGapNotice status={data.rawChatStatus} compact />
        </div>
      )}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Clock className="w-4 h-4 text-primary" />
            {t('Aktivität (letzte 90 Tage)')}
          </h4>
          {data.activityTimeline.length > 0 ? (
            <div className="h-[120px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={data.activityTimeline.slice(-30)}>
                  <XAxis dataKey="date" hide />
                  <YAxis hide />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: 'var(--color-popover)',
                      border: '1px solid var(--color-border)',
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
            <p className="text-xs text-text-secondary">{t('Keine Aktivitätsdaten')}</p>
          )}
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Share2 className="w-4 h-4 text-accent" />
            {t('Andere Channels')}
          </h4>
          {data.crossChannelPresence.length > 0 ? (
            <div className="space-y-2 max-h-[120px] overflow-y-auto">
              {data.crossChannelPresence.slice(0, 5).map(cc => (
                <div key={cc.streamer} className="flex items-center justify-between text-xs">
                  <span className="text-white font-medium truncate">{cc.streamer}</span>
                  <span className="text-text-secondary">{t('{count} Sessions', { count: cc.sessions })}</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-secondary">{t('Exklusiver Viewer')}</p>
          )}
        </div>

        <div className="bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <MessageSquare className="w-4 h-4 text-warning" />
            {t('Chat-Muster')}
          </h4>
          <div className="space-y-2 text-xs">
            <div className="flex justify-between">
              <span className="text-text-secondary">{t('Ø Messages/Session')}</span>
              <span className="text-white font-medium">{data.chatPatterns.avgMessagesPerSession}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-secondary">{t('Aktivster Tag')}</span>
              <span className="text-white font-medium">{t(data.chatPatterns.mostActiveDay)}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-text-secondary">{t('Peak-Stunden')}</span>
              <span className="text-white font-medium">
                {data.chatPatterns.peakHours.map(h => `${h}:00`).join(', ')}
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-text-secondary">{t('Trend')}</span>
              <span className="flex items-center gap-1">{trendIcon}</span>
            </div>
          </div>
        </div>
      </div>

      {data.personality && (
        <div className="mt-4 bg-card rounded-lg border border-border p-4">
          <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
            <Users className="w-4 h-4 text-success" />
            {t('Personality')}
            <span className="px-2 py-0.5 rounded-full text-xs font-bold bg-success/10 text-success border border-success/20">
              {personalityLabels(t)[data.personality.primary] || data.personality.primary}
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
                      backgroundColor: 'var(--color-popover)',
                      border: '1px solid var(--color-border)',
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
  const { t, locale } = useLanguage();
  const [sort, setSort] = useState<ViewerSortField>('sessions');
  const [order, setOrder] = useState<'asc' | 'desc'>('desc');
  const [filter, setFilter] = useState<ViewerFilterType>('all');
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [page, setPage] = useState(1);
  const [expandedViewer, setExpandedViewer] = useState<string | null>(null);

  useEffect(() => {
    if (search === debouncedSearch) return;
    const timeout = setTimeout(() => {
      setDebouncedSearch(search);
      setPage(1);
    }, 300);
    return () => clearTimeout(timeout);
  }, [search, debouncedSearch]);

  const {
    data: directory,
    isLoading: loadingDirectory,
    isFetching: fetchingDirectory,
    isPlaceholderData: placeholderDirectory,
    isError: directoryError,
    refetch: refetchDirectory,
  } = useViewerDirectory(streamer, days, sort, order, filter, debouncedSearch, page);

  const {
    data: segments,
    isLoading: loadingSegments,
    isError: segmentsError,
    refetch: refetchSegments,
  } = useViewerSegments(streamer, days);

  const directoryBusy = search !== debouncedSearch || fetchingDirectory || placeholderDirectory;

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

  const donutData = useMemo(() => {
    if (!directory?.summary) return [];
    return [
      { name: t('Exklusiv'), value: directory.summary.exclusiveViewers, color: '#C5A059' },
      { name: t('Shared'), value: directory.summary.sharedViewers, color: '#00D9FF' },
    ];
  }, [directory?.summary, t]);

  const totalPages = directory ? Math.ceil(directory.total / directory.perPage) : 1;

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">{t('Wähle einen Streamer aus')}</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {directory?.rawChatStatus && (
        <RawChatGapNotice status={directory.rawChatStatus} />
      )}

      {segmentsError && (
        <div role="alert" className="bg-error/10 border border-error/30 rounded-lg p-4 text-error">
          {t('Die Zuschauer-Übersicht konnte nicht geladen werden.')}
          <button onClick={() => refetchSegments()} className="ml-3 underline">
            {t('Übersicht erneut laden')}
          </button>
        </div>
      )}

      {segments && (
        <Rise
          className="grid grid-cols-2 md:grid-cols-5 gap-3"
        >
          {['dedicated', 'regular', 'casual', 'lurker', 'new'].map(seg => (
            <SegmentCard
              key={seg}
              name={seg}
              data={segments.segments[seg] || { count: 0, pct: 0, avgMessages: 0, avgSessions: 0 }}

            />
          ))}
        </Rise>
      )}

      <Rise
        step={{ seconds: 0.1 }}
        className="grid grid-cols-1 md:grid-cols-3 gap-4"
      >
        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-error/20 flex items-center justify-center">
              <AlertTriangle className="w-5 h-5 text-error" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">{t('Vermisste Chatter')}</h3>
              <p className="text-xs text-text-secondary">{t('Aktive Viewer die fehlen')}</p>
            </div>
          </div>
          {segments ? (
            <div className="space-y-2">
              <div className="flex items-baseline gap-2 mb-2">
                <span className="text-2xl font-bold text-error">{segments.churnRisk.atRisk}</span>
                <span className="text-sm text-text-secondary">{t('vermisst')}</span>
                <span className="text-text-secondary mx-1">·</span>
                <span className="text-sm text-text-secondary">{t('{count} abgewandert', { count: segments.churnRisk.recentlyChurned })}</span>
              </div>
              {segments.churnRisk.atRiskViewers.length > 0 ? (
                <div className="space-y-1.5 max-h-[140px] overflow-y-auto">
                  {segments.churnRisk.atRiskViewers.slice(0, 8).map(v => (
                    <div key={v.login} className="flex items-center justify-between text-xs">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="text-white font-medium truncate">{v.login}</span>
                        <span className="text-text-secondary shrink-0">{t('vor {days}d', { days: v.daysSinceLastSeen })}</span>
                      </div>
                      <div className="text-text-secondary truncate ml-2 text-right">
                        {v.recentlySeenAt && v.recentlySeenAt.length > 0
                          ? <span className="text-accent">{t('bei {channels}', { channels: v.recentlySeenAt.slice(0, 2).join(', ') })}</span>
                          : <span className="text-text-secondary/50">{t('offline')}</span>}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-success">{t('Keine vermissten Chatter')}</p>
              )}
            </div>
          ) : loadingSegments ? (
            <Loader2 aria-label={t('Zuschauer-Übersicht wird geladen')} className="w-5 h-5 animate-spin text-text-secondary" />
          ) : (
            <p className="text-xs text-text-secondary">{t('Keine Daten verfügbar')}</p>
          )}
        </div>

        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-accent/20 flex items-center justify-center">
              <Share2 className="w-5 h-5 text-accent" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">{t('Top Shared Channels')}</h3>
              <p className="text-xs text-text-secondary">
                {t('Ø {count} andere Channels', {
                  count: segments?.crossChannelStats.avgOtherChannels ?? directory?.summary.avgOtherChannels ?? '…',
                })}
              </p>
            </div>
          </div>
          {segments?.crossChannelStats.topSharedChannels && segments.crossChannelStats.topSharedChannels.length > 0 ? (
            <div className="space-y-1.5">
              {segments.crossChannelStats.topSharedChannels.slice(0, 5).map(ch => (
                <div key={ch.streamer} className="flex items-center justify-between text-xs">
                  <span className="text-white font-medium truncate">{ch.streamer}</span>
                  <span className="text-text-secondary">{t('{count} gemeinsam', { count: formatNumber(ch.sharedCount, locale) })}</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-text-secondary">{loadingSegments ? t('Wird geladen …') : t('Keine Shared-Channel-Daten')}</p>
          )}
        </div>

        <div className="bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-10 h-10 rounded-lg bg-success/20 flex items-center justify-center">
              <Users className="w-5 h-5 text-success" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-white">{t('Exklusiv vs Shared')}</h3>
              <p className="text-xs text-text-secondary">
                {segments
                  ? t('{pct}% exklusiv', { pct: segments.crossChannelStats.exclusiveViewersPct })
                  : loadingSegments
                    ? t('Wird geladen …')
                    : t('Keine Daten verfügbar')}
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
                      backgroundColor: 'var(--color-popover)',
                      border: '1px solid var(--color-border)',
                      borderRadius: '8px',
                      fontSize: '12px',
                    }}
                    formatter={(value) => formatNumber(Number(value), locale)}
                  />
                </PieChart>
              </ResponsiveContainer>
            </div>
          ) : null}
        </div>
      </Rise>

      <Rise
        step={{ seconds: 0.2 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-primary/20 flex items-center justify-center">
              <UserSearch className="w-5 h-5 text-primary" />
            </div>
            <div>
              <h3 className="text-lg font-bold text-white">{t('Viewer-Verzeichnis')}</h3>
              <p className="text-sm text-text-secondary">
                {directory
                  ? t('{count} Viewer', { count: formatNumber(directory.total, locale) })
                  : loadingDirectory
                    ? t('Viewer werden geladen …')
                    : t('Keine Viewer-Daten verfügbar')}
                {directory?.summary ? ` · ${t('{count} gesamt', { count: formatNumber(directory.summary.totalViewers, locale) })}` : ''}
                {` ${t('· Fenster {days} Tage', { days })}`}
              </p>
            </div>
          </div>
        </div>

        <div role="status" aria-live="polite" className="min-h-5 mb-2 text-xs text-text-secondary">
          {directoryBusy && directory && (
            <span className="inline-flex items-center gap-2">
              <Loader2 aria-hidden="true" className="w-3 h-3 animate-spin" />
              {t('Liste wird aktualisiert …')}
            </span>
          )}
        </div>

        <div className="flex flex-col sm:flex-row gap-3 mb-4">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
            <input
              type="text"
              placeholder={t('Viewer suchen...')}
              value={search}
              onChange={e => setSearch(e.target.value)}
              className="w-full pl-9 pr-3 py-2 rounded-lg bg-background border border-border text-white text-sm placeholder:text-text-secondary focus:outline-none focus:border-primary/50"
            />
          </div>
          <div className="flex gap-1.5 flex-wrap">
            {filterOptions(t).map(opt => (
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

        {directoryError && (
          <div role="alert" className="bg-error/10 border border-error/30 rounded-lg p-4 mb-4 text-error">
            <div className="flex items-center gap-2 font-semibold mb-2">
              <AlertCircle className="w-5 h-5" />
              <span>{t('Die Viewer-Liste konnte nicht geladen werden.')}</span>
            </div>
            <button
              onClick={() => refetchDirectory()}
              className="px-3 py-1.5 rounded-md bg-error/20 text-error text-sm font-semibold hover:bg-error/30"
            >
              {t('Erneut laden')}
            </button>
          </div>
        )}

        <div className="overflow-x-auto" aria-busy={directoryBusy}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-text-secondary text-left">
                <SortHeader label={t('Login')} field="first_seen" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label={t('Sessions')} field="sessions" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label={t('Messages')} field="messages" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label={t('Letzte Aktivität')} field="last_seen" currentSort={sort} order={order} onSort={handleSort} />
                <SortHeader label={t('Andere Ch.')} field="other_channels" currentSort={sort} order={order} onSort={handleSort} />
                <th className="py-2 px-4 font-semibold text-xs uppercase tracking-wide">{t('Kategorie')}</th>
                <th className="py-2 px-2 w-8"></th>
              </tr>
            </thead>
            <tbody className={directoryBusy ? 'opacity-60' : undefined}>
              {loadingDirectory && !directory && (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-text-secondary">
                    <span role="status" className="inline-flex items-center gap-2">
                      <Loader2 aria-hidden="true" className="w-4 h-4 animate-spin" />
                      {t('Viewer werden geladen …')}
                    </span>
                  </td>
                </tr>
              )}
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
              {directory?.viewers.length === 0 && !directoryBusy && (
                <tr>
                  <td colSpan={7} className="py-12 text-center text-text-secondary">
                    {t('Keine Viewer gefunden')}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {totalPages > 1 && (
          <div className="flex items-center justify-between mt-4 pt-4 border-t border-border">
            <button
              onClick={() => setPage(p => Math.max(1, p - 1))}
              disabled={directoryBusy || page <= 1}
              className="px-3 py-1.5 rounded-lg text-sm font-semibold bg-background border border-border text-text-secondary hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t('Zurück')}
            </button>
            <span className="text-sm text-text-secondary">
              {t('Seite {page} von {pages}', { page: directory?.page ?? page, pages: totalPages })}
            </span>
            <button
              onClick={() => setPage(p => Math.min(totalPages, p + 1))}
              disabled={directoryBusy || page >= totalPages}
              className="px-3 py-1.5 rounded-lg text-sm font-semibold bg-background border border-border text-text-secondary hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t('Weiter')}
            </button>
          </div>
        )}
      </Rise>
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
