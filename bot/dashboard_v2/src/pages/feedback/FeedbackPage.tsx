import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, Check, Inbox, Lightbulb, MessageSquare, Send } from 'lucide-react';
import { feedbackStatusLabels, feedbackStatuses, fetchFeedback, fetchFeedbackDetail, fetchFeedbackRoadmap, markFeedbackRead, submissionAttempt, submitFeedback, updateFeedback, validFeedbackResultPath } from '../../api/feedback';
import type { FeedbackDetail, FeedbackEntry, FeedbackKind, FeedbackStatus, FeedbackSubmission, FeedbackUpdate } from '../../api/feedback';
import { ApiHttpError } from '../../api/core';
import { useFeedbackIdentity } from '../../components/feedback/useFeedbackCounts';
import { FeedbackBadge } from '../../components/feedback/FeedbackBadge';
import '../../components/feedback/feedback.css';

function confirmLeave() {
  return !document.querySelector('[data-unsaved="true"]') || window.confirm('Ungesendeten Text und ungespeicherte Änderungen verwerfen?');
}
function ErrorNotice({ error }: { error: unknown }) {
  return <p className="feedback-notice mt-4" role="alert">{error instanceof Error ? error.message : 'Das hat gerade nicht geklappt. Bitte versuche es erneut.'}</p>;
}
function dateLabel(raw: string) { return new Date(raw).toLocaleString('de-DE', { dateStyle: 'medium', timeStyle: 'short' }); }
function Status({ status }: { status: FeedbackStatus }) { return <span className="feedback-status">{feedbackStatusLabels[status]}</span>; }

function FeedbackForm({ initialKind, csrfToken, onSaved, onCancel }: { initialKind: FeedbackKind; csrfToken?: string | null; onSaved: (id: number) => void; onCancel: () => void }) {
  const [draft, setDraft] = useState<FeedbackSubmission>({ kind: initialKind, title: '', body: '', area: (new URLSearchParams(window.location.search).get('bereich') || '').slice(0, 80) });
  const attempt = useRef<{ fingerprint: string; requestId: string } | undefined>(undefined);
  const mutation = useMutation({ mutationFn: () => {
    const payload = { ...draft, title: draft.title.trim(), body: draft.body.trim(), area: draft.area.trim() };
    attempt.current = submissionAttempt(payload, attempt.current);
    return submitFeedback(payload, attempt.current.requestId, csrfToken);
  }, onSuccess: saved => onSaved(saved.id) });
  function submit(event: FormEvent) { event.preventDefault(); if (!mutation.isPending) mutation.mutate(); }
  return <form onSubmit={submit} data-unsaved={draft.title || draft.body || mutation.isPending ? 'true' : undefined} className="panel-card feedback-card rounded-2xl p-5 md:p-6" aria-labelledby="feedback-form-title">
    <h2 id="feedback-form-title" className="text-xl font-bold text-white">Deine Rückmeldung</h2>
    <p className="mt-2 text-sm leading-6 text-text-secondary">Nur du und die Betreiber sehen deinen Text. Deine Antwort findest du später hier im Dashboard.</p>
    <fieldset disabled={mutation.isPending} className="mt-5 space-y-4">
      <label className="block text-sm font-medium">Worum geht es?<select className="feedback-field" value={draft.kind} onChange={e => setDraft({ ...draft, kind: e.target.value as FeedbackKind })}><option value="feedback">Kritik oder Feedback</option><option value="feature">Feature-Wunsch</option></select></label>
      {draft.kind === 'feature' && <p className="text-sm leading-6 text-text-secondary">Eine Spotify-Anbindung ist derzeit ausgeschlossen. Andere Wünsche sind willkommen.</p>}
      <label className="block text-sm font-medium">Kurzer Titel<input className="feedback-field" autoFocus required minLength={3} maxLength={120} value={draft.title} placeholder="Zum Beispiel: Den Overlay-Schalter finde ich nicht" onChange={e => setDraft({ ...draft, title: e.target.value })} /></label>
      <label className="block text-sm font-medium">Was stört dich oder was wünschst du dir?<textarea className="feedback-field min-h-40 resize-y" required minLength={10} maxLength={4000} value={draft.body} onChange={e => setDraft({ ...draft, body: e.target.value })} aria-describedby="feedback-length" /></label>
      <p id="feedback-length" className="text-right text-xs text-text-secondary">{draft.body.length} / 4000 Zeichen</p>
      <label className="block text-sm font-medium">Bereich <span className="font-normal text-text-secondary">(freiwillig)</span><input className="feedback-field" maxLength={80} value={draft.area} placeholder="Zum Beispiel: Verwaltung, Chat oder Overlay" onChange={e => setDraft({ ...draft, area: e.target.value })} /></label>
    </fieldset>
    {mutation.isError && <ErrorNotice error={mutation.error} />}
    <div className="mt-5 flex flex-wrap gap-3"><button type="submit" className="feedback-button feedback-primary" disabled={mutation.isPending}><Send className="h-4 w-4" aria-hidden="true" />{mutation.isPending ? 'Wird gespeichert …' : mutation.isError ? 'Erneut senden' : 'Rückmeldung senden'}</button><button type="button" className="feedback-button" disabled={mutation.isPending} onClick={() => { if ((!draft.title && !draft.body) || window.confirm('Deinen ungesendeten Text verwerfen?')) onCancel(); }}>Abbrechen</button></div>
  </form>;
}

