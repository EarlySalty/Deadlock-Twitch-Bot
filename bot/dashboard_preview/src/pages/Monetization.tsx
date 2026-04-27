import { TrendingDown, Zap, Gift, Radio, AlertCircle, Loader2, Clock, BarChart3, Lightbulb, ArrowLeftRight, Timer, BellOff, Calendar } from 'lucide-react';
import { motion } from 'framer-motion';
import { useMonetization, useAdsSchedule } from '@/hooks/useAnalytics';
import type { TimeRange, AdBucketData, RecoveryBucketData, AdsSchedule } from '@/types/analytics';

interface MonetizationProps {
  streamer: string | null;
  days: TimeRange;
}

function StatTile({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
      <span className="text-xs text-text-secondary uppercase tracking-wide">{label}</span>
      <span className="text-2xl font-bold text-white">{value}</span>
      {sub && <span className="text-xs text-text-secondary">{sub}</span>}
    </div>
  );
}

function fmt(n: number | null | undefined, fallback = '-'): string {
  if (n === null || n === undefined) return fallback;
  return n.toLocaleString('de-DE');
}

function fmtPct(n: number | null | undefined): string {
  if (n === null || n === undefined) return '-';
  const sign = n > 0 ? '+' : '';
  return `${sign}${n.toFixed(1)}%`;
}

function DropBar({ label, avgDrop, count, maxDrop }: { label: string; avgDrop: number | null; count: number; maxDrop: number }) {
  if (avgDrop === null || count === 0) {
    return (
      <div className="flex items-center gap-3">
        <span className="text-xs text-text-secondary w-16 text-right shrink-0">{label}</span>
        <div className="flex-1 h-6 bg-background rounded flex items-center px-2">
          <span className="text-xs text-text-secondary">Keine Daten</span>
        </div>
      </div>
    );
  }
  const width = maxDrop > 0 ? Math.max(4, (Math.abs(avgDrop) / maxDrop) * 100) : 0;
  const color = Math.abs(avgDrop) > 15 ? 'bg-error/70' : Math.abs(avgDrop) > 8 ? 'bg-warning/70' : 'bg-success/70';

  return (
    <div className="flex items-center gap-3">
      <span className="text-xs text-text-secondary w-16 text-right shrink-0">{label}</span>
      <div className="flex-1 h-6 bg-background rounded overflow-hidden relative">
        <div className={`h-full ${color} rounded`} style={{ width: `${width}%` }} />
        <span className="absolute inset-0 flex items-center px-2 text-xs font-medium text-white">
          {fmtPct(avgDrop)} ({count}x)
        </span>
      </div>
    </div>
  );
}

