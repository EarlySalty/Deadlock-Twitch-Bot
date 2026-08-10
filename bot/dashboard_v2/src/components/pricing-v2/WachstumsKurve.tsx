/**
 * Die eigene Wachstumskurve als Flaeche.
 *
 * Die Werte kommen normiert (0..1) aus `/premium-teaser`; absolute
 * Zuschauerzahlen liegen dem Frontend gar nicht vor. Der Weichzeichner ist
 * deshalb reine Gestaltung und kein Schutz — wer ihn in den Entwicklertools
 * abschaltet, sieht dieselbe Form, nur schaerfer.
 */

interface WachstumsKurveProps {
  punkte: number[];
  unscharf: boolean;
  /** Beschreibung fuer Screenreader; die Grafik selbst traegt keine Zahlen. */
  beschreibung: string;
}

const BREITE = 320;
const HOEHE = 72;

export function WachstumsKurve({ punkte, unscharf, beschreibung }: WachstumsKurveProps) {
  if (punkte.length < 2) return null;

  const schritt = BREITE / (punkte.length - 1);
  const koordinaten = punkte.map((wert, index) => {
    const x = index * schritt;
    // 4px Luft oben und unten, sonst klebt die Spitze am Rand.
    const y = HOEHE - 4 - Math.max(0, Math.min(1, wert)) * (HOEHE - 8);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const linie = `M ${koordinaten.join(' L ')}`;
  const flaeche = `${linie} L ${BREITE},${HOEHE} L 0,${HOEHE} Z`;

  return (
    <svg
      viewBox={`0 0 ${BREITE} ${HOEHE}`}
      preserveAspectRatio="none"
      className={`w-full h-[72px] ${unscharf ? 'blur-[6px]' : ''}`}
      role="img"
      aria-label={beschreibung}
    >
      <defs>
        <linearGradient id="wachstum-verlauf" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="currentColor" stopOpacity="0.35" />
          <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={flaeche} fill="url(#wachstum-verlauf)" className="text-primary" />
      <path
        d={linie}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="text-primary"
      />
    </svg>
  );
}