function AdminReply({ entry, csrfToken, identityKey, onUpdated }: { entry: FeedbackDetail; csrfToken?: string | null; identityKey: string; onUpdated: () => void }) {
  const queryClient = useQueryClient();
  const initial = (item: FeedbackDetail): FeedbackUpdate => ({ expected_revision: item.revision, status: item.own_status ?? item.status, reply: '', result_path: item.own_result_path ?? null, roadmap_id: item.roadmap_id ?? null, duplicate_of: item.duplicate_of ?? null, decision_reason: item.own_decision_reason ?? null });
  const [draft, setDraft] = useState<FeedbackUpdate>(() => initial(entry));
  const [refreshError, setRefreshError] = useState<unknown>();
  const [refreshing, setRefreshing] = useState(false);
  const attempt = useRef<{ fingerprint: string; requestId: string } | undefined>(undefined);
  const roadmap = useQuery({ queryKey: ['feedback', identityKey, 'roadmap'], queryFn: fetchFeedbackRoadmap, staleTime: 30_000 });
  const mutation = useMutation({ mutationFn: async () => {
    const payload = { ...draft, reply: draft.reply.trim(), result_path: draft.result_path?.trim() || null };
    attempt.current = submissionAttempt(payload, attempt.current);
    await updateFeedback(entry.id, payload, attempt.current.requestId, csrfToken);
    // Auch wenn das anschließende Lesen scheitert: die Wiederholung bleibt idempotent.
    return fetchFeedbackDetail(entry.id);
  }, onSuccess: current => {
    setDraft(initial(current)); attempt.current = undefined;
    queryClient.setQueryData(['feedback', identityKey, 'detail', entry.id], current); onUpdated();
  } });
  const conflict = mutation.error instanceof ApiHttpError && mutation.error.status === 409;
  async function reload() {
    setRefreshing(true); setRefreshError(undefined);
    try { const current = await fetchFeedbackDetail(entry.id); setDraft(old => ({ ...initial(current), reply: old.reply })); queryClient.setQueryData(['feedback', identityKey, 'detail', entry.id], current); attempt.current = undefined; mutation.reset(); }
    catch (error) { setRefreshError(error); } finally { setRefreshing(false); }
  }
  const linked = draft.duplicate_of !== null || draft.roadmap_id !== null;
  const pathRequired = draft.roadmap_id !== null || (draft.status === 'done' && draft.duplicate_of === null);
  return <form data-unsaved={JSON.stringify(draft) !== JSON.stringify(initial(entry)) || mutation.isPending ? 'true' : undefined} className="mt-6 border-t border-border pt-6" onSubmit={e => { e.preventDefault(); if (!mutation.isPending && !conflict) mutation.mutate(); }} aria-labelledby="feedback-admin-title">
    <h3 id="feedback-admin-title" className="text-lg font-bold text-white">Antwort und Bearbeitungsstand</h3>
    <fieldset disabled={mutation.isPending || refreshing} className="mt-4 space-y-4">
      <label className="block text-sm font-medium">Status<select className="feedback-field" value={draft.status} disabled={linked} onChange={e => setDraft({ ...draft, status: e.target.value as FeedbackStatus })}>{feedbackStatuses(entry.kind).map(status => <option key={status} value={status}>{feedbackStatusLabels[status]}</option>)}</select></label>
      {linked && <p className="text-sm text-text-secondary">Der sichtbare Stand folgt dem Hauptwunsch beziehungsweise dem öffentlichen Roadmap-Punkt.</p>}
      <label className="block text-sm font-medium">Antwort an den Einreicher<textarea className="feedback-field min-h-28 resize-y" maxLength={4000} required={draft.status === 'answered' && !linked} value={draft.reply} onChange={e => setDraft({ ...draft, reply: e.target.value })} /></label>
      {!linked && draft.status === 'declined' && <label className="block text-sm font-medium">Kurze Begründung<textarea className="feedback-field min-h-24 resize-y" required minLength={3} maxLength={800} value={draft.decision_reason ?? ''} onChange={e => setDraft({ ...draft, decision_reason: e.target.value })} /><span className="mt-1 block text-xs leading-5 text-text-secondary">Diese Begründung sehen auch die Einreicher gebündelter Wünsche. Bitte keine persönlichen Angaben übernehmen.</span></label>}
      {draft.duplicate_of === null && <label className="block text-sm font-medium">Link zur verfügbaren Funktion{pathRequired ? '' : ' (freiwillig)'}<input className="feedback-field" required={pathRequired} maxLength={200} placeholder="/twitch/verwaltung#overlay" value={draft.result_path ?? ''} onChange={e => { e.target.setCustomValidity(''); setDraft({ ...draft, result_path: e.target.value || null }); }} onBlur={e => e.target.setCustomValidity(e.target.value && !validFeedbackResultPath(e.target.value) ? 'Bitte eine Dashboard-Seite ohne private IDs oder Zugangsparameter eintragen.' : '')} /><span className="mt-1 block text-xs leading-5 text-text-secondary">Bei einer Roadmap-Zuordnung schon das Ziel eintragen; der Link erscheint für Nutzer erst bei „Umgesetzt“.</span></label>}
      {entry.kind === 'feature' && <>
        <label className="block text-sm font-medium">Öffentlicher Roadmap-Punkt<select className="feedback-field" disabled={draft.duplicate_of !== null || roadmap.isPending || roadmap.isError} value={draft.roadmap_id ?? ''} onChange={e => setDraft({ ...draft, roadmap_id: e.target.value ? Number(e.target.value) : null })}><option value="">Keine Zuordnung</option>{(roadmap.data ?? []).map(item => <option key={item.id} value={item.id}>{item.title} · {feedbackStatusLabels[item.status as FeedbackStatus] ?? item.status}</option>)}</select></label>
        {roadmap.isError && <div><ErrorNotice error={roadmap.error} /><button className="feedback-button mt-2" type="button" onClick={() => void roadmap.refetch()}>Roadmap erneut laden</button></div>}
        <label className="block text-sm font-medium">Mit Hauptwunsch bündeln <span className="font-normal text-text-secondary">(Nummer aus der Inbox, freiwillig)</span><input className="feedback-field" type="number" min={1} step={1} value={draft.duplicate_of ?? ''} onChange={e => { const duplicate = e.target.value ? Number(e.target.value) : null; setDraft({ ...draft, duplicate_of: duplicate, ...(duplicate ? { roadmap_id: null, result_path: null } : {}) }); }} /><span className="mt-1 block text-xs leading-5 text-text-secondary">Die Zuordnung bleibt intern. Fremde Texte und Antworten werden nicht geteilt.</span></label>
      </>}
    </fieldset>
    {mutation.isError && <ErrorNotice error={mutation.error} />}{refreshError != null && <ErrorNotice error={refreshError} />}
    {conflict && <button type="button" className="feedback-button mt-3" disabled={refreshing} onClick={() => void reload()}>Aktuellen Stand übernehmen, Antwort behalten</button>}
    <button className="feedback-button feedback-primary mt-5" disabled={mutation.isPending || conflict || refreshing} type="submit">{mutation.isPending ? 'Wird gespeichert …' : 'Antwort und Stand speichern'}</button>
    {mutation.isSuccess && <p role="status" className="mt-3 text-sm text-primary">Antwort und Stand sind gespeichert.</p>}
  </form>;
}

