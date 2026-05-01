import { useEffect, useState } from 'react';
import {
  AlertCircle,
  CheckCircle2,
  FileText,
  Loader2,
  Lock,
  MessageSquareText,
  Sparkles,
  Zap,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { fetchOverview } from '@/api/analytics';
import { usePlan } from '@/context/PlanContext';
import { useStreamReport } from '@/hooks/useAnalytics';
import type {
  LegacyStreamReportBody,
  StreamReport,
  StreamReportV2Body,
  StreamReportVariant,
  TimeRange,
} from '@/types/analytics';

interface StreamReportsProps {
  streamer: string | null;
  days: TimeRange;
}

function isV2(report: StreamReport['report']): report is StreamReportV2Body {
  return !!report && ('summary' in report || 'highlights' in report || 'recommendations' in report);
}

function isLegacy(report: StreamReport['report']): report is LegacyStreamReportBody {
  return !!report && !isV2(report);
}

function formatDate(value?: string | null): string {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return String(value);
  return date.toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' });
}

function VariantBadge({ variant }: { variant: StreamReportVariant }) {
  const isFull = variant === 'full';
  return (
    <span
      className={`rounded-full border px-2.5 py-1 text-xs font-bold uppercase tracking-wide ${
        isFull
          ? 'border-orange-400/30 bg-orange-500/15 text-orange-200'
          : 'border-cyan-400/30 bg-cyan-500/15 text-cyan-200'
      }`}
    >
      {isFull ? 'B / Full' : 'A / Compact'}
    </span>
  );
}

function ReportBody({ report }: { report: StreamReport }) {
  if (report.empty) {
    return <p className="text-sm text-text-secondary">Für diese Session liegt noch kein Report vor.</p>;
  }
  if (report.status === 'pending') {
    return (
      <div className="flex items-center gap-2 text-sm text-text-secondary">
        <Loader2 className="h-4 w-4 animate-spin text-accent" />
        Report wird erstellt...
      </div>
    );
  }
  if (report.status === 'failed') {
    return (
      <div className="rounded-xl border border-error/25 bg-error/10 p-4 text-sm text-error">
        Fehlgeschlagen: {report.error || 'Unbekannter Fehler'}
      </div>
    );
  }

  const body = report.report;
  if (!body) return <p className="text-sm text-text-secondary">Report ist leer.</p>;

  if (isV2(body)) {
    const summary = body.summary;
    return (
      <div className="space-y-5">
        {summary && (
          <div className="rounded-xl border border-purple-500/20 bg-purple-500/10 p-4">
            <div className="mb-2 flex items-center justify-between gap-3">
              <h3 className="font-semibold text-white">{summary.headline || 'Stream Report'}</h3>
              {summary.overall_rating && (
                <span className="rounded-full bg-white/10 px-2 py-1 text-xs text-white/70">
                  {summary.overall_rating}
                </span>
              )}
            </div>
            {(summary.tldr || []).length > 0 && (
              <ul className="space-y-1 text-sm text-text-secondary">
                {(summary.tldr || []).map((item, index) => (
                  <li key={index}>• {item}</li>
                ))}
              </ul>
            )}
          </div>
        )}

        {(body.highlights || []).length > 0 && (
          <section className="space-y-2">
            <h4 className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-green-400">
              <CheckCircle2 className="h-4 w-4" /> Highlights
            </h4>
            {(body.highlights || []).map((item, index) => (
              <div key={index} className="rounded-xl border border-green-500/20 bg-green-500/10 p-3">
                <p className="text-sm font-semibold text-green-200">{item.title}</p>
                {item.evidence && <p className="mt-1 text-xs text-white/60">Beleg: {item.evidence}</p>}
                {item.why_it_matters && <p className="mt-1 text-xs text-text-secondary">{item.why_it_matters}</p>}
              </div>
            ))}
          </section>
        )}

        {(body.problems || []).length > 0 && (
          <section className="space-y-2">
            <h4 className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-red-400">
              <AlertCircle className="h-4 w-4" /> Probleme
            </h4>
            {(body.problems || []).map((item, index) => (
              <div key={index} className="rounded-xl border border-red-500/20 bg-red-500/10 p-3">
                <p className="text-sm font-semibold text-red-200">{item.title}</p>
                {item.evidence && <p className="mt-1 text-xs text-white/60">Beleg: {item.evidence}</p>}
                {item.impact && <p className="mt-1 text-xs text-text-secondary">{item.impact}</p>}
              </div>
            ))}
          </section>
        )}

        {(body.recommendations || []).length > 0 && (
          <section className="space-y-2">
            <h4 className="flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-purple-300">
              <Zap className="h-4 w-4" /> Empfehlungen
            </h4>
            {(body.recommendations || []).map((item, index) => (
              <div key={index} className="rounded-xl border border-purple-500/20 bg-purple-500/10 p-3">
                <div className="mb-1 flex items-center gap-2">
                  <span className="rounded-full bg-purple-500/20 px-2 py-0.5 text-[10px] font-bold uppercase text-purple-200">
                    {item.priority || 'medium'}
                  </span>
                  <p className="text-sm font-semibold text-purple-200">{item.action}</p>
                </div>
                <p className="text-xs text-text-secondary">{item.reason}</p>
              </div>
            ))}
          </section>
        )}

        {body.chat_analysis && (
          <section className="rounded-xl border border-border bg-white/5 p-4">
            <h4 className="mb-2 flex items-center gap-2 text-xs font-bold uppercase tracking-wide text-text-secondary">
              <MessageSquareText className="h-4 w-4" /> Chat Analyse
            </h4>
            <div className="space-y-2 text-sm text-text-secondary">
              {body.chat_analysis.sentiment && <p>Sentiment: {body.chat_analysis.sentiment}</p>}
              {(body.chat_analysis.main_topics || []).length > 0 && (
                <p>Themen: {(body.chat_analysis.main_topics || []).join(', ')}</p>
              )}
              {(body.chat_analysis.questions_or_confusion || []).length > 0 && (
                <p>Fragen/Unklarheiten: {(body.chat_analysis.questions_or_confusion || []).join(' · ')}</p>
              )}
            </div>
          </section>
        )}
      </div>
    );
  }

  if (isLegacy(body)) {
    return (
      <div className="space-y-4">
        {(body.gut || []).map((item, index) => (
          <div key={`good-${index}`} className="rounded-xl border border-green-500/20 bg-green-500/10 p-3">
            <p className="text-sm font-semibold text-green-200">{item.punkt}</p>
            <p className="mt-1 text-xs text-text-secondary">{item.begruendung}</p>
          </div>
        ))}
        {(body.schlecht || []).map((item, index) => (
          <div key={`bad-${index}`} className="rounded-xl border border-red-500/20 bg-red-500/10 p-3">
            <p className="text-sm font-semibold text-red-200">{item.punkt}</p>
            <p className="mt-1 text-xs text-text-secondary">{item.begruendung}</p>
          </div>
        ))}
        {(body.empfehlungen || []).map((item, index) => (
          <div key={`rec-${index}`} className="rounded-xl border border-purple-500/20 bg-purple-500/10 p-3">
            <p className="text-sm font-semibold text-purple-200">{item.trend}</p>
            <p className="mt-1 text-xs text-text-secondary">{item.empfehlung}</p>
          </div>
        ))}
      </div>
    );
  }

  return <p className="text-sm text-text-secondary">Unbekanntes Report-Format.</p>;
}

function ReportColumn({ streamer, sessionId, variant }: { streamer: string | null; sessionId?: number; variant: StreamReportVariant }) {
  const { data, isLoading, error } = useStreamReport(streamer, sessionId, variant);
  const isFull = variant === 'full';

  return (
    <div className="panel-card rounded-2xl p-5">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="mb-2 flex items-center gap-2">
            <VariantBadge variant={variant} />
            <span className="text-xs text-white/40">{data?.model || 'MiniMax'}</span>
          </div>
          <h2 className="text-lg font-bold text-white">
            {isFull ? 'Full / alle Daten' : 'Compact / Evidence'}
          </h2>
          <p className="mt-1 text-xs text-text-secondary">
            {isFull
              ? 'Raw-heavy Prompt mit Chat, Chatter, Events und voller Timeline.'
              : 'Verdichtete Signale aus allen Daten, günstiger und stabiler.'}
          </p>
        </div>
        {data?.generated_at && <span className="text-xs text-white/35">{formatDate(data.generated_at)}</span>}
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center gap-2 py-12 text-sm text-text-secondary">
          <Loader2 className="h-5 w-5 animate-spin text-accent" /> Lade {variant}-Report...
        </div>
      ) : error ? (
        <div className="rounded-xl border border-error/25 bg-error/10 p-4 text-sm text-error">
          Report konnte nicht geladen werden.
        </div>
      ) : data ? (
        <ReportBody report={data} />
      ) : null}
    </div>
  );
}

