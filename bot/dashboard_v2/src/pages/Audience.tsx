import { motion } from 'framer-motion';
import { Rise } from '../motion/Rise';
import { Users, AlertCircle, Loader2, TrendingUp, TrendingDown, Target, Clock, UserPlus } from 'lucide-react';
import { useWatchTimeDistribution, useFollowerFunnel, useAudienceDemographics, useLurkerAnalysis, useViewerProfiles } from '@/hooks/useAnalytics';
import { WatchTimeDistribution } from '@/components/charts/WatchTimeDistribution';
import { FollowerFunnel } from '@/components/charts/FollowerFunnel';
import { AudienceDemographics } from '@/components/charts/AudienceDemographics';
import { LurkerAnalysis } from '@/components/charts/LurkerAnalysis';
import { ViewerProfiles } from '@/components/charts/ViewerProfiles';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import type { TimeRange } from '@/types/analytics';
import { useLanguage } from '@/context/LanguageContext';

interface AudienceProps {
  streamer: string;
  days: TimeRange;
}

export function Audience({ streamer, days }: AudienceProps) {
  const { t, locale } = useLanguage();
  const {
    data: watchTime,
    isLoading: loadingWatchTime,
    isError: watchTimeError,
    error: watchTimeErrorDetail,
    refetch: refetchWatchTime,
  } = useWatchTimeDistribution(streamer, days);
  const {
    data: funnel,
    isLoading: loadingFunnel,
    isError: funnelError,
    error: funnelErrorDetail,
    refetch: refetchFunnel,
  } = useFollowerFunnel(streamer, days);
  const {
    data: demographics,
    isLoading: loadingDemographics,
    isError: demographicsError,
    error: demographicsErrorDetail,
    refetch: refetchDemographics,
  } = useAudienceDemographics(streamer, days);
  const { data: lurkerData, isLoading: loadingLurker } = useLurkerAnalysis(streamer, days);
  const { data: viewerProfilesData, isLoading: loadingViewerProfiles } = useViewerProfiles(streamer, days);

  const isLoading = loadingWatchTime || loadingFunnel || loadingDemographics || loadingLurker || loadingViewerProfiles;
  const failedQueries = [
    { id: 'watchTimeDistribution', label: t('Watch Time'), isError: watchTimeError, error: watchTimeErrorDetail, retry: refetchWatchTime },
    { id: 'followerFunnel', label: t('Follower-Funnel'), isError: funnelError, error: funnelErrorDetail, retry: refetchFunnel },
    { id: 'audienceDemographics', label: t('Demographics'), isError: demographicsError, error: demographicsErrorDetail, retry: refetchDemographics },
  ].filter(q => q.isError);
  const formatError = (err: unknown) => (err instanceof Error ? err.message : t('Unbekannter Fehler'));
  const retryFailed = () => failedQueries.forEach(q => q.retry());

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">{t('Wähle einen Streamer aus')}</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  const watchTimeData = watchTime ?? null;
  const funnelData = funnel ?? null;
  const demographicsData = demographics ?? null;
  const hasReliableWatchTime = watchTimeData?.dataQuality?.method === 'real_samples';
  const lurkerRatio = lurkerData?.dataAvailable ? lurkerData.lurkerStats?.ratio : undefined;
  const lurkerPct = lurkerRatio !== undefined ? lurkerRatio * 100 : undefined;
  const hasWatchTimeInsight = Boolean(watchTimeData && hasReliableWatchTime);
  const hasFunnelInsight = Boolean(funnelData);
  const hasLurkerInsight = lurkerPct !== undefined;
  const hasDemographicsInsight = Boolean(demographicsData);
  const noData =
    !watchTimeData && !funnelData && !demographicsData && !lurkerData && !viewerProfilesData && failedQueries.length === 0;

  return (
    <div className="space-y-6">
      {failedQueries.length > 0 && (
        <div className="bg-error/10 border border-error/30 rounded-lg p-4 text-error space-y-2">
          <div className="flex items-center gap-2 font-semibold">
            <AlertCircle className="w-5 h-5" />
            <span>{t('Daten konnten nicht vollständig geladen werden.')}</span>
          </div>
          <ul className="text-sm text-error/80 list-disc pl-5 space-y-1">
            {failedQueries.map(q => (
              <li key={q.id}>{q.label}: {formatError(q.error)}</li>
            ))}
          </ul>
          <button
            onClick={retryFailed}
            className="px-3 py-1.5 rounded-md bg-error/20 text-error text-sm font-semibold hover:bg-error/30 transition"
          >
            {t('Erneut laden')}
          </button>
        </div>
      )}

      {noData && (
        <div className="flex flex-col items-center justify-center h-48 text-center space-y-2 border border-border rounded-lg">
          <AlertCircle className="w-8 h-8 text-text-secondary" />
          <p className="text-white font-medium">{t('Keine Daten für diesen Zeitraum.')}</p>
          <p className="text-sm text-text-secondary">{t('Bitte Zeitraum oder Streamer anpassen und erneut versuchen.')}</p>
        </div>
      )}

      {noData && <></>}
      {!noData && (
        <>
          {(() => {
            const cards = [];
            if (watchTimeData && hasReliableWatchTime) {
              cards.push(
                <QuickStatCard
                  key="watch"
                  icon={<Clock className="w-5 h-5" />}
                  label={t('Ø Watch Time')}
                  value={t('{minutes} Min', { minutes: watchTimeData.avgWatchTime.toFixed(0) })}
                  color="primary"
                />
              );
            }
            if (funnelData) {
              cards.push(
                <QuickStatCard
                  key="conv"
                  icon={<Target className="w-5 h-5" />}
                  label={t('Conversion Rate')}
                  value={`${funnelData.conversionRate.toFixed(2)}%`}
                  color="success"
                />
              );
              cards.push(
                <QuickStatCard
                  key="unique"
                  icon={<Users className="w-5 h-5" />}
                  label={t('Unique Viewer')}
                  value={funnelData.uniqueViewers.toLocaleString(locale)}
                  color="accent"
                />
              );
              cards.push(
                <QuickStatCard
                  key="newf"
                  icon={<UserPlus className="w-5 h-5" />}
                  label={t('Neue Follower')}
                  value={`+${funnelData.newFollowers}`}
                  color="warning"
                />
              );
            }
            return cards.length > 0 ? (
              <Rise
                className="grid grid-cols-2 md:grid-cols-4 gap-4"
              >
                {cards}
              </Rise>
            ) : null;
          })()}

          {(watchTimeData || funnelData) && (
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
              {watchTimeData && <WatchTimeDistribution data={watchTimeData} />}
              {funnelData && <FollowerFunnel data={funnelData} />}
            </div>
          )}

          {(demographicsData || lurkerData) && (
            <div className="space-y-4">
              <h3 className="px-1 text-base font-semibold text-text-secondary">{t('Audience-Komposition')}</h3>
              {demographicsData && <AudienceDemographics data={demographicsData} />}
              <PlanGateCard featureId="lurker_analysis" title={t('Lurker-Analyse')}>
                <div>
                  <h2 className="text-lg font-semibold text-white mb-4">{t('Lurker-Analyse')}</h2>
                  <LurkerAnalysis data={lurkerData} />
                </div>
              </PlanGateCard>
            </div>
          )}

          {viewerProfilesData && (
            <div className="space-y-4">
              <h3 className="px-1 text-base font-semibold text-text-secondary">{t('Zuschauer-Segmente')}</h3>
              <div className="panel-card rounded-2xl p-6">
                <div className="mb-6 flex items-center gap-3">
                  <Users className="h-6 w-6 text-accent" />
                  <h2 className="text-xl font-bold text-white">{t('Zuschauer-Profile')}</h2>
                </div>
                <ViewerProfiles data={viewerProfilesData} />
              </div>
            </div>
          )}

          {(hasWatchTimeInsight || hasFunnelInsight || hasLurkerInsight || hasDemographicsInsight) && (
            <Rise
              step={{ seconds: 0.3 }}
              className="bg-gradient-to-r from-primary/10 to-accent/10 rounded-xl border border-primary/20 p-6"
            >
              <h3 className="text-lg font-bold text-white mb-4 flex items-center gap-2">
                <Target className="w-5 h-5 text-primary" />
                {t('Audience Insights Zusammenfassung')}
              </h3>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {hasWatchTimeInsight && watchTimeData && (
                  <InsightCard
                    title={t('Viewer Engagement')}
                    description={
                      watchTimeData.avgWatchTime > 25
                        ? t('Starkes Engagement! Deine Viewer bleiben im Schnitt {minutes} Minuten.', { minutes: watchTimeData.avgWatchTime.toFixed(0) })
                        : t('Verbesserungspotential: Viewer bleiben nur {minutes} Min - teste interaktive Segmente.', { minutes: watchTimeData.avgWatchTime.toFixed(0) })
                    }
                    type={watchTimeData.avgWatchTime > 25 ? 'success' : 'warning'}
                  />
                )}

                {hasFunnelInsight && funnelData && (
                  <InsightCard
                    title={t('Follower Conversion')}
                    description={
                      funnelData.conversionRate > 5
                        ? t('Exzellente Conversion von {rate}%! Dein Content überzeugt.', { rate: funnelData.conversionRate.toFixed(2) })
                        : t('Conversion bei {rate}% - nutze mehr Call-to-Actions.', { rate: funnelData.conversionRate.toFixed(2) })
                    }
                    type={funnelData.conversionRate > 5 ? 'success' : 'info'}
                  />
                )}

                {hasLurkerInsight && lurkerPct !== undefined && (
                  <InsightCard
                    title={t('Lurker-Anteil')}
                    description={
                      lurkerPct > 70
                        ? t('{pct}% deiner Zuschauer sind stumme Lurker – teste mehr direkte Interaktion.', { pct: lurkerPct.toFixed(0) })
                        : t('Solider Lurker-Anteil von {pct}%. Ein guter Mix aus aktiven und passiven Zuschauern.', { pct: lurkerPct.toFixed(0) })
                    }
                    type={lurkerPct > 70 ? 'warning' : 'success'}
                  />
                )}

                {hasDemographicsInsight && (
                  <InsightCard
                    title={t('Audience-Zusammensetzung')}
                    description={t('Deine Audience-Demographiken zeigen die Viewer-Herkunft und -Zusammensetzung im gewählten Zeitraum.')}
                    type="info"
                  />
                )}
              </div>
            </Rise>
          )}
        </>
      )}
    </div>
  );
}

