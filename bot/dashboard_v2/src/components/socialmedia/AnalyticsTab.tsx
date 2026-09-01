import { useEffect, useId, useMemo, useState } from 'react';
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
  CalendarRange,
  FileText,
  Loader2,
  RefreshCw,
  Sparkles,
} from 'lucide-react';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import {
  fetchClipAnalytics,
  fetchClips,
  fetchReports,
  runReport,
  SocialMediaForbiddenError,
} from '@/api/socialMedia';
import type {
  ClipAnalytics,
  ClipStatus,
  SocialMediaReportKind,
} from '@/types/socialMedia';
import { useLanguage } from '@/context/LanguageContext';
import { fehlerText, REPORT_KIND_LABELS } from './labels';

const BUCKET_ORDER = ['24h', '7d', '30d'] as const;

/** Clip-Stati mit Plattform-IDs: beide liefern Analytics-Zahlen. */
const VEROEFFENTLICHTE_STATI: ClipStatus[] = ['published_all', 'published_partial'];

interface AnalyticsTabProps {
  streamer: string;
  /** Die Report-Knoepfe sind admin-only; ein Partner bekaeme nur 403. */
  isAdmin: boolean;
}

function toneClass(kind: SocialMediaReportKind): string {
  if (kind === 'streamer') return 'bg-orange/15 text-orange border-orange/35';
  if (kind === 'cross') return 'bg-accent/15 text-accent border-accent/35';
  return 'bg-bg/70 text-white border-border';
}