function AdStrategyRecommendation({ recommendations }: { recommendations: string[] }) {
  if (!recommendations || recommendations.length === 0) return null;

  return (
    <div className="bg-gradient-to-r from-primary/10 via-card to-accent/10 border border-primary/25 rounded-xl p-5">
      <div className="flex items-center gap-2 mb-3">
        <Lightbulb className="w-5 h-5 text-primary" />
        <h4 className="font-semibold text-white">Ad-Strategie Empfehlung</h4>
      </div>
      <ul className="space-y-2">
        {recommendations.map((rec, i) => (
          <li key={i} className="flex items-start gap-2 text-sm text-text-secondary">
            <span className="mt-0.5 shrink-0 w-5 h-5 rounded-full bg-primary/20 text-primary text-xs flex items-center justify-center font-semibold">
              {i + 1}
            </span>
            <span>{rec}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function AdScheduleSection({ scheduleData, loading }: { scheduleData: AdsSchedule | null; loading: boolean }) {
  if (loading) {
    return (
      <div className="mb-4 bg-card border border-border rounded-xl p-5 flex items-center gap-2 text-text-secondary text-sm">
        <Loader2 className="w-4 h-4 animate-spin" />
        Lade Ad-Zeitplan…
      </div>
    );
  }

  if (!scheduleData || !scheduleData.current) {
    return (
      <div className="mb-4 p-4 bg-card border border-border rounded-xl text-text-secondary text-sm">
        Keine Ad-Zeitplan-Daten vorhanden.
      </div>
    );
  }

  const { current, history } = scheduleData;

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      className="mb-4 space-y-4"
    >
      {/* Section Header */}
      <div className="flex items-center gap-2">
        <Calendar className="w-4 h-4 text-primary" />
        <h4 className="font-semibold text-white">Ad-Zeitplan</h4>
        <span className="text-xs text-text-secondary ml-auto">
          Stand: {formatTimestamp(current.snapshot_at)}
        </span>
      </div>

      {/* Current Status Tiles */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-3">
        <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
          <div className="flex items-center gap-1.5">
            <Clock className="w-3.5 h-3.5 text-accent" />
            <span className="text-xs text-text-secondary uppercase tracking-wide">Nächste Ad</span>
          </div>
          <span className="text-lg font-bold text-white">
            {current.next_ad_at ? formatRelativeTime(current.next_ad_at) : 'Keine geplant'}
          </span>
          {current.next_ad_at && (
            <span className="text-xs text-text-secondary">{formatTimestamp(current.next_ad_at)}</span>
          )}
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
          <div className="flex items-center gap-1.5">
            <Timer className="w-3.5 h-3.5 text-success" />
            <span className="text-xs text-text-secondary uppercase tracking-wide">Preroll-frei</span>
          </div>
          <span className="text-lg font-bold text-white">
            {formatDurationMin(current.preroll_free_time)}
          </span>
          <span className="text-xs text-text-secondary">verbleibend</span>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
          <div className="flex items-center gap-1.5">
            <BellOff className="w-3.5 h-3.5 text-warning" />
            <span className="text-xs text-text-secondary uppercase tracking-wide">Snooze</span>
          </div>
          <span className="text-lg font-bold text-white">
            {current.snooze_count !== null ? `${current.snooze_count}x` : '-'}
          </span>
          {current.snooze_refresh_at && (
            <span className="text-xs text-text-secondary">
              Refresh: {formatTimestamp(current.snooze_refresh_at)}
            </span>
          )}
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
          <div className="flex items-center gap-1.5">
            <Clock className="w-3.5 h-3.5 text-text-secondary" />
            <span className="text-xs text-text-secondary uppercase tracking-wide">Letzte Ad</span>
          </div>
          <span className="text-lg font-bold text-white">
            {current.last_ad_at ? formatTimestamp(current.last_ad_at) : '-'}
          </span>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex flex-col gap-1">
          <div className="flex items-center gap-1.5">
            <Timer className="w-3.5 h-3.5 text-text-secondary" />
            <span className="text-xs text-text-secondary uppercase tracking-wide">Dauer</span>
          </div>
          <span className="text-lg font-bold text-white">
            {current.duration !== null ? `${current.duration} s` : '-'}
          </span>
        </div>
      </div>

      {/* Schedule History */}
      {history.length > 1 && (
        <div className="bg-card border border-border rounded-xl p-5">
          <div className="flex items-center gap-2 mb-3">
            <BarChart3 className="w-4 h-4 text-accent" />
            <span className="text-sm font-medium text-white">Schedule-Verlauf</span>
            <span className="text-xs text-text-secondary ml-auto">Letzte {history.length} Snapshots</span>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-text-secondary border-b border-border">
                  <th className="text-left py-2 pr-4 font-medium">Snapshot</th>
                  <th className="text-left py-2 pr-4 font-medium">Nächste Ad</th>
                  <th className="text-right py-2 pr-4 font-medium">Dauer</th>
                  <th className="text-right py-2 font-medium">Preroll-frei</th>
                </tr>
              </thead>
              <tbody>
                {history.map((entry, i) => (
                  <tr key={i} className="border-b border-border/50 hover:bg-card/50">
                    <td className="py-2 pr-4 text-white font-mono text-xs">
                      {formatTimestamp(entry.snapshot_at)}
                    </td>
                    <td className="py-2 pr-4 text-text-secondary text-xs">
                      {entry.next_ad_at ? formatTimestamp(entry.next_ad_at) : 'Keine'}
                    </td>
                    <td className="py-2 pr-4 text-right text-text-secondary">
                      {entry.duration !== null ? `${entry.duration} s` : '-'}
                    </td>
                    <td className="py-2 text-right text-text-secondary">
                      {formatDurationMin(entry.preroll_free_time)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </motion.div>
  );
}

function formatTimestamp(iso: string | null): string {
  if (!iso) return '-';
  try {
    return new Date(iso).toLocaleString('de-DE', {
      day: '2-digit', month: '2-digit', year: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

function formatDurationMin(seconds: number | null): string {
  if (seconds === null || seconds === undefined) return '-';
  if (seconds < 60) return `${seconds} s`;
  const mins = Math.floor(seconds / 60);
  const hrs = Math.floor(mins / 60);
  if (hrs > 0) {
    const remainMin = mins % 60;
    return remainMin > 0 ? `${hrs} h ${remainMin} min` : `${hrs} h`;
  }
  return `${mins} min`;
}

function formatRelativeTime(iso: string | null): string {
  if (!iso) return '-';
  try {
    const target = new Date(iso);
    const now = new Date();
    const diffMs = target.getTime() - now.getTime();
    if (diffMs <= 0) return 'Abgelaufen';
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 60) return `in ${diffMin} min`;
    const diffH = Math.floor(diffMin / 60);
    const remainMin = diffMin % 60;
    return remainMin > 0 ? `in ${diffH} h ${remainMin} min` : `in ${diffH} h`;
  } catch {
    return iso;
  }
}

export function Monetization({ streamer, days }: MonetizationProps) {
  const { data, isLoading, isError } = useMonetization(streamer, days);
  const { data: adsSchedule, isLoading: scheduleLoading } = useAdsSchedule(streamer);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64 text-text-secondary">
        <Loader2 className="w-6 h-6 animate-spin mr-2" />
        Lade Monetization-Daten…
      </div>
    );
  }

  if (isError || !data) {
    return (
      <div className="flex items-center gap-3 p-6 bg-error/10 border border-error/20 rounded-xl text-error">
        <AlertCircle className="w-5 h-5 shrink-0" />
        <span>Monetization-Daten konnten nicht geladen werden.</span>
      </div>
    );
  }

  const { ads, hype_train, bits, subs, window_days } = data;
  const noAds = ads.total === 0;
  const viewerDropValue = ads.avg_viewer_drop_pct !== null ? fmtPct(ads.avg_viewer_drop_pct) : 'Keine Viewer-Timeline Daten';

  // Get max drop across all buckets for consistent bar scaling
  const allDropValues = [
    ...Object.values((ads.duration_impact ?? {}) as Record<string, AdBucketData>).map(b => Math.abs(b.avg_drop ?? 0)),
    ...Object.values((ads.position_impact ?? {}) as Record<string, AdBucketData>).map(b => Math.abs(b.avg_drop ?? 0)),
  ];
  const maxDrop = Math.max(1, ...allDropValues);

  const durationLabels: Record<string, string> = { '30s': '30s', '60s': '60s', '90s': '90s', '120s_plus': '120s+' };
  const positionLabels: Record<string, string> = {
    'early_0_30m': '0-30 Min',
    'mid_30_60m': '30-60 Min',
    'late_60_90m': '60-90 Min',
    'endgame_90m': '90+ Min',
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-xl font-bold text-white">Monetization &amp; Ad-Analyse</h2>
        <p className="text-text-secondary text-sm mt-1">Letzte {window_days} Tage</p>
      </div>

      {/* Ad Breaks Overview */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Radio className="w-4 h-4 text-accent" />
          <h3 className="font-semibold text-white">Ad Breaks</h3>
        </div>
        {noAds ? (
          <div className="p-4 bg-card border border-border rounded-xl text-text-secondary text-sm">
            Noch keine Ad-Break-Events in den letzten {window_days} Tagen.
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 sm:grid-cols-5 gap-3 mb-4">
              <StatTile label="Gesamt" value={fmt(ads.total)} />
              <StatTile label="Automatisch" value={fmt(ads.auto)} sub={`${fmt(ads.manual)} manuell`} />
              <StatTile label="Ø Dauer" value={`${ads.avg_duration_s.toFixed(0)} s`} />
              <StatTile
                label="Ø Viewer-Drop"
                value={viewerDropValue}
                sub={ads.avg_viewer_drop_pct !== null ? 'nach Ad-Break' : undefined}
              />
              <StatTile
                label="Ø Recovery"
                value={ads.avg_recovery_min != null ? `${ads.avg_recovery_min} Min` : '-'}
                sub="bis Pre-Ad Level"
              />
            </div>

            {/* Ad Analysis Grid */}
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-4">
              {/* Duration Impact */}
              {ads.duration_impact && (
                <div className="bg-card border border-border rounded-xl p-4">
                  <div className="flex items-center gap-2 mb-3">
                    <BarChart3 className="w-4 h-4 text-accent" />
                    <span className="text-sm font-medium text-white">Viewer-Drop nach Ad-Dauer</span>
                  </div>
                  <div className="space-y-2">
                    {Object.entries(durationLabels).map(([key, label]) => {
                      const bucket = (ads.duration_impact as Record<string, AdBucketData>)?.[key];
                      return (
                        <DropBar
                          key={key}
                          label={label}
                          avgDrop={bucket?.avg_drop ?? null}
                          count={bucket?.count ?? 0}
                          maxDrop={maxDrop}
                        />
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Position Impact */}
              {ads.position_impact && (
                <div className="bg-card border border-border rounded-xl p-4">
                  <div className="flex items-center gap-2 mb-3">
                    <Clock className="w-4 h-4 text-accent" />
                    <span className="text-sm font-medium text-white">Viewer-Drop nach Stream-Zeitpunkt</span>
                  </div>
                  <div className="space-y-2">
                    {Object.entries(positionLabels).map(([key, label]) => {
                      const bucket = (ads.position_impact as Record<string, AdBucketData>)?.[key];
                      return (
                        <DropBar
                          key={key}
                          label={label}
                          avgDrop={bucket?.avg_drop ?? null}
                          count={bucket?.count ?? 0}
                          maxDrop={maxDrop}
                        />
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Auto vs Manual */}
              {ads.auto_vs_manual && (ads.auto_vs_manual.auto_count > 0 || ads.auto_vs_manual.manual_count > 0) && (
                <div className="bg-card border border-border rounded-xl p-4">
                  <div className="flex items-center gap-2 mb-3">
                    <ArrowLeftRight className="w-4 h-4 text-accent" />
                    <span className="text-sm font-medium text-white">Auto vs Manuell</span>
                  </div>
                  <div className="space-y-2">
                    <DropBar
                      label="Auto"
                      avgDrop={ads.auto_vs_manual.auto_avg_drop}
                      count={ads.auto_vs_manual.auto_count}
                      maxDrop={maxDrop}
                    />
                    <DropBar
                      label="Manuell"
                      avgDrop={ads.auto_vs_manual.manual_avg_drop}
                      count={ads.auto_vs_manual.manual_count}
                      maxDrop={maxDrop}
                    />
                  </div>
                </div>
              )}

              {/* Recovery by Duration */}
              {ads.recovery_by_duration && (
                <div className="bg-card border border-border rounded-xl p-4">
                  <div className="flex items-center gap-2 mb-3">
                    <Clock className="w-4 h-4 text-success" />
                    <span className="text-sm font-medium text-white">Recovery-Zeit nach Ad-Dauer</span>
                  </div>
                  <div className="space-y-2">
                    {Object.entries(durationLabels).map(([key, label]) => {
                      const bucket = (ads.recovery_by_duration as Record<string, RecoveryBucketData>)?.[key];
                      const avgRec = bucket?.avg_recovery_min;
                      const count = bucket?.count ?? 0;
                      return (
                        <div key={key} className="flex items-center gap-3">
                          <span className="text-xs text-text-secondary w-16 text-right shrink-0">{label}</span>
                          <div className="flex-1 h-6 bg-background rounded flex items-center px-2">
                            {avgRec != null && count > 0 ? (
                              <span className="text-xs font-medium text-white">
                                Ø {avgRec} Min ({count}x)
                              </span>
                            ) : (
                              <span className="text-xs text-text-secondary">Keine Daten</span>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>

            {/* Recommendations */}
            <AdStrategyRecommendation recommendations={ads.recommendations ?? []} />

            {/* Worst Ads Table */}
            {ads.worst_ads.length > 0 && (
              <div className="mt-4">
                <div className="flex items-center gap-2 mb-2">
                  <TrendingDown className="w-4 h-4 text-error" />
                  <span className="text-sm font-medium text-white">Schlechteste Ads (Top 5)</span>
                </div>
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="text-text-secondary border-b border-border">
                        <th className="text-left py-2 pr-4 font-medium">Zeitpunkt</th>
                        <th className="text-right py-2 pr-4 font-medium">Dauer</th>
                        <th className="text-center py-2 pr-4 font-medium">Typ</th>
                        <th className="text-right py-2 pr-4 font-medium">Viewer-Drop</th>
                        <th className="text-right py-2 font-medium">Recovery</th>
                      </tr>
                    </thead>
                    <tbody>
                      {ads.worst_ads.map((ad, i) => (
                        <tr key={i} className="border-b border-border/50 hover:bg-card/50">
                          <td className="py-2 pr-4 text-white font-mono text-xs">{ad.started_at}</td>
                          <td className="py-2 pr-4 text-right text-text-secondary">{ad.duration_s} s</td>
                          <td className="py-2 pr-4 text-center">
                            <span className={`text-xs px-2 py-0.5 rounded-full ${ad.is_automatic ? 'bg-warning/20 text-warning' : 'bg-primary/20 text-primary'}`}>
                              {ad.is_automatic ? 'Auto' : 'Manuell'}
                            </span>
                          </td>
                          <td className="py-2 pr-4 text-right font-semibold text-error">
                            {fmtPct(ad.drop_pct)}
                          </td>
                          <td className="py-2 text-right text-text-secondary">
                            {ad.recovery_min != null ? `${ad.recovery_min} Min` : '-'}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </>
        )}
      </section>

      {/* Ad-Zeitplan (unabhängig von Ad-Break Events) */}
      <AdScheduleSection scheduleData={adsSchedule ?? null} loading={scheduleLoading} />

      {/* Hype Train */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Zap className="w-4 h-4 text-warning" />
          <h3 className="font-semibold text-white">Hype Trains</h3>
        </div>
        {hype_train.total === 0 ? (
          <div className="p-4 bg-card border border-border rounded-xl text-text-secondary text-sm">
            Noch keine Hype Trains in den letzten {window_days} Tagen.
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            <StatTile label="Gesamt" value={fmt(hype_train.total)} />
            <StatTile label="Ø Level" value={hype_train.avg_level.toFixed(1)} />
            <StatTile label="Max Level" value={fmt(hype_train.max_level)} />
            <StatTile label="Ø Dauer" value={`${hype_train.avg_duration_s.toFixed(0)} s`} />
          </div>
        )}
      </section>

      {/* Bits & Subs */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Gift className="w-4 h-4 text-success" />
          <h3 className="font-semibold text-white">Bits &amp; Subs</h3>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <StatTile label="Bits total" value={fmt(bits.total)} sub={`${fmt(bits.cheer_events)} Cheers`} />
          <StatTile label="Sub-Events" value={fmt(subs.total_events)} sub={`${fmt(subs.gifted)} Gifted`} />
        </div>
      </section>
    </div>
  );
}
