import { globalTimezoneUrl } from '@/utils/socialMediaChannel';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { ArrowDown, ArrowUp, Calendar, Clock3, ListOrdered, Trash2 } from 'lucide-react';
import { StudioSectionHeading } from '../../StudioSectionHeading';
import { ApiHttpError } from '@/api/core';
import { useLanguage, type LanguageContextValue } from '@/context/LanguageContext';
import { SOCIAL_EN } from '@/i18n/en/social';
import { platformIconUrl } from '../../platformIcon';
import { scheduleQueueApi } from './api';
import { ReviewJobActions } from './ReviewJobActions';
import type { QueueJob, SchedulePatch, ScheduleQueueApi, ScheduleQueueSnapshot, ScheduleSettings } from './api';
import './schedule-queue.css';

export interface ScheduleQueuePanelProps {
  twitchUserId?: string;
  api?: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey?: number;
  modeSlot?: ReactNode;
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

function slotLabel(value: string | null, timezone: string, locale: string, t: LanguageContextValue['t']): string {
  if (!value) return t('Noch kein Termin');
  try {
    return new Intl.DateTimeFormat(locale, {
      timeZone: timezone, weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
    }).format(new Date(value));
  } catch {
    return t('Termin nicht verfügbar');
  }
}

function errorText(error: unknown): string {
  if (error instanceof ApiHttpError && error.status === 401) return 'Bitte melde dich erneut an.';
  if (error instanceof ApiHttpError && error.status === 403) return 'Du hast keinen Zugriff auf den Zeitplan dieses Kanals.';
  if (error instanceof ApiHttpError && error.message === 'unknown_streamer') return 'Dieser Twitch-Kanal wurde nicht gefunden. Wähle einen anderen Kanal.';
  if (error instanceof ApiHttpError && error.status === 400 && error.message.startsWith('Server-Fehler')) return 'Wähle einen Twitch-Kanal und prüfe deine Eingaben.';
  if (error instanceof ApiHttpError && error.status >= 500) return 'Der Zeitplan ist gerade nicht verfügbar. Bitte versuche es später erneut.';
  if (error instanceof TypeError) return 'Die Verbindung ist unterbrochen. Bitte versuche es erneut.';
  return error instanceof ApiHttpError ? Object.entries(SOCIAL_EN).find(([, translated]) => translated === error.message)?.[0] ?? error.message : 'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.';
}

export function ScheduleQueuePanel(props: ScheduleQueuePanelProps) {
  return <ScheduleQueueOwner key={props.twitchUserId ?? 'self'} {...props} />;
}

function ScheduleQueueOwner({ twitchUserId, api = scheduleQueueApi, onChanged, reloadKey = 0, modeSlot }: ScheduleQueuePanelProps) {
  const { t, locale } = useLanguage();
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
          setDraft(result.settings);
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

  useEffect(() => { if (reloadKey) refresh.current?.(); }, [reloadKey]);

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

  const queueCard = (content: ReactNode) => <section className="studio-card sq-queue-card" aria-labelledby={`${id}-queue-heading`}>
    <StudioSectionHeading id={`${id}-queue-heading`} title={t('Produktionswarteschlange & Veröffentlichungsmodus')} icon={ListOrdered} />
    {modeSlot}
    {content}
  </section>;

  if (!snapshot || !draft) return <div className="studio-plan-top">
    <section className="studio-card schedule-queue" aria-busy={!error} data-unsaved={unsaved} data-saving={saving ? 'true' : undefined}>
      <h2>{t('Auto-Pilot und Zeitplan')}</h2>
      {error ? <><p role="alert">{t(error)}</p><button onClick={() => refresh.current?.('Aktualisiert.')}>{t('Erneut laden')}</button></> : <p role="status">{t('Zeitplan wird geladen.')}</p>}
    </section>
    {queueCard(null)}
  </div>;

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

  return <div className="studio-plan-top">
    <section className="studio-card schedule-queue" aria-labelledby={`${id}-heading`} data-unsaved={unsaved} data-saving={saving ? 'true' : undefined}>
    <StudioSectionHeading id={`${id}-heading`} title={t('Auto-Pilot und Zeitplan')} icon={Calendar}
      description={t('Ein Video pro Termin, gemeinsam für deine ausgewählten Plattformen.')} />
    <div className="sq-status" role="status">
      <Clock3 size={18} aria-hidden="true" />
      <div><strong>{t('Nächster freier Termin')}: {slotLabel(snapshot.next_slot, snapshot.settings.timezone, locale, t)}</strong>
        <p>{t(snapshot.production_hold ? HOLDS[snapshot.production_hold] : 'Produktion nach Bedarf. Fertige Videos bleiben zunächst zur Prüfung.')}</p>
      </div>
    </div>
    {error && <p className="sq-error" role="alert">{t(error)}</p>}
    {notice && <p className="sq-notice" role="status">{t(notice)}</p>}
      <form className="sq-editor" onSubmit={event => { event.preventDefault(); void act(() => api.save(patch, twitchUserId), 'Zeitplan gespeichert.', true); }}>
        <h3>{t('Wöchentliche Termine')}</h3>
        <fieldset disabled={busy}>
          <legend className="sq-sr-only">{t('Auto-Pilot-Einstellungen')}</legend>
          <div className="sq-toggles">
            <label><input type="checkbox" role="switch" checked={draft.enabled} onChange={event => edit({ enabled: event.target.checked })} /><span>{t('Auto-Pilot einschalten')}</span></label>
            <label><input type="checkbox" role="switch" checked={draft.paused} onChange={event => edit({ paused: event.target.checked })} /><span>{t('Neue Produktionen pausieren')}</span></label>
          </div>
          <p className="sq-help">{t('Die KI-Einstellung gilt erst beim Produktionsstart. Sie ersetzt nicht deine Zustimmung zur externen KI-Verarbeitung.')}</p>
          <fieldset className="sq-platforms">
            <legend>{t('Zielplattformen')}</legend>
            <div className="sq-toggles">
              {PLATFORMS.map(platform => {
                const connected = snapshot.connected_platforms.includes(platform.id);
                const selected = draft.target_platforms.includes(platform.id);
                return <label key={platform.id}>
                  <input type="checkbox" checked={selected} disabled={!connected && !selected}
                    onChange={event => edit({ target_platforms: event.target.checked
                      ? [...draft.target_platforms, platform.id]
                      : draft.target_platforms.filter(value => value !== platform.id) })} />
                  <span className="sq-platform-label"><img src={platformIconUrl(platform.id)} width="16" height="16" alt="" aria-hidden="true" />{platform.label}{!connected && ` (${t('nicht verbunden')})`}</span>
                </label>;
              })}
            </div>
            <p className="sq-help">{t('Verbinde fehlende Plattformen unter Konten. Wähle hier, wohin die Videos veröffentlicht werden sollen.')}</p>
            {!draft.target_platforms.length && <p className="sq-help">{t('Noch keine Zielplattform ausgewählt. Ohne Zielplattform wird kein Video veröffentlicht.')}</p>}
          </fieldset>
          <div className="sq-fields">
            <a href={globalTimezoneUrl(twitchUserId)}>{t('Zeitzone in Verwaltung ändern')}</a>
            <label htmlFor={`${id}-lead`}>{t('Vorlauf in Stunden')}<input id={`${id}-lead`} type="number" min="0" max="720" required value={Number.isFinite(draft.lead_time_hours) ? draft.lead_time_hours : ''} onChange={event => edit({ lead_time_hours: event.target.valueAsNumber })} /></label>
          </div>
          <p className="sq-help">{t('Sommerzeit wird berücksichtigt. Auch bei kürzerem Vorlauf kann die Produktion starten. Verpasste Termine werden nicht gesammelt.')}</p>
          <div className="sq-slots">
            {draft.schedule_slots.map((slot, index) => <div className="sq-slot" key={index}>
              <label><span className="sq-sr-only">{t('Wochentag für Termin {number}', { number: index + 1 })}</span><select value={slot.weekday} onChange={event => edit({ schedule_slots: draft.schedule_slots.map((value, i) => i === index ? { ...value, weekday: Number(event.target.value) } : value) })}>{WEEKDAYS.map((day, i) => <option value={i + 1} key={day}>{t(day)}</option>)}</select></label>
              <label><span className="sq-sr-only">{t('Uhrzeit für Termin {number}', { number: index + 1 })}</span><input type="time" required value={slot.local_time} onChange={event => edit({ schedule_slots: draft.schedule_slots.map((value, i) => i === index ? { ...value, local_time: event.target.value } : value) })} /></label>
              <button type="button" className="sq-icon" aria-label={t('Termin {number} entfernen', { number: index + 1 })} onClick={() => edit({ schedule_slots: draft.schedule_slots.filter((_, i) => i !== index) })}><Trash2 size={16} /></button>
            </div>)}
          </div>
          {!draft.schedule_slots.length && <p className="sq-help">{t('Noch kein Termin. Lege fest, wann ein vorbereitetes Video erscheinen soll.')}</p>}
          <div className="sq-actions"><button type="button" disabled={draft.schedule_slots.length >= 28} onClick={() => edit({ schedule_slots: [...draft.schedule_slots, { weekday: 1, local_time: '18:00' }] })}>{t('Termin hinzufügen')}</button><button className="sq-primary" type="submit" disabled={!hasChanges}>{t(busy ? 'Wird gespeichert' : 'Änderungen speichern')}</button></div>
        </fieldset>
      </form>
    </section>
    {queueCard(<div className="schedule-queue sq-queue">
        <h3>{t('Produktionswarteschlange')}</h3>
        <p className="sq-help">{t('Ein Auftrag nach dem anderen. Laufende Aufträge lassen sich nicht entfernen oder umordnen.')}</p>
        {!visible.length && <div className="sq-empty"><strong>{t('Die Warteschlange ist leer.')}</strong><p>{t('Passende Clips werden erst ausgewählt, wenn ein freier Termin im Vorlauf liegt.')}</p></div>}
        <ol className="sq-jobs">
          {visible.map(job => {
            const movable = job.status === 'queued' && !job.started_at;
            const position = unstarted.findIndex(value => value.id === job.id);
            return <li key={job.id} className="sq-job">
              <div className="sq-job-head"><strong>{job.clip_title || t('Clip {number}', { number: job.clip_id })}</strong><span data-status={job.status} className={job.status === 'running' ? 'sq-running' : 'sq-badge'}>{t(STATUS[job.status])}</span></div>
              <p className="sq-help">{slotLabel(job.scheduled_at, snapshot.settings.timezone, locale, t)} · {t(job.mode ? (job.mode === 'ai' ? 'KI-Bearbeitung' : 'Statisches Layout') : job.source === 'manual' ? 'Manueller Auftrag' : 'Modus wird beim Start festgelegt')}</p>
              {job.review_reasons.map(reason => <p className="sq-help" key={reason}>{t(REASONS[reason] ?? 'Die Produktion benötigt eine Prüfung.')}</p>)}
              {job.review_explanation && <p className="sq-help">{job.review_explanation}</p>}
              {job.last_error && <p className="sq-error">{t(job.last_error)}</p>}
              {job.status === 'review' && <ReviewJobActions busy={busy} clipTitle={job.clip_title || t('Clip {number}', { number: job.clip_id })}
                onResolve={(action, note) => { void act(() => api.resolve(job.id, action, note, twitchUserId), action === 'process' ? 'Der Clip ist erneut zur Verarbeitung eingeplant.' : 'Clip übersprungen.'); }} />}
              {movable && <div className="sq-job-actions">
                <button type="button" disabled={busy || position === 0} onClick={() => { void act(() => api.next(job.id, twitchUserId), 'Der Clip ist als Nächstes eingeplant.'); }}>{t('Als Nächstes')}</button>
                <button type="button" className="sq-icon" aria-label={t('{title} nach oben', { title: job.clip_title ?? '' })} disabled={busy || position === 0} onClick={() => move(job.id, -1)}><ArrowUp size={16} /></button>
                <button type="button" className="sq-icon" aria-label={t('{title} nach unten', { title: job.clip_title ?? '' })} disabled={busy || position === unstarted.length - 1} onClick={() => move(job.id, 1)}><ArrowDown size={16} /></button>
                <button type="button" disabled={busy} onClick={() => setRemoveId(job.id)}>{t('Entfernen')}</button>
              </div>}
              {removeId === job.id && <div className="sq-confirm"><p>{t('Diesen Auftrag entfernen? Der Clip wird nicht automatisch erneut ausgewählt.')}</p><button type="button" disabled={busy} onClick={() => { void act(() => api.cancel(job.id, twitchUserId), 'Auftrag entfernt.'); }}>{t('Auftrag entfernen')}</button><button type="button" disabled={busy} onClick={() => setRemoveId(null)}>{t('Behalten')}</button></div>}
            </li>;
          })}
        </ol>
      </div>)}
  </div>;
}
