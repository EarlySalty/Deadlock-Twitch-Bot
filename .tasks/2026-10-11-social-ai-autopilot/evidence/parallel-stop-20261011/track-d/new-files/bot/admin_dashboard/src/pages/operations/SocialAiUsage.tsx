import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { ApiError } from '@/api/client';
import { fetchSocialAiControls, fetchSocialAiUsage, formatMicroUsd, formatRuntime, saveSocialAiControls, saveSocialAiOverride } from '@/api/socialAi';
import type { ProcessingControls, UsageFilter, OverrideChange } from '@/api/socialAi';
import { PageHeader } from '@/components/layout/PageHeader';
import './socialAi.css';

const number = (value: number | null) => value === null ? 'Unbekannt' : new Intl.NumberFormat('de-DE').format(value);
const states: Record<string, string> = { started: 'Läuft oder wartet auf Klärung', succeeded: 'Erfolgreich', failed: 'Fehlgeschlagen' };

function ControlForm({ controls }: { controls: ProcessingControls }) {
  const client = useQueryClient();
  const [paused, setPaused] = useState(controls.paused);
  const [kind, setKind] = useState<'user' | 'plan'>('user');
  const [id, setId] = useState('');
  const [overridePaused, setOverridePaused] = useState(false);
  const [cap, setCap] = useState('');
  const [reason, setReason] = useState('');
  const [notice, setNotice] = useState('');
  const save = useMutation({
    mutationFn: (target: 'global' | 'override') => {
      const change: OverrideChange = { expected_revision: controls.revision, paused: target === 'global' ? paused : overridePaused, reason: reason.trim(), monthly_cap_micro_usd: cap === '' ? null : Number(cap) };
      return target === 'global' ? saveSocialAiControls(change) : saveSocialAiOverride(kind, id.trim(), change);
    },
    onSuccess: data => {
      client.setQueryData(['social-ai-controls'], data);
      setNotice('Änderung gespeichert. Neue KI-Anfragen beachten die aktuelle Grenze.');
    },
    onError: () => setNotice('Der Speicherstand ist nicht bestätigt. Vor einem erneuten Versuch den aktuellen Stand laden.'),
  });
  const blocked = save.isPending || save.isError;
  const validCap = cap === '' || (Number.isSafeInteger(Number(cap)) && Number(cap) >= 0 && Number(cap) <= controls.initial_total_cap_micro_usd);

  function selectOverride(scopeKind: 'user' | 'plan', scopeId: string) {
    const existing = controls.overrides.find(entry => entry.scope_kind === scopeKind && entry.scope_id === scopeId);
    setKind(scopeKind);
    setId(scopeId);
    setOverridePaused(existing?.paused ?? false);
    setCap(existing?.monthly_cap_micro_usd?.toString() ?? '');
  }

  return <div className="panel-card rounded-2xl p-5 space-y-6">
    <div>
      <h2 className="text-lg font-semibold">Verarbeitung steuern</h2>
      <p className="mt-2 text-sm text-text-secondary">Die gemeinsame Testgrenze beträgt {formatMicroUsd(controls.initial_total_cap_micro_usd)} für alle Nutzer und alle Versuche. Bereits gebunden: {formatMicroUsd(controls.committed_micro_usd)}. Reservierungen sind keine Rechnung.</p>
      <p className="mt-2 text-sm text-text-secondary">Eine Pause verhindert neue Anbieteranfragen, einschließlich Wiederholungen. Bereits zugestellte Anfragen werden nicht abgebrochen. Einwilligung und technische YAML-Grenzen bleiben erforderlich.</p>
    </div>
    <label className="flex items-center gap-3"><input type="checkbox" checked={paused} disabled={blocked} onChange={event => setPaused(event.target.checked)} /> Neue KI-Anfragen global pausieren</label>
    <div className="social-ai-form-grid">
      <label>Begründung für die Änderung<textarea className="admin-input mt-2" required maxLength={1000} rows={2} value={reason} disabled={blocked} onChange={event => setReason(event.target.value)} /></label>
      <div className="self-end"><button type="button" className="admin-button admin-button-primary" disabled={blocked || !reason.trim() || paused === controls.paused} onClick={() => save.mutate('global')}>{save.isPending ? 'Wird gespeichert …' : 'Globale Pause speichern'}</button></div>
    </div>
    <div className="border-t border-border pt-5 space-y-4">
      <h3 className="font-semibold">Nutzergrenzen und künftige Planregeln</h3>
      <p className="text-sm text-text-secondary">Nutzergrenzen gelten zusätzlich zur globalen Testgrenze. Monatsgrenzen zählen alle Versuche im laufenden UTC-Kalendermonat. Planregeln sind nur vorgemerkt und noch nicht wirksam; Preise oder Freikontingente werden hier nicht festgelegt.</p>
      <div className="social-ai-form-grid">
        <label>Geltungsbereich<select className="admin-input mt-2" value={kind} disabled={blocked} onChange={event => selectOverride(event.target.value as 'user' | 'plan', '')}><option value="user">Einzelner Nutzer</option><option value="plan">Bestehender Aboplan (vorgemerkt)</option></select></label>
        {kind === 'user' ? <label>Twitch-Nutzer-ID<input className="admin-input mt-2" inputMode="numeric" value={id} disabled={blocked} onChange={event => selectOverride('user', event.target.value)} /></label> : <label>Bestehender Plan<select className="admin-input mt-2" value={id} disabled={blocked} onChange={event => selectOverride('plan', event.target.value)}><option value="">Plan auswählen</option>{controls.plans.map(plan => <option key={plan.id} value={plan.id}>{plan.name}</option>)}</select></label>}
        <label>Monatsgrenze in Mikro-US-Dollar<input className="admin-input mt-2" type="number" min={0} max={controls.initial_total_cap_micro_usd} step={1} value={cap} disabled={blocked} aria-describedby="social-ai-cap-hint" onChange={event => setCap(event.target.value)} /><span id="social-ai-cap-hint" className="mt-2 block text-sm text-text-secondary">1.000.000 entspricht 1 USD. Leer bedeutet keine zusätzliche Monatsgrenze; 0 sperrt neue Anfragen.</span></label>
        <label className="flex items-center gap-3 self-center"><input type="checkbox" checked={overridePaused} disabled={blocked} onChange={event => setOverridePaused(event.target.checked)} /> {kind === 'user' ? 'Für diesen Nutzer pausieren' : 'Pause für diesen Plan vormerken'}</label>
      </div>
      <button type="button" className="admin-button admin-button-secondary" disabled={blocked || !id.trim() || !reason.trim() || !validCap} onClick={() => save.mutate('override')}>{kind === 'plan' ? 'Planregel vormerken' : 'Nutzergrenze speichern'}</button>
      {controls.overrides.length > 0 && <ul className="divide-y divide-border">{controls.overrides.map(entry => <li key={`${entry.scope_kind}:${entry.scope_id}`} className="py-3 flex flex-wrap items-center justify-between gap-3"><div><p>{entry.scope_kind === 'user' ? 'Nutzer' : 'Plan (vorgemerkt)'} {entry.scope_id}: {entry.paused ? 'pausiert' : 'nicht pausiert'}, {entry.monthly_cap_micro_usd === null ? 'keine zusätzliche Monatsgrenze' : `${formatMicroUsd(entry.monthly_cap_micro_usd)} pro Monat`}</p><p className="text-sm text-text-secondary break-words">{entry.reason}</p></div><button type="button" className="admin-button admin-button-secondary" disabled={blocked} onClick={() => selectOverride(entry.scope_kind, entry.scope_id)}>Bearbeiten</button></li>)}</ul>}
    </div>
    <p className="text-sm text-text-secondary">Stand {controls.revision}, {controls.updated_at}. {controls.reason}</p>
    {save.error && <p role="alert" className="text-red-300">{save.error instanceof ApiError && save.error.status === 409 ? 'Der Stand wurde inzwischen geändert. Bitte neu laden und die Änderung erneut prüfen.' : 'Speichern konnte nicht bestätigt werden. Bitte den aktuellen Stand neu laden.'}</p>}
    <p role="status" aria-live="polite" className="text-sm text-text-secondary">{notice}</p>
  </div>;
}

