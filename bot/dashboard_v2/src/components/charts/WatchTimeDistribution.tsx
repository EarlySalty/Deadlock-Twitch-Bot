import { formatNumber } from '@/utils/formatters';
import { motion } from 'framer-motion';
import { Rise } from '../../motion/Rise';
import { Clock, TrendingUp, TrendingDown, Users, Timer } from 'lucide-react';
import { NoDataCard } from '@/components/cards/NoDataCard';
import type { WatchTimeDistribution as WatchTimeDistributionType } from '@/types/analytics';
import { useLanguage } from '../../context/LanguageContext';

interface WatchTimeDistributionProps {
  data: WatchTimeDistributionType;
}

const SEGMENTS = [
  { key: 'under5min' as const, label: '< 5 Min', shortLabel: '<5m', barClass: 'bg-danger', borderClass: 'border-danger', description: 'Schnelle Absprünge' },
  { key: 'min5to15' as const, label: '5-15 Min', shortLabel: '5-15m', barClass: 'bg-warning', borderClass: 'border-warning', description: 'Kurze Sessions' },
  { key: 'min15to30' as const, label: '15-30 Min', shortLabel: '15-30m', barClass: 'bg-primary', borderClass: 'border-primary', description: 'Mittlere Sessions' },
  { key: 'min30to60' as const, label: '30-60 Min', shortLabel: '30-60m', barClass: 'bg-accent', borderClass: 'border-accent', description: 'Längere Sessions' },
  { key: 'over60min' as const, label: '> 60 Min', shortLabel: '60m+', barClass: 'bg-success', borderClass: 'border-success', description: 'Loyale Zuschauer' },
] as const;

