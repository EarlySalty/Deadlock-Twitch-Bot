import React, { useEffect, useId, useRef, useState } from 'react';
import { PLATFORM_NAMES, validateSchedule, normalizeSchedule } from '../model.js';
import { Icon } from './Icon.jsx';
const clone = value => structuredClone(value);
const MODES = [
    ['manual', 'Mit Freigabe', 'Jeder Clip wartet auf dein Okay.'],
    ['veto_window', 'Mit Einspruch', 'Bis zum Termin kannst du den Post stoppen.'],
    ['full_auto', 'Vollautomatisch', 'Veröffentlicht ohne manuelle Sichtung.'],
];
function Toggle({ checked, label, disabled, onChange }) {
    return <button type="button" className="switch" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)}/>;
}
/**
 * value: loaded PostingPlan, never a made-up fallback after a failed GET.
 * onSave(nextPlan): Promise<PostingPlan>. Return the canonical server state.
 * Mount with key={streamer}; this prevents channel-to-channel draft leakage.
 */
export function AutoPilotSchedule({ value, onSave, loadError = null, onReload }) {
    const [draft, setDraft] = useState(value ? clone(value) : null);
    const [baseline, setBaseline] = useState(value ? clone(value) : null);
    const [pending, setPending] = useState(false);
    const [errors, setErrors] = useState({});
    const [message, setMessage] = useState('');
    const [saveError, setSaveError] = useState('');
    const pendingRef = useRef(false);
    const mounted = useRef(true);
    const formRef = useRef(null);
    const id = useId();
    const dirty = JSON.stringify(draft) !== JSON.stringify(baseline);
    const dirtyRef = useRef(dirty);
    dirtyRef.current = dirty;
    useEffect(() => {
        // Background refetches must not overwrite unfinished edits.
        if (value && !dirtyRef.current && !pendingRef.current) {
            setDraft(clone(value));
            setBaseline(clone(value));
        }
    }, [value]);
    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; };
    }, []);
    useEffect(() => {
        function warn(event) { if (dirty) {
            event.preventDefault();
            event.returnValue = '';
        } }
        window.addEventListener('beforeunload', warn);
        return () => window.removeEventListener('beforeunload', warn);
    }, [dirty]);
    if (loadError)
        return <div className="card p-6" role="alert"><h2 className="text-base font-medium">Zeitplan nicht verfügbar</h2><p className="error-text mt-2">Der gespeicherte Stand ist unbekannt. Es werden keine Standardwerte übernommen.</p>{onReload && <button type="button" className="btn mt-4" onClick={onReload}>Erneut laden</button>}</div>;
    if (!draft)
        return <div className="card p-6" role="status" aria-live="polite">Zeitplan wird geladen…</div>;
    function change(patch) { setDraft(current => ({ ...current, ...patch })); setMessage(''); }
    function changePlatform(platform, patch) {
        setDraft(current => ({ ...current, platforms: current.platforms.map(item => item.platform === platform ? { ...item, ...patch } : item) }));
        setMessage('');
    }
    function errorFor(key) { return errors[key] ? <span id={`${id}-${key}`} className="error-text mt-1 block">{errors[key]}</span> : null; }
    function fieldProps(key) { return { 'aria-invalid': Boolean(errors[key]), 'aria-describedby': errors[key] ? `${id}-${key}` : undefined }; }
    async function submit(event) {
        event.preventDefault();
        if (pendingRef.current || !dirty)
            return;
        const validation = validateSchedule(draft);
        setErrors(validation);
        setSaveError('');
        if (Object.keys(validation).length) {
            requestAnimationFrame(() => formRef.current?.querySelector('[aria-invalid="true"]')?.focus());
            return;
        }
        pendingRef.current = true;
        setPending(true);
        try {
            const result = await onSave(normalizeSchedule(clone(draft)));
            if (!result?.platforms)
                throw new Error('Die Serverantwort enthält keinen vollständigen Zeitplan. Bitte den gespeicherten Stand neu laden.');
            if (!mounted.current)
                return;
            setBaseline(clone(result));
            setDraft(clone(result));
            setMessage('Zeitplan gespeichert.');
        }
        catch (cause) {
            if (mounted.current)
                setSaveError(cause instanceof Error ? cause.message : 'Speichern fehlgeschlagen. Deine Eingaben bleiben erhalten.');
        }
        finally {
            pendingRef.current = false;
            if (mounted.current)
                setPending(false);
        }
    }
    const zones = [...new Set([draft.timezone, 'Europe/Berlin', 'Europe/London', 'America/New_York', 'UTC'])];
    return (<form ref={formRef} onSubmit={submit} noValidate aria-busy={pending}>
      <fieldset disabled={pending} className="space-y-5 border-0 p-0">
        <section className="card p-5 sm:p-6">
          <h2 className="text-base font-medium">Freigabe & Automatisierung</h2>
          <p className="hint mt-1">Wie viel Kontrolle möchtest du vor jedem Post?</p>
          <div className="mt-5 grid gap-3 sm:grid-cols-3">
            {MODES.filter(([mode]) => !draft.approval_modes || draft.approval_modes.includes(mode)).map(([mode, title, description]) => (<label key={mode} className={`cursor-pointer rounded-lg border p-3.5 ${draft.approval_mode === mode ? 'border-primary/50 bg-primary/[0.06]' : 'border-border'}`}>
                <span className="flex items-center gap-2 text-sm font-medium"><input type="radio" name={`${id}-mode`} value={mode} checked={draft.approval_mode === mode} onChange={() => change({ approval_mode: mode })} className="accent-primary"/>{title}</span>
                <span className="mt-2 block text-xs leading-5 text-text-secondary">{description}</span>
              </label>))}
          </div>
          {draft.approval_mode === 'full_auto' && <p className="mt-4 rounded-lg border border-warning/20 bg-warning/[0.06] p-3 text-sm text-warning">Clips werden ohne Freigabe veröffentlicht. Die Änderung gilt erst nach dem Speichern.</p>}
        </section>
        <section className="card">
          <div className="border-b border-border p-5 sm:p-6">
            <h2 className="text-base font-medium">Plattformen & Posting-Zeiten</h2>
            <label className="mt-5 flex flex-wrap items-center gap-3 text-sm text-text-secondary">Zeitzone<select className="field !w-auto" value={draft.timezone} onChange={event => change({ timezone: event.target.value })} {...fieldProps('timezone')}>{zones.map(zone => <option key={zone}>{zone}</option>)}</select></label>
            {errorFor('timezone')}
            <p className="hint mt-3">Uhrzeiten gelten in der gewählten Zeitzone, nicht in der Zeitzone des Browsers.</p>
          </div>
          <div className="divide-y divide-white/[0.08]">
            {draft.platforms.map(platform => (<fieldset key={platform.platform} className="p-5 sm:p-6">
                <legend className="sr-only">{PLATFORM_NAMES[platform.platform]}</legend>
                <div className="mb-5 flex items-center justify-between gap-4">
                  <div><h3 className="text-sm font-medium">{PLATFORM_NAMES[platform.platform]}</h3><p className="mt-1 text-xs text-text-secondary">{platform.auto_post ? 'Posting-Zeitplan aktiv' : 'Pausiert. Werte bleiben erhalten.'}</p></div>
                  <Toggle checked={platform.auto_post} label={`${PLATFORM_NAMES[platform.platform]} automatisch posten`} onChange={auto_post => changePlatform(platform.platform, { auto_post })}/>
                </div>
                <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
                  <label className="text-sm text-text-secondary">Posts pro Woche<input className="field mt-2" type="number" min="0" max="70" step="1" inputMode="numeric" value={platform.posts_per_week} onChange={event => changePlatform(platform.platform, { posts_per_week: event.target.value })} {...fieldProps(`${platform.platform}.week`)}/>{errorFor(`${platform.platform}.week`)}</label>
                  <label className="text-sm text-text-secondary">Höchstens pro Tag<input className="field mt-2" type="number" min="0" max="10" step="1" inputMode="numeric" value={platform.max_posts_per_day} onChange={event => changePlatform(platform.platform, { max_posts_per_day: event.target.value })} {...fieldProps(`${platform.platform}.day`)}/>{errorFor(`${platform.platform}.day`)}</label>
                  <div className="col-span-2 sm:col-span-1">
                    <span className="text-sm text-text-secondary">Uhrzeiten</span>
                    <div className="mt-2 flex flex-wrap items-center gap-2">
                      {platform.post_times.map((time, index) => <div key={index} className="flex items-center gap-1"><input type="time" className="field !w-[128px]" aria-label={`${PLATFORM_NAMES[platform.platform]} Uhrzeit ${index + 1}`} value={time} onChange={event => changePlatform(platform.platform, { post_times: platform.post_times.map((current, i) => i === index ? event.target.value : current) })} {...fieldProps(`${platform.platform}.times`)}/>{platform.post_times.length > 1 && <button type="button" className="btn btn-ghost icon-btn !w-7 !p-1" aria-label={`Uhrzeit ${index + 1} entfernen`} onClick={() => changePlatform(platform.platform, { post_times: platform.post_times.filter((_, i) => i !== index) })}><Icon name="x" className="h-3 w-3"/></button>}</div>)}
                      <button type="button" className="btn btn-ghost icon-btn" disabled={platform.post_times.length >= 12} aria-label={`Uhrzeit für ${PLATFORM_NAMES[platform.platform]} hinzufügen`} onClick={() => { const time = Array.from({ length: 24 }, (_, index) => `${String(index).padStart(2, '0')}:00`).find(time => !platform.post_times.includes(time)); changePlatform(platform.platform, { post_times: [...platform.post_times, time] }); }}><Icon name="plus"/></button>
                    </div>
                    {errorFor(`${platform.platform}.times`)}
                  </div>
                </div>
              </fieldset>))}
          </div>
        </section>
        <section className="card space-y-5 p-5 sm:p-6">
          <h2 className="text-base font-medium">Filter & Aufbereitung</h2>
          {draft.categories.map(category => <div key={category.category_key} className="flex items-center justify-between gap-5"><div><p className="text-sm">{category.display_name}</p><p className="hint mt-1">{category.enrichment_enabled ? 'Mit spielbezogener Aufbereitung.' : 'Nur posten, ohne spielbezogene KI-Aufbereitung.'}</p></div><Toggle checked={category.auto_post} label={`${category.display_name} automatisch posten`} onChange={auto_post => change({ categories: draft.categories.map(item => item.category_key === category.category_key ? { ...item, auto_post } : item) })}/></div>)}
          <div className="flex items-center justify-between gap-5 border-t border-border pt-5"><div><p className="text-sm">Untertitel</p><p className="hint mt-1">Vorbereitete Untertitel in die Clips übernehmen.</p></div><Toggle checked={draft.subtitles_enabled} label="Untertitel aktivieren" onChange={subtitles_enabled => change({ subtitles_enabled })}/></div>
        </section>
        <div className="save-bar flex flex-wrap items-center justify-between gap-3">
          <p className={saveError ? 'error-text' : 'text-sm text-text-secondary'} role={saveError ? 'alert' : 'status'}>{saveError || (pending ? 'Wird gespeichert…' : dirty ? 'Ungespeicherte Änderungen' : message || 'Keine ungespeicherten Änderungen')}</p>
          <div className="flex gap-2"><button type="button" className="btn btn-ghost" disabled={!dirty || pending} onClick={() => { setDraft(clone(baseline)); setErrors({}); setSaveError(''); }}>Verwerfen</button><button type="submit" className="btn btn-primary" disabled={!dirty || pending}><Icon name="check"/>{pending ? 'Speichert…' : 'Änderungen speichern'}</button></div>
        </div>
      </fieldset>
    </form>);
}
