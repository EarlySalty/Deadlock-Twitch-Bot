import React, { useEffect, useId, useRef, useState } from 'react';
import { PLATFORM_NAMES, PLATFORMS, STATUS, formatDuration } from '../model.js';
import { Icon } from './Icon.jsx';
/**
 * clip uses the normalized view model from model.js (id, title, status, targets...).
 * Async callbacks must reject on failure and update the parent cache on success.
 * Unknown availability is fail-closed: supply availableTargets from loaded account data.
 */
export function ClipQueueCard({ clip, streamer, availableTargets = [], onApprove, onArchive, onStop, onEditLayout, onEditTranscript, }) {
    const menuRef = useRef(null);
    const summaryRef = useRef(null);
    const pendingRef = useRef(false);
    const mounted = useRef(true);
    const menuId = useId();
    const errorId = useId();
    const [pending, setPending] = useState(false);
    const [error, setError] = useState('');
    const [imageFailed, setImageFailed] = useState(false);
    const storedTargets = JSON.stringify(clip.targets || []);
    const [targets, setTargets] = useState(clip.targets || []);
    const status = STATUS[clip.status] || STATUS.new;
    const review = clip.status === 'review';
    const validTargets = targets.filter(platform => availableTargets.includes(platform));
    useEffect(() => { setTargets(JSON.parse(storedTargets)); }, [clip.id, storedTargets]);
    useEffect(() => { setImageFailed(false); }, [clip.thumbnailUrl]);
    useEffect(() => {
        mounted.current = true;
        function closeOutside(event) {
            if (menuRef.current && !menuRef.current.contains(event.target))
                menuRef.current.open = false;
        }
        function escape(event) {
            if (event.key === 'Escape' && menuRef.current?.open) {
                menuRef.current.open = false;
                summaryRef.current?.focus();
            }
        }
        document.addEventListener('pointerdown', closeOutside);
        document.addEventListener('keydown', escape);
        return () => {
            mounted.current = false;
            document.removeEventListener('pointerdown', closeOutside);
            document.removeEventListener('keydown', escape);
        };
    }, []);
    function closeMenu() {
        if (menuRef.current)
            menuRef.current.open = false;
        summaryRef.current?.focus();
    }
    function edit(callback) { closeMenu(); callback(clip); }
    async function run(action) {
        if (pendingRef.current)
            return;
        pendingRef.current = true;
        setPending(true);
        setError('');
        closeMenu();
        try {
            await action();
        }
        catch (cause) {
            if (mounted.current)
                setError(cause instanceof Error ? cause.message : 'Die Aktion konnte nicht gespeichert werden.');
        }
        finally {
            pendingRef.current = false;
            if (mounted.current)
                setPending(false);
        }
    }
    return (<article aria-label={clip.title} aria-busy={pending} className="queue-card card flex flex-col gap-4 p-3.5 md:flex-row md:items-center" data-status={clip.status}>
      <div className="thumb w-full shrink-0 md:w-[196px]">
        {clip.thumbnailUrl && !imageFailed ? <img src={clip.thumbnailUrl} alt="" loading="lazy" decoding="async" onError={() => setImageFailed(true)} className="h-full w-full object-cover"/> : <div className="grid h-full min-h-28 place-items-center text-text-secondary"><Icon name="film" className="h-8 w-8"/></div>}
        <span className="duration">{formatDuration(clip.duration)}</span>
      </div>
      <div className="min-w-0 flex-1 py-1">
        <span className={`badge badge-${status.tone}`}>{status.label}</span>
        <h3 className="mt-2 text-[15px] font-medium leading-6 text-text-primary">{clip.title}</h3>
        <p className="mt-1 text-[13px] text-text-secondary">{clip.source || 'Twitch'} / {streamer}{clip.views != null && ` · ${new Intl.NumberFormat('de-DE').format(clip.views)} Aufrufe`}</p>
        <p className="mt-3 text-xs text-text-secondary">{targets.length ? targets.map(platform => PLATFORM_NAMES[platform]).join(' · ') : 'Zielplattform im Menü wählen'}</p>
        {clip.scheduledLabel && clip.status === 'scheduled' && <p className="mt-2 flex items-center gap-2 text-xs text-text-secondary"><Icon name="clock" className="h-3 w-3"/>{clip.scheduledLabel}</p>}
        {clip.error && <p className="error-text mt-2">{clip.error}</p>}
        {error && <p id={errorId} role="alert" className="error-text mt-2">{error}</p>}
        {review && validTargets.length === 0 && <p className="mt-2 text-xs text-warning">Wähle eine verfügbare Zielplattform unter „Weitere Aktionen“.</p>}
      </div>
      <div className="clip-actions flex shrink-0 items-center gap-2">
        {review && <>
          <button type="button" className="btn btn-primary" disabled={pending || validTargets.length === 0} onClick={() => run(() => onApprove(clip.id, validTargets))} aria-describedby={error ? errorId : undefined}><Icon name="check"/>{pending ? 'Speichert…' : 'Freigeben'}</button>
          <button type="button" className="btn icon-btn" title="Ablehnen" aria-label={`Clip ablehnen: ${clip.title}`} disabled={pending} onClick={() => run(() => onArchive(clip.id))}><Icon name="x"/></button>
        </>}
        <details ref={menuRef} className="relative" onToggle={event => summaryRef.current?.setAttribute('aria-expanded', String(event.currentTarget.open))}>
          <summary ref={summaryRef} aria-label={`Weitere Aktionen für ${clip.title}`} aria-controls={menuId} aria-expanded="false" className="btn btn-ghost icon-btn list-none"><Icon name="more"/></summary>
          <div id={menuId} className="absolute right-0 z-30 mt-2 w-64 rounded-xl border border-white/10 bg-card p-2 shadow-2xl">
            {review && <fieldset className="mb-2 border-b border-border p-2 pb-3"><legend className="text-xs text-text-secondary">Zielplattformen</legend>{PLATFORMS.map(platform => <label key={platform} className="flex items-center gap-2.5 py-2 text-sm"><input type="checkbox" checked={targets.includes(platform)} disabled={pending || !availableTargets.includes(platform)} onChange={event => setTargets(current => event.target.checked ? [...new Set([...current, platform])] : current.filter(value => value !== platform))}/>{PLATFORM_NAMES[platform]}</label>)}</fieldset>}
            {[['crop', 'Layout anpassen', onEditLayout], ['text', 'Transkript bearbeiten', onEditTranscript]].map(([name, label, callback]) => callback && <button key={name} type="button" className="btn btn-ghost w-full justify-start" disabled={pending} onClick={() => edit(callback)}><Icon name={name}/>{label}</button>)}
            {clip.status === 'scheduled' && onStop && <button type="button" className="btn btn-ghost w-full justify-start text-warning" disabled={pending} onClick={() => run(() => onStop(clip.id))}><Icon name="clock"/>Geplanten Post stoppen</button>}
            {clip.status !== 'archived' && clip.status !== 'publishing' && <button type="button" className="btn btn-ghost w-full justify-start" disabled={pending} onClick={() => run(() => onArchive(clip.id))}><Icon name="archive"/>{review ? 'Ablehnen' : 'Archivieren'}</button>}
          </div>
        </details>
      </div>
    </article>);
}
