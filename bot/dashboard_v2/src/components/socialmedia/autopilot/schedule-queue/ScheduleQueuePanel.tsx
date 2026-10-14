import { useEffect, useId, useRef, useState } from 'react';
import { ArrowDown, ArrowUp, Clock3, RefreshCw, Trash2 } from 'lucide-react';
import { ApiHttpError, getBrowserTimezone } from '@/api/core';
import { scheduleQueueApi } from './api';
import { ReviewJobActions } from './ReviewJobActions';
import type { QueueJob, SchedulePatch, ScheduleQueueApi, ScheduleQueueSnapshot, ScheduleSettings } from './api';
import './schedule-queue.css';

export interface ScheduleQueuePanelProps {
  twitchUserId?: string;
  api?: ScheduleQueueApi;
  onChanged?: () => void;
}

const WEEKDAYS = ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag'];
const PLATFORMS = [{ id: 'youtube', label: 'YouTube' }, { id: 'tiktok', label: 'TikTok' }, { id: 'instagram', label: 'Instagram' }];
const HOLDS = {
  disabled: 'Der Auto-Pilot ist ausgeschaltet.',
  paused: 'Pausiert. Laufende Produktionen werden fertiggestellt.',
  review_backlog: 'Zwei automatische Produktionen warten auf deine Prüfung. Nach einer Entscheidung geht es weiter.',
  empty_pool: 'Keine passenden Clips verfügbar. Prüfe deinen Clip-Pool oder den Zeitraum.',
  no_schedule: 'Füge mindestens einen wöchentlichen Termin hinzu.',
  outside_lead: 'Der nächste freie Termin liegt noch außerhalb des Produktionsvorlaufs.',
};
const STATUS: Record<QueueJob['status'], string> = {
  queued: 'Wartet', running: 'In Produktion', review: 'Prüfung nötig', completed: 'Fertig', failed: 'Fehlgeschlagen', cancelled: 'Entfernt',
};
const REASONS: Record<string, string> = {
  ai_editor_unavailable: 'Der KI-Editor ist noch nicht verfügbar. Es wird kein statisches Ersatzvideo erstellt.',
  external_ai_consent_required: 'Die Zustimmung zur externen KI-Verarbeitung fehlt.',
  attempts_exhausted: 'Das Versuchslimit ist erreicht. Bitte prüfe den Auftrag.',
  production_interrupted: 'Die Produktion wurde unterbrochen und wird fortgesetzt.',
  unsuitable_clip: 'Die KI hält diesen Clip möglicherweise für ungeeignet. Entscheide, ob er trotzdem verarbeitet werden soll.',
};

function slotLabel(value: string | null, timezone: string): string {
  if (!value) return 'Noch kein Termin';
  try {
    return new Intl.DateTimeFormat('de-DE', {
      timeZone: timezone, weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
    }).format(new Date(value));
  } catch {
    return 'Termin nicht verfügbar';
  }
}

function errorText(error: unknown): string {
  if (error instanceof ApiHttpError && error.status === 401) return 'Bitte melde dich erneut an.';
  if (error instanceof ApiHttpError && error.status === 403) return 'Du hast keinen Zugriff auf den Zeitplan dieses Kanals.';
  if (error instanceof ApiHttpError && error.message === 'unknown_streamer') return 'Dieser Twitch-Kanal wurde nicht gefunden. Wähle einen anderen Kanal.';
  if (error instanceof ApiHttpError && error.status === 400 && error.message.startsWith('Server-Fehler')) return 'Wähle einen Twitch-Kanal und prüfe deine Eingaben.';
  if (error instanceof ApiHttpError && error.status >= 500) return 'Der Zeitplan ist gerade nicht verfügbar. Bitte versuche es später erneut.';
  if (error instanceof TypeError) return 'Die Verbindung ist unterbrochen. Bitte versuche es erneut.';
  return error instanceof ApiHttpError ? error.message : 'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.';
}

export function ScheduleQueuePanel(props: ScheduleQueuePanelProps) {
  return <ScheduleQueueOwner key={props.twitchUserId ?? 'self'} {...props} />;
}