function EntryDetail({ id, inbox, identityKey, csrfToken, onBack }: { id: number; inbox: boolean; identityKey: string; csrfToken?: string | null; onBack: () => void }) {
  const queryClient = useQueryClient();
  const detail = useQuery({ queryKey: ['feedback', identityKey, 'detail', id], queryFn: () => fetchFeedbackDetail(id), refetchOnWindowFocus: false, staleTime: 0 });
  const readAttempt = useRef('');
  const reader = useMutation({ mutationFn: (observedAt: string) => markFeedbackRead(id, observedAt, inbox, csrfToken), onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ['feedback', identityKey, 'counts'] }); void queryClient.invalidateQueries({ queryKey: ['feedback', identityKey, 'list'] }); } });
  const mark = reader.mutate;
  useEffect(() => {
    const current = detail.data;
    if (!current || (inbox ? !current.admin_unread : !current.unread)) return;
    const key = `${id}:${inbox}:${current.observed_at}`;
    if (readAttempt.current === key) return;
    readAttempt.current = key; mark(current.observed_at);
  }, [detail.data, id, inbox, mark]);
  const entry = detail.data;
  const heading = useRef<HTMLHeadingElement>(null);
  const entryId = entry?.id;
  useEffect(() => { if (entryId != null) heading.current?.focus(); }, [entryId]);
  return <section className="panel-card feedback-card rounded-2xl p-5 md:p-6">
    <button type="button" className="feedback-button mb-5" onClick={onBack}><ArrowLeft className="h-4 w-4" aria-hidden="true" />Zur Übersicht</button>
    {detail.isPending && <p role="status" className="text-text-secondary">Rückmeldung wird geladen …</p>}
    {detail.isError && <><ErrorNotice error={detail.error} /><button className="feedback-button mt-3" onClick={() => void detail.refetch()}>Erneut laden</button></>}
    {entry && <>
      <div className="flex flex-wrap items-center gap-2"><Status status={entry.status} /><span className="text-xs text-text-secondary">{entry.kind === 'feature' ? 'Feature-Wunsch' : 'Feedback'} · {dateLabel(entry.created_at)}</span></div>
      <h2 ref={heading} tabIndex={-1} className="feedback-text mt-4 text-2xl font-bold text-white">{entry.title}</h2>
      {entry.area && <p className="feedback-text mt-2 text-sm text-text-secondary">Bereich: {entry.area}</p>}
      {inbox && <p className="feedback-text mt-2 text-sm text-text-secondary">#{entry.id} · {entry.author_label} · Twitch-ID {entry.owner_id}</p>}
      <p className="feedback-text mt-5 text-sm leading-7">{entry.body}</p>
      {entry.status === 'declined' && entry.decision_reason && <p className="feedback-notice feedback-text mt-4">{entry.decision_reason}</p>}
      {entry.bundled && <p className="mt-4 text-sm text-text-secondary">Mit einem ähnlichen Wunsch gebündelt. Den gemeinsamen Stand siehst du hier.</p>}
      {entry.roadmap && <div className="mt-4 rounded-xl border border-border bg-background p-4"><p className="text-xs text-text-secondary">Öffentlicher Plan</p><p className="feedback-text mt-1 text-sm font-semibold">{entry.roadmap.title}</p><p className="mt-1 text-xs text-text-secondary">Der Bearbeitungsstand wird aus der Roadmap übernommen.</p></div>}
      {entry.status === 'done' && entry.result_path && validFeedbackResultPath(entry.result_path) && <a className="feedback-button feedback-primary mt-4" href={entry.result_path}>Umgesetzte Funktion öffnen</a>}
      <h3 className="mt-7 text-lg font-bold text-white">Verlauf</h3>
      <ol className="mt-3 space-y-3"><li className="rounded-xl border border-border bg-background p-4"><p className="text-sm font-semibold">Eingegangen</p><p className="mt-1 text-xs text-text-secondary">{dateLabel(entry.created_at)} · Deine Rückmeldung ist gespeichert.</p></li>{entry.events.map(event => <li key={event.id} className="rounded-xl border border-border bg-background p-4"><div className="flex flex-wrap items-center gap-2"><Status status={event.status} /><span className="text-xs text-text-secondary">{dateLabel(event.created_at)}</span></div>{event.decision_reason && <p className="feedback-text mt-3 text-sm leading-7">{event.decision_reason}</p>}{event.reply && <p className="feedback-text mt-3 text-sm leading-7">{event.reply}</p>}</li>)}</ol>
      {reader.isError && <div><ErrorNotice error={reader.error} /><button className="feedback-button mt-3" disabled={reader.isPending} onClick={() => reader.mutate(entry.observed_at)}>Als gelesen markieren</button></div>}
      {inbox && <AdminReply key={id} entry={entry} identityKey={identityKey} csrfToken={csrfToken} onUpdated={() => { void queryClient.invalidateQueries({ queryKey: ['feedback', identityKey] }); }} />}
    </>}
  </section>;
}
function EntryListItem({ entry, inbox, onOpen }: { entry: FeedbackEntry; inbox: boolean; onOpen: () => void }) {
  return <li>
    <button type="button" data-feedback-entry={entry.id} className="w-full rounded-xl border border-border bg-background p-4 text-left transition-colors hover:border-primary" onClick={onOpen}>
      <div className="flex flex-wrap items-center gap-2">
        <Status status={entry.status} />
        {(inbox ? entry.admin_unread : entry.unread) && <span className="rounded-full bg-primary px-2 py-0.5 text-xs font-bold text-black">{inbox ? 'Neu' : 'Ungelesen'}</span>}
        <span className="text-xs text-text-secondary">{entry.kind === 'feature' ? 'Wunsch' : 'Feedback'}</span>
      </div>
      <p className="feedback-text mt-3 font-semibold text-white">{entry.title}</p>
      <p className="mt-2 text-xs text-text-secondary">{inbox ? `#${entry.id} · ${entry.author_label} · ` : ''}{dateLabel(entry.created_at)}</p>
    </button>
  </li>;
}

