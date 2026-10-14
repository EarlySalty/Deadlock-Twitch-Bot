import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { RefreshCw, ReceiptText } from 'lucide-react';
import { fetchUsage, socialAiKeys } from '@/api/socialAi';
import type { StageUsage, UsageFilter, UsageGrouping, UsageTotals } from '@/api/socialAiTypes';
import { EmptyState } from '@/components/shared/EmptyState';
import { costTotal, formatCount, formatDate, formatDuration, formatMicroUsd } from '@/utils/socialAi';
import { QueryState } from './SocialAiLayout';

function Totals({ totals }: { totals: UsageTotals }) {
  return <dl className="social-ai-summary">
    <div><dt>Anbieteranfragen</dt><dd>{formatCount(totals.attempts)}</dd></div>
    <div><dt>Kostenschätzung</dt><dd>{formatMicroUsd(costTotal(totals, 'estimated_micro_usd'))}</dd></div>
    <div><dt>Gemessene Anbieterkosten</dt><dd>{formatMicroUsd(costTotal(totals, 'measured_micro_usd'))}</dd></div>
    <div><dt>Anbieterbeleg</dt><dd>{formatMicroUsd(costTotal(totals, 'invoiced_micro_usd'))}</dd></div>
    <div><dt>Reservierungen, keine Rechnung</dt><dd>{formatMicroUsd(totals.held_micro_usd)}</dd></div>
    <div><dt>Konservativ belastetes Budget</dt><dd>{formatMicroUsd(totals.charged_micro_usd)}</dd></div>
    <div><dt>Anbieterzeit insgesamt</dt><dd>{formatDuration(totals.provider_latency_ms)}</dd></div>
    <div><dt>Eingabe- / Ausgabetoken</dt><dd>{formatCount(totals.input_tokens)} / {formatCount(totals.output_tokens)}</dd></div>
    <div><dt>Cache geschrieben / gelesen</dt><dd>{formatCount(totals.cache_creation_input_tokens)} / {formatCount(totals.cache_read_input_tokens)}</dd></div>
  </dl>;
}

const STAGE_LABELS = { analysis: 'Analyse', planning: 'Schnittplanung', other: 'Weitere Schritte' } as const;

function StageCosts({ stages }: { stages: StageUsage[] }) {
  if (stages.length === 0) return null;
  return <div className="space-y-3">
    <h3 className="text-lg font-semibold">Kosten nach Arbeitsschritt</h3>
    <p className="text-sm text-text-secondary">Erstproduktion und Revision werden getrennt ausgewiesen. Eine Revision kann Analyse und Schnittplanung enthalten.</p>
    <div className="social-ai-table" role="region" aria-label="Kosten nach Arbeitsschritt, horizontal scrollbar" tabIndex={0}>
      <table><caption className="sr-only">KI-Verbrauch nach Arbeitsschritt und Durchlauf</caption>
        <thead><tr><th scope="col">Arbeitsschritt</th><th scope="col">Durchlauf</th><th scope="col">Anfragen</th><th scope="col">Kostenschätzung</th><th scope="col">Belastetes Budget</th><th scope="col">Anbieterzeit</th></tr></thead>
        <tbody>{stages.map(stage => <tr key={`${stage.stage}:${stage.run}`}>
          <th scope="row">{STAGE_LABELS[stage.stage]}</th><td>{stage.run === 'revision' ? 'Revision' : 'Erstproduktion'}</td><td>{formatCount(stage.attempts)}</td><td>{formatMicroUsd(costTotal(stage, 'estimated_micro_usd'))}</td><td>{formatMicroUsd(stage.charged_micro_usd)}</td><td>{formatDuration(stage.provider_latency_ms)}</td>
        </tr>)}</tbody>
      </table>
    </div>
  </div>;
}

