import { Lock, Sparkles } from 'lucide-react';
import { PREVIEW_PRICING_ROUTE } from '../../preview/routes';

/**
 * Gesperrter "Entwicklung & Coaching"-Block für Free-Nutzer.
 * Zeigt eine geblurrte Wachstumskurven-Andeutung + Lock-Overlay + Feature-Bullets + CTA.
 */
export function PaidTeaserBlock() {
  // Fake-Balken für die geblurrte Kurven-Andeutung — keine echten Daten nötig
  const fakeBarHeights = [38, 52, 45, 64, 58, 71, 66, 83, 77, 90];

  const features = [
    'Wachstumskurve über alle Streams',
    'Stammzuschauer über Zeit',
    'Retention-Trend',
    'Wochen- & Monatsvergleich',
    'Handlungsempfehlungen',
    'Post-Stream-Bericht',
  ];

  return (
    <div className="relative rounded-2xl border border-border overflow-hidden bg-background">
      {/* Geblurrter Platzhalter-Inhalt */}
      <div className="blur-md opacity-40 pointer-events-none select-none p-6">
        <p className="text-base font-semibold text-white mb-4">Entwicklung &amp; Coaching</p>

        {/* Fake-Wachstumskurve */}
        <div className="flex items-end gap-2 h-28 mb-4 rounded-xl border border-border bg-gradient-to-b from-primary/5 to-warning/5 px-4 py-3">
          {fakeBarHeights.map((h, i) => (
            <div
              key={i}
              className="flex-1 rounded-t-sm bg-gradient-to-t from-warning/60 to-warning/30"
              style={{ height: `${h}%` }}
            />
          ))}
        </div>

        {/* Fake-Kacheln */}
        <div className="grid grid-cols-3 gap-3">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-14 rounded-xl bg-card border border-border" />
          ))}
        </div>
      </div>

      {/* Lock-Overlay */}
      <div className="absolute inset-0 flex flex-col items-center justify-center text-center px-6 py-8 bg-bg/80 backdrop-blur-[2px]">
        {/* Lock-Icon */}
        <div className="w-12 h-12 rounded-full flex items-center justify-center border border-warning/40 bg-warning/10 mb-4">
          <Lock className="w-5 h-5 text-warning" />
        </div>

        <h3 className="text-lg font-bold text-white mb-2">Sieh, wohin du dich entwickelst</h3>

        {/* Feature-Bullets */}
        <ul className="flex flex-wrap justify-center gap-x-5 gap-y-1.5 mb-5 max-w-md">
          {features.map((f) => (
            <li key={f} className="flex items-center gap-1.5 text-xs text-text-secondary">
              <span className="w-1.5 h-1.5 rounded-full bg-primary flex-shrink-0" />
              {f}
            </li>
          ))}
        </ul>

        {/* CTA */}
        <a
          href={PREVIEW_PRICING_ROUTE}
          className="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-sm font-semibold bg-gradient-to-r from-warning to-orange text-bg hover:brightness-110 transition-[filter,box-shadow,transform,translate,scale] shadow-lg shadow-warning/20 mb-3"
        >
          <Sparkles className="w-4 h-4" />
          30 Tage gratis testen
        </a>

        {/* Beruhigungszeile */}
        <p className="text-[11px] text-text-secondary tracking-wide">
          Kein Abo-Zwang · jederzeit kündbar · dein Verlauf wird ab Tag 1 mitgezeichnet
        </p>
      </div>
    </div>
  );
}