function ScheduleQueueOwner({ twitchUserId, api = scheduleQueueApi, onChanged }: ScheduleQueuePanelProps) {
  const id = useId();
  const [snapshot, setSnapshot] = useState<ScheduleQueueSnapshot | null>(null);
  const [draft, setDraft] = useState<ScheduleSettings | null>(null);
  const [patch, setPatch] = useState<SchedulePatch>({});
  const [pending, setPending] = useState<'read' | 'mutation' | null>(null);
  const busy = pending !== null;
  const saving = pending === 'mutation';
  const hasChanges = Object.keys(patch).length > 0;
  const [edited, setEdited] = useState(false);
  const unsaved = edited || saving ? 'true' : undefined;
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [removeId, setRemoveId] = useState<number | null>(null);
  const live = useRef(true);
  const dirty = useRef(false);
  const epoch = useRef(0);
  const mutating = useRef(false);
  const refresh = useRef<((message?: string) => void) | null>(null);
  const changed = useRef(onChanged);
  useEffect(() => { changed.current = onChanged; }, [onChanged]);

  useEffect(() => {
    const controller = new AbortController();
    let loading = false;
    let queued = false;
    let queuedMessage = '';
    live.current = true;
    const load = async (message = '') => {
      if (!live.current || controller.signal.aborted) return;
      if (message) {
        queuedMessage = message;
        setPending('read');
        setError('');
      }
      if (loading || mutating.current) {
        queued = true;
        return;
      }
      loading = true;
      queued = false;
      const readMessage = queuedMessage;
      queuedMessage = '';
      const notifyChanged = changed.current;
      const generation = ++epoch.current;
      try {
        const result = await api.load(twitchUserId, controller.signal);
        if (readMessage) notifyChanged?.();
        if (!live.current || controller.signal.aborted || generation !== epoch.current) return;
        setSnapshot(result);
        if (!dirty.current) {
          const timezone = result.persisted ? result.settings.timezone : getBrowserTimezone();
          setDraft({ ...result.settings, timezone });
          if (!result.persisted) {
            setPatch({ timezone });
            dirty.current = true;
          }
        }
        setError('');
        if (readMessage) {
          setRemoveId(null);
          setNotice(readMessage);
        }
      } catch (failure) {
        if (!controller.signal.aborted && live.current && generation === epoch.current) setError(errorText(failure));
      } finally {
        loading = false;
        if (!controller.signal.aborted && live.current) {
          if (generation === epoch.current && !mutating.current) setPending(queuedMessage ? 'read' : null);
          if (queued && !mutating.current) void load();
        }
      }
    };
    refresh.current = message => { void load(message); };
    void load();
    const timer = window.setInterval(() => { void load(); }, 15000);
    return () => { live.current = false; refresh.current = null; controller.abort(); window.clearInterval(timer); };
  }, [api, twitchUserId]);

  function edit(change: SchedulePatch) {
    dirty.current = true;
    setEdited(true);
    setDraft(current => current ? { ...current, ...change } : current);
    setPatch(current => ({ ...current, ...change }));
    setNotice('');
  }

  async function act(request: () => Promise<ScheduleQueueSnapshot>, message: string, saved = false) {
    if (mutating.current) return;
    mutating.current = true;
    let succeeded = false;
    const generation = ++epoch.current;
    const notifyChanged = onChanged;
    setPending('mutation');
    setError('');
    try {
      const result = await request();
      succeeded = true;
      notifyChanged?.();
      if (!live.current || generation !== epoch.current) return;
      setSnapshot(result);
      if (saved || !dirty.current) setDraft(result.settings);
      if (saved) { dirty.current = false; setEdited(false); setPatch({}); }
      setRemoveId(null);
      setNotice(message);
    } catch (failure) {
      if (live.current) setError(errorText(failure));
    } finally {
      mutating.current = false;
      if (live.current) {
        setPending(null);
        if (succeeded) refresh.current?.();
      }
    }
  }

  if (!snapshot || !draft) return <section className="schedule-queue" aria-busy={!error} data-unsaved={unsaved} data-saving={saving ? 'true' : undefined}>
    <h2>Auto-Pilot und Zeitplan</h2>
    {error ? <><p role="alert">{error}</p><button onClick={() => refresh.current?.('Aktualisiert.')}>Erneut laden</button></> : <p role="status">Zeitplan wird geladen.</p>}
  </section>;

  const unstarted = snapshot.jobs.filter(job => job.status === 'queued' && !job.started_at);
  const visible = snapshot.jobs.filter(job => job.status !== 'completed' && job.status !== 'cancelled');
  function move(jobId: number, offset: number) {
    const order = unstarted.map(job => job.id);
    const index = order.indexOf(jobId);
    const destination = index + offset;
    if (index < 0 || destination < 0 || destination >= order.length) return;
    [order[index], order[destination]] = [order[destination], order[index]];
    void act(() => api.reorder(order, twitchUserId), 'Reihenfolge gespeichert.');
  }

  return <section className="schedule-queue" aria-labelledby={`${id}-heading`} data-unsaved={unsaved} data-saving={saving ? 'true' : undefined}>
    <header className="sq-heading">
      <div><h2 id={`${id}-heading`}>Auto-Pilot und Zeitplan</h2><p>Ein Video pro Termin, gemeinsam für deine ausgewählten Plattformen.</p></div>
      <button type="button" disabled={busy} onClick={() => refresh.current?.('Aktualisiert.')}><RefreshCw size={16} />Aktualisieren</button>
    </header>
    <div className="sq-status" role="status">
      <Clock3 size={18} aria-hidden="true" />
      <div><strong>Nächster freier Termin: {slotLabel(snapshot.next_slot, snapshot.settings.timezone)}</strong>
        <p>{snapshot.production_hold ? HOLDS[snapshot.production_hold] : 'Produktion nach Bedarf. Fertige Videos bleiben zunächst zur Prüfung.'}</p>
      </div>
    </div>
    {error && <p className="sq-error" role="alert">{error}</p>}
    {notice && <p className="sq-notice" role="status">{notice}</p>}
    <div className="sq-columns">
      <form className="sq-editor" onSubmit={event => { event.preventDefault(); void act(() => api.save(patch, twitchUserId), 'Zeitplan gespeichert.', true); }}>
        <h3>Wöchentliche Termine</h3>
        <fieldset disabled={busy}>
          <legend className="sq-sr-only">Auto-Pilot-Einstellungen</legend>
          <div className="sq-toggles">
            <label><input type="checkbox" checked={draft.enabled} onChange={event => edit({ enabled: event.target.checked })} /><span>Auto-Pilot einschalten</span></label>
            <label><input type="checkbox" checked={draft.paused} onChange={event => edit({ paused: event.target.checked })} /><span>Neue Produktionen pausieren</span></label>
            <label><input type="checkbox" checked={draft.ai_editing_enabled} onChange={event => edit({ ai_editing_enabled: event.target.checked })} /><span>KI-Bearbeitung verwenden</span></label>
          </div>
          <p className="sq-help">Die KI-Einstellung gilt erst beim Produktionsstart. Sie ersetzt nicht deine Zustimmung zur externen KI-Verarbeitung.</p>
          <fieldset className="sq-platforms">
            <legend>Zielplattformen</legend>
            <div className="sq-toggles">
              {PLATFORMS.map(platform => {
                const connected = snapshot.connected_platforms.includes(platform.id);
                const selected = draft.target_platforms.includes(platform.id);
                return <label key={platform.id}>
                  <input type="checkbox" checked={selected} disabled={!connected && !selected}
                    onChange={event => edit({ target_platforms: event.target.checked
                      ? [...draft.target_platforms, platform.id]
                      : draft.target_platforms.filter(value => value !== platform.id) })} />
                  <span>{platform.label}{!connected && ' (nicht verbunden)'}</span>
                </label>;
              })}
            </div>
            <p className="sq-help">Verbinde fehlende Plattformen unter Konten. Wähle hier, wohin die Videos veröffentlicht werden sollen.</p>
            {!draft.target_platforms.length && <p className="sq-help">Noch keine Zielplattform ausgewählt. Ohne Zielplattform wird kein Video veröffentlicht.</p>}
          </fieldset>
          <div className="sq-fields">
            <label htmlFor={`${id}-zone`}>Zeitzone<input id={`${id}-zone`} value={draft.timezone} required list={`${id}-zones`} onChange={event => edit({ timezone: event.target.value })} /></label>
            <datalist id={`${id}-zones`}><option value="Europe/Berlin" /><option value="Europe/Vienna" /><option value="Europe/Zurich" /><option value="UTC" /></datalist>
            <label htmlFor={`${id}-lead`}>Vorlauf in Stunden<input id={`${id}-lead`} type="number" min="0" max="720" required value={Number.isFinite(draft.lead_time_hours) ? draft.lead_time_hours : ''} onChange={event => edit({ lead_time_hours: event.target.valueAsNumber })} /></label>
          </div>
          <p className="sq-help">Sommerzeit wird berücksichtigt. Auch bei kürzerem Vorlauf kann die Produktion starten. Verpasste Termine werden nicht gesammelt.</p>
          <div className="sq-slots">
            {draft.schedule_slots.map((slot, index) => <div className="sq-slot" key={index}>
              <label><span className="sq-sr-only">Wochentag für Termin {index + 1}</span><select value={slot.weekday} onChange={event => edit({ schedule_slots: draft.schedule_slots.map((value, i) => i === index ? { ...value, weekday: Number(event.target.value) } : value) })}>{WEEKDAYS.map((day, i) => <option value={i + 1} key={day}>{day}</option>)}</select></label>
              <label><span className="sq-sr-only">Uhrzeit für Termin {index + 1}</span><input type="time" required value={slot.local_time} onChange={event => edit({ schedule_slots: draft.schedule_slots.map((value, i) => i === index ? { ...value, local_time: event.target.value } : value) })} /></label>
              <button type="button" className="sq-icon" aria-label={`Termin ${index + 1} entfernen`} onClick={() => edit({ schedule_slots: draft.schedule_slots.filter((_, i) => i !== index) })}><Trash2 size={16} /></button>
            </div>)}
          </div>
          {!draft.schedule_slots.length && <p className="sq-help">Noch kein Termin. Lege fest, wann ein vorbereitetes Video erscheinen soll.</p>}
          <div className="sq-actions"><button type="button" disabled={draft.schedule_slots.length >= 28} onClick={() => edit({ schedule_slots: [...draft.schedule_slots, { weekday: 1, local_time: '18:00' }] })}>Termin hinzufügen</button><button className="sq-primary" type="submit" disabled={!hasChanges}>{busy ? 'Wird gespeichert' : 'Änderungen speichern'}</button></div>
        </fieldset>
      </form>
      <div className="sq-queue">
        <h3>Produktionswarteschlange</h3>
        <p className="sq-help">Ein Auftrag nach dem anderen. Laufende Aufträge lassen sich nicht entfernen oder umordnen.</p>
        {!visible.length && <div className="sq-empty"><strong>Die Warteschlange ist leer.</strong><p>Passende Clips werden erst ausgewählt, wenn ein freier Termin im Vorlauf liegt.</p></div>}
        <ol className="sq-jobs">
          {visible.map(job => {
            const movable = job.status === 'queued' && !job.started_at;
            const position = unstarted.findIndex(value => value.id === job.id);
            return <li key={job.id} className="sq-job">
              <div className="sq-job-head"><strong>{job.clip_title || `Clip ${job.clip_id}`}</strong><span className={job.status === 'running' ? 'sq-running' : 'sq-badge'}>{STATUS[job.status]}</span></div>
              <p className="sq-help">{slotLabel(job.scheduled_at, snapshot.settings.timezone)} · {job.mode ? (job.mode === 'ai' ? 'KI-Bearbeitung' : 'Statisches Layout') : job.source === 'manual' ? 'Manueller Auftrag' : 'Modus wird beim Start festgelegt'}</p>
              {job.review_reasons.map(reason => <p className="sq-help" key={reason}>{REASONS[reason] ?? 'Die Produktion benötigt eine Prüfung.'}</p>)}
              {job.review_explanation && <p className="sq-help">{job.review_explanation}</p>}
              {job.last_error && <p className="sq-error">{job.last_error}</p>}
              {job.status === 'review' && <ReviewJobActions busy={busy} clipTitle={job.clip_title || `Clip ${job.clip_id}`}
                onResolve={(action, note) => { void act(() => api.resolve(job.id, action, note, twitchUserId), action === 'process' ? 'Der Clip ist erneut zur Verarbeitung eingeplant.' : 'Clip übersprungen.'); }} />}
              {movable && <div className="sq-job-actions">
                <button type="button" disabled={busy || position === 0} onClick={() => { void act(() => api.next(job.id, twitchUserId), 'Der Clip ist als Nächstes eingeplant.'); }}>Als Nächstes</button>
                <button type="button" className="sq-icon" aria-label={`${job.clip_title} nach oben`} disabled={busy || position === 0} onClick={() => move(job.id, -1)}><ArrowUp size={16} /></button>
                <button type="button" className="sq-icon" aria-label={`${job.clip_title} nach unten`} disabled={busy || position === unstarted.length - 1} onClick={() => move(job.id, 1)}><ArrowDown size={16} /></button>
                <button type="button" disabled={busy} onClick={() => setRemoveId(job.id)}>Entfernen</button>
              </div>}
              {removeId === job.id && <div className="sq-confirm"><p>Diesen Auftrag entfernen? Der Clip wird nicht automatisch erneut ausgewählt.</p><button type="button" disabled={busy} onClick={() => { void act(() => api.cancel(job.id, twitchUserId), 'Auftrag entfernt.'); }}>Auftrag entfernen</button><button type="button" disabled={busy} onClick={() => setRemoveId(null)}>Behalten</button></div>}
            </li>;
          })}
        </ol>
      </div>
    </div>
  </section>;
}