export function SocialAiUsagePage() {
  const [filter, setFilter] = useState<UsageFilter>({ group_by: 'user', offset: 0, twitch_user_id: '', provider: '', job_id: '' });
  const [draft, setDraft] = useState({ twitch_user_id: '', provider: '', job_id: '' });
  const query = useQuery({ queryKey: socialAiKeys.usage(filter), queryFn: ({ signal }) => fetchUsage(filter, signal), retry: false, refetchOnWindowFocus: false });
  const data = query.data;
  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="text-xl font-semibold">Verbrauch & Kosten</h2><button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching} onClick={() => void query.refetch()}><RefreshCw size={16} aria-hidden="true" /> Stand neu laden</button></div>
    <form className="panel-card rounded-2xl p-5 space-y-4" onSubmit={event => { event.preventDefault(); setFilter({ ...filter, ...draft, offset: 0 }); }}>
      <div className="social-ai-form-grid">
        <label>Gruppierung<select className="admin-input mt-2" value={filter.group_by} onChange={event => setFilter({ ...filter, group_by: event.target.value as UsageGrouping, offset: 0 })}><option value="user">Pro Nutzer</option><option value="provider">Pro Anbieter und Modell</option><option value="job">Pro Auftrag</option></select></label>
        {([{ key: 'twitch_user_id', label: 'Twitch-Nutzer-ID' }, { key: 'provider', label: 'Anbieter' }, { key: 'job_id', label: 'Auftrags-ID' }] as const).map(field => <label key={field.key}>{field.label}<input className="admin-input mt-2" value={draft[field.key]} maxLength={128} onChange={event => setDraft({ ...draft, [field.key]: event.target.value })} /></label>)}
      </div>
      <div className="flex flex-wrap gap-3"><button type="submit" className="admin-button admin-button-secondary" disabled={query.isFetching}>Filter anwenden</button><button type="button" className="admin-button admin-button-secondary" onClick={() => { setDraft({ twitch_user_id: '', provider: '', job_id: '' }); setFilter({ group_by: filter.group_by, offset: 0, twitch_user_id: '', provider: '', job_id: '' }); }}>Filter zurücksetzen</button></div>
    </form>
    <p className="text-sm text-text-secondary">Schätzungen aus Tokenpreisen sind keine Rechnung. Gemessene Kosten und Anbieterbelege liegen derzeit nicht vor. Ungeklärte Anfragen belasten das Budget weiterhin konservativ; fehlende Angaben bedeuten nicht Nullverbrauch.</p>
    <QueryState pending={query.isPending} error={query.error}>
      {data && <section className="panel-card rounded-2xl p-5 space-y-5">
        <h3 className="text-lg font-semibold">Verbrauch im angewendeten Filter</h3>
        <p className="text-sm text-text-secondary">Zeitraum: {formatDate(data.period_start)} bis {formatDate(data.period_end)}. Summen beziehen sich auf den gesamten Filter, die Tabelle ist seitenweise geladen.</p>
        <Totals totals={data.totals} />
        <StageCosts stages={data.stages} />
        {(data.totals.incomplete_attempts > 0 || data.totals.unknown_cost_attempts > 0) && <p role="status" className="social-ai-notice">Offen oder ungeklärt: {formatCount(data.totals.incomplete_attempts)}. Ohne Kostenangaben: {formatCount(data.totals.unknown_cost_attempts)}. Vollständige Kostensummen sind unbekannt.</p>}
        {data.groups.length === 0 ? <EmptyState icon={ReceiptText} title="Keine Verbrauchsdaten in diesem Filter" description="Ohne Anbieteranfragen liegen keine gemessenen Kosten vor." /> : <>
          <div className="social-ai-table" role="region" aria-label="Verbrauch nach Gruppierung, horizontal scrollbar" tabIndex={0}>
            <table><caption className="sr-only">Verbrauch nach {filter.group_by === 'user' ? 'Nutzer' : filter.group_by === 'provider' ? 'Anbieter und Modell' : 'Auftrag'}</caption><thead><tr><th scope="col">Nutzer / Auftrag</th><th scope="col">Anbieter / Modell</th><th scope="col">Anfragen / ungeklärt</th><th scope="col">Token ein / aus</th><th scope="col">Schätzung</th><th scope="col">Gemessen / Beleg</th><th scope="col">Anbieterzeit</th>{filter.group_by === 'job' && <th scope="col">Gesamtlaufzeit</th>}{filter.group_by !== 'provider' && <th scope="col">FFmpeg-Laufzeit</th>}</tr></thead>
              <tbody>{data.groups.map((group, index) => <tr key={`${filter.offset}:${index}`}>
                <td>{group.twitch_user_id ?? 'Nicht zugeordnet'}<small>{group.job_id ?? 'Gruppierte Aufträge'}</small></td><td>{group.provider ?? 'Gruppierte Anbieter'}<small>{group.model ?? 'Modell nicht einzeln ausgewiesen'}</small></td><td>{formatCount(group.attempts)} / {formatCount(group.incomplete_attempts)}<small>Unbekannte Kosten: {formatCount(group.unknown_cost_attempts)}</small></td><td>{formatCount(group.input_tokens)} / {formatCount(group.output_tokens)}</td><td>{formatMicroUsd(costTotal(group, 'estimated_micro_usd'))}</td><td>{formatMicroUsd(costTotal(group, 'measured_micro_usd'))}<small>Beleg: {formatMicroUsd(costTotal(group, 'invoiced_micro_usd'))}</small></td><td>{formatDuration(group.provider_latency_ms)}</td>{filter.group_by === 'job' && <td>{formatDuration(group.processing_ms)}</td>}{filter.group_by !== 'provider' && <td>{formatDuration(group.rendering_ms)}</td>}
              </tr>)}</tbody>
            </table>
          </div>
          <p className="text-sm text-text-secondary">FFmpeg-Laufzeit ist interne Rechenzeit und kein direkt berechneter API-Preis. Anbieterzeit, Rendering und Gesamtlaufzeit werden nicht zusammengerechnet.</p>
        </>}
        <div className="flex flex-wrap items-center justify-between gap-3"><button type="button" className="admin-button admin-button-secondary" disabled={filter.offset === 0 || query.isFetching} onClick={() => setFilter({ ...filter, offset: Math.max(0, filter.offset - 25) })}>Zurück</button><span>Seite {filter.offset / 25 + 1}</span><button type="button" className="admin-button admin-button-secondary" disabled={!data.has_more || query.isFetching} onClick={() => setFilter({ ...filter, offset: filter.offset + 25 })}>Weiter</button></div>
      </section>}
    </QueryState>
  </div>;
}
