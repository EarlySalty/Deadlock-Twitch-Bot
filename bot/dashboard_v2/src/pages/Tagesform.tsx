import { Users, TrendingUp, Clock, MessageSquare, Lock } from 'lucide-react';
import { useOverview } from '@/hooks/useAnalytics';
import { KpiCard } from '@/components/cards/KpiCard';
import { PaidTeaserBlock } from '@/components/cards/PaidTeaserBlock';
import { formatNumber, formatPercent, formatDuration, formatDateFull } from '@/utils/formatters';
import { PREVIEW_PRICING_ROUTE } from '@/preview/routes';
import { useT } from '@/context/LanguageContext';
import type { TimeRange } from '@/types/analytics';

interface TagesformProps {
  streamer: string | null;
  days: TimeRange;
  onSessionClick?: (sessionId: number) => void;
}

export function Tagesform({ streamer, days, onSessionClick: _onSessionClick }: TagesformProps) {
  const t = useT();
  const { data: overview, isLoading, error } = useOverview(streamer, days);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <div className="flex items-center gap-3 text-accent">
          <div className="w-8 h-8 border-2 border-accent border-t-transparent rounded-full animate-spin" />
          <span>{t('Lade Tagesform…')}</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-8 text-center">
        <h2 className="text-danger text-xl font-bold mb-2">{t('Fehler beim Laden')}</h2>
        <p className="text-text-secondary">{(error as Error).message}</p>
      </div>
    );
  }

  if (!overview || overview.empty || !overview.sessions?.length) {
    return (
      <div className="p-8 text-center text-text-secondary">
        {overview?.error ? t(overview.error) : t('Noch keine Stream-Daten vorhanden.')}
      </div>
    );
  }

  const lastSession = overview.sessions[0];

  const followerArtifact =
    lastSession.followersEnd === 0 && lastSession.followersStart > 0;
  const sessionFollowerDelta = followerArtifact
    ? null
    : lastSession.followersEnd - lastSession.followersStart;

  const sessionWatchtimeH = lastSession.avgViewers * (lastSession.duration / 3600);

  const sessionDateLabel = lastSession.date
    ? formatDateFull(lastSession.date)
    : '–';

  const sessionDurationLabel = lastSession.duration
    ? formatDuration(lastSession.duration)
    : '–';

  const sessionStartLabel = lastSession.startTime
    ? lastSession.startTime.substring(0, 5)
    : '–';

  return (
    <div className="space-y-6">

      <div className="panel-card rounded-2xl p-5 md:p-7">
        <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-warning mb-2">
          {t('Dein letzter Stream')}
        </p>
        <h1 className="display-font text-2xl md:text-3xl font-bold text-white mb-1">
          {sessionDateLabel}
        </h1>
        <div className="flex flex-wrap gap-x-3 gap-y-1 text-sm text-text-secondary mt-1">
          <span>{t('{duration} live', { duration: sessionDurationLabel })}</span>
          {sessionStartLabel !== '–' && (
            <>
              <span className="text-border">·</span>
              <span>{t('ab {time} Uhr', { time: sessionStartLabel })}</span>
            </>
          )}
          {lastSession.title && (
            <>
              <span className="text-border">·</span>
              <span className="truncate max-w-xs">{lastSession.title}</span>
            </>
          )}
        </div>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
        <KpiCard
          title={t('Ø Zuschauer')}
          value={formatNumber(lastSession.avgViewers, 1)}
          subValue={t('im Schnitt live')}
          icon={Users}
          color="blue"
        />

        <KpiCard
          title="Peak"
          value={formatNumber(lastSession.peakViewers)}
          subValue={t('höchster Moment')}
          icon={TrendingUp}
          color="purple"
        />

        <KpiCard
          title={t('Neue Follower')}
          value={
            sessionFollowerDelta === null
              ? '–'
              : `${sessionFollowerDelta >= 0 ? '+' : ''}${formatNumber(sessionFollowerDelta)}`
          }
          subValue={sessionFollowerDelta === null ? t('kein Messwert') : t('während des Streams')}
          icon={TrendingUp}
          color={
            sessionFollowerDelta === null
              ? 'blue'
              : sessionFollowerDelta >= 0
                ? 'green'
                : 'red'
          }
        />

        <KpiCard
          title={t('Retention 10 Min')}
          value={formatPercent(lastSession.retention10m, 0)}
          subValue={t('ggü. Früh-Peak')}
          icon={Clock}
          color="yellow"
        />

        <KpiCard
          title={t('Watchtime')}
          value={t('{hours} Std', { hours: sessionWatchtimeH.toFixed(0) })}
          subValue={t('Zuschauer-Stunden')}
          icon={Clock}
          color="blue"
        />

        <KpiCard
          title="Chatter"
          value={formatNumber(lastSession.uniqueChatters)}
          subValue={t('haben mitgeschrieben')}
          icon={MessageSquare}
          color="purple"
        />
      </div>

      <a
        href={PREVIEW_PRICING_ROUTE}
        className="flex items-center gap-4 rounded-2xl border border-warning/40 bg-gradient-to-r from-warning/10 via-warning/5 to-transparent p-4 md:p-5 hover:border-warning/60 transition-colors group relative overflow-hidden"
        aria-label={t('War das heute über oder unter deinem Schnitt? Verlauf ansehen')}
      >
        <div className="absolute inset-0 bg-gradient-to-r from-transparent via-warning/5 to-transparent -translate-x-full group-hover:translate-x-full transition-[transform,translate,scale] duration-700 pointer-events-none" />

        <div className="w-9 h-9 rounded-xl flex items-center justify-center border border-warning/40 bg-warning/10 flex-shrink-0">
          <Lock className="w-4 h-4 text-warning" />
        </div>

        <div className="flex-1 min-w-0">
          <p className="text-sm font-semibold text-warning">
            {t('War das heute über oder unter deinem Schnitt?')}
          </p>
          <p className="text-xs text-text-secondary mt-0.5">
            {t('Der Vergleich zu deinen letzten Streams steckt im Verlauf.')}
          </p>
        </div>

        <span className="flex-shrink-0 text-xs font-semibold text-warning bg-warning/15 border border-warning/30 px-3 py-1.5 rounded-lg whitespace-nowrap group-hover:bg-warning/25 transition-colors">
          {t('Verlauf ansehen →')}
        </span>
      </a>

      <PaidTeaserBlock />

      <p className="text-[11px] text-text-secondary text-center pb-2">
        {t('Zahlen aus deinem letzten Stream. Wir zeigen nur, was wir wirklich messen.')}
      </p>
    </div>
  );
}