export function StreamReports({ streamer, days }: StreamReportsProps) {
  const { isFeatureLocked } = usePlan();
  const locked = isFeatureLocked('post_stream_report');
  const [selectedSessionId, setSelectedSessionId] = useState<number | undefined>(undefined);

  const { data: overview, isLoading: loadingSessions } = useQuery({
    queryKey: ['overview', streamer, days, 'stream-reports'],
    queryFn: () => fetchOverview(streamer, days),
    enabled: !!streamer && !locked,
    staleTime: 2 * 60 * 1000,
  });

  const sessions = overview?.sessions || [];

  useEffect(() => {
    if (!selectedSessionId && sessions.length > 0) {
      setSelectedSessionId(sessions[0].id);
    }
  }, [selectedSessionId, sessions]);

  if (locked) {
    return (
      <div className="flex flex-col items-center justify-center h-80 gap-5">
        <div className="relative">
          <div className="w-20 h-20 rounded-2xl bg-background/80 border border-border flex items-center justify-center">
            <FileText className="w-9 h-9 text-text-secondary opacity-40" />
          </div>
          <div className="absolute -bottom-1 -right-1 w-7 h-7 rounded-full bg-border flex items-center justify-center">
            <Lock className="w-3.5 h-3.5 text-text-secondary" />
          </div>
        </div>
        <div className="text-center max-w-sm">
          <p className="text-white font-semibold text-lg mb-1">Stream Reports brauchen KI-Zugang</p>
          <p className="text-text-secondary text-sm leading-relaxed">
            Automatische Post-Stream-Reports sind ab dem KI-Plan verfügbar.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="panel-card rounded-2xl p-6">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex items-center gap-4">
            <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-xl border border-purple-500/20 bg-purple-500/10">
              <Sparkles className="h-6 w-6 text-purple-300" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-white">Stream Reports</h1>
              <p className="mt-1 text-sm text-text-secondary">
                A/B-Vergleich nach jedem Stream: Compact gegen Full-Rohdaten.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <VariantBadge variant="compact" />
            <VariantBadge variant="full" />
          </div>
        </div>
      </div>

      {loadingSessions ? (
        <div className="flex items-center justify-center gap-2 py-16 text-text-secondary">
          <Loader2 className="h-6 w-6 animate-spin text-accent" /> Lade Sessions...
        </div>
      ) : sessions.length === 0 ? (
        <div className="panel-card rounded-2xl p-8 text-center">
          <AlertCircle className="mx-auto mb-3 h-10 w-10 text-text-secondary" />
          <p className="text-white font-semibold">Keine Sessions gefunden</p>
          <p className="mt-1 text-sm text-text-secondary">Nach dem nächsten abgeschlossenen Stream erscheinen hier Reports.</p>
        </div>
      ) : (
        <>
          <div className="panel-card rounded-2xl p-4">
            <div className="mb-3 text-xs font-bold uppercase tracking-wide text-text-secondary">
              Session auswählen
            </div>
            <div className="flex gap-2 overflow-x-auto pb-1">
              {sessions.slice(0, 20).map((session) => (
                <button
                  key={session.id}
                  type="button"
                  onClick={() => setSelectedSessionId(session.id)}
                  className={`min-w-[220px] rounded-xl border px-4 py-3 text-left transition-colors ${
                    selectedSessionId === session.id
                      ? 'border-primary/50 bg-primary/15 text-white'
                      : 'border-border bg-white/5 text-text-secondary hover:border-white/20 hover:text-white'
                  }`}
                >
                  <div className="truncate text-sm font-semibold">{session.title || 'Untitled Stream'}</div>
                  <div className="mt-1 text-xs opacity-70">
                    {session.date} · Ø {Math.round(session.avgViewers)} · Peak {session.peakViewers}
                  </div>
                </button>
              ))}
            </div>
          </div>

          <div className="grid gap-5 xl:grid-cols-2">
            <ReportColumn streamer={streamer} sessionId={selectedSessionId} variant="compact" />
            <ReportColumn streamer={streamer} sessionId={selectedSessionId} variant="full" />
          </div>
        </>
      )}
    </div>
  );
}
