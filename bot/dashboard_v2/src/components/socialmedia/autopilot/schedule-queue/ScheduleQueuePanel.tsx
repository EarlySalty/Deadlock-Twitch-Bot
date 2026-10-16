import { globalTimezoneUrl } from '@/utils/socialMediaChannel';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { ArrowDown, ArrowUp, ListOrdered } from 'lucide-react';
import { StudioSectionHeading } from '../../StudioSectionHeading';
import { ApiHttpError } from '@/api/core';
import { useLanguage, type LanguageContextValue } from '@/context/LanguageContext';
import { SOCIAL_EN } from '@/i18n/en/social';
import { PipelinePlanControls } from '../../PipelinePlanControls';
import { planText } from '../../pipelinePlan';
import { scheduleQueueApi } from './api';
import { ReviewJobActions } from './ReviewJobActions';
import type { QueueJob, ScheduleQueueApi, ScheduleQueueSnapshot } from './api';
import './schedule-queue.css';

export interface ScheduleQueuePanelProps {
  twitchUserId?: string;
  api?: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey?: number;
  modeSlot?: ReactNode;
}

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

export function ScheduleQueuePanel({ twitchUserId, api = scheduleQueueApi, onChanged, reloadKey = 0, modeSlot }: ScheduleQueuePanelProps) {
  const text = planText(useLanguage().language);
  const [planSaves, setPlanSaves] = useState(0);
  const planChanged = () => { setPlanSaves(value => value + 1); onChanged?.(); };
  return <div className="pp-stack">
    <PipelinePlanControls key={twitchUserId ?? 'self'} twitchUserId={twitchUserId} api={api} onChanged={planChanged} reloadKey={reloadKey}
      timezoneLink={<a href={globalTimezoneUrl(twitchUserId)}>{text('Zeitzone in Verwaltung ändern')}</a>} />
    <QueueCard key={`queue-${twitchUserId ?? 'self'}`} twitchUserId={twitchUserId} api={api} onChanged={onChanged} reloadKey={reloadKey + planSaves} modeSlot={modeSlot} />
  </div>;
}

interface QueueCardProps {
  twitchUserId?: string;
  api: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey: number;
  modeSlot?: ReactNode;
}

function QueueCard({ twitchUserId, api, onChanged, reloadKey, modeSlot }: QueueCardProps) {
  const { t, locale } = useLanguage();
  const id = useId();
  const [snapshot, setSnapshot] = useState<ScheduleQueueSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [removeId, setRemoveId] = useState<number | null>(null);
  const [refreshes, setRefreshes] = useState(0);
  const mutating = useRef(false);
  const epoch = useRef(0);
  const changed = useRef(onChanged);
  useEffect(() => { changed.current = onChanged; }, [onChanged]);

  useEffect(() => {
    const controller = new AbortController();
    const load = async () => {
      if (mutating.current) return;
      const generation = ++epoch.current;
      try {
        const result = await api.load(twitchUserId, controller.signal);
        if (controller.signal.aborted || generation !== epoch.current) return;
        setSnapshot(result);
        setError('');
      } catch (failure) {
        if (!controller.signal.aborted && generation === epoch.current) setError(errorText(failure));
      }
    };
    void load();
    const timer = window.setInterval(() => { void load(); }, 15000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [api, twitchUserId, reloadKey, refreshes]);

  async function act(request: () => Promise<ScheduleQueueSnapshot>, message: string) {
    if (mutating.current) return;
    mutating.current = true;
    const generation = ++epoch.current;
    setBusy(true);
    setError('');
    try {
      const result = await request();
      changed.current?.();
      if (generation !== epoch.current) return;
      setSnapshot(result);
      setRemoveId(null);
      setNotice(message);
    } catch (failure) {
      setError(errorText(failure));
    } finally {
      mutating.current = false;
      setBusy(false);
    }
  }

  const unstarted = snapshot?.jobs.filter(job => job.status === 'queued' && !job.started_at) ?? [];
  const visible = snapshot?.jobs.filter(job => job.status !== 'completed' && job.status !== 'cancelled') ?? [];
  function move(jobId: number, offset: number) {
    const order = unstarted.map(job => job.id);
    const index = order.indexOf(jobId);
    const destination = index + offset;
    if (index < 0 || destination < 0 || destination >= order.length) return;
    [order[index], order[destination]] = [order[destination], order[index]];
    void act(() => api.reorder(order, twitchUserId), 'Reihenfolge gespeichert.');
  }

  return <section className="studio-card sq-queue-card" aria-labelledby={`${id}-queue-heading`}>
    <StudioSectionHeading id={`${id}-queue-heading`} title={t('Produktionswarteschlange & Veröffentlichungsmodus')} icon={ListOrdered} />
    {modeSlot}
    <div className="schedule-queue sq-queue">
      <h3>{t('Produktionswarteschlange')}</h3>
      <p className="sq-help">{t('Ein Auftrag nach dem anderen. Laufende Aufträge lassen sich nicht entfernen oder umordnen.')}</p>
      {error && <p className="sq-error" role="alert">{t(error)}</p>}
      {error && !snapshot && <button type="button" onClick={() => setRefreshes(value => value + 1)}>{t('Erneut laden')}</button>}
      {notice && <p className="sq-notice" role="status">{t(notice)}</p>}
      {!snapshot && !error && <p role="status">{t('Zeitplan wird geladen.')}</p>}
      {snapshot && !visible.length && <div className="sq-empty"><strong>{t('Die Warteschlange ist leer.')}</strong><p>{t('Passende Clips werden erst ausgewählt, wenn ein freier Termin im Vorlauf liegt.')}</p></div>}
      {snapshot && <ol className="sq-jobs">
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
      </ol>}
    </div>
  </section>;
}
