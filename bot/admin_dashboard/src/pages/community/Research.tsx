import { useState } from 'react';
import type { FormEvent } from 'react';
import { useQuery } from '@tanstack/react-query';
import { motion, useReducedMotion } from 'framer-motion';
import { BadgeCheck, BarChart3, Search, SearchX } from 'lucide-react';
import { fetchAdminResearch } from '@/api/client';
import type { ResearchResponse, ResearchScoreComponent } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { EmptyState } from '@/components/shared/EmptyState';

const COPY = {
  pageTitle: 'Streamer-Research',
  pageDescription: 'Wie wertvoll wäre ein Onboarding? Deadlock-Aktivität und Viewer im Vergleich zu unseren Partnern.',
  loginLabel: 'Twitch-Login',
  loginPlaceholder: 'z. B. earlysalty',
  submit: 'Analysieren',
  periodLabel: 'Zeitraum',
  scoreLabel: 'Onboarding-Value',
  scoreScale: 'von 100',
  partnerBadge: 'Bereits Partner',
  partnerStatus: 'Status',
  componentsTitle: 'Score-Komponenten (Perzentil gegenüber Partnern)',
  viewersComponent: 'Ø Viewer',
  hoursComponent: 'Deadlock-Stunden',
  consistencyComponent: 'Aktive Tage',
  percentileSuffix: 'Perzentil',
  comparisonTitle: 'Vergleich mit Partner-Median',
  metricColumn: 'Kennzahl',
  subjectColumn: 'Streamer',
  baselineColumn: 'Partner-Median',
  avgViewers: 'Ø Viewer',
  peakViewers: 'Peak-Viewer',
  totalHours: 'Deadlock-Stunden',
  activeDays: 'Aktive Tage',
  sessions: 'Streams (Sessions)',
  deShare: 'Anteil deutschsprachig',
  lastSeen: 'Zuletzt in der Kategorie gesehen',
  recentTitles: 'Letzte Stream-Titel',
  loading: 'Daten werden geladen …',
  errorTitle: 'Research nicht verfügbar',
  errorDescription: 'Die Daten konnten gerade nicht geladen werden. Kurz warten und nochmal versuchen.',
  initialTitle: 'Streamer nachschlagen',
  initialDescription: 'Twitch-Login eingeben. Die Bewertung basiert auf unseren Snapshots der Deadlock-Kategorie.',
  notFoundTitle: 'Nicht in der Deadlock-Kategorie gesehen',
  notFoundDescription: 'Dieser Login ist im gewählten Zeitraum in keinem Deadlock-Stream aufgetaucht. Der Onboarding-Value ist damit praktisch null.',
  baselineContext: 'Partner in der Vergleichsgruppe',
  hoursUnit: 'Std.',
  noValue: '—',
} as const;

const DAY_OPTIONS = [
  { days: 7, label: '7 Tage' },
  { days: 30, label: '30 Tage' },
  { days: 90, label: '90 Tage' },
] as const;

const numberFormatter = new Intl.NumberFormat('de-DE', { maximumFractionDigits: 1 });

function formatNumber(value: number): string {
  return numberFormatter.format(value);
}

function formatDate(value: string | null): string {
  return value ? new Date(value).toLocaleString('de-DE') : COPY.noValue;
}

function pillClass(active: boolean): string {
  return [
    'rounded-full border px-4 py-2 text-sm font-medium transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary',
    active
      ? 'border-primary/60 bg-primary/20 text-white'
      : 'border-white/10 bg-white/[0.03] text-text-secondary hover:border-white/25 hover:text-white',
  ].join(' ');
}

function ComponentBar({ label, component }: { label: string; component: ResearchScoreComponent }) {
  const percentile = Math.max(0, Math.min(100, component.percentile));
  return (
    <div className="rounded-2xl border border-white/8 bg-black/15 p-4">
      <div className="flex items-center justify-between gap-3 text-sm">
        <span className="font-medium text-white">{label}</span>
        <span className="text-text-secondary">
          {percentile}. {COPY.percentileSuffix}
        </span>
      </div>
      <div
        aria-label={label}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={percentile}
        className="mt-3 h-2 overflow-hidden rounded-full bg-white/8"
        role="progressbar"
      >
        <div
          className="h-full rounded-full bg-gradient-to-r from-primary/70 to-accent"
          style={{ width: `${percentile}%` }}
        />
      </div>
    </div>
  );
}

