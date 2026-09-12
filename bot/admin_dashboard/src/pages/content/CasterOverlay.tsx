import { useBlocker } from 'react-router';
import { useEffect, useRef, useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { fetchCasterOverlay, saveCasterOverlay, type CasterDocument, type CasterScene } from '@/api/client';

const OBS_URL = 'https://deutsche-deadlock-community.de/twitch/caster-overlay';
const OBS_ORIGIN = new URL(OBS_URL).origin;
const button = 'min-h-11 rounded-lg border border-white/20 px-3 py-2 text-sm hover:border-amber-400 focus-visible:outline-2 focus-visible:outline-amber-400 disabled:opacity-40';
const input = 'min-h-11 w-full rounded-lg border border-white/15 bg-black/50 px-3 text-white focus-visible:outline-2 focus-visible:outline-amber-400';

export default function CasterOverlayPage() {
  const [document, setDocument] = useState<CasterDocument | null>(null);
  const [saved, setSaved] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [handle, setHandle] = useState('');
  const [dragged, setDragged] = useState<string | null>(null);
  const iframe = useRef<HTMLIFrameElement>(null);
  const scene = document?.scene;
  const dirty = !!scene && JSON.stringify(scene) !== saved;
  const blocker = useBlocker(dirty);
  const slots = scene?.slots.map(id => scene.roster.find(person => person.id === id) ?? null) ?? [null, null];
  function preview() { iframe.current?.contentWindow?.postMessage({ type: 'ddc-caster-preview', slots: slots.map(person => person && ({ name: person.name, handle: person.handle })) }, OBS_ORIGIN); }
  useEffect(preview, [scene]);
  async function load() {
    setBusy(true); setError('');
    try { const result = await fetchCasterOverlay(); setDocument(result); setSaved(JSON.stringify(result.scene)); setStatus('Gespeicherte Szene geladen.'); }
    catch (cause) { setError(cause instanceof Error ? cause.message : 'Szene konnte nicht geladen werden.'); }
    finally { setBusy(false); }
  }
  useEffect(() => { void load(); }, []);
  useEffect(() => { if (!dirty) return; const guard = (event: BeforeUnloadEvent) => event.preventDefault(); window.addEventListener('beforeunload', guard); return () => window.removeEventListener('beforeunload', guard); }, [dirty]);
  function edit(change: (value: CasterScene) => CasterScene) { setDocument(current => current && ({ ...current, scene: change(current.scene) })); setStatus('Entwurf geändert – noch nicht in OBS.'); }
  function assign(index: number, id: string | null) { edit(value => ({ ...value, slots: value.slots.map((current, slot) => slot === index ? id : current) as CasterScene['slots'] })); }
  async function publish() {
    if (!document) return;
    setBusy(true); setError('');
    try { const result = await saveCasterOverlay(document); setDocument(result); setSaved(JSON.stringify(result.scene)); setStatus('Live übernommen. OBS aktualisiert sich innerhalb von zwei Sekunden.'); }
    catch (cause) { setError(cause instanceof Error ? cause.message : 'Speichern fehlgeschlagen. Deine Änderungen bleiben im Entwurf.'); }
    finally { setBusy(false); }
  }
  function addPerson() { const trimmed = name.trim(); if (!trimmed) return; edit(value => ({ ...value, roster: [...value.roster, { id: crypto.randomUUID(), name: trimmed, handle: handle.trim().replace(/^@+/, '') }] })); setName(''); setHandle(''); }
  return <div className="space-y-6 pb-8">
    {blocker.state === 'blocked' && <div role="alert" className="rounded-xl border border-amber-400/40 bg-zinc-950 p-4 text-white">Deine Änderungen sind noch nicht live. Seite wirklich verlassen?<div className="mt-3 flex gap-3"><button className={button} onClick={() => blocker.reset()}>Weiter bearbeiten</button><button className={button} onClick={() => blocker.proceed()}>Entwurf verwerfen</button></div></div>}
    <PageHeader title="Caster-Overlay" description="Namen auf ihre Plätze ziehen. Szene übernehmen. Dieselbe OBS-Adresse bleibt." />
    {error && <div role="alert" className="rounded-xl border border-red-400/50 bg-red-950/40 p-4 text-red-100">{error}<button className={`${button} ml-3`} disabled={busy} onClick={() => { if (!dirty || window.confirm('Ungespeicherte Änderungen verwerfen und die aktuelle Szene laden?')) void load(); }}>Neu laden</button></div>}
    {!scene ? <p role="status">{busy ? 'Caster-Szene wird geladen …' : 'Die Szene ist noch nicht verfügbar.'}</p> : <>
      <section className="panel-card space-y-4 rounded-2xl p-5">
        <div className="flex flex-wrap items-center justify-between gap-3"><div><h2 className="text-lg font-semibold text-white">DACH-Caster · zwei Kameras</h2><p className="text-sm text-white/60">1920 × 1080 · {dirty ? 'Vorschau deines Entwurfs' : 'Gespeicherte Szene'}</p></div><span className={`rounded-full px-3 py-1 text-sm ${dirty ? 'bg-amber-400/15 text-amber-200' : 'bg-emerald-400/15 text-emerald-200'}`}>{dirty ? 'Noch nicht live' : 'Gespeichert'}</span></div>
        <div className="flex max-h-28 flex-wrap gap-2 overflow-y-auto" aria-label="Caster zum Ziehen">{scene.roster.map(person => <span key={person.id} data-caster-drag={person.id} draggable={!busy} onDragStart={event => { event.dataTransfer.setData('text/plain', person.id); event.dataTransfer.effectAllowed = 'copy'; setDragged(person.id); }} onDragEnd={() => setDragged(null)} className="cursor-grab rounded-lg border border-amber-400/30 bg-black/40 px-3 py-2 text-sm text-amber-100" title={`${person.name} auf einen Kameraplatz ziehen`}>⠿ {person.name}</span>)}</div>
        <div className="relative aspect-video overflow-hidden rounded-xl border border-white/15" style={{ background: 'repeating-conic-gradient(#232326 0% 25%, #171719 0% 50%) 0 / 24px 24px' }}>
          <iframe ref={iframe} title="Vorschau der Caster-Szene" src={`${OBS_URL}?preview=1`} onLoad={preview} className="pointer-events-none absolute inset-0 h-full w-full border-0" />
          {[0, 1].map(index => <button key={index} aria-label={`${index === 0 ? 'Linker' : 'Rechter'} Kameraplatz${slots[index] ? `: ${slots[index]?.name}` : ': frei'}`} disabled={busy} onDragOver={event => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; }} onDrop={event => { event.preventDefault(); const id = event.dataTransfer.getData('text/plain'); if (scene.roster.some(person => person.id === id)) assign(index, id); setDragged(null); }} className={`absolute flex items-center justify-center border-2 border-dashed text-sm text-white/70 focus-visible:outline-2 focus-visible:outline-amber-400 ${dragged ? 'border-amber-300 bg-amber-400/10' : 'border-white/20'}`} style={{ left: `${(index === 0 ? 185 : 1043) / 19.2}%`, top: `${282 / 10.8}%`, width: `${694 / 19.2}%`, height: `${429 / 10.8}%` }} onClick={() => globalThis.document.getElementById(`caster-slot-${index}`)?.focus()}>{dragged ? 'Hier ablegen' : 'Kamera aus OBS'}</button>)}
        </div>
        <div className="grid gap-3 sm:grid-cols-[1fr_auto_1fr]">{[0, 1].map(index => <div key={index} className={index ? 'sm:col-start-3 sm:row-start-1' : ''}><label className="mb-1 block text-sm text-white/70" htmlFor={`caster-slot-${index}`}>{index === 0 ? 'Links' : 'Rechts'}</label><select id={`caster-slot-${index}`} disabled={busy} className={input} value={scene.slots[index] ?? ''} onChange={event => assign(index, event.target.value || null)}><option value="">Platz leeren</option>{scene.roster.map(person => <option key={person.id} value={person.id}>{person.name}</option>)}</select></div>)}<button disabled={busy} className={`${button} self-end sm:col-start-2 sm:row-start-1`} onClick={() => edit(value => ({ ...value, slots: [value.slots[1], value.slots[0]] }))}>⇄ Tauschen</button></div>
      </section>
      <section className="panel-card space-y-4 rounded-2xl p-5"><h2 className="text-lg font-semibold text-white">Caster-Liste</h2><p className="text-sm text-white/60">Personen wiederverwenden: am Griff auf einen Kameraplatz ziehen oder „Links“ / „Rechts“ wählen.</p>
        {scene.roster.length === 0 && <p className="text-white/60">Lege die ersten Caster an, zum Beispiel Leo, Nimo oder ZeRo.</p>}
        <div className="space-y-3">{scene.roster.map(person => <div key={person.id} className="grid items-center gap-2 rounded-xl border border-white/10 bg-black/30 p-3 sm:grid-cols-[auto_1fr_1fr_auto]">
          <span draggable={!busy} onDragStart={event => { event.dataTransfer.setData('text/plain', person.id); event.dataTransfer.effectAllowed = 'copy'; setDragged(person.id); }} onDragEnd={() => setDragged(null)} className="cursor-grab px-2 py-3 text-xl text-amber-300" title={`${person.name} auf einen Kameraplatz ziehen`} aria-hidden="true">⠿</span>
          <input aria-label={`Name von ${person.name}`} disabled={busy} maxLength={60} className={input} value={person.name} onChange={event => edit(value => ({ ...value, roster: value.roster.map(item => item.id === person.id ? { ...item, name: event.target.value } : item) }))} />
          <input aria-label={`Handle von ${person.name}`} disabled={busy} maxLength={60} placeholder="Handle (optional)" className={input} value={person.handle} onChange={event => edit(value => ({ ...value, roster: value.roster.map(item => item.id === person.id ? { ...item, handle: event.target.value.replace(/^@+/, '') } : item) }))} />
          <div className="flex flex-wrap gap-2"><button className={button} disabled={busy} onClick={() => assign(0, person.id)}>Links</button><button className={button} disabled={busy} onClick={() => assign(1, person.id)}>Rechts</button><button aria-label={`${person.name} entfernen`} className={button} disabled={busy} onClick={() => edit(value => ({ ...value, roster: value.roster.filter(item => item.id !== person.id), slots: value.slots.map(id => id === person.id ? null : id) as CasterScene['slots'] }))}>Entfernen</button></div>
        </div>)}</div>
        <form className="grid gap-3 border-t border-white/10 pt-4 sm:grid-cols-[1fr_1fr_auto]" onSubmit={event => { event.preventDefault(); addPerson(); }}><label className="text-sm text-white/70">Neuer Name<input className={`${input} mt-1`} maxLength={60} value={name} onChange={event => setName(event.target.value)} disabled={busy} placeholder="z. B. Nimo" required /></label><label className="text-sm text-white/70">Handle (optional)<input className={`${input} mt-1`} maxLength={60} value={handle} onChange={event => setHandle(event.target.value)} disabled={busy} placeholder="z. B. zro_dl" /></label><button className={`${button} self-end`} disabled={busy || !name.trim() || scene.roster.length >= 100}>Person hinzufügen</button></form>
      </section>
      <div className="sticky bottom-3 z-10 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-400/40 bg-zinc-950 p-4 shadow-xl"><p role="status" className="text-sm text-white/75">{status || 'Bereit. Änderungen erscheinen nach „Live übernehmen“ in OBS.'}</p><button className={`${button} border-amber-400 bg-amber-400/15 font-semibold text-amber-100`} disabled={busy || !dirty || scene.roster.some(person => !person.name.trim())} onClick={() => void publish()}>{busy ? 'Wird gespeichert …' : 'Live übernehmen'}</button></div>
      <section className="panel-card space-y-3 rounded-2xl p-5"><h2 className="text-lg font-semibold text-white">Einmal in OBS einrichten</h2><p className="text-sm text-white/70">Browser-Quelle mit 1920 × 1080 anlegen und über die beiden Kameraquellen legen. Die Kamerafenster sind transparent. Kameras darunter einmal passend ausrichten.</p><label className="block text-sm text-white/70" htmlFor="caster-obs-url">Feste Browser-Adresse</label><div className="flex flex-wrap gap-2"><input id="caster-obs-url" readOnly className={input} value={OBS_URL} onFocus={event => event.target.select()} /><button className={button} onClick={async () => { try { await navigator.clipboard.writeText(OBS_URL); setStatus('OBS-Adresse kopiert.'); } catch { setError('Kopieren nicht möglich. Bitte die Adresse markieren und kopieren.'); } }}>Kopieren</button></div><p className="text-sm text-white/55">Danach genügt „Live übernehmen“. Adresse und Kameraquellen bleiben gleich, auch nach einem Bot-Neustart. Die Namen dieser gemeinsamen Szene sind öffentlich sichtbar.</p><p className="text-xs text-white/45">Kamerapositionen: links X 185 / Y 282, rechts X 1043 / Y 282 · jeweils 694 × 429 Pixel.</p></section>
    </>}
  </div>;
}
