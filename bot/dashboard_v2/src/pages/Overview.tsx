import { Users, TrendingUp, Clock, MessageSquare, Target } from 'lucide-react';
import { useOverview, useHourlyHeatmap, useCalendarHeatmap, useViewerCountTimeline } from '@/hooks/useAnalytics';
import { KpiCard } from '@/components/cards/KpiCard';
import { HealthScoreCard } from '@/components/cards/HealthScoreCard';
import { ScoreGauge } from '@/components/cards/ScoreGauge';
import { ViewerTrendChart } from '@/components/charts/ViewerTrendChart';
import { ViewerTimelineChart } from '@/components/charts/ViewerTimelineChart';
import { RetentionRadar } from '@/components/charts/RetentionRadar';
import { SessionTable } from '@/components/tables/SessionTable';
import { HourlyHeatmap } from '@/components/heatmaps/HourlyHeatmap';
import { CalendarHeatmap } from '@/components/heatmaps/CalendarHeatmap';
import { InsightsPanel } from '@/components/cards/InsightsPanel';
import { PostStreamReportCard } from '@/components/cards/PostStreamReportCard';
import { CategoryRankBadge } from '@/components/cards/CategoryRankBadge';
import { formatNumber, formatPercent, formatHours } from '@/utils/formatters';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import type { TimeRange } from '@/types/analytics';
import { kalenderFenster } from '@/utils/zeitraum';
import { useT } from '@/context/LanguageContext';

interface OverviewProps {
  streamer: string | null;
  days: TimeRange;
  onSessionClick?: (sessionId: number) => void;
}

export function Overview({ streamer, days, onSessionClick }: OverviewProps) {
  const t = useT();
  const { data: overview, isLoading, error } = useOverview(streamer, days);
  const { data: hourlyData } = useHourlyHeatmap(streamer, days);
  const { data: calendarData } = useCalendarHeatmap(streamer, kalenderFenster(days));
  const { data: timelineData } = useViewerCountTimeline(streamer, days);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <div className="flex items-center gap-3 text-accent">
          <div className="w-8 h-8 border-2 border-accent border-t-transparent rounded-full animate-spin" />
          <span>{t('Lade Analytics...')}</span>
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

  if (!overview || overview.empty) {
    return (
      <div className="p-8 text-center text-text-secondary">
        {overview?.error ? t(overview.error) : t('Keine Daten verfügbar')}
      </div>
    );
  }

  const { scores, summary, sessions, findings, actions, network } = overview;
  const followersPerHour = `${summary.followersPerHour >= 0 ? '+' : ''}${summary.followersPerHour.toFixed(2)}`;

  return (
    <div className="space-y-8">
      <div data-tour-id="tour-analytics-kpis" className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
        <PlanGateCard featureId="health_scores" title={t('Health Scores')}>
          <HealthScoreCard scores={scores} />
        </PlanGateCard>

        <KpiCard
          title={t('Ø Viewers')}
          value={formatNumber(summary.avgViewers, 1)}
          subValue={t('Peak: {value}', { value: formatNumber(summary.peakViewers) })}
          trend={summary.avgViewersTrend ?? undefined}
          icon={Users}
          color="blue"
        />

        <KpiCard
          title={t('Hours Watched')}
          value={formatHours(summary.totalHoursWatched)}
          subValue={t('{value} Airtime', { value: formatHours(summary.totalAirtime) })}
          icon={Clock}
          color="purple"
        />

        <KpiCard
          title={t('Follower')}
          value={`${summary.followersDelta >= 0 ? '+' : ''}${formatNumber(summary.followersDelta)}`}
          subValue={
            summary.followersGained && summary.followersGained !== summary.followersDelta
              ? t('+{gained} gewonnen · {perHour}/h netto', { gained: formatNumber(summary.followersGained), perHour: followersPerHour })
              : t('{perHour} / Stunde', { perHour: followersPerHour })
          }
          trend={
            summary.followersTrend != null
              ? Math.max(-999, Math.min(999, summary.followersTrend))
              : undefined
          }
          icon={TrendingUp}
          color="green"
        />

        <KpiCard
          title={t('Retention (10m)')}
          value={summary.retentionReliable === false ? '—' : formatPercent(summary.retention10m)}
          subValue={summary.retentionReliable === false ? t('Zu wenig Daten') : t('Ziel: >40%')}
          trend={summary.retentionReliable === false ? undefined : summary.retentionTrend}
          icon={Target}
          color="yellow"
        />
      </div>

      {overview.categoryRank != null && overview.categoryTotal != null && (
        <CategoryRankBadge rank={overview.categoryRank} total={overview.categoryTotal} />
      )}

      <div data-tour-id="tour-analytics-insights">
        <PlanGateCard featureId="insights_panel" title={t('Insights')}>
          {findings && findings.length > 0 && (
            <InsightsPanel findings={findings} actions={actions} />
          )}
        </PlanGateCard>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div className="lg:col-span-2 h-full">
          {timelineData && timelineData.length > 0 ? (
            <ViewerTimelineChart data={timelineData} title={t('Viewer Timeline (Live-Daten)')} />
          ) : (
            <ViewerTrendChart sessions={sessions} title={t('Viewer Entwicklung')} />
          )}
        </div>
        <div className="h-full">
          <RetentionRadar scores={scores} title={t('Performance Mix')} />
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {hourlyData && <HourlyHeatmap data={hourlyData} />}
        <PlanGateCard featureId="calendar_heatmap" title={t('Kalender-Heatmap')}>
          {calendarData && <CalendarHeatmap data={calendarData} days={kalenderFenster(days)} />}
        </PlanGateCard>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div className="lg:col-span-2 grid grid-cols-2 sm:grid-cols-4 gap-4">
          <ScoreGauge score={scores.growth} label={t('Growth')} hint={t('Neue Follower je Stunde')} />
          <ScoreGauge score={scores.monetization} label={t('Revenue')} hint={t('Subs, Bits und Hype Trains je Stream')} />
          <ScoreGauge score={scores.network} label={t('Network')} hint={t('Raids je Stream, gesendet und erhalten')} />
          <ScoreGauge score={scores.retention} label={t('Retention')} hint={t('Ø Zuschauer im Verhältnis zum Peak')} />
        </div>

        <div className="bg-card p-5 rounded-xl border border-border">
          <h4 className="text-sm font-bold text-text-secondary mb-4 uppercase tracking-wide flex items-center gap-2">
            <MessageSquare className="w-4 h-4" />
            {t('Raid-Aktivität')}
          </h4>
          <div className="space-y-3">
            <div className="flex justify-between items-center p-3 bg-black/20 rounded-lg">
              <span className="text-text-secondary text-sm">{t('Raids gesendet')}</span>
              <span className="text-white font-bold">{network?.sent ?? 0}</span>
            </div>
            <div className="flex justify-between items-center p-3 bg-black/20 rounded-lg">
              <span className="text-text-secondary text-sm">{t('Raids erhalten')}</span>
              <span className="text-white font-bold">{network?.received ?? 0}</span>
            </div>
            <div className="flex justify-between items-center p-3 bg-black/20 rounded-lg">
              <span className="text-text-secondary text-sm">{t('Viewer weitergeleitet')}</span>
              <span className="text-white font-bold">{formatNumber(network?.sentViewers ?? 0)}</span>
            </div>
          </div>
        </div>
      </div>

      <SessionTable
        sessions={sessions}
        limit={10}
        onSessionClick={onSessionClick}
      />

      <PostStreamReportCard streamer={streamer} />
    </div>
  );
}
