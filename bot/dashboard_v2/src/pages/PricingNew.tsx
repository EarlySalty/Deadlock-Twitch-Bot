import { useMemo, useState } from 'react';
import { useAuthStatus, useBillingCatalog, useMonthlyStats } from '../hooks/useAnalytics';
import { getPlanCheckoutHref } from '../preview/routes';
import { GrowthBlur } from '../components/pricing-v2/GrowthBlur';

const TAX_NOTE = 'Kein Ausweis von Umsatzsteuer gemäß § 19 UStG.';

export default function PricingNew() {
  const [cycle, setCycle] = useState<1 | 12>(12);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const { data: catalog } = useBillingCatalog(cycle);
  const { data: auth } = useAuthStatus();
  const streamer = auth?.twitchLogin ?? null;
  const { data: monthly } = useMonthlyStats(streamer, 12);

  const series = useMemo(() => extractSeries(monthly), [monthly]);
  const premium = (catalog?.plans ?? []).find((plan) => plan.id === 'premium');
  const priceLabel = cycle === 12 ? '29,90 € / Jahr' : '2,99 € / Monat';

  return (
    <div className="mx-auto max-w-xl px-4 py-16">
      <article className="rise-in rounded-3xl border border-white/10 bg-white/5 p-8">
        <p className="text-sm text-white/45">Dein Wachstum, seit du dabei bist</p>
        <div className="mt-6">
          <GrowthBlur values={series.values} days={series.days} peak={series.peak} />
        </div>
        <p className="mt-6 text-lg text-white/80">Deine Zahlen liegen bereit.</p>
        <p className="text-white/55">Premium macht sie sichtbar.</p>

        <div className="mt-8 space-y-2">
          <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-white/10 px-4 py-3">
            <input type="radio" checked={cycle === 12} onChange={() => setCycle(12)} />
            <span>29,90 € / Jahr, 2 Monate gratis</span>
          </label>
          <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-white/10 px-4 py-3">
            <input type="radio" checked={cycle === 1} onChange={() => setCycle(1)} />
            <span>2,99 € / Monat</span>
          </label>
        </div>

        <a
          href={getPlanCheckoutHref('premium', false, cycle)}
          className="mt-6 inline-flex w-full items-center justify-center rounded-2xl bg-white/12 px-4 py-3 font-medium"
          data-press
        >
          Freischalten
        </a>
        <p className="mt-2 text-center text-xs text-white/40">jederzeit kündbar</p>
        <p className="mt-4 text-center text-xs text-white/35">{TAX_NOTE}</p>

        <button
          type="button"
          className="mt-6 text-sm text-white/45 underline-offset-2 hover:underline"
          onClick={() => setDetailsOpen((open) => !open)}
        >
          {detailsOpen ? 'Details schließen' : 'Was ist enthalten?'}
        </button>
        {detailsOpen ? (
          <ul className="mt-3 space-y-1 text-sm text-white/55">
            {(premium?.features ?? [
              'Voller Verlauf, Vergleiche und Wachstum',
              'KI-Analyse, KI-Chat, Coaching',
              'Clip- und Social-Pipeline',
            ]).map((feature) => (
              <li key={feature}>{feature}</li>
            ))}
          </ul>
        ) : null}
        <p className="mt-6 text-center text-xs text-white/30">{priceLabel}</p>
      </article>
    </div>
  );
}

function extractSeries(monthly: unknown): { values: number[]; days: number | null; peak: number | null } {
  const rows = Array.isArray(monthly) ? monthly : [];
  const values = rows
    .map((row) => {
      const record = row as Record<string, unknown>;
      const raw =
        record.avgViewers ??
        record.avg_viewers ??
        record.average_viewers ??
        record.viewers ??
        record.value;
      return typeof raw === 'number' ? raw : Number(raw);
    })
    .filter((value) => Number.isFinite(value));
  const peak = values.length ? Math.max(...values) : null;
  return { values, days: values.length ? values.length * 30 : null, peak };
}