function ResearchResult({ data }: { data: ResearchResponse }) {
  if (!data.found) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8">
        <EmptyState icon={SearchX} title={COPY.notFoundTitle} description={COPY.notFoundDescription} />
        <p className="mt-5 text-center text-xs text-text-secondary">
          {COPY.baselineContext}: {data.baseline.partner_count}
        </p>
      </div>
    );
  }

  const comparisonRows = [
    [COPY.avgViewers, formatNumber(data.subject.avg_viewers), formatNumber(data.baseline.avg_viewers.median)],
    [COPY.peakViewers, formatNumber(data.subject.peak_viewers), COPY.noValue],
    [COPY.totalHours, `${formatNumber(data.subject.total_hours)} ${COPY.hoursUnit}`, `${formatNumber(data.baseline.total_hours.median)} ${COPY.hoursUnit}`],
    [COPY.activeDays, formatNumber(data.subject.active_days), formatNumber(data.baseline.active_days.median)],
    [COPY.sessions, formatNumber(data.subject.sessions_count), COPY.noValue],
    [COPY.deShare, `${formatNumber(data.subject.de_share * 100)}%`, COPY.noValue],
    [COPY.lastSeen, formatDate(data.subject.last_seen), COPY.noValue],
  ];

  return (
    <div className="space-y-5">
      {data.is_already_partner ? (
        <div className="flex items-center gap-3 rounded-2xl border border-primary/35 bg-primary/10 px-4 py-3 text-sm text-white">
          <BadgeCheck className="h-5 w-5 text-primary" />
          <span className="font-semibold">{COPY.partnerBadge}</span>
          <span className="text-text-secondary">
            {COPY.partnerStatus}: {data.partner_status ?? COPY.noValue}
          </span>
        </div>
      ) : null}

      <div className="grid gap-5 xl:grid-cols-[0.8fr_1.2fr]">
        <article className="panel-card relative overflow-hidden rounded-[1.8rem] p-7">
          <div className="absolute inset-x-0 top-0 h-1 bg-gradient-to-r from-primary via-accent to-primary" />
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">{COPY.scoreLabel}</p>
          <div className="mt-4 flex items-end gap-3">
            <span className="text-7xl font-semibold leading-none text-white">{data.score.total}</span>
            <span className="pb-2 text-sm text-text-secondary">{COPY.scoreScale}</span>
          </div>
          <div className="mt-6 inline-flex rounded-full border border-accent/30 bg-accent/10 px-4 py-2 text-sm font-semibold text-white">
            {data.score.tier.label}
          </div>
          <p className="mt-5 text-xs text-text-secondary">
            {COPY.baselineContext}: {data.baseline.partner_count}
          </p>
        </article>

        <article className="panel-card rounded-[1.8rem] p-6">
          <h2 className="text-sm font-semibold uppercase tracking-[0.18em] text-text-secondary">
            {COPY.componentsTitle}
          </h2>
          <div className="mt-5 grid gap-3">
            <ComponentBar label={COPY.viewersComponent} component={data.score.components.viewers} />
            <ComponentBar label={COPY.hoursComponent} component={data.score.components.hours} />
            <ComponentBar label={COPY.consistencyComponent} component={data.score.components.consistency} />
          </div>
        </article>
      </div>

      <article className="panel-card overflow-hidden rounded-[1.8rem]">
        <div className="flex items-center gap-3 border-b border-white/8 px-6 py-5">
          <BarChart3 className="h-5 w-5 text-primary" />
          <h2 className="font-semibold text-white">{COPY.comparisonTitle}</h2>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[620px] text-left text-sm">
            <thead className="bg-white/[0.025] text-xs uppercase tracking-[0.14em] text-text-secondary">
              <tr>
                <th className="px-6 py-4 font-medium">{COPY.metricColumn}</th>
                <th className="px-6 py-4 font-medium">{COPY.subjectColumn}</th>
                <th className="px-6 py-4 font-medium">{COPY.baselineColumn}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/8">
              {comparisonRows.map(([label, subject, baseline]) => (
                <tr key={label}>
                  <th className="px-6 py-4 font-medium text-text-secondary">{label}</th>
                  <td className="px-6 py-4 font-semibold text-white">{subject}</td>
                  <td className="px-6 py-4 text-text-secondary">{baseline}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </article>

      {data.subject.recent_titles.length ? (
        <article className="panel-card rounded-[1.8rem] p-6">
          <h2 className="font-semibold text-white">{COPY.recentTitles}</h2>
          <ol className="mt-4 space-y-2">
            {data.subject.recent_titles.map((title) => (
              <li key={title} className="rounded-2xl border border-white/8 bg-black/10 px-4 py-3 text-sm text-text-secondary">
                {title}
              </li>
            ))}
          </ol>
        </article>
      ) : null}
    </div>
  );
}

export default function ResearchPage() {
  const reduceMotion = useReducedMotion();
  const [login, setLogin] = useState('');
  const [days, setDays] = useState(30);
  const [submitted, setSubmitted] = useState<{ login: string; days: number } | null>(null);
  const query = useQuery({
    queryKey: ['admin-research', submitted?.login, submitted?.days],
    queryFn: () => fetchAdminResearch(submitted!.login, submitted!.days),
    enabled: submitted !== null,
    retry: false,
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalized = login.trim();
    if (normalized) {
      setSubmitted({ login: normalized, days });
    }
  }

  return (
    <motion.section
      initial={reduceMotion ? false : { opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-5"
    >
      <PageHeader title={COPY.pageTitle} description={COPY.pageDescription} />

      <form className="panel-card rounded-[1.8rem] p-6" onSubmit={submit}>
        <div className="grid gap-5 lg:grid-cols-[1fr_auto] lg:items-end">
          <label className="block">
            <span className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">{COPY.loginLabel}</span>
            <div className="mt-2 flex rounded-2xl border border-white/10 bg-black/15 focus-within:border-primary/60 focus-within:ring-2 focus-within:ring-primary/20">
              <Search className="ml-4 mt-3.5 h-5 w-5 shrink-0 text-text-secondary" />
              <input
                aria-label={COPY.loginLabel}
                autoComplete="off"
                className="w-full bg-transparent px-3 py-3 text-white outline-none placeholder:text-text-secondary/60"
                maxLength={25}
                onChange={(event) => setLogin(event.target.value)}
                placeholder={COPY.loginPlaceholder}
                value={login}
              />
            </div>
          </label>
          <button
            className="rounded-2xl bg-primary px-6 py-3 font-semibold text-white transition hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background"
            type="submit"
          >
            {COPY.submit}
          </button>
        </div>

        <fieldset className="mt-5 border-t border-white/8 pt-4">
          <legend className="sr-only">{COPY.periodLabel}</legend>
          <div className="flex flex-wrap gap-2">
            {DAY_OPTIONS.map((option) => (
              <button
                className={pillClass(days === option.days)}
                key={option.days}
                onClick={() => {
                  setDays(option.days);
                  setSubmitted((current) => current && { ...current, days: option.days });
                }}
                type="button"
              >
                {option.label}
              </button>
            ))}
          </div>
        </fieldset>
      </form>

      {query.isLoading ? (
        <div className="panel-card rounded-[1.8rem] p-8 text-center text-sm text-text-secondary">{COPY.loading}</div>
      ) : query.isError ? (
        <div className="panel-card rounded-[1.8rem] p-8">
          <EmptyState icon={SearchX} title={COPY.errorTitle} description={COPY.errorDescription} />
        </div>
      ) : query.data ? (
        <ResearchResult data={query.data} />
      ) : (
        <div className="panel-card rounded-[1.8rem] p-8">
          <EmptyState icon={Search} title={COPY.initialTitle} description={COPY.initialDescription} />
        </div>
      )}
    </motion.section>
  );
}
