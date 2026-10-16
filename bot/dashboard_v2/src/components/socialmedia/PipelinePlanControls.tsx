import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { CalendarClock, Plus, X } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import { platformIconUrl } from './platformIcon';
import type { PolicyScope, ScheduleQueueApi, WeeklySlot } from './autopilot/schedule-queue/api';
import {
  DISTRIBUTIONS, HOLDS, MODES, PLAN_PLATFORMS, REVIEWS, SCOPES, SHARED_TEXT, TEXT_SCOPES, WEEKDAYS, addTime, failureText, isDirty, loadPlan, planDraft, planText,
  savePlan, switchScope, validateDraft, type PlanDraft, type PlanState, type PlanText, type ScopeSwitch, type TextDraft,
} from './pipelinePlan';

function slotLabel(value: string | undefined, timezone: string, locale: string, text: PlanText): string {
  if (!value) return text('Noch kein Termin');
  try {
    return new Intl.DateTimeFormat(locale, { timeZone: timezone, weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' }).format(new Date(value));
  } catch {
    return text('Termin nicht verfügbar');
  }
}

export interface PipelinePlanControlsProps {
  twitchUserId?: string;
  api: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey?: number;
  timezoneLink?: ReactNode;
}

export function PipelinePlanControls({ twitchUserId, api, onChanged, reloadKey = 0, timezoneLink }: PipelinePlanControlsProps) {
  const { language, locale } = useLanguage();
  const text = planText(language);
  const id = useId();
  const [base, setBase] = useState<PlanState | null>(null);
  const [draft, setDraft] = useState<PlanDraft | null>(null);
  const [loadError, setLoadError] = useState('');
  const [saveError, setSaveError] = useState('');
  const [notice, setNotice] = useState('');
  const [saving, setSaving] = useState(false);
  const [choices, setChoices] = useState<ScopeSwitch['choices']>();
  const dirtyRef = useRef(false);
  const savingRef = useRef(false);
  const changed = useRef(onChanged);
  useEffect(() => { changed.current = onChanged; }, [onChanged]);

  const dirty = base !== null && draft !== null && isDirty(base, draft);
  useEffect(() => { dirtyRef.current = dirty; }, [dirty]);

  const reload = useCallback(async (force: boolean, signal?: AbortSignal) => {
    if (savingRef.current || (dirtyRef.current && !force)) return;
    try {
      const state = await loadPlan(api, twitchUserId, signal);
      if (signal?.aborted || savingRef.current || (dirtyRef.current && !force)) return;
      setBase(state);
      setDraft(planDraft(state));
      setChoices(undefined);
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

  const scheduleScope = (scope: PolicyScope) => {
    const result = switchScope(draft, scope);
    if (result.choices) { setChoices(result.choices); return; }
    setChoices(undefined);
    edit(result.draft);
  };

  return <section className="studio-card schedule-queue pp-plan" aria-labelledby={`${id}-heading`} data-unsaved={dirty || saving ? 'true' : undefined} data-saving={saving ? 'true' : undefined}>
    <header className="pp-head">
      <div>
        <h2 id={`${id}-heading`}><CalendarClock size={18} aria-hidden="true" />{text('Planen')}</h2>
        <p className="sq-help">{text('Lege fest, wie deine Clips vorbereitet werden, wer sie freigibt und wann sie erscheinen.')}</p>
      </div>
      <NextSlots base={base} targets={draft.target_platforms} locale={locale} text={text} />
    </header>
    <form className="pp-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
      <fieldset className="pp-columns" disabled={saving}>
        <div className="pp-column">
          <ChoiceGroup legend={text('Was passiert mit deinen Clips?')} name={`${id}-mode`} options={MODES} value={draft.pipeline_mode} text={text} onChange={pipeline_mode => edit({ pipeline_mode })}
            hint={draft.pipeline_mode !== base.policy.pipeline_mode ? text('Änderungen gelten für neue Clips.') : undefined} />
          <ChoiceGroup legend={text('Vor Veröffentlichung prüfen')} name={`${id}-review`} options={REVIEWS} value={draft.review_policy} text={text} onChange={review_policy => edit({ review_policy })}
            hint={base.policy.review_policy !== 'off' && draft.review_policy !== base.policy.review_policy ? text('Clips, die schon auf deine Freigabe warten, bleiben dort, bis du entscheidest.') : undefined} />
          <Targets draft={draft} connected={base.snapshot.connected_platforms} text={text} onChange={target_platforms => edit({ target_platforms })} />
          <TextDefaultsEditor id={id} draft={draft} text={text} onChange={edit} />
          <fieldset className="pp-group">
            <legend>{text('Auto-Pilot')}</legend>
            <div className="sq-toggles">
              <label><input type="checkbox" role="switch" checked={draft.enabled} onChange={event => edit({ enabled: event.target.checked })} /><span>{text('Auto-Pilot einschalten')}</span></label>
              <label><input type="checkbox" role="switch" checked={draft.paused} onChange={event => edit({ paused: event.target.checked })} /><span>{text('Neue Produktionen pausieren')}</span></label>
            </div>
            <label className="pp-lead" htmlFor={`${id}-lead`}>{text('Vorlauf in Stunden')}<input id={`${id}-lead`} type="number" min="0" max="720" required value={Number.isFinite(draft.lead_time_hours) ? draft.lead_time_hours : ''} onChange={event => edit({ lead_time_hours: event.target.valueAsNumber })} /></label>
            {base.snapshot.production_hold && <p className="sq-help">{text(HOLDS[base.snapshot.production_hold])}</p>}
          </fieldset>
        </div>
        <div className="pp-column">
          <ChoiceGroup legend={text('Wann wird gepostet?')} name={`${id}-scope`} options={SCOPES} value={draft.schedule_scope} text={text} onChange={scheduleScope} compact />
          {choices && <SharedChoice choices={choices} text={text} onPick={slots => { setChoices(undefined); edit({ schedule_scope: 'shared', shared: slots }); }} onCancel={() => setChoices(undefined)} />}
          <p className="sq-help pp-timezone">{text('Zeitzone: {timezone}', { timezone: base.snapshot.settings.timezone })} · {timezoneLink}</p>
          {draft.schedule_scope === 'shared'
            ? <WeekEditor label={text('Gemeinsam')} slots={draft.shared} text={text} onChange={shared => edit({ shared })} />
            : draft.target_platforms.length
              ? draft.target_platforms.map(platform => <WeekEditor key={platform} platform={platform} label={PLAN_PLATFORMS.find(item => item.id === platform)?.label ?? platform} slots={draft.per_platform[platform] ?? []} text={text}
                onChange={slots => edit({ per_platform: { ...draft.per_platform, [platform]: slots } })} />)
              : <p className="sq-help">{text('Wähle zuerst eine Zielplattform aus.')}</p>}
          <p className="sq-help">{text('Sommerzeit wird berücksichtigt. Verpasste Termine werden nicht nachgeholt.')}</p>
          <ChoiceGroup legend={text('Verteilung der Clips')} name={`${id}-distribution`} options={DISTRIBUTIONS} value={draft.distribution_policy} text={text} onChange={distribution_policy => edit({ distribution_policy })} />
        </div>
      </fieldset>
      <footer className="pp-footer">
        <div className="pp-messages" aria-live="polite">
          {problems.map(problem => <p className="sq-error" key={problem}>{problem}</p>)}
          {saveError && <p className="sq-error" role="alert">{text(saveError)}</p>}
          {notice && !dirty && <p className="sq-notice" role="status">{text(notice)}</p>}
          {dirty && !saveError && <p className="pp-dirty">{text('Ungespeicherte Änderungen')}</p>}
        </div>
        <div className="sq-actions">
          {saveError && <button type="button" disabled={saving} onClick={() => { void reload(true); }}>{text('Gespeicherten Stand laden')}</button>}
          <button type="button" disabled={!dirty || saving} onClick={() => { setDraft(planDraft(base)); setChoices(undefined); setSaveError(''); }}>{text('Verwerfen')}</button>
          <button className="sq-primary" type="submit" disabled={!dirty || saving || problems.length > 0}>{text(saving ? 'Wird gespeichert' : 'Speichern')}</button>
        </div>
      </footer>
    </form>
  </section>;
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
}

function ChoiceGroup<T extends string>({ legend, name, options, value, text, onChange, hint, compact }: ChoiceGroupProps<T>) {
  return <fieldset className="pp-group">
    <legend>{legend}</legend>
    <div className={compact ? 'pp-choices pp-compact' : 'pp-choices'}>
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
    <legend>{text('Standardtexte')}</legend>
    <p className="sq-help">{text('Werden genutzt, wenn ein Clip keinen eigenen Titel, keine Beschreibung oder keine Hashtags hat.')}</p>
    <div className="pp-choices pp-compact">
      {TEXT_SCOPES.map(option => <label className="pp-choice" key={option.id}>
        <input type="radio" name={`${id}-texts`} value={option.id} checked={draft.metadata_scope === option.id} onChange={() => onChange({ metadata_scope: option.id })} />
        <span><strong>{text(option.title)}</strong><small>{text(option.text)}</small></span>
      </label>)}
    </div>
    {keys.map(key => <TextFields key={key} id={`${id}-text-${key}`} label={key === SHARED_TEXT ? text('Für alle Plattformen') : PLAN_PLATFORMS.find(item => item.id === key)?.label ?? key}
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

function Targets({ draft, connected, text, onChange }: { draft: PlanDraft; connected: string[]; text: PlanText; onChange: (targets: string[]) => void }) {
  return <fieldset className="pp-group">
    <legend>{text('Zielplattformen')}</legend>
    <div className="sq-toggles">
      {PLAN_PLATFORMS.map(platform => {
        const isConnected = connected.includes(platform.id);
        const selected = draft.target_platforms.includes(platform.id);
        return <label key={platform.id}>
          <input type="checkbox" role="switch" checked={selected} disabled={!isConnected && !selected}
            onChange={event => onChange(event.target.checked ? [...draft.target_platforms, platform.id] : draft.target_platforms.filter(value => value !== platform.id))} />
          <span className="sq-platform-label"><img src={platformIconUrl(platform.id)} width="16" height="16" alt="" aria-hidden="true" />{platform.label}{!isConnected && ` (${text('nicht verbunden')})`}</span>
        </label>;
      })}
    </div>
    {connected.length < PLAN_PLATFORMS.length && <p className="sq-help">{text('Verbinde fehlende Plattformen unter Konten.')}</p>}
    {!draft.target_platforms.length && <p className="sq-help">{text('Noch keine Zielplattform ausgewählt. Ohne Zielplattform wird nichts veröffentlicht.')}</p>}
  </fieldset>;
}

function WeekEditor({ label, platform, slots, text, onChange }: { label: string; platform?: string; slots: WeeklySlot[]; text: PlanText; onChange: (slots: WeeklySlot[]) => void }) {
  return <div className="pp-week" role="group" aria-label={label}>
    {platform && <h3 className="sq-platform-label"><img src={platformIconUrl(platform)} width="16" height="16" alt="" aria-hidden="true" />{label}</h3>}
    {WEEKDAYS.map((day, dayIndex) => {
      const weekday = dayIndex + 1;
      const dayName = text(day);
      return <div className="pp-day" key={day}>
        <span className="pp-day-name">{dayName}</span>
        <div className="pp-times">
          {slots.map((slot, index) => slot.weekday !== weekday ? null : <span className="pp-time" key={index}>
            <input type="time" required aria-label={text('Uhrzeit am {day}', { day: dayName })} value={slot.local_time}
              onChange={event => onChange(slots.map((value, i) => i === index ? { ...value, local_time: event.target.value } : value))} />
            <button type="button" className="sq-icon" aria-label={text('{day} {time} entfernen', { day: dayName, time: slot.local_time })} onClick={() => onChange(slots.filter((_, i) => i !== index))}><X size={14} aria-hidden="true" /></button>
          </span>)}
          {!slots.some(slot => slot.weekday === weekday) && <span className="pp-empty">{text('Keine Zeit')}</span>}
          <button type="button" className="pp-add" aria-label={text('{day}: Zeit hinzufügen', { day: dayName })} onClick={() => onChange(addTime(slots, weekday))}><Plus size={14} aria-hidden="true" /><span>{text('Zeit hinzufügen')}</span></button>
        </div>
      </div>;
    })}
  </div>;
}

function SharedChoice({ choices, text, onPick, onCancel }: { choices: NonNullable<ScopeSwitch['choices']>; text: PlanText; onPick: (slots: WeeklySlot[]) => void; onCancel: () => void }) {
  const label = (platforms: string[]) => platforms.map(platform => PLAN_PLATFORMS.find(item => item.id === platform)?.label ?? platform).join(', ');
  const summary = (slots: WeeklySlot[]) => slots.length ? slots.map(slot => `${text(WEEKDAYS[slot.weekday - 1]).slice(0, 2)} ${slot.local_time}`).join(' · ') : text('Keine Zeiten');
  return <div className="sq-confirm pp-shared-choice" role="group" aria-label={text('Welche Zeiten sollen für alle gelten?')}>
    <strong>{text('Welche Zeiten sollen für alle gelten?')}</strong>
    <p className="sq-help">{text('Die Plattformen haben unterschiedliche Zeiten. Wähle aus, welche du behalten willst.')}</p>
    {choices.map(choice => <button type="button" key={choice.platforms.join()} onClick={() => onPick(choice.slots)}>
      <span><strong>{text('Zeiten von {platforms}', { platforms: label(choice.platforms) })}</strong><small>{summary(choice.slots)}</small></span>
    </button>)}
    <button type="button" onClick={onCancel}>{text('Abbrechen')}</button>
  </div>;
}

function NextSlots({ base, targets, locale, text }: { base: PlanState; targets: string[]; locale: string; text: PlanText }) {
  const timezone = base.snapshot.settings.timezone;
  const next = base.snapshot.next_slots ?? {};
  const shown = targets.filter(platform => base.snapshot.settings.target_platforms.includes(platform));
  return <dl className="pp-next" aria-label={text('Nächster Termin')}>
    {shown.length ? shown.map(platform => <div key={platform}>
      <dt className="sq-platform-label"><img src={platformIconUrl(platform)} width="16" height="16" alt="" aria-hidden="true" />{PLAN_PLATFORMS.find(item => item.id === platform)?.label ?? platform}</dt>
      <dd>{slotLabel(next[platform], timezone, locale, text)}</dd>
    </div>) : <div><dt>{text('Nächster Termin')}</dt><dd>{slotLabel(base.snapshot.next_slot ?? undefined, timezone, locale, text)}</dd></div>}
  </dl>;
}