export function WatchTimeDistribution({ data }: WatchTimeDistributionProps) {
  const { t, locale } = useLanguage();
  const method = data?.dataQuality?.method;
  const isNoData =
    !data ||
    method === 'no_data' ||
    method === 'low_coverage' ||
    method === 'insufficient_real_data' ||
    method === 'no_real_data';

  if (isNoData) {
    const sampleCount = data?.dataQuality?.sample_count ?? 0;
    const coveragePct = formatNumber(((data?.dataQuality?.coverage ?? 0) * 100), 1, locale);
    const minSamples = data?.dataQuality?.required_min_samples ?? 25;
    const minCoveragePct = formatNumber(((data?.dataQuality?.required_min_coverage ?? 0.15) * 100), 0, locale);
    const submessage =
      method === 'low_coverage'
        ? t('Echte Samples vorhanden ({samples}), aber Coverage {coverage}% ist noch unter der Schwelle ({minSamples} Samples und {minCoverage}% Coverage).', {
            samples: sampleCount,
            coverage: coveragePct,
            minSamples,
            minCoverage: minCoveragePct,
          })
        : t('Zu wenig echte Chatters-Samples – Ergebnisse werden angezeigt, sobald belastbare Daten vorliegen.');
    return (
      <NoDataCard
        message={t('Nicht genug Daten für Watch Time')}
        submessage={submessage}
      />
    );
  }

  const totalLoyalViewers = (data.min30to60 ?? 0) + (data.over60min ?? 0);
  const hasPrevious = data.previous && data.previous.sessionCount !== undefined && data.previous.sessionCount > 0;
  const prevLoyalViewers = hasPrevious ? ((data.previous!.min30to60 ?? 0) + (data.previous!.over60min ?? 0)) : 0;

  const avgDelta = data.deltas?.avgWatchTime;
  const avgTrendUp = avgDelta != null && avgDelta >= 0;

  return (
    <Rise
      className="bg-card rounded-xl border border-border p-6"
    >
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-primary/20 flex items-center justify-center">
            <Clock className="w-5 h-5 text-primary" />
          </div>
          <div>
            <h3 className="text-lg font-bold text-white">{t('Watch Time Verteilung')}</h3>
            <p className="text-sm text-text-secondary">{t('Wie lange bleiben deine Viewer?')}</p>
          </div>
        </div>
        {hasPrevious && (
          <div className="text-xs text-text-secondary bg-background px-2 py-1 rounded">
            {t('vs. Vorperiode ({count} Sessions)', { count: data.previous!.sessionCount ?? '' })}
          </div>
        )}
      </div>

      {/* Main Stats */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        <div className="bg-background rounded-lg p-4">
          <div className="flex items-center gap-2 text-text-secondary text-sm mb-1">
            <Timer className="w-4 h-4" />
            {t('Ø Watch Time')}
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-white">{formatNumber(data.avgWatchTime, 1, locale)}</span>
            <span className="text-text-secondary">Min</span>
            {avgDelta != null && (
              <span className={`flex items-center gap-1 text-sm ${avgTrendUp ? 'text-success' : 'text-error'}`}>
                {avgTrendUp ? <TrendingUp className="w-4 h-4" /> : <TrendingDown className="w-4 h-4" />}
                {formatNumber(Math.abs(avgDelta), 1, locale)}%
              </span>
            )}
          </div>
          {hasPrevious && (
            <div className="text-xs text-text-secondary mt-1">
              {t('Vorher: {value} Min', { value: formatNumber(data.previous!.avgWatchTime, 1, locale) })}
            </div>
          )}
        </div>
        <div className="bg-background rounded-lg p-4">
          <div className="flex items-center gap-2 text-text-secondary text-sm mb-1">
            <Users className="w-4 h-4" />
            {t('Loyale Viewer')}
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold text-success">{formatNumber(totalLoyalViewers, 1, locale)}%</span>
            <span className="text-text-secondary text-sm">{t('> 30 Min')}</span>
          </div>
          {hasPrevious && (
            <div className="text-xs text-text-secondary mt-1">
              {t('Vorher: {value}%', { value: formatNumber(prevLoyalViewers, 1, locale) })}
              {totalLoyalViewers !== prevLoyalViewers && (
                <span className={totalLoyalViewers > prevLoyalViewers ? 'text-success ml-1' : 'text-error ml-1'}>
                  ({totalLoyalViewers > prevLoyalViewers ? '+' : ''}{formatNumber((totalLoyalViewers - prevLoyalViewers), 1, locale)}pp)
                </span>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Distribution Comparison Bars */}
      <div className="space-y-3 mb-6">
        {SEGMENTS.map((segment, i) => {
          const currVal = data[segment.key];
          const prevVal = hasPrevious ? data.previous![segment.key] : null;
          const maxVal = Math.max(
            currVal,
            prevVal ?? 0,
            ...SEGMENTS.map(s => data[s.key]),
            ...(hasPrevious ? SEGMENTS.map(s => data.previous![s.key]) : [0]),
          );
          const barMax = Math.max(maxVal, 1);

          return (
            <motion.div
              key={segment.key}
              initial={{ opacity: 0, x: -20 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: Math.min(i * 0.04, 0.24) }}
            >
              <div className="flex items-center gap-3">
                {/* Label */}
                <div className="w-16 text-xs text-text-secondary text-right shrink-0">
                  {segment.shortLabel}
                </div>

                {/* Bars container */}
                <div className="flex-1 space-y-1">
                  {/* Current bar */}
                  <div className="flex items-center gap-2">
                    <div className="flex-1 h-5 bg-background rounded overflow-hidden">
                      <motion.div
                        initial={{ width: 0 }}
                        animate={{ width: `${(currVal / barMax) * 100}%` }}
                        transition={{ delay: Math.min(0.1 + i * 0.04, 0.24), duration: 0.5 }}
                        className={`h-full rounded ${segment.barClass}`}
                        style={{ minWidth: currVal > 0 ? '2px' : '0' }}
                      />
                    </div>
                    <span className="text-xs font-medium text-white w-12 text-right">{formatNumber(currVal, 1, locale)}%</span>
                  </div>

                  {/* Previous bar (if available) */}
                  {hasPrevious && prevVal != null && (
                    <div className="flex items-center gap-2">
                      <div className="flex-1 h-3 bg-background rounded overflow-hidden">
                        <motion.div
                          initial={{ width: 0 }}
                          animate={{ width: `${(prevVal / barMax) * 100}%` }}
                          transition={{ delay: Math.min(0.1 + i * 0.04, 0.24), duration: 0.5 }}
                          className={`h-full rounded border bg-transparent ${segment.borderClass}`}
                          style={{ minWidth: prevVal > 0 ? '2px' : '0' }}
                        />
                      </div>
                      <span className="text-[10px] text-text-secondary w-12 text-right">{formatNumber(prevVal, 1, locale)}%</span>
                    </div>
                  )}
                </div>

                {/* Delta */}
                <div className="w-14 text-right shrink-0">
                  {hasPrevious && prevVal != null && prevVal > 0 ? (
                    <DeltaBadge current={currVal} previous={prevVal} inverted={segment.key === 'under5min'} />
                  ) : (
                    <span className="text-xs text-text-secondary">—</span>
                  )}
                </div>
              </div>
            </motion.div>
          );
        })}
      </div>

      {/* Legend */}
      {hasPrevious && (
        <div className="flex items-center gap-4 text-xs text-text-secondary mb-4">
          <span className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded bg-primary" /> {t('Aktuell ({count} Sessions)', { count: data.sessionCount ?? '?' })}
          </span>
          <span className="flex items-center gap-1.5">
            <div className="w-3 h-3 rounded border border-primary bg-transparent" /> {t('Vorperiode ({count} Sessions)', { count: data.previous!.sessionCount ?? '?' })}
          </span>
        </div>
      )}

      {/* Insights */}
      <div className="pt-4 border-t border-border">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {data.under5min > 30 && (
            <InsightBadge
              type="warning"
              text={t('{percent}% springen in den ersten 5 Min ab - optimiere deinen Stream-Start!', { percent: formatNumber(data.under5min, 0, locale) })}
            />
          )}
          {totalLoyalViewers > 40 && (
            <InsightBadge
              type="success"
              text={t('{percent}% bleiben über 30 Min - starke Community-Bindung!', { percent: formatNumber(totalLoyalViewers, 0, locale) })}
            />
          )}
          {hasPrevious && avgDelta != null && avgDelta > 10 && (
            <InsightBadge
              type="success"
              text={t('Watch Time um {percent}% gestiegen vs. Vorperiode!', { percent: formatNumber(avgDelta, 0, locale) })}
            />
          )}
          {hasPrevious && avgDelta != null && avgDelta < -10 && (
            <InsightBadge
              type="warning"
              text={t('Watch Time um {percent}% gesunken - prüfe was sich geändert hat.', { percent: formatNumber(Math.abs(avgDelta), 0, locale) })}
            />
          )}
          {data.avgWatchTime < 15 && (
            <InsightBadge
              type="warning"
              text={t('Durchschnittliche Watch Time unter 15 Min - teste längere Engagement-Segmente')}
            />
          )}
        </div>
      </div>
    </Rise>
  );
}

function DeltaBadge({ current, previous, inverted = false }: { current: number; previous: number; inverted?: boolean }) {
  const { locale } = useLanguage();
  const diff = current - previous;
  // For "under5min", less is better (inverted)
  const isPositive = inverted ? diff < 0 : diff > 0;
  const color = Math.abs(diff) < 0.5 ? 'text-text-secondary' : isPositive ? 'text-success' : 'text-error';

  return (
    <span className={`text-[11px] font-medium ${color}`}>
      {diff > 0 ? '+' : ''}{formatNumber(diff, 1, locale)}pp
    </span>
  );
}

interface InsightBadgeProps {
  type: 'success' | 'warning' | 'info';
  text: string;
}

function InsightBadge({ type, text }: InsightBadgeProps) {
  const styles = {
    success: 'bg-success/10 border-success/20 text-success',
    warning: 'bg-warning/10 border-warning/20 text-warning',
    info: 'bg-primary/10 border-primary/20 text-primary',
  };

  return (
    <div className={`px-3 py-2 rounded-lg border text-xs ${styles[type]}`}>
      {text}
    </div>
  );
}

export default WatchTimeDistribution;