export function SocialAiUsagePage() {
  const [filter, setFilter] = useState<UsageFilter>({ page: 1 });
  const [draft, setDraft] = useState({ user_id: '', provider: '', job_id: '' });
  const usage = useQuery({ queryKey: ['social-ai-usage', filter], queryFn: () => fetchSocialAiUsage(filter), refetchOnWindowFocus: false });
  const controls = useQuery({ queryKey: ['social-ai-controls'], queryFn: fetchSocialAiControls, refetchOnWindowFocus: false });
  return <section className="social-ai space-y-5">
    <PageHeader title="KI-Verbrauch & Grenzen" description="Alle Anbieteranfragen der Social-Media-KI, einschließlich Wiederholungen und fehlgeschlagener Versuche. Kosten aus Tokenverbrauch sind Schätzungen, keine Anbieterrechnung." actions={<button type="button" className="admin-button admin-button-secondary" disabled={controls.isFetching || usage.isFetching} onClick={() => { void usage.refetch(); void controls.refetch(); }}><RefreshCw size={16} aria-hidden="true" /> Stand neu laden</button>} />
    {controls.isPending && <p role="status">Verarbeitungsgrenzen werden geladen …</p>}
    {controls.error && <p role="alert">Verarbeitungsgrenzen konnten nicht geladen werden. Bitte erneut laden.</p>}
    {controls.data && !controls.error && <ControlForm key={controls.data.revision} controls={controls.data} />}
    <form className="panel-card rounded-2xl p-5 space-y-4" onSubmit={event => { event.preventDefault(); setFilter({ ...draft, page: 1 }); }}>
      <h2 className="text-lg font-semibold">Verbrauch filtern</h2>
      <div className="social-ai-form-grid">{([{ key: 'user_id', label: 'Twitch-Nutzer-ID' }, { key: 'provider', label: 'Anbieter' }, { key: 'job_id', label: 'Auftrags-ID' }] as const).map(field => <label key={field.key}>{field.label}<input className="admin-input mt-2" value={draft[field.key]} maxLength={128} onChange={event => setDraft({ ...draft, [field.key]: event.target.value })} /></label>)}</div>
      <button type="submit" className="admin-button admin-button-secondary" disabled={usage.isFetching}>Filter anwenden</button>
    </form>
    {usage.isPending && <p role="status">Verbrauch wird geladen …</p>}
    {usage.error && <p role="alert">Verbrauch konnte nicht geladen werden. Bitte erneut laden. Es wird kein Nullverbrauch angenommen.</p>}
    {usage.data && !usage.error && <div className="panel-card rounded-2xl p-5 space-y-5">
      <h2 className="text-lg font-semibold">Anbieteranfragen</h2>
      <dl className="social-ai-summary">
        <div><dt>Versuche im Filter</dt><dd>{number(usage.data.totals.attempts)}</dd></div>
        <div><dt>Kostenschätzung (Tokenpreise)</dt><dd>{formatMicroUsd(usage.data.totals.estimated_micro_usd)}</dd></div>
        <div><dt>Gemessene Anbieterkosten</dt><dd>{formatMicroUsd(usage.data.totals.measured_micro_usd)}</dd></div>
        <div><dt>Anbieterzeit insgesamt</dt><dd>{formatRuntime(usage.data.totals.provider_latency_ms)}</dd></div>
        <div><dt>Eingabe- / Ausgabetoken</dt><dd>{number(usage.data.totals.input_tokens)} / {number(usage.data.totals.output_tokens)}</dd></div>
        <div><dt>Cache geschrieben / gelesen</dt><dd>{number(usage.data.totals.cache_creation_input_tokens)} / {number(usage.data.totals.cache_read_input_tokens)}</dd></div>
      </dl>
      {usage.data.totals.unfinished_attempts > 0 && <p role="status" className="text-warning">{usage.data.totals.unfinished_attempts} Versuche sind noch offen oder ungeklärt. Die Kostensumme ist unbekannt; Reservierungen bleiben gebunden.</p>}
      {usage.data.attempts.length === 0 ? <p className="empty-state">Keine Anbieteranfragen für diesen Filter. Es liegen keine gemessenen Kosten vor.</p> : <div className="social-ai-table" role="region" aria-label="Anbieteranfragen, horizontal scrollbar" tabIndex={0}><table><thead><tr><th>Zeit / Versuch</th><th>Nutzer / Auftrag</th><th>Anbieter / Modell</th><th>Ergebnis</th><th>Token ein / aus</th><th>Schätzung</th><th>Reservierung</th><th>Anbieterzeit</th></tr></thead><tbody>{usage.data.attempts.map(attempt => <tr key={attempt.id}><td>{attempt.timestamp}<small>Versuch {attempt.id}</small></td><td>{attempt.twitch_user_id ?? 'Unbekannt'}<small>{attempt.job_id ?? 'Unbekannter Auftrag'} · {attempt.stage ?? 'Unbekannte Phase'}</small></td><td>{attempt.provider ?? 'Unbekannt'}<small>{attempt.model ?? 'Unbekannt'}</small></td><td>{states[attempt.state ?? ''] ?? 'Unbekannt'}</td><td>{number(attempt.input_tokens)} / {number(attempt.output_tokens)}</td><td>{formatMicroUsd(attempt.estimated_micro_usd)}<small>{attempt.cost_basis === 'usage_priced_estimate' ? 'Tokenpreis-Schätzung' : 'Keine vollständige Kostenschätzung'}</small></td><td>{formatMicroUsd(attempt.reserved_micro_usd)}</td><td>{formatRuntime(attempt.provider_latency_ms)}</td></tr>)}</tbody></table></div>}
      <div className="flex items-center justify-between gap-3"><button type="button" className="admin-button admin-button-secondary" disabled={(filter.page ?? 1) <= 1 || usage.isFetching} onClick={() => setFilter({ ...filter, page: (filter.page ?? 1) - 1 })}>Zurück</button><span>Seite {filter.page ?? 1}</span><button type="button" className="admin-button admin-button-secondary" disabled={!usage.data.has_more || usage.isFetching} onClick={() => setFilter({ ...filter, page: (filter.page ?? 1) + 1 })}>Weiter</button></div>
      {usage.data.jobs.length > 0 && <><h3 className="font-semibold">Laufzeit der Aufträge auf dieser Seite</h3><p className="text-sm text-text-secondary">FFmpeg-Zeit ist interne Rechenzeit, kein direkt berechneter API-Preis. Gesamtlaufzeit und Anbieterzeit können Wartezeiten enthalten und werden getrennt ausgewiesen.</p><div className="social-ai-table" role="region" aria-label="Auftragslaufzeiten, horizontal scrollbar" tabIndex={0}><table><thead><tr><th>Auftrag</th><th>Nutzer</th><th>Gesamtlaufzeit</th><th>FFmpeg-Laufzeit</th></tr></thead><tbody>{usage.data.jobs.map(job => <tr key={job.job_id}><td>{job.job_id}</td><td>{job.twitch_user_id}</td><td>{formatRuntime(job.total_processing_ms)}</td><td>{formatRuntime(job.ffmpeg_runtime_ms)}</td></tr>)}</tbody></table></div></>}
    </div>}
  </section>;
}