interface QuickStatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  color: 'primary' | 'success' | 'accent' | 'warning';
  trend?: number;
}

function QuickStatCard({ icon, label, value, color, trend }: QuickStatCardProps) {
  const colorClasses = {
    primary: 'bg-primary/10 text-primary',
    success: 'bg-success/10 text-success',
    accent: 'bg-accent/10 text-accent',
    warning: 'bg-warning/10 text-warning',
  };

  const TrendIcon = trend === undefined ? null : trend >= 0 ? TrendingUp : TrendingDown;
  const trendColor = trend === undefined ? '' : trend >= 0 ? 'text-success' : 'text-error';

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className="bg-card rounded-xl border border-border p-4"
    >
      <div className={`w-10 h-10 rounded-lg ${colorClasses[color]} flex items-center justify-center mb-3`}>
        {icon}
      </div>
      <div className="text-sm text-text-secondary mb-1">{label}</div>
      <div className="flex items-center gap-2">
        <span className="text-xl font-bold text-white">{value}</span>
        {TrendIcon && (
          <span className={`flex items-center gap-1 text-xs ${trendColor}`}>
            <TrendIcon className="w-3 h-3" />
            {Math.abs(trend!).toFixed(1)}%
          </span>
        )}
      </div>
    </motion.div>
  );
}

interface InsightCardProps {
  title: string;
  description: string;
  type: 'success' | 'warning' | 'info';
}

function InsightCard({ title, description, type }: InsightCardProps) {
  const styles = {
    success: 'bg-success/10 border-success/20',
    warning: 'bg-warning/10 border-warning/20',
    info: 'bg-primary/10 border-primary/20',
  };

  const iconStyles = {
    success: 'text-success',
    warning: 'text-warning',
    info: 'text-primary',
  };

  const Icon = type === 'success' ? TrendingUp : type === 'warning' ? AlertCircle : Target;

  return (
    <div className={`p-4 rounded-lg border ${styles[type]}`}>
      <div className="flex items-center gap-2 mb-2">
        <Icon className={`w-4 h-4 ${iconStyles[type]}`} />
        <span className="font-medium text-white text-sm">{title}</span>
      </div>
      <p className="text-sm text-text-secondary">{description}</p>
    </div>
  );
}

export default Audience;
