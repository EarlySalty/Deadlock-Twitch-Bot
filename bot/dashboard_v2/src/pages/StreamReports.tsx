import * as React from 'react';
import {
  AlertCircle,
  CheckCircle2,
  FileText,
  Loader2,
  Lock,
  MessageSquare,
  MessageSquareText,
  Send,
  Sparkles,
  ThumbsDown,
  ThumbsUp,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { fetchOverview } from '@/api/analytics';
import { buildApiUrl, withCookieCredentials } from '@/api/core';
import { usePlan } from '@/context/PlanContext';
import { useStreamReport } from '@/hooks/useAnalytics';
import type {
  LegacyStreamReportBody,
  StreamReport,
  StreamReportRating,
  StreamReportV2Body,
  StreamReportV3Body,
  StreamReportVariant,
  TimeRange,
} from '@/types/analytics';

interface StreamReportsProps {
  streamer: string | null;
  days: TimeRange;
}

type SnapshotRating = StreamReportV3Body['snapshot']['bewertung'];
type MomentType = StreamReportV3Body['momente'][number]['typ'];
type RatingValue = StreamReportRating['rating'];

const SNAPSHOT_TONES: Record<
  SnapshotRating,
  { banner: string; badge: string; accent: string }
> = {
  stark: {
    banner: 'border-green-400/30 bg-green-500/10',
    badge: 'border-green-400/40 bg-green-500/20 text-green-100',
    accent: 'text-green-200',
  },
  solide: {
    banner: 'border-cyan-400/30 bg-cyan-500/10',
    badge: 'border-cyan-400/40 bg-cyan-500/20 text-cyan-100',
    accent: 'text-cyan-200',
  },
  gemischt: {
    banner: 'border-yellow-400/30 bg-yellow-500/10',
    badge: 'border-yellow-400/40 bg-yellow-500/20 text-yellow-100',
    accent: 'text-yellow-100',
  },
  schwach: {
    banner: 'border-red-400/30 bg-red-500/10',
    badge: 'border-red-400/40 bg-red-500/20 text-red-100',
    accent: 'text-red-200',
  },
};

const MOMENT_TONES: Record<MomentType, string> = {
  peak: 'bg-green-400/80',
  einbruch: 'bg-red-400/80',
  stabil: 'bg-white/25',
  volatil: 'bg-yellow-400/80',
};

const MOMENT_BADGES: Record<MomentType, string> = {
  peak: 'bg-green-500/15 text-green-200',
  einbruch: 'bg-red-500/15 text-red-200',
  stabil: 'bg-white/10 text-white/70',
  volatil: 'bg-yellow-500/15 text-yellow-100',
};

const TREND_BADGES: Record<string, string> = {
  wachsend: 'border-green-400/30 bg-green-500/15 text-green-200',
  stagnierend: 'border-white/15 bg-white/10 text-white/75',
  ruecklaeufig: 'border-red-400/30 bg-red-500/15 text-red-200',
  'zu wenig Daten': 'border-yellow-400/30 bg-yellow-500/15 text-yellow-100',
};

const ACTION_TONES = {
  critical: 'border-red-400/35 bg-red-500/10',
  warning: 'border-orange-400/35 bg-orange-500/10',
  neutral: 'border-border bg-white/5',
};

const RATING_OPTIONS: Array<{
  value: RatingValue;
  label: string;
  icon: LucideIcon;
  activeClass: string;
  idleClass: string;
}> = [
  {
    value: 'gut',
    label: 'Gut',
    icon: ThumbsUp,
    activeClass: 'border-green-400/40 bg-green-500/15 text-green-200',
    idleClass: 'border-border bg-white/5 text-text-secondary hover:border-green-400/25 hover:text-white',
  },
  {
    value: 'neutral',
    label: 'Neutral',
    icon: MessageSquare,
    activeClass: 'border-white/20 bg-white/10 text-white',
    idleClass: 'border-border bg-white/5 text-text-secondary hover:border-white/20 hover:text-white',
  },
  {
    value: 'schlecht',
    label: 'Schlecht',
    icon: ThumbsDown,
    activeClass: 'border-red-400/40 bg-red-500/15 text-red-200',
    idleClass: 'border-border bg-white/5 text-text-secondary hover:border-red-400/25 hover:text-white',
  },
];

function isV3(report: StreamReport['report']): report is StreamReportV3Body {
  return !!report && 'snapshot' in report && 'momente' in report;
}

function isV2(report: StreamReport['report']): report is StreamReportV2Body {
  return !!report && ('summary' in report || 'highlights' in report || 'recommendations' in report);
}

function isLegacy(report: StreamReport['report']): report is LegacyStreamReportBody {
  return !!report && !isV3(report) && !isV2(report);
}

function formatDate(value?: string | null): string {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return String(value);
  return date.toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' });
}

function formatPercent(value?: number | null): string {
  if (value === undefined || value === null || Number.isNaN(value)) return 'n/a';
  return `${value.toFixed(1).replace('.', ',')}%`;
}

function formatDelta(value?: number | null): string {
  if (value === undefined || value === null || Number.isNaN(value)) return 'n/a';
  return `${value > 0 ? '+' : ''}${value}`;
}

function formatRatingLabel(value: RatingValue): string {
  if (value === 'gut') return 'Gut';
  if (value === 'schlecht') return 'Schlecht';
  return 'Neutral';
}

function sentimentBadgeClass(value: string): string {
  const normalized = value.toLowerCase();
  if (normalized.startsWith('positiv')) return 'border-green-400/30 bg-green-500/15 text-green-200';
  if (normalized.startsWith('negativ')) return 'border-red-400/30 bg-red-500/15 text-red-200';
  if (normalized.startsWith('gemischt')) return 'border-yellow-400/30 bg-yellow-500/15 text-yellow-100';
  return 'border-white/15 bg-white/10 text-white/75';
}

function trendBadgeClass(value: string): string {
  return TREND_BADGES[value] || 'border-white/15 bg-white/10 text-white/75';
}

async function readMutationError(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as { message?: string; error?: string };
    return payload.message || payload.error || `Server-Fehler (HTTP ${response.status})`;
  } catch {
    return `Server-Fehler (HTTP ${response.status})`;
  }
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

function SectionHeading({
  icon: Icon,
  title,
  accent = 'text-text-secondary',
}: {
  icon: LucideIcon;
  title: string;
  accent?: string;
}) {
  return (
    <h3 className={`flex items-center gap-2 text-xs font-bold uppercase tracking-wide ${accent}`}>
      <Icon className="h-4 w-4" />
      {title}
    </h3>
  );
}

function EmptyState({ text }: { text: string }) {
  return <p className="text-sm text-text-secondary">{text}</p>;
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

  if (isV3(body)) {
    const snapshotTone = SNAPSHOT_TONES[body.snapshot.bewertung];
    const sortedActions = [...(body.massnahmen || [])].sort((left, right) => left.prioritaet - right.prioritaet);

    return (
      <div className="space-y-4">
        <section className={`rounded-2xl border p-4 ${snapshotTone.banner}`}>
          <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
            <div className="space-y-2">
              <div className={`inline-flex rounded-full border px-3 py-1 text-xs font-bold uppercase tracking-[0.18em] ${snapshotTone.badge}`}>
                {body.snapshot.bewertung}
              </div>
              <h3 className="text-lg font-bold text-white">{body.snapshot.ein_satz}</h3>
              <p className={`text-sm ${snapshotTone.accent}`}>{body.snapshot.wichtigste_erkenntnis}</p>
            </div>
            <div className="rounded-xl border border-white/10 bg-black/10 px-3 py-2 text-xs text-white/60">
              Snapshot
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <SectionHeading icon={Zap} title="Kritische Momente" accent="text-white/85" />
          {(body.momente || []).length > 0 ? (
            <div className="space-y-2">
              {body.momente.map((moment, index) => (
                <div
                  key={`${moment.minute}-${index}`}
                  className="relative overflow-hidden rounded-xl border border-border bg-white/5 p-3 pl-4"
                >
                  <div className={`absolute inset-y-0 left-0 w-1 ${MOMENT_TONES[moment.typ]}`} />
                  <div className="mb-2 flex flex-wrap items-center gap-2">
                    <span className="rounded-full bg-white/10 px-2 py-1 text-[11px] font-bold uppercase tracking-wide text-white/70">
                      Minute {moment.minute}
                    </span>
                    <span className={`rounded-full px-2 py-1 text-[11px] font-bold uppercase tracking-wide ${MOMENT_BADGES[moment.typ]}`}>
                      {moment.typ}
                    </span>
                  </div>
                  <p className="text-sm font-semibold text-white">{moment.beobachtung}</p>
                  <p className="mt-1 text-xs text-text-secondary">{moment.interpretation}</p>
                </div>
              ))}
            </div>
          ) : (
            <EmptyState text="Keine markanten Momente im Report hinterlegt." />
          )}
        </section>

        <section className="space-y-2">
          <SectionHeading icon={MessageSquareText} title="Audience" />
          <div className="grid gap-3 md:grid-cols-3">
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Chat-Rate</p>
              <p className="mt-2 text-lg font-bold text-white">{formatPercent(body.audience.chat_rate_prozent)}</p>
              <p className="mt-1 text-xs text-text-secondary">{body.audience.chat_rate_einordnung}</p>
            </div>
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Stammchatter</p>
              <p className="mt-2 text-lg font-bold text-white">
                {formatPercent(body.audience.stammchatter_anteil_prozent)}
              </p>
              <p className="mt-1 text-xs text-text-secondary">Anteil wiederkehrender Kern-Chatters</p>
            </div>
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Bindung</p>
              <p className="mt-2 text-sm font-semibold text-white">{body.audience.bindung}</p>
              <p className="mt-1 text-xs text-text-secondary">{body.audience.auffaelligkeit}</p>
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <SectionHeading icon={MessageSquareText} title="Chat-Diagnose" />
          <div className="grid gap-3 md:grid-cols-2">
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Top-Themen</p>
              <div className="mt-3 flex flex-wrap gap-2">
                {(body.chat_diagnose.top_themen || []).length > 0 ? (
                  body.chat_diagnose.top_themen.map((item, index) => (
                    <span
                      key={`${item}-${index}`}
                      className="rounded-full border border-cyan-400/20 bg-cyan-500/10 px-2.5 py-1 text-xs text-cyan-100"
                    >
                      {item}
                    </span>
                  ))
                ) : (
                  <span className="text-sm text-text-secondary">Keine Themen markiert.</span>
                )}
              </div>
            </div>

            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Explosions-Momente</p>
              {(body.chat_diagnose.explosions_momente || []).length > 0 ? (
                <ul className="mt-3 space-y-2 text-sm text-text-secondary">
                  {body.chat_diagnose.explosions_momente.map((item, index) => (
                    <li key={`${item}-${index}`}>• {item}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-sm text-text-secondary">Keine Explosionen im Chat erkannt.</p>
              )}
            </div>

            <div className="rounded-xl border border-orange-400/20 bg-orange-500/10 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-orange-100/80">Verwirrung / Fragen</p>
              {(body.chat_diagnose.verwirrung_oder_fragen || []).length > 0 ? (
                <ul className="mt-3 space-y-2 text-sm text-orange-100/85">
                  {body.chat_diagnose.verwirrung_oder_fragen.map((item, index) => (
                    <li key={`${item}-${index}`}>• {item}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-sm text-orange-100/75">Keine nennenswerten Fragezeichen im Chat.</p>
              )}
            </div>

            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Stimmung</p>
              <div className="mt-3">
                <span className={`inline-flex rounded-full border px-2.5 py-1 text-xs font-semibold ${sentimentBadgeClass(body.chat_diagnose.stimmung)}`}>
                  {body.chat_diagnose.stimmung}
                </span>
              </div>
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <SectionHeading icon={Sparkles} title="Wachstum" />
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Follower Delta</p>
              <p
                className={`mt-2 text-lg font-bold ${
                  body.wachstum.follower_delta > 0
                    ? 'text-green-200'
                    : body.wachstum.follower_delta < 0
                      ? 'text-red-200'
                      : 'text-white'
                }`}
              >
                {formatDelta(body.wachstum.follower_delta)}
              </p>
            </div>
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">vs. Schnitt</p>
              <p className="mt-2 text-sm text-white">{body.wachstum.follower_vs_schnitt}</p>
            </div>
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Monetarisierung</p>
              <p className="mt-2 text-sm text-white">{body.wachstum.monetarisierung}</p>
            </div>
            <div className="rounded-xl border border-border bg-white/5 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-text-secondary">Raid-Einfluss</p>
              <p className="mt-2 text-sm text-white">{body.wachstum.raid_einfluss}</p>
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <SectionHeading icon={AlertCircle} title="Vergleich" />
            <span className={`rounded-full border px-2.5 py-1 text-xs font-bold uppercase tracking-wide ${trendBadgeClass(body.vergleich.trend)}`}>
              {body.vergleich.trend}
            </span>
          </div>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="rounded-xl border border-green-400/20 bg-green-500/10 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-green-200/80">Besser als sonst</p>
              {(body.vergleich.besser_als_sonst || []).length > 0 ? (
                <ul className="mt-3 space-y-2 text-sm text-green-100/90">
                  {body.vergleich.besser_als_sonst.map((item, index) => (
                    <li key={`${item}-${index}`}>• {item}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-sm text-green-100/75">Keine positiven Abweichungen markiert.</p>
              )}
            </div>
            <div className="rounded-xl border border-red-400/20 bg-red-500/10 p-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-red-200/80">Schlechter als sonst</p>
              {(body.vergleich.schlechter_als_sonst || []).length > 0 ? (
                <ul className="mt-3 space-y-2 text-sm text-red-100/90">
                  {body.vergleich.schlechter_als_sonst.map((item, index) => (
                    <li key={`${item}-${index}`}>• {item}</li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-sm text-red-100/75">Keine negativen Abweichungen markiert.</p>
              )}
            </div>
          </div>
        </section>

        <section className="space-y-2">
          <SectionHeading icon={Zap} title="Maßnahmen" accent="text-orange-200" />
          {sortedActions.length > 0 ? (
            <div className="space-y-2">
              {sortedActions.map((item, index) => {
                const tone =
                  item.prioritaet === 1
                    ? ACTION_TONES.critical
                    : item.prioritaet === 2
                      ? ACTION_TONES.warning
                      : ACTION_TONES.neutral;

                return (
                  <div key={`${item.prioritaet}-${index}`} className={`rounded-xl border p-3 ${tone}`}>
                    <div className="mb-2 flex items-center gap-2">
                      <span className="inline-flex h-7 w-7 items-center justify-center rounded-full bg-black/20 text-xs font-bold text-white">
                        {item.prioritaet}
                      </span>
                      <p className="text-sm font-semibold text-white">{item.was}</p>
                    </div>
                    <p className="text-xs text-text-secondary">{item.warum}</p>
                    <p className="mt-2 text-xs text-white/70">Erwarteter Effekt: {item.erwarteter_effekt}</p>
                  </div>
                );
              })}
            </div>
          ) : (
            <EmptyState text="Keine Maßnahmen hinterlegt." />
          )}
        </section>

        {(body.admin_notizen || []).length > 0 && (
          <section className="rounded-xl border border-border bg-white/[0.03] p-3">
            <SectionHeading icon={MessageSquare} title="Admin-Notizen" />
            <div className="mt-3 space-y-2">
              {body.admin_notizen.map((note, index) => (
                <p key={`${note}-${index}`} className="text-xs text-white/55">
                  • {note}
                </p>
              ))}
            </div>
          </section>
        )}
      </div>
    );
  }

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

function RatingBar({
  streamer,
  sessionId,
  variant,
  initialRating,
}: {
  streamer: string | null;
  sessionId?: number | null;
  variant: StreamReportVariant;
  initialRating?: StreamReportRating | null;
}) {
  const [currentRating, setCurrentRating] = React.useState<StreamReportRating | null>(initialRating ?? null);
  const [pendingRating, setPendingRating] = React.useState<RatingValue | null>(initialRating?.rating ?? null);
  const [pendingComment, setPendingComment] = React.useState(initialRating?.comment ?? '');
  const [saving, setSaving] = React.useState(false);
  const [saved, setSaved] = React.useState(false);
  const [saveError, setSaveError] = React.useState<string | null>(null);

  React.useEffect(() => {
    setCurrentRating(initialRating ?? null);
    setPendingRating(initialRating?.rating ?? null);
    setPendingComment(initialRating?.comment ?? '');
    setSaving(false);
    setSaved(false);
    setSaveError(null);
  }, [sessionId, variant, initialRating?.rating, initialRating?.comment, initialRating?.updated_at]);

  const canSave =
    !!streamer &&
    !!sessionId &&
    !!pendingRating &&
    !saving &&
    (pendingRating !== currentRating?.rating || pendingComment.trim() !== (currentRating?.comment ?? ''));

  const handleSelect = (value: RatingValue) => {
    setPendingRating(value);
    setSaved(false);
    setSaveError(null);
  };

  const handleSave = async () => {
    if (!streamer || !sessionId || !pendingRating) return;

    const previous = currentRating;
    const trimmedComment = pendingComment.trim();
    const optimisticRating: StreamReportRating = {
      rating: pendingRating,
      comment: trimmedComment || undefined,
      updated_at: new Date().toISOString(),
    };

    setSaving(true);
    setSaved(false);
    setSaveError(null);
    setCurrentRating(optimisticRating);

    try {
      const response = await fetch(
        buildApiUrl('/stream-report/rate'),
        withCookieCredentials({
          method: 'POST',
          headers: {
            Accept: 'application/json',
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            session_id: sessionId,
            streamer,
            variant,
            rating: pendingRating,
            comment: trimmedComment || undefined,
          }),
        })
      );

      if (!response.ok) {
        throw new Error(await readMutationError(response));
      }

      const payload = (await response.json().catch(() => null)) as
        | { ok?: boolean; rating?: RatingValue; comment?: string }
        | null;
      const confirmedRating: StreamReportRating = {
        rating: payload?.rating || pendingRating,
        comment: payload?.comment ?? (trimmedComment || undefined),
        updated_at: new Date().toISOString(),
      };

      setCurrentRating(confirmedRating);
      setPendingRating(confirmedRating.rating);
      setPendingComment(confirmedRating.comment ?? '');
      setSaved(true);
    } catch (error) {
      setCurrentRating(previous ?? null);
      setSaveError(error instanceof Error ? error.message : 'Bewertung konnte nicht gespeichert werden.');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mt-5 border-t border-border pt-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm font-semibold text-white">
          <MessageSquare className="h-4 w-4 text-text-secondary" />
          Report bewerten
        </div>
        {currentRating && (
          <div className="text-xs text-text-secondary">
            Aktuell gespeichert: <span className="font-semibold text-white">{formatRatingLabel(currentRating.rating)}</span>
            {currentRating.updated_at ? ` · ${formatDate(currentRating.updated_at)}` : ''}
          </div>
        )}
      </div>

      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        {RATING_OPTIONS.map((option) => {
          const Icon = option.icon;
          const isActive = pendingRating === option.value;
          return (
            <button
              key={option.value}
              type="button"
              disabled={saving}
              onClick={() => handleSelect(option.value)}
              className={`flex items-center justify-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${
                isActive ? option.activeClass : option.idleClass
              }`}
            >
              <Icon className="h-4 w-4" />
              {option.label}
            </button>
          );
        })}
      </div>

      {pendingRating && (
        <div className="mt-3 space-y-3">
          <textarea
            value={pendingComment}
            disabled={saving}
            onChange={(event) => {
              setPendingComment(event.target.value);
              setSaved(false);
              setSaveError(null);
            }}
            rows={3}
            placeholder="Optionaler Kommentar zur Report-Qualität ..."
            className="w-full rounded-xl border border-border bg-white/5 px-3 py-2 text-sm text-white placeholder:text-text-secondary/60 focus:border-primary/50 focus:outline-none disabled:cursor-not-allowed disabled:opacity-60"
          />

          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-3 text-xs">
              {saved && (
                <span className="inline-flex items-center gap-1 text-green-300">
                  <CheckCircle2 className="h-4 w-4" />
                  Gespeichert
                </span>
              )}
              {saveError && <span className="text-red-300">{saveError}</span>}
            </div>

            <button
              type="button"
              disabled={!canSave}
              onClick={() => void handleSave()}
              className="inline-flex items-center gap-2 rounded-xl border border-primary/30 bg-primary/15 px-3 py-2 text-sm font-semibold text-white transition-colors hover:border-primary/50 hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Send className="h-4 w-4" />}
              Speichern
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function ReportColumn({
  streamer,
  sessionId,
  variant,
}: {
  streamer: string | null;
  sessionId?: number;
  variant: StreamReportVariant;
}) {
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
        <>
          <ReportBody report={data} />
          {data.status === 'done' && !data.empty && (
            <RatingBar
              streamer={streamer}
              sessionId={data.session_id ?? sessionId}
              variant={variant}
              initialRating={data.rating}
            />
          )}
        </>
      ) : null}
    </div>
  );
}

export function StreamReports({ streamer, days }: StreamReportsProps) {
  const { isFeatureLocked } = usePlan();
  const locked = isFeatureLocked('post_stream_report');
  const [selectedSessionId, setSelectedSessionId] = React.useState<number | undefined>(undefined);

  const { data: overview, isLoading: loadingSessions } = useQuery({
    queryKey: ['overview', streamer, days, 'stream-reports'],
    queryFn: () => fetchOverview(streamer, days),
    enabled: !!streamer && !locked,
    staleTime: 2 * 60 * 1000,
  });

  const sessions = overview?.sessions || [];

  React.useEffect(() => {
    if (!selectedSessionId && sessions.length > 0) {
      setSelectedSessionId(sessions[0].id);
    }
  }, [selectedSessionId, sessions]);

  if (locked) {
    return (
      <div className="flex h-80 flex-col items-center justify-center gap-5">
        <div className="relative">
          <div className="flex h-20 w-20 items-center justify-center rounded-2xl border border-border bg-background/80">
            <FileText className="h-9 w-9 text-text-secondary opacity-40" />
          </div>
          <div className="absolute -bottom-1 -right-1 flex h-7 w-7 items-center justify-center rounded-full bg-border">
            <Lock className="h-3.5 w-3.5 text-text-secondary" />
          </div>
        </div>
        <div className="max-w-sm text-center">
          <p className="mb-1 text-lg font-semibold text-white">Stream Reports brauchen KI-Zugang</p>
          <p className="text-sm leading-relaxed text-text-secondary">
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
          <p className="font-semibold text-white">Keine Sessions gefunden</p>
          <p className="mt-1 text-sm text-text-secondary">
            Nach dem nächsten abgeschlossenen Stream erscheinen hier Reports.
          </p>
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
