import React from 'react';
import type { UplinkDestination } from '../api/uplink';
import { twitchOutputFormular, TWITCH_OUTPUT_LABEL } from '../uplinkOutputMode';
import type { UplinkTwitchOutputMode } from '../uplinkOutputMode';

export function UplinkOutputMode({ ziel, entwurf, disabled, onChange }: {
  ziel: UplinkDestination | undefined;
  entwurf: UplinkTwitchOutputMode | null;
  disabled: boolean;
  onChange: (mode: UplinkTwitchOutputMode) => void;
}) {
  const id = React.useId();
  const modus = twitchOutputFormular(ziel, entwurf);
  return (
    <fieldset disabled={disabled} aria-describedby={`${id}-hinweis`} className="min-w-0 space-y-3 rounded-xl border border-border/60 bg-background/40 p-3">
      <legend className="px-1 text-sm font-semibold text-white">Twitch-Betriebsart</legend>
      <div className="grid gap-2 sm:grid-cols-2">
        {(['single', 'enhanced'] as const).map((mode) => (
          <label key={mode} className="flex min-h-11 cursor-pointer items-start gap-3 rounded-xl border border-border bg-background/70 p-3 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-primary">
            <input type="radio" name={`${id}-modus`} value={mode} checked={modus.auswahl === mode}
              onChange={() => onChange(mode)} aria-describedby={`${id}-${mode}-beschreibung`}
              className="mt-0.5 h-4 w-4 shrink-0 accent-primary" />
            <span className="min-w-0 space-y-1">
              <span className="block text-sm font-semibold text-white">{TWITCH_OUTPUT_LABEL[mode]}</span>
              <span id={`${id}-${mode}-beschreibung`} className="block text-xs text-text-secondary">
                {mode === 'single' ? 'Eine Qualitätsstufe, weniger Rechenaufwand.' : 'Mehrere Qualitätsstufen nach Twitch-Freigabe, mehr Rechenaufwand.'}
              </span>
            </span>
          </label>
        ))}
      </div>
      <p id={`${id}-hinweis`} className="text-xs text-text-secondary">Wähle die Betriebsart und speichere sie für den nächsten Stream.</p>
      <div role="status" aria-atomic="true" className="space-y-1 text-xs text-text-secondary">
        <p>Gespeichert: {modus.gespeichert ? TWITCH_OUTPUT_LABEL[modus.gespeichert] : 'noch nicht bestätigt'}.</p>
        <p>Laufende Ausgabe: {modus.aktiv === 'enhanced' && modus.stufen.length < 2
          ? 'Mehrere Qualitätsstufen noch nicht bestätigt'
          : modus.aktiv ? TWITCH_OUTPUT_LABEL[modus.aktiv] : 'noch nicht bestätigt'}.</p>
        {modus.fallback ? <p className="text-warning">{modus.fallback}</p> : null}
        {modus.geaendert ? <p className="text-primary">Betriebsart noch nicht gespeichert.</p> : null}
      </div>
      {modus.stufen.length > 0 ? (
        <div className="space-y-1 text-xs text-text-secondary">
          <p>Aktuell gesendete Qualitätsstufen:</p>
          <ul aria-label="Aktuell gesendete Qualitätsstufen" className="list-inside list-disc space-y-1">
            {modus.stufen.map((stufe, index) => <li key={`${index}-${stufe}`}>{stufe}</li>)}
          </ul>
        </div>
      ) : <p className="text-xs text-text-secondary">Aktuell gesendete Qualitätsstufen: noch nicht bestätigt.</p>}
    </fieldset>
  );
}
