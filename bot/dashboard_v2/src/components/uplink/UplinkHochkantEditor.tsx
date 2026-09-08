import { useEffect, useId, useMemo, useRef, useState } from 'react';
import type {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from 'react';
import { Camera, Layers, Maximize2, RotateCcw, Save } from 'lucide-react';
import {
  hochkantAnfang,
  hochkantGleich,
  hochkantPruefen,
  naechsterEntwurf,
  rahmenBegrenzen,
  rahmenZiehen,
  seitenverhaeltnisSperren,
  vorschauAusschnitt,
  zielPixel,
  zielverhaeltnisse,
} from './hochkantLayout';
import type {
  HochkantBandLage,
  HochkantGriff,
  HochkantLayout,
  HochkantModus,
  HochkantQuelle,
  HochkantRahmen,
  HochkantStand,
  HochkantZiel,
} from './hochkantLayout';

export interface UplinkHochkantEditorProps {
  quelle: HochkantQuelle | null;
  ziel: HochkantZiel;
  standbildUrl: string | null;
  standbildHinweis?: string | null;
  stand: HochkantStand | null;
  beschaeftigt: boolean;
  fehlerText?: string | null;
  onSpeichern: (layout: HochkantLayout) => void;
  onZuruecksetzen: () => void;
}

const STANDARD_QUELLE: HochkantQuelle = { breite: 1920, hoehe: 1080 };
const ECKEN: HochkantGriff[] = ['nw', 'ne', 'sw', 'se'];

const FARBEN = {
  gold: { rand: 'rgba(197, 160, 89, 0.95)', fuellung: 'rgba(197, 160, 89, 0.16)' },
  weiss: { rand: 'rgba(255, 255, 255, 0.92)', fuellung: 'rgba(255, 255, 255, 0.12)' },
} as const;

const RASTER =
  'repeating-linear-gradient(45deg, rgba(255,255,255,0.05) 0 12px, rgba(255,255,255,0.01) 12px 24px)';

type Farbe = keyof typeof FARBEN;

function kameraVerhaeltnis(kamera: HochkantRahmen, quelle: HochkantQuelle): number {
  return (kamera.w * quelle.breite) / (kamera.h * quelle.hoehe);
}

function ausrichten(layout: HochkantLayout, ziel: HochkantZiel, quelle: HochkantQuelle): HochkantLayout {
  const ratios = zielverhaeltnisse(layout, ziel);
  const gameplay = seitenverhaeltnisSperren(layout.gameplay, ratios.gameplay, quelle);
  let kamera = layout.kamera;
  if (kamera && layout.modus === 'gestapelt' && ratios.kamera != null) {
    kamera = seitenverhaeltnisSperren(kamera, ratios.kamera, quelle);
  }
  let kameraBox = layout.kameraBox;
  if (layout.modus === 'bild_im_bild' && kamera && kameraBox) {
    kameraBox = seitenverhaeltnisSperren(kameraBox, kameraVerhaeltnis(kamera, quelle), {
      breite: ziel.breite,
      hoehe: ziel.hoehe,
    });
  }
  return { ...layout, gameplay, kamera, kameraBox };
}

function modusWechseln(
  layout: HochkantLayout,
  modus: HochkantModus,
  ziel: HochkantZiel,
  quelle: HochkantQuelle,
  vorlage: HochkantLayout,
): HochkantLayout {
  const kameraVorrat = layout.kamera ?? vorlage.kamera;
  const bandVorrat = layout.kameraBand ?? vorlage.kameraBand ?? { hoehe: 0.25, lage: 'unten' as HochkantBandLage };
  const boxVorrat = layout.kameraBox ?? vorlage.kameraBox;
  if (modus === 'nur_gameplay') {
    return ausrichten({ ...layout, modus, kamera: null, kameraBand: null, kameraBox: null }, ziel, quelle);
  }
  if (modus === 'gestapelt') {
    return ausrichten(
      { ...layout, modus, kamera: kameraVorrat, kameraBand: bandVorrat, kameraBox: null },
      ziel,
      quelle,
    );
  }
  return ausrichten(
    { ...layout, modus, kamera: kameraVorrat, kameraBand: null, kameraBox: boxVorrat },
    ziel,
    quelle,
  );
}

interface Ziehbar {
  id: string;
  rahmen: HochkantRahmen;
  farbe: Farbe;
  ariaLabel: string;
  verhaeltnis: number | null;
  bezug: { breite: number; hoehe: number };
  inhalt?: ReactNode;
}

interface DragStand {
  pointerId: number;
  id: string;
  griff: HochkantGriff;
  startX: number;
  startY: number;
  startRahmen: HochkantRahmen;
  breitePx: number;
  hoehePx: number;
}

function Ziehflaeche({
  aspect,
  hintergrund,
  rahmen,
  ausgewaehlt,
  aufAuswahl,
  aufAendern,
  style,
}: {
  aspect: number;
  hintergrund: ReactNode;
  rahmen: Ziehbar[];
  ausgewaehlt: string | null;
  aufAuswahl: (id: string) => void;
  aufAendern: (id: string, naechster: HochkantRahmen) => void;
  style?: CSSProperties;
}) {
  const flaeche = useRef<HTMLDivElement | null>(null);
  const zug = useRef<DragStand | null>(null);

  const beginn = (e: ReactPointerEvent, ziehbar: Ziehbar, griff: HochkantGriff) => {
    const box = flaeche.current;
    if (!box) return;
    e.preventDefault();
    e.stopPropagation();
    aufAuswahl(ziehbar.id);
    const rect = box.getBoundingClientRect();
    zug.current = {
      pointerId: e.pointerId,
      id: ziehbar.id,
      griff,
      startX: e.clientX,
      startY: e.clientY,
      startRahmen: { ...ziehbar.rahmen },
      breitePx: rect.width,
      hoehePx: rect.height,
    };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };

  const bewegung = (e: ReactPointerEvent) => {
    const stand = zug.current;
    if (!stand || stand.pointerId !== e.pointerId) return;
    const ziehbar = rahmen.find((r) => r.id === stand.id);
    if (!ziehbar) return;
    const dx = (e.clientX - stand.startX) / stand.breitePx;
    const dy = (e.clientY - stand.startY) / stand.hoehePx;
    aufAendern(stand.id, rahmenZiehen(stand.startRahmen, stand.griff, dx, dy, ziehbar.verhaeltnis, ziehbar.bezug));
  };

  const ende = (e: ReactPointerEvent) => {
    if (zug.current?.pointerId === e.pointerId) zug.current = null;
  };

  const tastatur = (e: ReactKeyboardEvent, ziehbar: Ziehbar) => {
    const schritt = e.shiftKey ? 0.05 : 0.01;
    let dx = 0;
    let dy = 0;
    if (e.key === 'ArrowLeft') dx = -schritt;
    else if (e.key === 'ArrowRight') dx = schritt;
    else if (e.key === 'ArrowUp') dy = -schritt;
    else if (e.key === 'ArrowDown') dy = schritt;
    else return;
    e.preventDefault();
    aufAendern(ziehbar.id, rahmenBegrenzen({ ...ziehbar.rahmen, x: ziehbar.rahmen.x + dx, y: ziehbar.rahmen.y + dy }));
  };

  return (
    <div
      ref={flaeche}
      onPointerMove={bewegung}
      onPointerUp={ende}
      onPointerCancel={ende}
      className="relative w-full select-none overflow-hidden rounded-xl border border-border"
      style={{ aspectRatio: `${aspect}`, background: 'linear-gradient(135deg,#16100d,#0d0806)', ...style }}
    >
      {hintergrund}
      {rahmen.map((ziehbar) => {
        const { rand, fuellung } = FARBEN[ziehbar.farbe];
        const gewaehlt = ausgewaehlt === ziehbar.id;
        return (
          <div
            key={ziehbar.id}
            role="group"
            aria-label={ziehbar.ariaLabel}
            tabIndex={0}
            onPointerDown={(e) => beginn(e, ziehbar, 'mitte')}
            onKeyDown={(e) => tastatur(e, ziehbar)}
            className="absolute cursor-move"
            style={{
              left: `${ziehbar.rahmen.x * 100}%`,
              top: `${ziehbar.rahmen.y * 100}%`,
              width: `${ziehbar.rahmen.w * 100}%`,
              height: `${ziehbar.rahmen.h * 100}%`,
              border: `2px solid ${rand}`,
              background: ziehbar.inhalt ? 'transparent' : fuellung,
              boxShadow: gewaehlt ? `0 0 0 3px ${rand}, 0 8px 22px rgba(0,0,0,0.45)` : '0 3px 12px rgba(0,0,0,0.35)',
              zIndex: gewaehlt ? 20 : 10,
            }}
          >
            {ziehbar.inhalt}
            <span
              className="absolute left-1 top-1 rounded px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-[0.12em]"
              style={{ background: rand, color: '#241A12' }}
            >
              {ziehbar.ariaLabel}
            </span>
            {ECKEN.map((ecke) => {
              const oben = ecke === 'nw' || ecke === 'ne';
              const links = ecke === 'nw' || ecke === 'sw';
              return (
                <button
                  key={ecke}
                  type="button"
                  aria-label={`${ziehbar.ariaLabel} Ecke ${ecke}`}
                  onPointerDown={(e) => beginn(e, ziehbar, ecke)}
                  className="absolute h-3.5 w-3.5 rounded"
                  style={{
                    top: oben ? -7 : 'auto',
                    bottom: oben ? 'auto' : -7,
                    left: links ? -7 : 'auto',
                    right: links ? 'auto' : -7,
                    background: rand,
                    border: '2px solid #0d0806',
                  }}
                />
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

function useFlaechenGroesse(fallback: { breite: number; hoehe: number }) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [groesse, setGroesse] = useState(fallback);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const beobachter = new ResizeObserver((eintraege) => {
      const box = eintraege[0]?.contentRect;
      if (box && box.width > 0 && box.height > 0) setGroesse({ breite: box.width, hoehe: box.height });
    });
    beobachter.observe(el);
    return () => beobachter.disconnect();
  }, []);
  return [ref, groesse] as const;
}

function Fuellung({
  rahmen,
  quelle,
  standbildUrl,
}: {
  rahmen: HochkantRahmen;
  quelle: HochkantQuelle;
  standbildUrl: string | null;
}) {
  const [ref, groesse] = useFlaechenGroesse({ breite: 200, hoehe: 356 });
  if (!standbildUrl) {
    return <div ref={ref} className="absolute inset-0" style={{ background: RASTER }} />;
  }
  const css = vorschauAusschnitt(rahmen, quelle, groesse);
  return (
    <div
      ref={ref}
      className="absolute inset-0"
      style={{
        backgroundImage: `url("${standbildUrl}")`,
        backgroundRepeat: 'no-repeat',
        backgroundSize: css.backgroundSize,
        backgroundPosition: css.backgroundPosition,
      }}
    />
  );
}

function datumText(iso: string): string {
  const datum = new Date(iso);
  if (Number.isNaN(datum.getTime())) return iso;
  return datum.toLocaleDateString('de-DE');
}

export function UplinkHochkantEditor({
  quelle,
  ziel,
  standbildUrl,
  standbildHinweis,
  stand,
  beschaeftigt,
  fehlerText,
  onSpeichern,
  onZuruecksetzen,
}: UplinkHochkantEditorProps) {
  const basisId = useId();
  const quelleEff = quelle ?? STANDARD_QUELLE;
  const anfang = useMemo(
    () => hochkantAnfang(quelleEff, ziel),
    [quelleEff.breite, quelleEff.hoehe, ziel.breite, ziel.hoehe],
  );
  const [entwurf, setEntwurf] = useState<HochkantLayout>(() => stand?.layout ?? anfang);
  const [ausgewaehlt, setAusgewaehlt] = useState<string | null>('gameplay');
  const basis = useRef<HochkantLayout>(stand?.layout ?? anfang);

  useEffect(() => {
    const neu = stand?.layout ?? anfang;
    const gewaehlt = naechsterEntwurf(entwurf, basis.current, neu);
    if (gewaehlt !== entwurf) {
      basis.current = neu;
      setEntwurf(gewaehlt);
    } else if (hochkantGleich(entwurf, basis.current)) {
      basis.current = neu;
    }
  }, [stand, anfang, entwurf]);

  const ratios = zielverhaeltnisse(entwurf, ziel);
  const fehler = hochkantPruefen(entwurf, ziel, quelleEff);
  const unveraendert = hochkantGleich(entwurf, basis.current);
  const speichernGesperrt = beschaeftigt || fehler.length > 0 || unveraendert;

  const pixel = zielPixel(entwurf, quelleEff, ziel);

  const quellRahmen: Ziehbar[] = [
    {
      id: 'gameplay',
      rahmen: entwurf.gameplay,
      farbe: 'gold',
      ariaLabel: 'Gameplay-Rahmen',
      verhaeltnis: ratios.gameplay,
      bezug: quelleEff,
    },
  ];
  if (entwurf.kamera && entwurf.modus !== 'nur_gameplay') {
    quellRahmen.push({
      id: 'kamera',
      rahmen: entwurf.kamera,
      farbe: 'weiss',
      ariaLabel: 'Kamera-Rahmen',
      verhaeltnis:
        entwurf.modus === 'gestapelt' && ratios.kamera != null
          ? ratios.kamera
          : kameraVerhaeltnis(entwurf.kamera, quelleEff),
      bezug: quelleEff,
    });
  }

  const setQuellRahmen = (id: string, naechster: HochkantRahmen) => {
    setEntwurf((prev) => {
      if (id === 'gameplay') return { ...prev, gameplay: naechster };
      if (id === 'kamera') return { ...prev, kamera: naechster };
      return prev;
    });
  };

  const setKameraBox = (_id: string, naechster: HochkantRahmen) => {
    setEntwurf((prev) => ({ ...prev, kameraBox: naechster }));
  };

  const wechsleModus = (modus: HochkantModus) => {
    setEntwurf((prev) => modusWechseln(prev, modus, ziel, quelleEff, anfang));
  };

  const setzeKamera = (an: boolean) => {
    if (an) wechsleModus(entwurf.modus === 'nur_gameplay' ? 'bild_im_bild' : entwurf.modus);
    else wechsleModus('nur_gameplay');
  };

  const setzeBandHoehe = (prozent: number) => {
    setEntwurf((prev) => {
      if (!prev.kameraBand) return prev;
      return ausrichten(
        { ...prev, kameraBand: { ...prev.kameraBand, hoehe: prozent / 100 } },
        ziel,
        quelleEff,
      );
    });
  };

  const setzeBandLage = (lage: HochkantBandLage) => {
    setEntwurf((prev) => (prev.kameraBand ? { ...prev, kameraBand: { ...prev.kameraBand, lage } } : prev));
  };

  const kameraAn = entwurf.kamera != null && entwurf.modus !== 'nur_gameplay';

  const zielHintergrund = (() => {
    if (entwurf.modus === 'gestapelt' && entwurf.kamera && entwurf.kameraBand) {
      const bandAnteil = entwurf.kameraBand.hoehe;
      const oben = entwurf.kameraBand.lage === 'oben';
      const bandStil: CSSProperties = { position: 'absolute', left: 0, right: 0, height: `${bandAnteil * 100}%` };
      const spielStil: CSSProperties = { position: 'absolute', left: 0, right: 0, height: `${(1 - bandAnteil) * 100}%` };
      return (
        <>
          <div style={{ ...bandStil, top: oben ? 0 : 'auto', bottom: oben ? 'auto' : 0 }}>
            <Fuellung rahmen={entwurf.kamera} quelle={quelleEff} standbildUrl={standbildUrl} />
          </div>
          <div style={{ ...spielStil, top: oben ? 'auto' : 0, bottom: oben ? 0 : 'auto' }}>
            <Fuellung rahmen={entwurf.gameplay} quelle={quelleEff} standbildUrl={standbildUrl} />
          </div>
        </>
      );
    }
    return <Fuellung rahmen={entwurf.gameplay} quelle={quelleEff} standbildUrl={standbildUrl} />;
  })();

  const zielRahmen: Ziehbar[] =
    entwurf.modus === 'bild_im_bild' && entwurf.kamera && entwurf.kameraBox
      ? [
          {
            id: 'box',
            rahmen: entwurf.kameraBox,
            farbe: 'weiss',
            ariaLabel: 'Kamerafenster',
            verhaeltnis: kameraVerhaeltnis(entwurf.kamera, quelleEff),
            bezug: { breite: ziel.breite, hoehe: ziel.hoehe },
            inhalt: <Fuellung rahmen={entwurf.kamera} quelle={quelleEff} standbildUrl={standbildUrl} />,
          },
        ]
      : [];

  const modusOptionen: { modus: HochkantModus; titel: string; text: string; Icon: typeof Camera }[] = [
    { modus: 'nur_gameplay', titel: 'Nur Gameplay', text: 'Nur dein Spielbild füllt das Hochkantbild.', Icon: Maximize2 },
    { modus: 'gestapelt', titel: 'Gestapelt', text: 'Spielbild und Kamera liegen übereinander.', Icon: Layers },
    { modus: 'bild_im_bild', titel: 'Bild im Bild', text: 'Kamera als Fenster über dem Spielbild.', Icon: Camera },
  ];

  const standZeile = (() => {
    if (!stand || stand.revision === 0) return 'Noch kein Stand gespeichert.';
    const wann = stand.gespeichertAm ? ` am ${datumText(stand.gespeichertAm)}` : '';
    return `Gespeichert als Stand ${stand.revision}${wann}.`;
  })();

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
          <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
            Quelle mit Ausschnitt
          </span>
          <Ziehflaeche
            aspect={quelleEff.breite / quelleEff.hoehe}
            hintergrund={
              standbildUrl ? (
                <div
                  className="absolute inset-0"
                  style={{ backgroundImage: `url("${standbildUrl}")`, backgroundSize: 'cover', backgroundPosition: 'center' }}
                />
              ) : (
                <div className="absolute inset-0" style={{ background: RASTER }} />
              )
            }
            rahmen={quellRahmen}
            ausgewaehlt={ausgewaehlt}
            aufAuswahl={setAusgewaehlt}
            aufAendern={setQuellRahmen}
          />
          {quelle ? null : (
            <p className="text-xs text-text-secondary">
              Vorschau nimmt 1920×1080 an, bis dein Stream gemessen ist.
            </p>
          )}
        </div>

        <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
          <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
            Hochkant-Vorschau
          </span>
          <div className="mx-auto" style={{ maxWidth: `${(ziel.breite / ziel.hoehe) * 60}vh` }}>
            <Ziehflaeche
              aspect={ziel.breite / ziel.hoehe}
              hintergrund={zielHintergrund}
              rahmen={zielRahmen}
              ausgewaehlt={ausgewaehlt}
              aufAuswahl={setAusgewaehlt}
              aufAendern={setKameraBox}
            />
          </div>
          <p className="text-[11px] text-text-secondary">
            {pixel.kamera
              ? `Gameplay ${pixel.gameplay.w}×${pixel.gameplay.h} Pixel, Kamera ${pixel.kamera.w}×${pixel.kamera.h} Pixel`
              : `Gameplay ${pixel.gameplay.w}×${pixel.gameplay.h} Pixel`}
          </p>
          {standbildHinweis ? <p className="text-[11px] text-text-secondary">{standbildHinweis}</p> : null}
          <p className="text-[11px] text-text-secondary">
            Die Vorschau ist eine Bedienhilfe, das fertige Bild rechnet Uplink beim Streamstart.
          </p>
        </div>
      </div>

      <fieldset className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
        <legend className="px-1 text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
          Aufteilung
        </legend>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
          {modusOptionen.map(({ modus, titel, text, Icon }) => {
            const aktiv = entwurf.modus === modus;
            return (
              <label
                key={modus}
                className={`flex cursor-pointer gap-2 rounded-xl border p-3 ${
                  aktiv ? 'border-primary/60 bg-primary/10' : 'border-border/60 bg-background/40'
                }`}
              >
                <input
                  type="radio"
                  name={`${basisId}-modus`}
                  checked={aktiv}
                  onChange={() => wechsleModus(modus)}
                  className="mt-1"
                />
                <span className="space-y-1">
                  <span className="flex items-center gap-1.5 text-sm font-semibold text-white">
                    <Icon size={14} aria-hidden="true" />
                    {titel}
                  </span>
                  <span className="block text-[11px] text-text-secondary">{text}</span>
                </span>
              </label>
            );
          })}
        </div>

        <label className="flex items-center gap-2 text-sm text-white">
          <input type="checkbox" checked={kameraAn} onChange={(e) => setzeKamera(e.target.checked)} />
          Kamera anzeigen
        </label>

        {entwurf.modus === 'gestapelt' && entwurf.kameraBand ? (
          <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
            <label className="block text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">
              Höhe des Kamerabands: {Math.round(entwurf.kameraBand.hoehe * 100)} Prozent
              <input
                type="range"
                min={10}
                max={50}
                value={Math.round(entwurf.kameraBand.hoehe * 100)}
                onChange={(e) => setzeBandHoehe(Number(e.target.value))}
                className="mt-1 w-full"
              />
            </label>
            <div role="group" aria-label="Lage des Kamerabands" className="flex overflow-hidden rounded-lg border border-border text-xs font-semibold">
              {(['oben', 'unten'] as HochkantBandLage[]).map((lage) => {
                const aktiv = entwurf.kameraBand?.lage === lage;
                return (
                  <button
                    key={lage}
                    type="button"
                    aria-pressed={aktiv}
                    onClick={() => setzeBandLage(lage)}
                    className={`min-h-10 flex-1 px-3 py-1 ${aktiv ? 'bg-primary text-[#0D0806]' : 'text-text-secondary hover:text-white'}`}
                  >
                    {lage === 'oben' ? 'Kamera oben' : 'Kamera unten'}
                  </button>
                );
              })}
            </div>
          </div>
        ) : null}

        <p className="text-[11px] text-text-secondary">
          Seitenverhältnis wird automatisch gehalten, damit keine schwarzen Balken entstehen.
        </p>
      </fieldset>

      <div className="space-y-2 rounded-xl border border-border/60 bg-background/40 p-3">
        <p className="text-xs text-text-secondary">{standZeile}</p>
        {stand && stand.aktiveRevision != null ? (
          <p className="text-xs text-text-secondary">
            Im laufenden Stream aktiv: Stand {stand.aktiveRevision}. Ein neuer Stand gilt ab dem nächsten Stream.
          </p>
        ) : null}
      </div>

      {fehler.length > 0 ? (
        <ul className="space-y-1 rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          {fehler.map((zeile) => (
            <li key={zeile}>{zeile}</li>
          ))}
        </ul>
      ) : null}

      {fehlerText ? (
        <p className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">{fehlerText}</p>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          disabled={speichernGesperrt}
          onClick={() => onSpeichern(entwurf)}
          className="inline-flex min-h-11 items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:opacity-50"
        >
          <Save size={15} aria-hidden="true" />
          Als neuen Stand speichern
        </button>
        <button
          type="button"
          disabled={beschaeftigt}
          onClick={onZuruecksetzen}
          className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-border px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          <RotateCcw size={15} aria-hidden="true" />
          Zurücksetzen
        </button>
      </div>
    </div>
  );
}
