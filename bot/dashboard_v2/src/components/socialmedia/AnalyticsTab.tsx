import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
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
  fetchReports,
  runReport,
  SocialMediaForbiddenError,
} from '@/api/socialMedia';
import type {
  ClipAnalytics,
  SocialClip,
  SocialMediaReportKind,
} from '@/types/socialMedia';

const BUCKET_ORDER = ['24h', '7d', '30d'] as const;

interface AnalyticsTabProps {
  streamer: string;
  clips: SocialClip[];
}

function kindLabel(kind: SocialMediaReportKind): string {
  if (kind === 'streamer') return 'Streamer';
  if (kind === 'cross') return 'Cross';
  return 'Admin';
}

function toneClass(kind: SocialMediaReportKind): string {
  if (kind === 'streamer') return 'bg-orange/15 text-orange border-orange/35';
  if (kind === 'cross') return 'bg-teal/15 text-teal border-teal/35';
  return 'bg-bg/70 text-white border-border';
}

function formatDate(value: string | null | undefined): string {
  if (!value) return '—';
  return new Date(value).toLocaleString('de-DE', {
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

export function AnalyticsTab({ streamer, clips }: AnalyticsTabProps) {
  const queryClient = useQueryClient();
  const [selectedClipId, setSelectedClipId] = useState<number | null>(null);
  const eligibleClips = useMemo(
    () => clips.filter((clip) => clip.platform_status.youtube || clip.platform_status.tiktok || clip.platform_status.instagram),
    [clips],
  );

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
    enabled: !!streamer,
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

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 xl:grid-cols-[minmax(0,1.1fr)_minmax(320px,0.9fr)] gap-6">
        <section className="panel-card rounded-2xl p-5 md:p-6 space-y-5 overflow-hidden relative">
          <div className="absolute -top-12 -right-10 h-44 w-44 rounded-full bg-orange/12 blur-3xl pointer-events-none" />
          <div className="relative flex flex-wrap items-center gap-3">
            <div>
              <div className="inline-flex items-center gap-2 text-[11px] uppercase tracking-[0.16em] font-bold text-orange/90">
                <BarChart3 className="w-3.5 h-3.5" /> Phase 3 · Performance
              </div>
              <h3 className="text-xl font-bold text-white mt-1">Analytics je Clip und Plattform</h3>
            </div>
            <div className="ml-auto flex items-center gap-2">
              <select
                value={selectedClipId ?? ''}
                onChange={(event) => setSelectedClipId(Number(event.target.value))}
                className="rounded-xl border border-border bg-bg/70 px-3 py-2 text-sm text-white min-w-[220px]"
              >
                {eligibleClips.map((clip) => (
                  <option key={clip.clip_db_id} value={clip.clip_db_id}>
                    {clip.title}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {!eligibleClips.length ? (
            <div className="rounded-2xl border border-border bg-bg/40 p-8 text-sm text-text-secondary text-center">
              Noch keine veroeffentlichten Clips mit Plattform-ID vorhanden.
            </div>
          ) : analyticsQuery.isLoading ? (
            <div className="h-[320px] flex items-center justify-center">
              <Loader2 className="w-5 h-5 text-orange animate-spin" />
            </div>
          ) : (
            <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
              <div className="rounded-2xl border border-border bg-bg/35 p-4">
                <div className="text-xs font-bold uppercase tracking-[0.14em] text-text-secondary mb-3">
                  Views nach Bucket
                </div>
                <div className="h-[260px]">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={analyticsRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis dataKey="bucket" stroke="#96a0b5" tickLine={false} axisLine={false} />
                      <YAxis stroke="#96a0b5" tickLine={false} axisLine={false} />
                      <Tooltip
                        contentStyle={{ background: '#0f1720', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 16 }}
                      />
                      <Legend />
                      <Bar dataKey="youtube_views" name="YouTube" fill="#ff7a18" radius={[8, 8, 0, 0]} />
                      <Bar dataKey="tiktok_views" name="TikTok" fill="#10b7ad" radius={[8, 8, 0, 0]} />
                      <Bar dataKey="instagram_views" name="Instagram" fill="#ffb38a" radius={[8, 8, 0, 0]} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>

              <div className="rounded-2xl border border-border bg-bg/35 p-4">
                <div className="text-xs font-bold uppercase tracking-[0.14em] text-text-secondary mb-3">
                  Engagement-Rate
                </div>
                <div className="h-[260px]">
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={analyticsRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis dataKey="bucket" stroke="#96a0b5" tickLine={false} axisLine={false} />
                      <YAxis stroke="#96a0b5" tickLine={false} axisLine={false} />
                      <Tooltip
                        formatter={(value) => (value == null ? '—' : `${Number(value).toFixed(2)}%`)}
                        contentStyle={{ background: '#0f1720', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 16 }}
                      />
                      <Legend />
                      <Line type="monotone" dataKey="youtube_er" name="YouTube" stroke="#ff7a18" strokeWidth={2.5} dot={{ r: 4 }} />
                      <Line type="monotone" dataKey="tiktok_er" name="TikTok" stroke="#10b7ad" strokeWidth={2.5} dot={{ r: 4 }} />
                      <Line type="monotone" dataKey="instagram_er" name="Instagram" stroke="#ffd0aa" strokeWidth={2.5} dot={{ r: 4 }} />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </div>
          )}
        </section>

        <aside className="panel-card rounded-2xl p-5 md:p-6 space-y-4">
          <div className="flex items-center gap-2">
            <Sparkles className="w-4 h-4 text-teal" />
            <h3 className="text-lg font-bold text-white">LLM-Reports</h3>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <button
              type="button"
              onClick={() => streamerReportMutation.mutate()}
              disabled={streamerReportMutation.isPending}
              className="rounded-xl border border-orange/30 bg-orange/12 px-4 py-3 text-left hover:bg-orange/18 transition disabled:opacity-50"
            >
              <div className="text-xs font-bold uppercase tracking-[0.14em] text-orange">Streamer</div>
              <div className="text-sm text-white mt-1">Wochenreport fuer {streamer}</div>
            </button>
            <button
              type="button"
              onClick={() => crossReportMutation.mutate()}
              disabled={crossReportMutation.isPending}
              className="rounded-xl border border-teal/30 bg-teal/12 px-4 py-3 text-left hover:bg-teal/18 transition disabled:opacity-50"
            >
              <div className="text-xs font-bold uppercase tracking-[0.14em] text-teal">Cross</div>
              <div className="text-sm text-white mt-1">Monatsreport ueber alle Streamer</div>
            </button>
          </div>
          {(streamerReportMutation.isPending || crossReportMutation.isPending) && (
            <div className="text-xs text-text-secondary inline-flex items-center gap-2">
              <RefreshCw className="w-3.5 h-3.5 animate-spin" /> Report wird generiert…
            </div>
          )}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="rounded-xl border border-border bg-bg/40 p-3">
              <div className="text-[11px] uppercase tracking-[0.14em] text-text-secondary font-bold">Letzter Streamer-Report</div>
              <div className="text-sm text-white mt-1">{latestStreamerReport ? formatDate(latestStreamerReport.created_at) : '—'}</div>
            </div>
            <div className="rounded-xl border border-border bg-bg/40 p-3">
              <div className="text-[11px] uppercase tracking-[0.14em] text-text-secondary font-bold">Letzter Admin-DM-Stand</div>
              <div className="text-sm text-white mt-1">{latestAdminReport ? formatDate(latestAdminReport.created_at) : '—'}</div>
            </div>
          </div>
        </aside>
      </div>

      <section className="panel-card rounded-2xl p-5 md:p-6 space-y-4">
        <div className="flex items-center gap-2">
          <FileText className="w-4 h-4 text-orange" />
          <h3 className="text-lg font-bold text-white">Gespeicherte Reports</h3>
          <div className="ml-auto text-xs text-text-secondary inline-flex items-center gap-1.5">
            <CalendarRange className="w-3.5 h-3.5" /> {reportItems.length} Eintraege
          </div>
        </div>

        {reportsQuery.isLoading ? (
          <div className="py-10 flex items-center justify-center">
            <Loader2 className="w-5 h-5 text-orange animate-spin" />
          </div>
        ) : reportItems.length === 0 ? (
          <div className="rounded-2xl border border-border bg-bg/35 p-8 text-sm text-text-secondary text-center">
            Noch keine Reports gespeichert.
          </div>
        ) : (
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
            {reportItems.map((report) => (
              <article key={report.id} className="rounded-2xl border border-border bg-bg/35 p-4 space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className={`text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border ${toneClass(report.kind)}`}>
                    {kindLabel(report.kind)}
                  </span>
                  {report.streamer_login && (
                    <span className="text-[10px] font-mono text-text-secondary bg-bg/60 px-2 py-1 rounded-md border border-border">
                      @{report.streamer_login}
                    </span>
                  )}
                  <span className="ml-auto text-[11px] text-text-secondary">
                    {formatDate(report.created_at)}
                  </span>
                </div>
                <div className="text-[11px] text-text-secondary">
                  Zeitraum: {formatDate(report.period_start)} bis {formatDate(report.period_end)}
                </div>
                <div className="rounded-xl border border-border bg-[#0d141d] p-3 max-h-[280px] overflow-auto">
                  <pre className="whitespace-pre-wrap text-[12px] leading-6 text-slate-100 font-sans">
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
    </div>
  );
}