function FeedbackContent({ isAdmin, csrfToken, identityKey, hasUserId }: { isAdmin: boolean; csrfToken?: string | null; identityKey: string; hasUserId: boolean }) {
  const queryClient = useQueryClient();
  const initialKind = new URLSearchParams(window.location.search).get('typ');
  const [form, setForm] = useState<FeedbackKind | null>(initialKind === 'feature' || initialKind === 'feedback' ? initialKind : null);
  const [selected, setSelected] = useState<number | null>(null);
  const [saved, setSaved] = useState(false);
  const lastEntry = useRef<number | null>(null);
  const restoreFocus = () => requestAnimationFrame(() => {
    const target = document.querySelector<HTMLElement>(`[data-feedback-entry="${lastEntry.current}"]`)
      ?? document.querySelector<HTMLElement>('[data-feedback-list-title]');
    target?.focus();
  });
  const changeView = (change: () => void) => { if (confirmLeave()) { change(); requestAnimationFrame(() => { if (!document.querySelector('[aria-labelledby="feedback-form-title"]')) restoreFocus(); }); } };
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (document.querySelector('[data-unsaved="true"]')) { event.preventDefault(); event.returnValue = ''; }
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, []);
  const [inbox, setInbox] = useState(isAdmin && !hasUserId);
  const entries = useInfiniteQuery({ queryKey: ['feedback', identityKey, 'list', inbox], queryFn: ({ pageParam }) => fetchFeedback(inbox, pageParam), initialPageParam: undefined as number | undefined, getNextPageParam: page => page.next ?? undefined, enabled: inbox || hasUserId, retry: 1, refetchOnWindowFocus: true });
  const allEntries = entries.data?.pages.flatMap(page => page.items) ?? [];
  return <div className="feedback-card mx-auto w-full max-w-5xl space-y-5">
    <section className="panel-card rounded-2xl p-5 md:p-6"><div className="flex items-start gap-3"><MessageSquare className="mt-1 h-7 w-7 shrink-0 text-primary" aria-hidden="true" /><div><h1 className="text-2xl font-bold text-white">Kritik und Wünsche</h1><p className="mt-2 text-sm leading-6 text-text-secondary"><strong className="text-white">Kritik ist ausdrücklich erwünscht.</strong> Was fehlt dir? Was nervt? Sag es uns.</p></div></div>
      <p className="mt-3 text-sm leading-6 text-text-secondary">Deine Rückmeldungen bleiben privat. Antworten und den aktuellen Stand findest du auf dieser Seite.</p>
      {!form && hasUserId && <div className="mt-5 flex flex-wrap gap-3"><button className="feedback-button feedback-primary" onClick={() => changeView(() => { setSelected(null); setSaved(false); setForm('feedback'); })}>Feedback geben</button><button className="feedback-button" onClick={() => changeView(() => { setSelected(null); setSaved(false); setForm('feature'); })}><Lightbulb className="h-4 w-4" aria-hidden="true" />Feature wünschen</button></div>}
      {isAdmin && !form && <div className="mt-5 flex flex-wrap gap-2" aria-label="Rückmeldungsansicht">{hasUserId && <button className={`feedback-button ${!inbox ? 'feedback-primary' : ''}`} aria-pressed={!inbox} onClick={() => changeView(() => { setInbox(false); setSelected(null); })}>Meine Rückmeldungen <FeedbackBadge /></button>}<button className={`feedback-button ${inbox ? 'feedback-primary' : ''}`} aria-pressed={inbox} onClick={() => changeView(() => { setInbox(true); setSelected(null); })}><Inbox className="h-4 w-4" aria-hidden="true" />Betreiber-Inbox <FeedbackBadge isAdmin /></button></div>}
    </section>
    {saved && <p role="status" className="feedback-notice flex items-center gap-2"><Check className="h-5 w-5 shrink-0 text-primary" aria-hidden="true" />Deine Rückmeldung ist gespeichert. Danke, dass du uns sagst, was besser werden kann.</p>}
    {form && hasUserId ? <FeedbackForm initialKind={form} csrfToken={csrfToken} onCancel={() => { setForm(null); restoreFocus(); }} onSaved={id => { setForm(null); setSelected(id); setInbox(false); setSaved(true); void queryClient.invalidateQueries({ queryKey: ['feedback', identityKey] }); }} />
      : selected !== null ? <EntryDetail key={`${selected}:${inbox}`} id={selected} inbox={inbox} identityKey={identityKey} csrfToken={csrfToken} onBack={() => changeView(() => setSelected(null))} />
        : <section className="panel-card rounded-2xl p-5 md:p-6"><h2 data-feedback-list-title tabIndex={-1} className="text-xl font-bold text-white">{inbox ? 'Betreiber-Inbox' : 'Meine Rückmeldungen'}</h2>
          {entries.isPending && <p className="mt-4 text-sm text-text-secondary" role="status">Rückmeldungen werden geladen …</p>}
          {entries.isError && <><ErrorNotice error={entries.error} /><button className="feedback-button mt-3" onClick={() => void entries.refetch()}>Erneut laden</button></>}
          {!entries.isPending && !entries.isError && allEntries.length === 0 && <p className="mt-4 text-sm leading-6 text-text-secondary">{inbox ? 'Bisher sind keine Rückmeldungen eingegangen.' : 'Du hast noch keine Rückmeldung gesendet. Wenn dich etwas stört oder dir etwas fehlt, kannst du es oben direkt sagen.'}</p>}
          <ul className="mt-4 space-y-3">{allEntries.map(entry => <EntryListItem key={entry.id} entry={entry} inbox={inbox} onOpen={() => { lastEntry.current = entry.id; setSelected(entry.id); setSaved(false); }} />)}</ul>
          {entries.hasNextPage && <button className="feedback-button mt-4" disabled={entries.isFetchingNextPage} onClick={() => void entries.fetchNextPage()}>{entries.isFetchingNextPage ? 'Wird geladen …' : 'Ältere Rückmeldungen laden'}</button>}
        </section>}
  </div>;
}
export function FeedbackPage({ isAdmin, csrfToken }: { isAdmin?: boolean; csrfToken?: string | null }) {
  const identity = useFeedbackIdentity();
  if (identity.loading) return <p role="status" className="text-text-secondary">Dein Konto wird geprüft …</p>;
  if (!identity.authenticated || (!identity.userId && !identity.isAdmin)) return <p className="feedback-notice">Bitte melde dich mit Twitch an, damit du deine privaten Rückmeldungen sehen und senden kannst.</p>;
  return <FeedbackContent key={identity.key} identityKey={identity.key} isAdmin={(isAdmin ?? identity.isAdmin) && identity.isAdmin} csrfToken={csrfToken ?? identity.csrfToken} hasUserId={!!identity.userId} />;
}
export default FeedbackPage;