function formatDate(value: string | null | undefined, locale: string): string {
  if (!value) return '—';
  return new Date(value).toLocaleString(locale, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function normalizeChartRows(items: ClipAnalytics[]) {
  return BUCKET_ORDER.map((bucket) => {
    const current = items.filter((item) => item.bucket === bucket);
    const byPlatform = Object.fromEntries(current.map((item) => [item.platform, item]));
    return {
      bucket,
      youtube_views: byPlatform.youtube?.views ?? 0,
      tiktok_views: byPlatform.tiktok?.views ?? 0,
      instagram_views: byPlatform.instagram?.views ?? 0,
      youtube_er: byPlatform.youtube?.engagement_rate ?? null,
      tiktok_er: byPlatform.tiktok?.engagement_rate ?? null,
      instagram_er: byPlatform.instagram?.engagement_rate ?? null,
    };
  });
}

export function AnalyticsTab({ streamer, isAdmin }: AnalyticsTabProps) {
  const queryClient = useQueryClient();
  const { t, locale } = useLanguage();
  const interfaceId = useId();
  const [selectedClipId, setSelectedClipId] = useState<number | null>(null);

  // Eigene Abfragen statt der Liste aus dem Clip-Pool: die haengt am
  // Pool-Filter (Default `pending`) und liefert deshalb praktisch nie einen
  // veroeffentlichten Clip.
  //
  // Beide Stati werden gebraucht: ein teilveroeffentlichter Clip hat auf den
  // geglueckten Plattformen eine ID und damit Zahlen. Nur `published_all`
  // abzufragen liess ihn aus der Auswahl fallen, sobald eine der drei
  // Plattformen gescheitert war.
  const publishedQueries = useQueries({
    queries: VEROEFFENTLICHTE_STATI.map((status) => ({
      queryKey: ['social-media', 'clips', streamer, status],
      queryFn: () => fetchClips({ status, streamer, page: 1, page_size: 100 }),
      enabled: !!streamer,
      retry: (failureCount: number, err: Error) => {
        if (err instanceof SocialMediaForbiddenError) return false;
        return failureCount < 2;
      },
    })),
  });
  const publishedLoading = publishedQueries.some((abfrage) => abfrage.isLoading);
  const publishedError = publishedQueries.find((abfrage) => abfrage.error)?.error ?? null;
  // Stabile Abhaengigkeit: das Array aus `useQueries` ist bei jedem Rendern neu.
  const publishedStand = publishedQueries.map((abfrage) => abfrage.dataUpdatedAt).join(':');

  const eligibleClips = useMemo(() => {
    const items = publishedQueries.flatMap((abfrage) => abfrage.data?.items ?? []);
    return items.filter(
      (clip) =>
        clip.platform_status.youtube || clip.platform_status.tiktok || clip.platform_status.instagram,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [publishedStand]);

  useEffect(() => {
    if (!eligibleClips.length) {
      setSelectedClipId(null);
      return;
    }
    setSelectedClipId((current) => (
      current && eligibleClips.some((clip) => clip.clip_db_id === current)
        ? current
        : eligibleClips[0].clip_db_id
    ));
  }, [eligibleClips]);

  const analyticsQuery = useQuery({
    queryKey: ['social-media', 'analytics', selectedClipId],
    queryFn: () => fetchClipAnalytics(selectedClipId!),
    enabled: !!selectedClipId,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const reportsQuery = useQuery({
    queryKey: ['social-media', 'reports', streamer],
    queryFn: () => fetchReports({ streamer, limit: 12 }),
    enabled: !!streamer && isAdmin,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const streamerReportMutation = useMutation({
    mutationFn: () => runReport({ kind: 'streamer', streamer }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'reports'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'analytics'] });
    },
  });

  const crossReportMutation = useMutation({
    mutationFn: () => runReport({ kind: 'cross' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'reports'] });
    },
  });

  const analyticsRows = normalizeChartRows(analyticsQuery.data?.items ?? []);
  const reportItems = reportsQuery.data?.items ?? [];
  const latestAdminReport = reportItems.find((item) => item.kind === 'admin');
  const latestStreamerReport = reportItems.find((item) => item.kind === 'streamer');
  const reportMutationFehler =
    fehlerText(streamerReportMutation.error, t) ?? fehlerText(crossReportMutation.error, t);
  const reportMutationPending =
    streamerReportMutation.isPending || crossReportMutation.isPending;
  const reportMutationErfolg =
    !reportMutationFehler &&
    !reportMutationPending &&
    (streamerReportMutation.isSuccess || crossReportMutation.isSuccess);

  return (
    <div className="space-y-6">
      <div
        className={`grid grid-cols-1 gap-6 ${
          isAdmin ? 'xl:grid-cols-[minmax(0,1.1fr)_minmax(320px,0.9fr)]' : ''
        }`}
      >
        <section className="panel-card rounded-2xl p-5 md:p-6 space-y-5 overflow-hidden relative">
          <div className="absolute -top-12 -right-10 h-44 w-44 rounded-full bg-orange/12 blur-3xl pointer-events-none" />
          <div className="relative flex flex-wrap items-center gap-3">
            <div>
              <div className="inline-flex items-center gap-2 text-[11px] uppercase tracking-[0.16em] font-bold text-orange/90">
                <BarChart3 aria-hidden="true" className="w-3.5 h-3.5" /> {t('Phase 3 · Performance')}
              </div>
              <h3 className="text-xl font-bold text-white mt-1">{t('Analytics je Clip und Plattform')}</h3>
            </div>
            <div className="ml-auto flex min-w-0 flex-col gap-1.5">
              <label
                htmlFor={`${interfaceId}-clip`}
                className="text-xs font-semibold text-text-secondary"
              >
                {t('Clip für Analytics auswählen')}
              </label>
              <select
                id={`${interfaceId}-clip`}
                value={selectedClipId ?? ''}
                onChange={(event) => setSelectedClipId(Number(event.target.value))}
                disabled={publishedLoading || !!publishedError || !eligibleClips.length}
                className="min-h-11 w-full min-w-0 rounded-xl border border-orange/70 bg-bg/70 px-3 py-2 text-sm text-white transition focus-visible:border-orange focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-orange disabled:cursor-not-allowed disabled:opacity-60 sm:min-w-[220px]"
              >
                {!eligibleClips.length && (
                  <option value="">{t('Kein Clip verfügbar')}</option>
                )}
                {eligibleClips.map((clip) => (
                  <option key={clip.clip_db_id} value={clip.clip_db_id}>
                    {clip.title}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {publishedLoading ? (
            <div
              role="status"
              aria-live="polite"
              className="h-[320px] flex items-center justify-center gap-2 text-sm text-text-secondary"
            >
              <Loader2 aria-hidden="true" className="w-5 h-5 text-orange animate-spin" />
              {t('Veröffentlichte Clips werden geladen…')}
            </div>
          ) : publishedError ? (
            <div
              role="alert"
              className="rounded-2xl border border-danger/35 bg-danger/10 p-8 text-sm text-danger text-center"
            >
              {fehlerText(publishedError, t)}
            </div>
          ) : !eligibleClips.length ? (
            <div className="rounded-2xl border border-border bg-bg/40 p-8 text-sm text-text-secondary text-center">
              {t('Noch keine veröffentlichten Clips mit Plattform-ID vorhanden.')}
            </div>
          ) : analyticsQuery.isLoading ? (
            <div
              role="status"
              aria-live="polite"
              className="h-[320px] flex items-center justify-center gap-2 text-sm text-text-secondary"
            >
              <Loader2 aria-hidden="true" className="w-5 h-5 text-orange animate-spin" />
              {t('Analytics werden geladen…')}
            </div>
          ) : analyticsQuery.error ? (
            <div
              role="alert"
              className="rounded-2xl border border-danger/35 bg-danger/10 p-8 text-sm text-danger text-center"
            >
              {fehlerText(analyticsQuery.error, t)}
            </div>
          ) : (
            <div className="space-y-4">
              <p
                id={`${interfaceId}-chart-help`}
                className="rounded-xl border border-border bg-bg/35 px-4 py-3 text-sm text-text-secondary"
              >
                {t('Tastaturhilfe: Diagramm mit Tab fokussieren, mit Pfeil links und rechts Werte durchgehen und mit Enter Details ein- oder ausblenden. Alle Werte stehen zusätzlich in der Datentabelle.')}
              </p>
              <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
                <div className="rounded-2xl border border-border bg-bg/35 p-4">
                  <h4 className="text-xs font-bold uppercase tracking-[0.14em] text-text-secondary mb-3">
                    {t('Aufrufe nach Zeitraum')}
                  </h4>
                  <div className="h-[260px]">
                    <ResponsiveContainer width="100%" height="100%">
                      <BarChart
                        data={analyticsRows}
                        accessibilityLayer
                        title={t('Aufrufe nach Zeitraum')}
                        desc={t('Balkendiagramm mit den Aufrufen auf YouTube, TikTok und Instagram für 24 Stunden, 7 Tage und 30 Tage.')}
                        aria-describedby={`${interfaceId}-chart-help`}
                      >
                        <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                        <XAxis dataKey="bucket" stroke="var(--color-text-secondary)" tickLine={false} axisLine={false} />
                        <YAxis stroke="var(--color-text-secondary)" tickLine={false} axisLine={false} />
                        <Tooltip
                          contentStyle={{ background: 'var(--color-popover)', border: '1px solid var(--color-border)', borderRadius: 16 }}
                        />
                        <Legend />
                        <Bar dataKey="youtube_views" name="YouTube" fill="#C5A059" radius={[8, 8, 0, 0]} />
                        <Bar dataKey="tiktok_views" name="TikTok" fill="#00D9FF" radius={[8, 8, 0, 0]} />
                        <Bar dataKey="instagram_views" name="Instagram" fill="#FF5A3C" radius={[8, 8, 0, 0]} />
                      </BarChart>
                    </ResponsiveContainer>
                  </div>
                </div>

                <div className="rounded-2xl border border-border bg-bg/35 p-4">
                  <h4 className="text-xs font-bold uppercase tracking-[0.14em] text-text-secondary mb-3">
                    {t('Engagement-Rate')}
                  </h4>
                  <div className="h-[260px]">
                    <ResponsiveContainer width="100%" height="100%">
                      <LineChart
                        data={analyticsRows}
                        accessibilityLayer
                        title={t('Engagement-Rate')}
                        desc={t('Liniendiagramm mit den Engagement-Raten auf YouTube, TikTok und Instagram für 24 Stunden, 7 Tage und 30 Tage.')}
                        aria-describedby={`${interfaceId}-chart-help`}
                      >
                        <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                        <XAxis dataKey="bucket" stroke="var(--color-text-secondary)" tickLine={false} axisLine={false} />
                        <YAxis stroke="var(--color-text-secondary)" tickLine={false} axisLine={false} />
                        <Tooltip
                          formatter={(value) => (value == null ? '—' : `${Number(value).toFixed(2)}%`)}
                          contentStyle={{ background: 'var(--color-popover)', border: '1px solid var(--color-border)', borderRadius: 16 }}
                        />
                        <Legend />
                        <Line type="monotone" dataKey="youtube_er" name="YouTube" stroke="#C5A059" strokeWidth={2.5} dot={{ r: 4 }} />
                        <Line type="monotone" dataKey="tiktok_er" name="TikTok" stroke="#00D9FF" strokeWidth={2.5} dot={{ r: 4 }} />
                        <Line type="monotone" dataKey="instagram_er" name="Instagram" stroke="#FF5A3C" strokeWidth={2.5} dot={{ r: 4 }} />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                </div>
              </div>

              <details className="rounded-2xl border border-border bg-bg/35">
                <summary className="flex min-h-11 cursor-pointer items-center px-4 py-3 text-sm font-semibold text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-orange">
                  {t('Datentabelle zu den Diagrammen anzeigen')}
                </summary>
                <div className="overflow-x-auto border-t border-border p-4">
                  <table className="w-full min-w-[760px] border-collapse text-left text-sm">
                    <caption className="sr-only">{t('Analytics-Werte des ausgewählten Clips')}</caption>
                    <thead>
                      <tr className="border-b border-border text-text-secondary">
                        <th scope="col" className="px-3 py-2 font-semibold">{t('Zeitraum')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('YouTube-Aufrufe')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('TikTok-Aufrufe')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('Instagram-Aufrufe')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('YouTube-Engagement')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('TikTok-Engagement')}</th>
                        <th scope="col" className="px-3 py-2 font-semibold">{t('Instagram-Engagement')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {analyticsRows.map((row) => (
                        <tr key={row.bucket} className="border-b border-border/70 last:border-b-0">
                          <th scope="row" className="px-3 py-2 font-semibold text-white">{row.bucket}</th>
                          <td className="px-3 py-2 text-text-primary">{row.youtube_views.toLocaleString(locale)}</td>
                          <td className="px-3 py-2 text-text-primary">{row.tiktok_views.toLocaleString(locale)}</td>
                          <td className="px-3 py-2 text-text-primary">{row.instagram_views.toLocaleString(locale)}</td>
                          <td className="px-3 py-2 text-text-primary">{row.youtube_er == null ? '—' : `${row.youtube_er.toLocaleString(locale, { maximumFractionDigits: 2 })} %`}</td>
                          <td className="px-3 py-2 text-text-primary">{row.tiktok_er == null ? '—' : `${row.tiktok_er.toLocaleString(locale, { maximumFractionDigits: 2 })} %`}</td>
                          <td className="px-3 py-2 text-text-primary">{row.instagram_er == null ? '—' : `${row.instagram_er.toLocaleString(locale, { maximumFractionDigits: 2 })} %`}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </details>
            </div>
          )}
        </section>

        {isAdmin && (
        <aside className="panel-card rounded-2xl p-5 md:p-6 space-y-4">
          <div className="flex items-center gap-2">
            <Sparkles aria-hidden="true" className="w-4 h-4 text-accent" />
            <h3 className="text-lg font-bold text-white">{t('LLM-Reports')}</h3>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <button
              type="button"
              onClick={() => {
                crossReportMutation.reset();
                streamerReportMutation.mutate();
              }}
              disabled={reportMutationPending}
              className="min-h-11 rounded-xl border border-orange/30 bg-orange/12 px-4 py-3 text-left hover:bg-orange/18 transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-orange disabled:opacity-50"
            >
              <div className="text-xs font-bold uppercase tracking-[0.14em] text-orange">{t('Streamer')}</div>
              <div className="text-sm text-white mt-1">{t('Wochenreport für {streamer}', { streamer })}</div>
            </button>
            <button
              type="button"
              onClick={() => {
                streamerReportMutation.reset();
                crossReportMutation.mutate();
              }}
              disabled={reportMutationPending}
              className="min-h-11 rounded-xl border border-accent/30 bg-accent/12 px-4 py-3 text-left hover:bg-accent/18 transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:opacity-50"
            >
              <div className="text-xs font-bold uppercase tracking-[0.14em] text-accent">{t('Cross')}</div>
              <div className="text-sm text-white mt-1">{t('Monatsreport über alle Streamer')}</div>
            </button>
          </div>
          {reportMutationPending && (
            <div role="status" aria-live="polite" className="text-xs text-text-secondary inline-flex items-center gap-2">
              <RefreshCw aria-hidden="true" className="w-3.5 h-3.5 animate-spin" /> {t('Report wird generiert…')}
            </div>
          )}
          {reportMutationFehler && (
            <div role="alert" className="rounded-xl border border-danger/35 bg-danger/10 p-3 text-xs text-danger">
              {reportMutationFehler}
            </div>
          )}
          {reportMutationErfolg && (
            <div role="status" aria-live="polite" className="rounded-xl border border-success/35 bg-success/10 p-3 text-xs text-success">
              {t('Der Report wurde erstellt.')}
            </div>
          )}
          {!reportsQuery.isLoading && !reportsQuery.error && (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="rounded-xl border border-border bg-bg/40 p-3">
              <div className="text-[11px] uppercase tracking-[0.14em] text-text-secondary font-bold">{t('Letzter Streamer-Report')}</div>
              <div className="text-sm text-white mt-1">{latestStreamerReport ? formatDate(latestStreamerReport.created_at, locale) : '—'}</div>
            </div>
            <div className="rounded-xl border border-border bg-bg/40 p-3">
              <div className="text-[11px] uppercase tracking-[0.14em] text-text-secondary font-bold">{t('Letzter Admin-DM-Stand')}</div>
              <div className="text-sm text-white mt-1">{latestAdminReport ? formatDate(latestAdminReport.created_at, locale) : '—'}</div>
            </div>
          </div>
          )}
        </aside>
        )}
      </div>

      {isAdmin && (
      <section className="panel-card rounded-2xl p-5 md:p-6 space-y-4">
        <div className="flex items-center gap-2">
          <FileText aria-hidden="true" className="w-4 h-4 text-orange" />
          <h3 className="text-lg font-bold text-white">{t('Gespeicherte Reports')}</h3>
          {!reportsQuery.isLoading && !reportsQuery.error && (
            <div className="ml-auto text-xs text-text-secondary inline-flex items-center gap-1.5">
              <CalendarRange aria-hidden="true" className="w-3.5 h-3.5" /> {t('{count} Einträge', { count: reportItems.length })}
            </div>
          )}
        </div>

        {reportsQuery.isLoading ? (
          <div role="status" aria-live="polite" className="py-10 flex items-center justify-center gap-2 text-sm text-text-secondary">
            <Loader2 aria-hidden="true" className="w-5 h-5 text-orange animate-spin" />
            {t('Gespeicherte Reports werden geladen…')}
          </div>
        ) : reportsQuery.error ? (
          <div role="alert" className="rounded-2xl border border-danger/35 bg-danger/10 p-8 text-sm text-danger text-center">
            {fehlerText(reportsQuery.error, t)}
          </div>
        ) : reportItems.length === 0 ? (
          <div className="rounded-2xl border border-border bg-bg/35 p-8 text-sm text-text-secondary text-center">
            {t('Noch keine Reports gespeichert.')}
          </div>
        ) : (
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
            {reportItems.map((report) => (
              <article key={report.id} className="rounded-2xl border border-border bg-bg/35 p-4 space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className={`text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border ${toneClass(report.kind)}`}>
                    {t(REPORT_KIND_LABELS[report.kind])}
                  </span>
                  {report.streamer_login && (
                    <span className="text-[10px] font-mono text-text-secondary bg-bg/60 px-2 py-1 rounded-md border border-border">
                      @{report.streamer_login}
                    </span>
                  )}
                  <span className="ml-auto text-[11px] text-text-secondary">
                    {formatDate(report.created_at, locale)}
                  </span>
                </div>
                <div className="text-[11px] text-text-secondary">
                  {t('Zeitraum: {from} bis {to}', {
                    from: formatDate(report.period_start, locale),
                    to: formatDate(report.period_end, locale),
                  })}
                </div>
                <div className="rounded-xl border border-border bg-[var(--color-popover)] p-3 max-h-[280px] overflow-auto">
                  <pre className="whitespace-pre-wrap text-[12px] leading-6 text-text-primary font-sans">
                    {report.content_md}
                  </pre>
                </div>
                {report.model && (
                  <div className="text-[11px] text-text-secondary font-mono">
                    {report.model}
                  </div>
                )}
              </article>
            ))}
          </div>
        )}
      </section>
      )}
    </div>
  );
}
