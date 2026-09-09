import { useEffect, useRef, useState } from 'react';
import { ArrowRight, Pause, RefreshCw, X } from 'lucide-react';
import { useOnboarding } from './onboardingState';
import { canonicalBookmarkLocation, isConnectionStep, stepDefinition, stepState, type OnboardingStepId } from './steps';
import { LesezeichenHinweis } from './LesezeichenHinweis';

const descriptions: Record<Exclude<OnboardingStepId, 'bookmark'>, string[]> = {
  discord: ['Verknüpfe Discord mit deinem Twitch-Konto. Darüber ordnen wir anschließend deinen Steam-Account zu.', 'Die Verknüpfung verbindet deine Konten. Dem Community-Server beizutreten ist ein eigener Schritt.'],
  steam: ['Mit Steam können dein Deadlock-Rang und deine Spielstatistiken deinem Konto zugeordnet werden.', 'Discord muss zuerst verbunden sein. Nach der Steam-Anmeldung kannst du hier den Status erneut prüfen.'],
  chat: ['Hier schaltest du Rückgrüße, !lurk, !clip und weitere vorhandene Befehle an oder aus. Bei ausgeschalteter Begrüßung antwortet der Bot nicht auf Grüße; andere Funktionen bleiben aktiv.', 'Diese Schalter speichern sofort. Du musst zum Kennenlernen nichts ändern – auch „Aus“ kann die richtige Wahl für deinen Kanal sein.'],
  bot: ['Unter „Was der Bot übernimmt“ wählst du die vier Schutzaufgaben. Änderungen gelten erst nach „Speichern“. Weiter im Rundgang speichert keine Einstellungen.', 'Die stillen Ban- und Raid-Hinweise stehen weiter unten. Ein stiller Raid unterdrückt nur die Chatnachricht; der Raid selbst findet weiterhin statt.'],
  overlay: ['Im Overlay-Baukasten wählst du Theme, Darstellung, sichtbare Module und Deckkraft und siehst direkt eine Vorschau.', 'Der Baukasten erzeugt die passende OBS-Adresse. Wenn du eine neue Variante erstellst, übernimmst du deren Adresse in OBS. Dieser Schritt ist freiwillig.'],
  advertising: ['Hier liegt der experimentelle Werbemanager. Seine Verfügbarkeit und fehlende Twitch-Rechte stehen direkt im Bereich.', 'Du musst ihn nicht aktivieren. Im Rundgang wird keine Werbung ausgelöst. Änderungen am Werbeplan brauchen den Speicherknopf.'],
  feedback: ['Was fehlt dir? Was nervt? Sag uns, was unklar ist, dich stört oder dir noch fehlt.', 'Über „Kritik & Wünsche“ findest du dauerhaft deine Rückmeldungen, Antworten und den aktuellen Stand. Etwas einzureichen ist freiwillig.'],
};

