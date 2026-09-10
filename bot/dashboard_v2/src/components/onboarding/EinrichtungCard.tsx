import { ArrowRight, BookOpen, CheckCircle2, Circle, RotateCcw } from 'lucide-react';
import { ONBOARDING_STEPS, stepState } from './steps';
import { useOnboarding } from './onboardingState';
import { OnboardingGuide } from './OnboardingGuide';

export function EinrichtungCard({ help = false }: { help?: boolean }) {
  const {enabled, status, error, loading, pending, refresh, open} = useOnboarding();
  if (!enabled) return help ? <section className="panel-card rounded-2xl p-6"><h1 className="text-2xl font-bold text-white">Hilfe & Einrichtung</h1><p className="mt-3 text-text-secondary">Öffne deine persönliche Partner-Ansicht, um deine Konten zu verbinden und den Rundgang zu nutzen.</p><a className="mt-3 inline-block text-primary underline" href="/twitch/verwaltung">Zur Verwaltung</a></section> : null;
  // Abgeschlossener Rundgang bleibt geschlossen; Neustart nur über die Hilfe-Panel-Karte bzw. die Seitenleiste.
  if (!help && status?.completed && status.paused) return null;
  return <section id="einrichtung" className="panel-card rounded-2xl p-5 md:p-6">
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div><p className="mb-1 text-xs font-bold uppercase tracking-wider text-primary">Dein Einstieg</p><h2 className="text-xl font-bold text-white">Hier richtest du deinen Bot ein</h2><p className="mt-2 max-w-2xl text-sm leading-relaxed text-text-secondary">Speichere dieses Dashboard als Lesezeichen im Browser. So findest du es später mit einem Klick wieder. Verbinde danach deine Konten und entscheide selbst, was der Bot macht. Wir zeigen dir die Einstellungen direkt in der Verwaltung.</p></div>
      <BookOpen className="hidden h-7 w-7 shrink-0 text-primary sm:block" aria-hidden />
    </div>
    {error && <div role="alert" className="mt-4 rounded-lg border border-warning/40 p-3 text-sm text-warning">{error} <button type="button" onClick={refresh} className="ml-2 underline">Erneut versuchen</button></div>}
    {!status ? <p className="mt-4 text-sm text-text-secondary">{loading ? 'Einrichtung wird geladen …' : 'Dein Fortschritt ist gerade nicht verfügbar.'}</p> : <>
      {status.completed && status.completed_step_ids.includes('feedback') && <p data-onboarding-complete tabIndex={-1} role="status" className="mt-4 text-sm text-success">Du weißt jetzt, wo du deine Einstellungen findest. Offene Punkte kannst du jederzeit nachholen.</p>}
      <div className="mt-5 grid gap-2 sm:grid-cols-2">
        {ONBOARDING_STEPS.map(step => {
          const state = stepState(step.id, status);
          const done = state === 'done';
          return <button key={step.id} type="button" onClick={() => void open(step.id)} disabled={pending} className="flex min-h-14 items-center gap-3 rounded-xl border border-border bg-black/30 px-3.5 py-3 text-left transition-colors hover:border-primary/60 focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50">
            {done ? <CheckCircle2 className="h-5 w-5 shrink-0 text-success" aria-hidden /> : <Circle className="h-5 w-5 shrink-0 text-primary/70" aria-hidden />}
            <span className="flex-1"><span className="block text-sm font-semibold text-white">{step.short}</span><span className="text-xs text-text-secondary">{done ? (step.id === 'discord' || step.id === 'steam' ? 'Verbunden' : step.id === 'bookmark' ? 'Von dir bestätigt' : 'Angesehen') : state === 'error' ? 'Verbindung gerade nicht prüfbar' : 'optional' in step ? 'Optional' : 'Noch offen'}</span></span>
            <ArrowRight className="h-4 w-4 shrink-0 text-text-secondary" aria-hidden />
          </button>;
        })}
      </div>
      {status.paused && <div className="mt-5 flex flex-wrap items-center gap-4"><button type="button" disabled={pending} onClick={() => void open(status.active_step)} className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-bold text-bg disabled:opacity-50"><ArrowRight className="h-4 w-4" aria-hidden />{status.completed ? 'Offene Punkte ansehen' : status.completed_step_ids.length ? 'Rundgang fortsetzen' : 'Zeig mir das Dashboard'}</button>{!help && <a href="/twitch/hilfe" className="text-sm text-text-secondary underline underline-offset-4">Hilfe & Einrichtung</a>}</div>}
      {help && <button type="button" disabled={pending} onClick={() => void open('bookmark')} className="mt-4 inline-flex min-h-11 items-center gap-2 text-sm font-semibold text-primary"><RotateCcw className="h-4 w-4" aria-hidden />Tour erneut zeigen</button>}
    </>}
    <OnboardingGuide home />
  </section>;
}
