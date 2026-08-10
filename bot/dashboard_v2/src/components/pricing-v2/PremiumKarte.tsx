import { useState } from 'react';
import { WachstumsKurve } from './WachstumsKurve';
import { euroAusCents } from './preis';
import { getPlanCheckoutHref } from '../../preview/routes';

/**
 * Die Premium-Flaeche: eine Karte, kein Vergleichsraster.
 *
 * Inhalt sind die eigenen Zahlen des Streamers, unscharf, darunter der Preis.
 * Der Jahrespreis steht vorausgewaehlt, weil er der Anker ist. Keine
 * Haekchen-Liste im Sichtfeld — wer Details will, klappt sie auf.
 */

interface PremiumKarteProps {
  tage: number;
  punkte: number[];
  monatCents: number;
  jahrCents: number;
  leistungen: string[];
  steuerhinweis: string | null;
  /** Kurve scharf zeigen (laufender Trial oder Premium aktiv). */
  entsperrt?: boolean;
  ueberschrift?: string;
  einleitung?: string;
  knopfText?: string;
}

function Balken({ breite }: { breite: string }) {
  return (
    <span
      aria-hidden="true"
      className={`inline-block h-[0.7em] ${breite} translate-y-[1px] rounded-[3px] bg-white/25 blur-[3px]`}
    />
  );
}

export function PremiumKarte({
  tage,
  punkte,
  monatCents,
  jahrCents,
  leistungen,
  steuerhinweis,
  entsperrt = false,
  ueberschrift = 'Dein Wachstum, seit du dabei bist',
  einleitung = 'Deine Zahlen liegen bereit. Premium macht sie sichtbar.',
  knopfText = 'Freischalten',
}: PremiumKarteProps) {
  const [zyklus, setZyklus] = useState<1 | 12>(12);
  const hatKurve = punkte.length >= 2;

  return (
    <div className="rounded-2xl border border-border bg-card p-6 md:p-8">
      <h2 className="text-lg font-semibold text-white">{ueberschrift}</h2>

      <div className="mt-5">
        {hatKurve ? (
          <WachstumsKurve
            punkte={punkte}
            unscharf={!entsperrt}
            beschreibung={`Verlauf deiner Zuschauerzahlen ueber ${tage} Tage`}
          />
        ) : (
          <p className="text-sm text-white/40">
            Sobald dein erster Stream ausgewertet ist, steht deine Kurve hier.
          </p>
        )}
        <p className="mt-3 text-sm text-white/50">
          {tage} Tage
          {hatKurve && (
            <>
              {' · '}Ø <Balken breite="w-8" />
              {' · '}Peak <Balken breite="w-12" />
            </>
          )}
        </p>
      </div>

      <p className="mt-6 text-white/80">{einleitung}</p>

      <fieldset className="mt-6 space-y-2">
        <legend className="sr-only">Abrechnungszeitraum</legend>
        <label
          data-press="soft"
          className={`flex cursor-pointer items-center gap-3 rounded-xl border px-4 py-3 ${
            zyklus === 12 ? 'border-primary/50 bg-primary/10' : 'border-border bg-white/[0.02]'
          }`}
        >
          <input
            type="radio"
            name="zyklus"
            className="accent-primary"
            checked={zyklus === 12}
            onChange={() => setZyklus(12)}
          />
          <span className="text-white">{euroAusCents(jahrCents)} / Jahr</span>
          <span className="text-sm text-white/50">2 Monate gratis</span>
        </label>
        <label
          data-press="soft"
          className={`flex cursor-pointer items-center gap-3 rounded-xl border px-4 py-3 ${
            zyklus === 1 ? 'border-primary/50 bg-primary/10' : 'border-border bg-white/[0.02]'
          }`}
        >
          <input
            type="radio"
            name="zyklus"
            className="accent-primary"
            checked={zyklus === 1}
            onChange={() => setZyklus(1)}
          />
          <span className="text-white">{euroAusCents(monatCents)} / Monat</span>
        </label>
      </fieldset>

      <a
        href={getPlanCheckoutHref('premium', false, zyklus)}
        data-press
        className="mt-5 flex w-full items-center justify-center rounded-xl bg-primary px-4 py-3 font-semibold text-[#0D0806]"
      >
        {knopfText}
      </a>
      <p className="mt-2 text-center text-sm text-white/40">jederzeit kündbar</p>

      {leistungen.length > 0 && (
        <details className="mt-6 border-t border-white/5 pt-4">
          <summary className="cursor-pointer text-sm text-white/50 hover:text-white/70">
            Was drin ist
          </summary>
          <ul className="mt-3 space-y-1 text-sm text-white/60">
            {leistungen.map((leistung) => (
              <li key={leistung}>{leistung}</li>
            ))}
          </ul>
        </details>
      )}

      {steuerhinweis && <p className="mt-4 text-xs text-white/30">{steuerhinweis}</p>}
    </div>
  );
}