export function OnboardingGuide({ home = false, tab, overlayBuilder = false }: {home?: boolean; tab?: string; overlayBuilder?: boolean}) {
  const {status, error, pending, loading, advance: saveAdvance, open, pause, refresh} = useOnboarding();
  const id = status?.active_step ?? 'bookmark';
  const step = stepDefinition(id);
  const active = Boolean(status && !status.paused);
  const matches = overlayBuilder ? id === 'overlay' : home ? id === 'bookmark' || id === 'feedback' : step.path === `/twitch/verwaltung#${tab}`;
  const [targetState, setTargetState] = useState<'loading' | 'ready' | 'missing'>('loading');
  const [dirty, setDirty] = useState(false);
  const [dirtyHint, setDirtyHint] = useState('');
  const panel = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!active || !matches) return;
    const esc = (event: KeyboardEvent) => { if (event.key === 'Escape' && !event.defaultPrevented && panel.current?.contains(event.target as Node) && !(event.target instanceof HTMLSelectElement)) { event.preventDefault(); void pause(); } };
    window.addEventListener('keydown', esc);
    return () => window.removeEventListener('keydown', esc);
  }, [active, matches, pause]);

  useEffect(() => {
    if (!active || !matches) return;
    let highlighted: HTMLElement | null = null;
    let timedOut = false;
    const inspect = () => {
      const target = document.querySelector<HTMLElement>(`[data-tour-id="${step.anchor}"]`);
      const ready = target?.dataset.tourReady === 'true';
      setTargetState(id === 'bookmark' || id === 'feedback' || ready ? 'ready' : timedOut ? 'missing' : 'loading');
      const unsaved = document.querySelector<HTMLElement>('[data-unsaved="true"]');
      setDirty(Boolean(unsaved));
      setDirtyHint(unsaved?.dataset.unsavedHint ?? 'Du hast ungespeicherte Änderungen. Speichere sie im Formular, bevor du weitergehst. Du kannst den Rundgang auch pausieren und hierbleiben.');
      if (ready && target && highlighted !== target) {
        highlighted?.removeAttribute('data-onboarding-highlight');
        target.setAttribute('data-onboarding-highlight', 'true');
        highlighted = target;
      }
    };
    inspect();
    const observer = new MutationObserver(inspect);
    observer.observe(document.body, {subtree: true, childList: true, attributes: true, attributeFilter: ['data-tour-ready', 'data-unsaved']});
    const timer = window.setTimeout(() => { timedOut = true; inspect(); }, 8000);
    panel.current?.focus({preventScroll: true});
    return () => { observer.disconnect(); clearTimeout(timer); highlighted?.removeAttribute('data-onboarding-highlight'); };
  }, [active, matches, id, step.anchor]);

  if (!active || !status) return null;
  if (!matches) return home ? <div className="mt-4 flex flex-wrap items-center gap-3 text-sm"><button type="button" onClick={() => void open(id)} disabled={pending} className="min-h-11 rounded-lg border border-primary/50 px-4 py-2 font-semibold text-primary">Rundgang fortsetzen: {step.short}</button><button type="button" onClick={() => void pause()} className="min-h-11 px-3 text-text-secondary">Später</button></div> : null;
  const connected = isConnectionStep(id) && stepState(id, status) === 'done';
  const canConfirm = targetState === 'ready' && (!isConnectionStep(id) || connected) && (id !== 'bookmark' || canonicalBookmarkLocation(window.location));
  const advance = async (confirm: boolean) => {
    if (dirty || document.querySelector('[data-unsaved="true"]')) return;
    await saveAdvance(id, confirm);
  };
  const showTarget = () => {
    const target = document.querySelector<HTMLElement>(`[data-tour-id="${step.anchor}"]`);
    target?.scrollIntoView({block:'center', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth'});
    target?.focus({preventScroll: true});
  };
  return <section ref={panel} tabIndex={-1} aria-labelledby="onboarding-station-title" className={`${home ? 'mt-5' : ''} rounded-xl border border-primary/55 bg-black/40 p-4 focus-visible:outline-2 focus-visible:outline-primary md:p-5`}>
    <div className="flex items-start justify-between gap-4"><div><p className="text-xs font-semibold uppercase tracking-wider text-primary">Dein Rundgang{'optional' in step ? ' · freiwillig' : ''}</p><h3 id="onboarding-station-title" className="mt-1 text-lg font-bold text-white">{step.title}</h3></div><button type="button" onClick={() => void pause()} aria-label="Rundgang pausieren" className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg border border-border text-text-secondary hover:text-white focus-visible:outline-2 focus-visible:outline-primary"><X className="h-5 w-5" /></button></div>
    <div className="mt-3 space-y-3">{id === 'bookmark' ? <LesezeichenHinweis /> : descriptions[id].map(text => <p key={text} className="max-w-3xl text-sm leading-relaxed text-text-secondary">{text}</p>)}</div>
    {isConnectionStep(id) && <p className="mt-3 text-sm font-semibold" role="status">{loading ? 'Verbindung wird geprüft …' : connected ? 'Verbindung bestätigt.' : stepState(id, status) === 'error' ? 'Die Verbindung ist gerade nicht prüfbar. Ein Fehler bedeutet nicht, dass sie fehlt.' : id === 'steam' && status.discord_status !== 'connected' ? 'Verbinde zuerst Discord.' : 'Noch nicht verbunden. Nutze den Knopf in der markierten Karte.'}</p>}
    {targetState === 'loading' && <p role="status" className="mt-3 text-sm text-text-secondary">Der Bereich wird geladen …</p>}
    {targetState === 'missing' && <p role="status" className="mt-3 text-sm text-warning">Dieser Bereich ist gerade nicht verfügbar. Du kannst ohne Häkchen weitergehen und ihn später über die Einrichtung öffnen.</p>}
    {dirty && <p role="status" className="mt-3 text-sm text-warning">{dirtyHint}</p>}
    {error && <p role="alert" className="mt-3 text-sm text-warning">{error} <button onClick={refresh} type="button" className="underline">Erneut versuchen</button></p>}
    <div className="mt-4 flex flex-wrap items-center gap-3">
      {canConfirm && <button type="button" disabled={pending || dirty || (isConnectionStep(id) && loading)} onClick={() => void advance(true)} className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-bold text-bg disabled:opacity-50">{id === 'bookmark' ? 'Habe ich gespeichert' : id === 'feedback' ? 'Rundgang abschließen' : connected ? 'Weiter' : 'Verstanden, weiter'}<ArrowRight className="h-4 w-4" aria-hidden /></button>}
      {id !== 'bookmark' && id !== 'feedback' && targetState === 'ready' && <button type="button" onClick={showTarget} className="min-h-11 rounded-lg border border-border px-3 py-2 text-sm text-white">{isConnectionStep(id) ? 'Verbindung zeigen' : 'Einstellung zeigen'}</button>}
      {isConnectionStep(id) && <button type="button" disabled={loading} onClick={refresh} className="inline-flex min-h-11 items-center gap-2 text-sm text-primary"><RefreshCw className="h-4 w-4" aria-hidden />Status prüfen</button>}
      {(!canConfirm || id === 'bookmark' || 'optional' in step) && <button type="button" disabled={pending || dirty} onClick={() => void advance(false)} className="min-h-11 px-2 text-sm text-text-secondary underline underline-offset-4 disabled:opacity-50">{isConnectionStep(id) ? 'Ohne Verbindung weiter' : 'Ohne Häkchen weiter'}</button>}
      <button type="button" onClick={() => void pause()} className="inline-flex min-h-11 items-center gap-2 px-2 text-sm text-text-secondary"><Pause className="h-4 w-4" aria-hidden />Später</button>
    </div>
  </section>;
}
