import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { CalendarClock, Plus, X } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import { platformIconUrl } from './platformIcon';
import type { PolicyScope, ScheduleQueueApi, WeeklySlot } from './autopilot/schedule-queue/api';
import {
  AI_MODES, HOLDS, PATHS, PLAN_PLATFORMS, REVIEWS, SCOPES, SHARED_TEXT, TAB_ORDER, TEXT_SCOPES, WEEKDAYS, addTime, failureText, isDirty, loadPlan, normalizeTime, planDraft, planText,
  savePlan, switchScope, validateDraft, type PlanDraft, type PlanState, type PlanText, type TextDraft,
} from './pipelinePlan';

function slotLabel(value: string | undefined, timezone: string, locale: string, text: PlanText): string {
  if (!value) return text('Noch kein Termin');
  const at = Date.parse(value);
  if (Number.isNaN(at)) return text('Termin nicht verfügbar');
  if (at <= Date.now()) return text('Noch kein Termin');
  try {
    return new Intl.DateTimeFormat(locale, { timeZone: timezone, weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(new Date(value));
  } catch {
    return text('Termin nicht verfügbar');
  }
}

function platformLabel(platform: string): string {
  return PLAN_PLATFORMS.find(item => item.id === platform)?.label ?? platform;
}

export interface PipelinePlanControlsProps {
  twitchUserId?: string;
  api: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey?: number;
  timezoneLink?: ReactNode;
  layoutSlot?: ReactNode;
  aiOptionsSlot?: ReactNode;
}

export function PipelinePlanControls({ twitchUserId, api, onChanged, reloadKey = 0, timezoneLink, layoutSlot, aiOptionsSlot }: PipelinePlanControlsProps) {
  const { language, locale } = useLanguage();
  const text = planText(language);
  const id = useId();
  const [base, setBase] = useState<PlanState | null>(null);
  const [draft, setDraft] = useState<PlanDraft | null>(null);
  const [loadError, setLoadError] = useState('');
  const [saveError, setSaveError] = useState('');
  const [notice, setNotice] = useState('');
  const [saving, setSaving] = useState(false);
  const [tab, setTab] = useState('');
  const dirtyRef = useRef(false);
  const savingRef = useRef(false);
  const changed = useRef(onChanged);
  useEffect(() => { changed.current = onChanged; }, [onChanged]);

  const dirty = base !== null && draft !== null && isDirty(base, draft);
  useEffect(() => { dirtyRef.current = dirty; }, [dirty]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(''), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const reload = useCallback(async (force: boolean, signal?: AbortSignal) => {
    if (savingRef.current || (dirtyRef.current && !force)) return;
    try {
      const state = await loadPlan(api, twitchUserId, signal);
      if (signal?.aborted || savingRef.current || (dirtyRef.current && !force)) return;
      setBase(state);
      setDraft(planDraft(state));
      setLoadError('');
      if (force) setSaveError('');
    } catch (failure) {
      if (!signal?.aborted) setLoadError(failureText(failure));
    }
  }, [api, twitchUserId]);

  useEffect(() => {
    const controller = new AbortController();
    void reload(false, controller.signal);
    const timer = window.setInterval(() => { void reload(false, controller.signal); }, 60000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [reload, reloadKey]);

  if (!base || !draft) {
    return <section className="studio-card schedule-queue pp-plan" aria-busy={!loadError}>
      <h2>{text('Planen')}</h2>
      {loadError ? <><p className="sq-error" role="alert">{text(loadError)}</p><button type="button" onClick={() => { void reload(true); }}>{text('Erneut laden')}</button></> : <p role="status">{text('Plan wird geladen.')}</p>}
    </section>;
  }

  const problems = validateDraft(draft, text);
  const edit = (change: Partial<PlanDraft>) => {
    setDraft(current => current ? { ...current, ...change } : current);
    setNotice('');
  };

  async function submit() {
    if (!base || !draft || savingRef.current || problems.length) return;
    savingRef.current = true;
    setSaving(true);
    setSaveError('');
    const outcome = await savePlan(api, twitchUserId, base, draft);
    savingRef.current = false;
    setSaving(false);
    setBase(outcome.state);
    if (outcome.failure) {
      setSaveError(outcome.partial ? 'Ein Teil ist gespeichert, der Rest nicht. Speichere erneut oder lade den gespeicherten Stand.' : failureText(outcome.failure));
      if (outcome.state !== base) changed.current?.();
      return;
    }
    setDraft(planDraft(outcome.state));
    setNotice('Gespeichert.');
    changed.current?.();
  }

  const scheduleScope = (scope: PolicyScope) => edit(switchScope(draft, scope));

  const aiPath = draft.pipeline_mode !== 'upload';
  const choosePath = (path: 'template' | 'ai') => {
    if (path === 'template') { edit({ pipeline_mode: 'upload' }); return; }
    if (!aiPath) edit({ pipeline_mode: base.policy.pipeline_mode !== 'upload' ? base.policy.pipeline_mode : 'upload_text' });
  };

  const tabs = TAB_ORDER.filter(platform => draft.target_platforms.includes(platform)).concat(draft.target_platforms.filter(platform => !TAB_ORDER.includes(platform)));
  const activeTab = tabs.includes(tab) ? tab : tabs[0];
  const barShown = dirty || saving || saveError !== '';

  return <section className="schedule-queue pp-plan" aria-labelledby={`${id}-heading`} data-unsaved={dirty || saving ? 'true' : undefined} data-saving={saving ? 'true' : undefined}>
    <header className="pp-head">
      <h2 id={`${id}-heading`}><CalendarClock size={18} aria-hidden="true" />{text('Planen')}</h2>
      <p className="sq-help">{text('Lege fest, wie deine Clips vorbereitet werden, wer sie freigibt und wann sie erscheinen.')}</p>
    </header>
    <form className="pp-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
      <fieldset className="pp-steps" disabled={saving}>
        <Step id={`${id}-step-1`} number={1} title={text('Postingplan')} question={text('Wann soll gepostet werden?')} text={text}
          actions={<ScopeSegment name={`${id}-scope`} value={draft.schedule_scope} text={text} onChange={scheduleScope} />}>
          <div className="pp-infoline">
            <span className="pp-info-item pp-timezone">{text('Zeitzone: {timezone}', { timezone: base.snapshot.settings.timezone })} · {timezoneLink}</span>
            <label className="pp-info-item pp-lead" htmlFor={`${id}-lead`}>
              <span>{text('Vorlauf')}</span>
              <input id={`${id}-lead`} type="number" min="0" max="720" required value={Number.isFinite(draft.lead_time_hours) ? draft.lead_time_hours : ''} onChange={event => edit({ lead_time_hours: event.target.valueAsNumber })} />
              <span>{text('h vor Termin')}</span>
            </label>
            <NextSlots base={base} targets={draft.target_platforms} locale={locale} text={text} />
          </div>
          {!draft.target_platforms.length && <p className="sq-help">{text('Noch keine Plattform für Auto-Posting aktiv. Aktiviere sie unter Einstellungen bei den Konten.')}</p>}
          {draft.schedule_scope === 'shared'
            ? <WeekEditor label={text('Gemeinsamer Plan (Standard)')} slots={draft.shared} text={text} onChange={shared => edit({ shared })} />
            : activeTab && <div className="pp-platform-plan">
              <div className="pp-tabs" role="tablist" aria-label={text('Plattform wählen')}>
                {tabs.map(platform => <button type="button" role="tab" key={platform} id={`${id}-tab-${platform}`} aria-selected={platform === activeTab} aria-controls={`${id}-panel`} onClick={() => setTab(platform)}>
                  <img src={platformIconUrl(platform)} width="16" height="16" alt="" aria-hidden="true" />{platformLabel(platform)}
                </button>)}
              </div>
              <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-tab-${activeTab}`}>
                <WeekEditor key={activeTab} label={text('Zeiten für {platform}', { platform: platformLabel(activeTab) })} slots={draft.per_platform[activeTab] ?? []} text={text}
                  onChange={slots => edit({ per_platform: { ...draft.per_platform, [activeTab]: slots } })} />
              </div>
            </div>}
          {draft.target_platforms.length > 1 && <div className="sq-toggles pp-distribution">
            <label><input type="checkbox" role="switch" checked={draft.distribution_policy === 'independent_clips'} onChange={event => edit({ distribution_policy: event.target.checked ? 'independent_clips' : 'same_clip' })} />{text('Unterschiedliche Clips je Plattform posten')}</label>
          </div>}
          <p className="sq-help">{text('Sommerzeit wird berücksichtigt. Verpasste Termine werden nicht nachgeholt.')}</p>
          {base.snapshot.production_hold && <p className="sq-help">{text(HOLDS[base.snapshot.production_hold])}</p>}
        </Step>
        <Step id={`${id}-step-2`} number={2} title={text('Video-Vorbereitung')} question={text('Wie sollen Clips geschnitten & betextet werden?')} text={text}>
          <ChoiceGroup legend={text('Video-Vorbereitung')} hideLegend name={`${id}-path`} options={PATHS} value={aiPath ? 'ai' : 'template'} text={text} onChange={choosePath} compact
            hint={draft.pipeline_mode !== base.policy.pipeline_mode ? text('Änderungen gelten für neue Clips.') : undefined} />
          {aiPath
            ? <div className="pp-path-body">
              <ChoiceGroup legend={text('Was soll die KI übernehmen?')} name={`${id}-mode`} options={AI_MODES} value={draft.pipeline_mode} text={text} onChange={pipeline_mode => edit({ pipeline_mode })} compact />
              {aiOptionsSlot}
            </div>
            : layoutSlot}
          <details className="pp-advanced">
            <summary>{text('Standardtexte & Hashtags (Fallback)')}</summary>
            <TextDefaultsEditor id={id} draft={draft} text={text} onChange={edit} />
          </details>
        </Step>
        <Step id={`${id}-step-3`} number={3} title={text('Kontrolle')} question={text('Prüfen oder automatisch posten?')} text={text}>
          <ChoiceGroup legend={text('Kontrolle')} hideLegend name={`${id}-review`} options={REVIEWS} value={draft.review_policy} text={text} onChange={review_policy => edit({ review_policy })} wide
            hint={base.policy.review_policy !== 'off' && draft.review_policy !== base.policy.review_policy ? text('Clips, die schon auf deine Freigabe warten, bleiben dort, bis du entscheidest.') : undefined} />
        </Step>
      </fieldset>
      {notice && !barShown && <p className="sq-notice pp-toast" role="status">{text(notice)}</p>}
      {barShown && <footer className="pp-footer">
        <div className="pp-messages" aria-live="polite">
          {problems.map(problem => <p className="sq-error" key={problem}>{problem}</p>)}
          {saveError && <p className="sq-error" role="alert">{text(saveError)}</p>}
          {dirty && !saveError && !problems.length && <p className="pp-dirty">{text('Ungespeicherte Änderungen')}</p>}
        </div>
        <div className="sq-actions">
          {saveError && <button type="button" disabled={saving} onClick={() => { void reload(true); }}>{text('Gespeicherten Stand laden')}</button>}
          <button type="button" disabled={!dirty || saving} onClick={() => { setDraft(planDraft(base)); setSaveError(''); }}>{text('Verwerfen')}</button>
          <button className="sq-primary" type="submit" disabled={!dirty || saving || problems.length > 0}>{text(saving ? 'Wird gespeichert' : 'Speichern')}</button>
        </div>
      </footer>}
    </form>
  </section>;
}

function Step({ id, number, title, question, text, actions, children }: { id: string; number: number; title: string; question: string; text: PlanText; actions?: ReactNode; children: ReactNode }) {
  return <section className="pp-step" aria-labelledby={id}>
    <header className="pp-step-head">
      <div className="pp-step-title">
        <span className="pp-step-number">{text('Schritt {number}', { number })}</span>
        <h3 id={id}>{title}</h3>
        <p className="sq-help">{question}</p>
      </div>
      {actions}
    </header>
    {children}
  </section>;
}

function ScopeSegment({ name, value, text, onChange }: { name: string; value: PolicyScope; text: PlanText; onChange: (value: PolicyScope) => void }) {
  return <fieldset className="pp-segment">
    <legend className="sq-sr-only">{text('Postingplan-Art')}</legend>
    {SCOPES.map(option => <label key={option.id}>
      <input type="radio" name={name} value={option.id} checked={value === option.id} onChange={() => onChange(option.id)} />
      <span>{text(option.title)}</span>
    </label>)}
  </fieldset>;
}

interface ChoiceGroupProps<T extends string> {
  legend: string;
  name: string;
  options: { id: T; title: string; text: string }[];
  value: T;
  text: PlanText;
  onChange: (value: T) => void;
  hint?: string;
  compact?: boolean;
  wide?: boolean;
  hideLegend?: boolean;
}

function ChoiceGroup<T extends string>({ legend, name, options, value, text, onChange, hint, compact, wide, hideLegend }: ChoiceGroupProps<T>) {
  return <fieldset className="pp-group">
    <legend className={hideLegend ? 'sq-sr-only' : undefined}>{legend}</legend>
    <div className={wide ? 'pp-choices pp-wide' : compact ? 'pp-choices pp-compact' : 'pp-choices'}>
      {options.map(option => <label className="pp-choice" key={option.id}>
        <input type="radio" name={name} value={option.id} checked={value === option.id} onChange={() => onChange(option.id)} />
        <span><strong>{text(option.title)}</strong><small>{text(option.text)}</small></span>
      </label>)}
    </div>
    {hint && <p className="sq-help">{hint}</p>}
  </fieldset>;
}

const EMPTY_TEXT: TextDraft = { title: '', description: '', hashtags: '' };

function TextDefaultsEditor({ id, draft, text, onChange }: { id: string; draft: PlanDraft; text: PlanText; onChange: (change: Partial<PlanDraft>) => void }) {
  const keys = draft.metadata_scope === 'shared' ? [SHARED_TEXT] : [SHARED_TEXT, ...draft.target_platforms];
  const update = (key: string, value: TextDraft) => onChange({ texts: { ...draft.texts, [key]: value } });
  return <fieldset className="pp-group pp-texts">
    <p className="sq-help">{text('Werden genutzt, wenn ein Clip keinen eigenen Titel, keine Beschreibung oder keine Hashtags hat.')}</p>
    <div className="pp-choices pp-compact">
      {TEXT_SCOPES.map(option => <label className="pp-choice" key={option.id}>
        <input type="radio" name={`${id}-texts`} value={option.id} checked={draft.metadata_scope === option.id} onChange={() => onChange({ metadata_scope: option.id })} />
        <span><strong>{text(option.title)}</strong><small>{text(option.text)}</small></span>
      </label>)}
    </div>
    {keys.map(key => <TextFields key={key} id={`${id}-text-${key}`} label={key === SHARED_TEXT ? text('Für alle Plattformen') : platformLabel(key)}
      platform={key === SHARED_TEXT ? undefined : key} value={draft.texts[key] ?? EMPTY_TEXT} text={text} onChange={value => update(key, value)} />)}
  </fieldset>;
}

function TextFields({ id, label, platform, value, text, onChange }: { id: string; label: string; platform?: string; value: TextDraft; text: PlanText; onChange: (value: TextDraft) => void }) {
  return <div className="pp-text" role="group" aria-label={label}>
    <h3 className="sq-platform-label">{platform && <img src={platformIconUrl(platform)} width="16" height="16" alt="" aria-hidden="true" />}{label}</h3>
    <label htmlFor={`${id}-title`}>{text('Titel')}<input id={`${id}-title`} type="text" maxLength={100} value={value.title} onChange={event => onChange({ ...value, title: event.target.value })} /></label>
    <label htmlFor={`${id}-description`}>{text('Beschreibung')}<textarea id={`${id}-description`} rows={3} maxLength={2200} value={value.description} onChange={event => onChange({ ...value, description: event.target.value })} /></label>
    <label htmlFor={`${id}-hashtags`}>{text('Hashtags')}<input id={`${id}-hashtags`} type="text" value={value.hashtags} placeholder="#deadlock #clips" onChange={event => onChange({ ...value, hashtags: event.target.value })} /></label>
    <small className="sq-help">{text('Mit Leerzeichen trennen, zum Beispiel #deadlock #clips')}</small>
  </div>;
}

function WeekEditor({ label, slots, text, onChange }: { label: string; slots: WeeklySlot[]; text: PlanText; onChange: (slots: WeeklySlot[]) => void }) {
  const setTime = (index: number, local_time: string) => onChange(slots.map((value, i) => i === index ? { ...value, local_time } : value));
  return <div className="pp-week" role="group" aria-label={label}>
    {WEEKDAYS.map((day, dayIndex) => {
      const weekday = dayIndex + 1;
      const dayName = text(day);
      return <div className="pp-day" key={day}>
        <span className="pp-day-name">{dayName}</span>
        <div className="pp-times">
          {slots.map((slot, index) => slot.weekday !== weekday ? null : <span className="pp-time" key={index}>
            <input type="text" inputMode="numeric" required maxLength={5} placeholder="18:00" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" aria-label={text('Uhrzeit am {day}', { day: dayName })} value={slot.local_time}
              onChange={event => setTime(index, event.target.value)} onBlur={event => setTime(index, normalizeTime(event.target.value))} />
            <button type="button" className="sq-icon" aria-label={text('{day} {time} entfernen', { day: dayName, time: slot.local_time })} onClick={() => onChange(slots.filter((_, i) => i !== index))}><X size={14} aria-hidden="true" /></button>
          </span>)}
          {!slots.some(slot => slot.weekday === weekday) && <span className="pp-empty">{text('Keine Zeit')}</span>}
          <button type="button" className="pp-add" aria-label={text('{day}: Zeit hinzufügen', { day: dayName })} onClick={() => onChange(addTime(slots, weekday))}><Plus size={14} aria-hidden="true" /><span>{text('Zeit hinzufügen')}</span></button>
        </div>
      </div>;
    })}
  </div>;
}

function NextSlots({ base, targets, locale, text }: { base: PlanState; targets: string[]; locale: string; text: PlanText }) {
  const timezone = base.snapshot.settings.timezone;
  const next = base.snapshot.next_slots ?? {};
  const shown = targets.filter(platform => base.snapshot.settings.target_platforms.includes(platform));
  const titleId = useId();
  return <div className="pp-info-item pp-next-block">
    <span id={titleId} className="pp-next-title">{text('Nächster Termin')}</span>
    <dl className="pp-next" aria-labelledby={titleId}>
      {shown.length ? shown.map(platform => <div key={platform}>
        <dt><img src={platformIconUrl(platform)} width="14" height="14" alt={platformLabel(platform)} /></dt>
        <dd>{slotLabel(next[platform], timezone, locale, text)}</dd>
      </div>) : <div><dt>{text('Alle Plattformen')}</dt><dd>{slotLabel(base.snapshot.next_slot ?? undefined, timezone, locale, text)}</dd></div>}
    </dl>
  </div>;
}
