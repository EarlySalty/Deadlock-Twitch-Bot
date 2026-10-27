import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ExternalLink, Play, RefreshCw, Scale } from 'lucide-react';
import { fetchObserverReport, observerKeys, rateTrial, startComparison } from '@/api/socialAiObservers';
import type { ObserverComparison, ObserverReport, ObserverScore, ObserverTrial } from '@/api/socialAiObservers';
import { EmptyState } from '@/components/shared/EmptyState';
import { formatDate, formatDuration } from '@/utils/socialAi';
import { actionError, QueryState } from './SocialAiLayout';

const STATUS = { queued: 'Wartet', running: 'Schaut gerade', answered: 'Fertig', failed: 'Fehlgeschlagen' } as const;
const FAILURES: Record<string, string> = {
  source: 'Clip-Datei nicht lesbar',
  browser_unavailable: 'Browser nicht erreichbar',
  not_logged_in: 'Gemini nicht angemeldet',
  model_unavailable: 'Modell oder Denkstufe nicht wählbar',
  rate_limited: 'Gemini-Limit erreicht',
  refused: 'Gemini hat abgelehnt',
  timeout: 'Zeitüberschreitung',
  ui: 'Gemini-Oberfläche hat nicht reagiert',
  configuration: 'Konfiguration ungültig',
};

function variantName(item: { model_item: string; thinking_item: string }) {
  return `${item.model_item} · ${item.thinking_item}`;
}

function seconds(value: number | null) {
  return value === null ? '–' : formatDuration(Math.round(value));
}

function pending(report: ObserverReport | undefined) {
  return !!report?.comparisons.some(comparison => comparison.status === 'queued'
    || comparison.trials.some(trial => trial.status === 'queued' || trial.status === 'running'));
}

function Scoreboard({ scores }: { scores: ObserverScore[] }) {
  if (scores.length === 0) return null;
  return <section className="panel-card rounded-2xl p-5 space-y-3">
    <h2 className="text-xl font-semibold">Rangliste</h2>
    <p className="text-sm text-text-secondary">Sortiert nach deiner Durchschnittsbewertung. Nur bewertete Antworten zählen in den Schnitt.</p>
    <div className="social-ai-table" role="region" aria-label="Rangliste der Modelle, horizontal scrollbar" tabIndex={0}>
      <table>
        <thead><tr><th>Modell</th><th>Ø Bewertung</th><th>Bewertet</th><th>Antworten</th><th>Fehler</th><th>Ø Dauer</th></tr></thead>
        <tbody>{scores.map(score => <tr key={score.observer}>
          <td className="font-semibold">{variantName(score)}</td>
          <td>{score.avg_rating === null ? '–' : score.avg_rating.toLocaleString('de-DE', { maximumFractionDigits: 2 })}</td>
          <td>{score.rated}</td>
          <td>{score.answered}{score.pending > 0 ? ` (+${score.pending} offen)` : ''}</td>
          <td>{score.failed}</td>
          <td>{seconds(score.avg_elapsed_ms)}</td>
        </tr>)}</tbody>
      </table>
    </div>
  </section>;
}

function Rating({ trial, disabled, onSave }: { trial: ObserverTrial; disabled: boolean; onSave: (rating: number, note: string) => void }) {
  const [rating, setRating] = useState(trial.rating ?? 0);
  const [note, setNote] = useState(trial.rating_note ?? '');
  const changed = rating !== (trial.rating ?? 0) || note.trim() !== (trial.rating_note ?? '');
  return <div className="space-y-2 border-t border-border pt-3">
    <div className="flex flex-wrap items-center gap-1" role="radiogroup" aria-label={`Bewertung für ${variantName(trial)}`}>
      {[1, 2, 3, 4, 5].map(value => <button key={value} type="button" role="radio" aria-checked={rating === value} disabled={disabled}
        className={`social-ai-star ${rating >= value ? 'social-ai-star-on' : ''}`} onClick={() => setRating(value)}>
        {value}
      </button>)}
    </div>
    <textarea className="admin-input" rows={2} maxLength={1000} placeholder="Notiz, z. B. was falsch erkannt wurde" value={note} disabled={disabled} onChange={event => setNote(event.target.value)} />
    <button type="button" className="admin-button admin-button-secondary" disabled={disabled || rating === 0 || !changed} onClick={() => onSave(rating, note.trim())}>
      Bewertung speichern
    </button>
  </div>;
}

function TrialCard({ trial, disabled, onRate }: { trial: ObserverTrial; disabled: boolean; onRate: (rating: number, note: string) => void }) {
  return <article className="social-ai-trial">
    <header className="flex flex-wrap items-baseline justify-between gap-2">
      <h4 className="font-semibold">{variantName(trial)}</h4>
      <span className={`social-ai-status social-ai-status-${trial.status}`}>{STATUS[trial.status]}</span>
    </header>
    <p className="text-sm text-text-secondary">
      {trial.elapsed_ms !== null ? `Dauer ${formatDuration(trial.elapsed_ms)}` : trial.started_at ? `Gestartet ${formatDate(trial.started_at)}` : 'Noch nicht gestartet'}
    </p>
    {trial.status === 'failed' && <p className="social-ai-notice">{FAILURES[trial.failure_code ?? ''] ?? 'Unbekannter Fehler'}</p>}
    {trial.answer && <pre className="social-ai-answer">{trial.answer}</pre>}
    {trial.status === 'answered' && <Rating key={`${trial.id}:${trial.rated_at ?? ''}`} trial={trial} disabled={disabled} onSave={onRate} />}
  </article>;
}

function ComparisonCard({ comparison, disabled, onRate }: { comparison: ObserverComparison; disabled: boolean; onRate: (trialId: number, rating: number, note: string) => void }) {
  const done = comparison.trials.filter(trial => trial.status === 'answered' || trial.status === 'failed').length;
  return <section className="panel-card rounded-2xl p-5 space-y-4">
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 className="text-lg font-semibold">{comparison.title || 'Ohne Titel'}</h3>
        <p className="text-sm text-text-secondary">{comparison.login} · Clip {comparison.clip_db_id} · {formatDate(comparison.created_at)} · {done} von {comparison.trials.length || '…'} fertig</p>
      </div>
      {comparison.clip_url && <a className="admin-button admin-button-secondary" href={comparison.clip_url} target="_blank" rel="noreferrer"><ExternalLink size={16} aria-hidden="true" /> Clip ansehen</a>}
    </div>
    {comparison.trials.length === 0
      ? <p className="text-text-secondary">Der Vergleich wird gerade vorbereitet.</p>
      : <div className="social-ai-trials">{comparison.trials.map(trial => <TrialCard key={trial.id} trial={trial} disabled={disabled} onRate={(rating, note) => onRate(trial.id, rating, note)} />)}</div>}
  </section>;
}

export function SocialAiObserversPage() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: observerKeys.report,
    queryFn: ({ signal }) => fetchObserverReport(signal),
    retry: false,
    refetchOnWindowFocus: false,
    refetchInterval: ({ state }) => (pending(state.data) ? 15_000 : false),
  });
  const [clip, setClip] = useState('');
  const [notice, setNotice] = useState('');
  const start = useMutation({
    mutationFn: (id: number) => startComparison(id),
    retry: false,
    onSuccess: data => { client.setQueryData(observerKeys.report, data); setClip(''); setNotice('Vergleich gestartet. Die Modelle schauen den Clip nacheinander, das dauert pro Modell einige Minuten.'); },
  });
  const rate = useMutation({
    mutationFn: (change: { id: number; rating: number; note: string }) => rateTrial(change.id, change.rating, change.note),
    retry: false,
    onSuccess: data => { client.setQueryData(observerKeys.report, data); setNotice('Bewertung gespeichert.'); },
  });
  const busy = start.isPending || rate.isPending;
  const clipId = Number(clip);

  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h2 className="text-xl font-semibold">Gemini-Modelle vergleichen</h2>
      <button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching} onClick={() => void query.refetch()}><RefreshCw size={16} aria-hidden="true" /> Stand neu laden</button>
    </div>
    <p className="text-sm text-text-secondary">Jedes eingestellte Gemini-Modell schaut denselben Clip mit derselben Aufgabe wie im echten Ablauf. Bewerte danach, wie gut es den Clip verstanden hat. Die Rangliste zeigt, welches Modell vorne liegt.</p>
    <QueryState pending={query.isPending} error={query.error}>
      {query.data && <>
        <section className="panel-card rounded-2xl p-5 space-y-3">
          <h2 className="text-xl font-semibold">Neuer Vergleich</h2>
          <div className="flex flex-wrap items-end gap-3">
            <label className="min-w-0 flex-1">Clip auswählen
              <select className="admin-input mt-1" value={clip} disabled={busy} onChange={event => setClip(event.target.value)}>
                <option value="">Bitte wählen</option>
                {query.data.clips.map(item => <option key={item.clip_db_id} value={item.clip_db_id}>
                  {item.login}: {item.title || 'Ohne Titel'} ({item.clip_db_id}){item.compared ? ' · schon verglichen' : ''}
                </option>)}
              </select>
            </label>
            <button type="button" className="admin-button" disabled={busy || !Number.isSafeInteger(clipId) || clipId <= 0} onClick={() => { setNotice(''); start.mutate(clipId); }}>
              <Play size={16} aria-hidden="true" /> Vergleich starten
            </button>
          </div>
        </section>
        <Scoreboard scores={query.data.scoreboard} />
        {query.data.comparisons.length === 0
          ? <EmptyState icon={Scale} title="Noch kein Vergleich" description="Wähle oben einen Clip und starte den ersten Vergleich." />
          : query.data.comparisons.map(comparison => <ComparisonCard key={comparison.id} comparison={comparison} disabled={busy}
            onRate={(id, rating, note) => { setNotice(''); rate.mutate({ id, rating, note }); }} />)}
      </>}
    </QueryState>
    {(start.error || rate.error) && <p role="alert" className="social-ai-notice">{actionError(start.error || rate.error)}</p>}
    <p role="status" aria-live="polite" className="text-sm text-text-secondary">{notice}</p>
  </div>;
}
