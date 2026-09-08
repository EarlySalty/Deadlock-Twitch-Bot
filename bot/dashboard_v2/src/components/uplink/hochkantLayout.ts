export interface HochkantRahmen {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type HochkantModus = 'nur_gameplay' | 'gestapelt' | 'bild_im_bild';
export type HochkantBandLage = 'oben' | 'unten';

export interface HochkantBand {
  hoehe: number;
  lage: HochkantBandLage;
}

export interface HochkantLayout {
  version: 1;
  modus: HochkantModus;
  gameplay: HochkantRahmen;
  kamera: HochkantRahmen | null;
  kameraBand: HochkantBand | null;
  kameraBox: HochkantRahmen | null;
}

export interface HochkantQuelle {
  breite: number;
  hoehe: number;
}

export interface HochkantZiel {
  breite: number;
  hoehe: number;
}

export interface HochkantStand {
  revision: number;
  layout: HochkantLayout | null;
  gespeichertAm: string | null;
  aktiveRevision: number | null;
}

export type HochkantGriff = 'mitte' | 'nw' | 'ne' | 'sw' | 'se';

export const MIN_KANTE = 0.05;
const VERHAELTNIS_TOLERANZ = 0.01;

function begrenzen(wert: number, unten: number, oben: number): number {
  return Math.min(Math.max(wert, unten), oben);
}

function geradePixel(wert: number): number {
  const gerundet = Math.round(wert);
  return Math.max(2, gerundet - (gerundet % 2));
}

function quellVerhaeltnis(quelle: HochkantQuelle): number {
  return quelle.breite / quelle.hoehe;
}

function rahmenVerhaeltnis(rahmen: HochkantRahmen, quelle: HochkantQuelle): number {
  return (rahmen.w * quelle.breite) / (rahmen.h * quelle.hoehe);
}

function verhaeltnisImZielraum(rahmen: HochkantRahmen, ziel: HochkantZiel): number {
  return (rahmen.w * ziel.breite) / (rahmen.h * ziel.hoehe);
}

function nahBeiVerhaeltnis(ist: number, soll: number): boolean {
  if (soll <= 0) return false;
  return Math.abs(ist - soll) / soll <= VERHAELTNIS_TOLERANZ;
}

function imBild(rahmen: HochkantRahmen): boolean {
  return (
    rahmen.x >= -1e-9 &&
    rahmen.y >= -1e-9 &&
    rahmen.x + rahmen.w <= 1 + 1e-9 &&
    rahmen.y + rahmen.h <= 1 + 1e-9
  );
}

function kanteZuKlein(rahmen: HochkantRahmen): boolean {
  return rahmen.w < MIN_KANTE - 1e-9 || rahmen.h < MIN_KANTE - 1e-9;
}

export function rahmenBegrenzen(rahmen: HochkantRahmen, minKante: number = MIN_KANTE): HochkantRahmen {
  const w = begrenzen(rahmen.w, minKante, 1);
  const h = begrenzen(rahmen.h, minKante, 1);
  const x = begrenzen(rahmen.x, 0, 1 - w);
  const y = begrenzen(rahmen.y, 0, 1 - h);
  return { x, y, w, h };
}

export function seitenverhaeltnisSperren(
  rahmen: HochkantRahmen,
  verhaeltnis: number,
  quelle: HochkantQuelle,
  minKante: number = MIN_KANTE,
): HochkantRahmen {
  const sv = quellVerhaeltnis(quelle);
  let w = begrenzen(rahmen.w, minKante, 1);
  let h = (w * sv) / verhaeltnis;
  if (h > 1) {
    h = 1;
    w = (h * verhaeltnis) / sv;
  }
  if (h < minKante) {
    h = minKante;
    w = (h * verhaeltnis) / sv;
  }
  if (w > 1) {
    w = 1;
    h = (w * sv) / verhaeltnis;
  }
  const x = begrenzen(rahmen.x, 0, 1 - w);
  const y = begrenzen(rahmen.y, 0, 1 - h);
  return { x, y, w, h };
}

export function rahmenZiehen(
  rahmen: HochkantRahmen,
  griff: HochkantGriff,
  dx: number,
  dy: number,
  verhaeltnis: number | null,
  quelle: HochkantQuelle,
  minKante: number = MIN_KANTE,
): HochkantRahmen {
  if (griff === 'mitte') {
    return rahmenBegrenzen({ ...rahmen, x: rahmen.x + dx, y: rahmen.y + dy }, minKante);
  }
  const rechts = griff === 'ne' || griff === 'se';
  const unten = griff === 'se' || griff === 'sw';
  const ankerX = rechts ? rahmen.x : rahmen.x + rahmen.w;
  const ankerY = unten ? rahmen.y : rahmen.y + rahmen.h;
  const platzX = rechts ? 1 - ankerX : ankerX;
  const platzY = unten ? 1 - ankerY : ankerY;

  let w = rechts ? rahmen.w + dx : rahmen.w - dx;
  let h = unten ? rahmen.h + dy : rahmen.h - dy;

  if (verhaeltnis != null) {
    const sv = quellVerhaeltnis(quelle);
    const wAusPlatzY = (platzY * verhaeltnis) / sv;
    const wMax = Math.max(minKante, Math.min(platzX, wAusPlatzY));
    const wMinAusKante = (minKante * verhaeltnis) / sv;
    w = begrenzen(w, Math.max(minKante, wMinAusKante), wMax);
    h = (w * sv) / verhaeltnis;
  } else {
    w = begrenzen(w, minKante, Math.max(minKante, platzX));
    h = begrenzen(h, minKante, Math.max(minKante, platzY));
  }

  const x = rechts ? ankerX : ankerX - w;
  const y = unten ? ankerY : ankerY - h;
  return { x, y, w, h };
}

export function hochkantAnfang(quelle: HochkantQuelle, ziel: HochkantZiel): HochkantLayout {
  const sv = quellVerhaeltnis(quelle);
  const zielSV = ziel.breite / ziel.hoehe;
  const gameplayBreite = zielSV / sv;
  const gameplay: HochkantRahmen = {
    x: (1 - gameplayBreite) / 2,
    y: 0,
    w: gameplayBreite,
    h: 1,
  };
  const kameraBreite = 0.2;
  const kamera: HochkantRahmen = {
    x: 0.78,
    y: 0.05,
    w: kameraBreite,
    h: kameraBreite * sv,
  };
  const kameraVerhaeltnis = rahmenVerhaeltnis(kamera, quelle);
  const boxBreite = 0.35;
  const boxHoehe = (boxBreite * ziel.breite) / (kameraVerhaeltnis * ziel.hoehe);
  const kameraBox: HochkantRahmen = {
    x: 1 - boxBreite - 0.03,
    y: 0.03,
    w: boxBreite,
    h: boxHoehe,
  };
  return {
    version: 1,
    modus: 'bild_im_bild',
    gameplay,
    kamera,
    kameraBand: null,
    kameraBox,
  };
}

export function zielverhaeltnisse(
  layout: HochkantLayout,
  ziel: HochkantZiel,
): { gameplay: number; kamera: number | null } {
  const zielSV = ziel.breite / ziel.hoehe;
  if (layout.modus === 'gestapelt') {
    const band = layout.kameraBand ? layout.kameraBand.hoehe : 0.25;
    const gameplayFlaeche = ziel.hoehe * (1 - band);
    const kameraFlaeche = ziel.hoehe * band;
    return {
      gameplay: ziel.breite / gameplayFlaeche,
      kamera: ziel.breite / kameraFlaeche,
    };
  }
  return { gameplay: zielSV, kamera: null };
}

export function hochkantPruefen(
  layout: HochkantLayout,
  ziel: HochkantZiel,
  quelle: HochkantQuelle,
): string[] {
  const fehler: string[] = [];

  if (!imBild(layout.gameplay)) {
    fehler.push('Der Gameplay-Rahmen liegt nicht vollständig im Bild.');
  }
  if (kanteZuKlein(layout.gameplay)) {
    fehler.push('Der Gameplay-Rahmen unterschreitet die Mindestkante von fünf Prozent.');
  }

  if (layout.kamera) {
    if (!imBild(layout.kamera)) {
      fehler.push('Der Kamera-Rahmen liegt nicht vollständig im Bild.');
    }
    if (kanteZuKlein(layout.kamera)) {
      fehler.push('Der Kamera-Rahmen unterschreitet die Mindestkante von fünf Prozent.');
    }
  } else if (layout.modus !== 'nur_gameplay') {
    fehler.push('Ohne Kamera ist nur der Modus nur Gameplay möglich.');
  }

  const ratios = zielverhaeltnisse(layout, ziel);
  if (!nahBeiVerhaeltnis(rahmenVerhaeltnis(layout.gameplay, quelle), ratios.gameplay)) {
    fehler.push('Das Gameplay-Seitenverhältnis passt nicht zum Zielbild.');
  }

  if (layout.modus === 'gestapelt') {
    if (!layout.kamera) {
      fehler.push('Der gestapelte Modus braucht eine Kamera.');
    }
    if (!layout.kameraBand) {
      fehler.push('Im gestapelten Modus fehlt das Kameraband.');
    } else {
      if (layout.kameraBand.hoehe < 0.1 - 1e-9 || layout.kameraBand.hoehe > 0.5 + 1e-9) {
        fehler.push('Die Bandhöhe muss zwischen zehn und fünfzig Prozent liegen.');
      }
      if (layout.kamera && ratios.kamera != null) {
        if (!nahBeiVerhaeltnis(rahmenVerhaeltnis(layout.kamera, quelle), ratios.kamera)) {
          fehler.push('Das Kamera-Seitenverhältnis passt nicht zum Kameraband.');
        }
      }
    }
  } else if (layout.kameraBand) {
    fehler.push('Das Kameraband gehört nur in den gestapelten Modus.');
  }

  if (layout.modus === 'bild_im_bild') {
    if (!layout.kameraBox) {
      fehler.push('Im Bild-im-Bild-Modus fehlt das Kamerafenster.');
    } else {
      if (!imBild(layout.kameraBox)) {
        fehler.push('Das Kamerafenster liegt nicht vollständig im Zielbild.');
      }
      if (kanteZuKlein(layout.kameraBox)) {
        fehler.push('Das Kamerafenster unterschreitet die Mindestkante von fünf Prozent.');
      }
      if (layout.kamera) {
        const boxIst = verhaeltnisImZielraum(layout.kameraBox, ziel);
        const kameraIst = rahmenVerhaeltnis(layout.kamera, quelle);
        if (!nahBeiVerhaeltnis(boxIst, kameraIst)) {
          fehler.push('Das Kamerafenster hat ein anderes Seitenverhältnis als die Kamera.');
        }
      }
    }
  } else if (layout.kameraBox) {
    fehler.push('Das Kamerafenster gehört nur in den Bild-im-Bild-Modus.');
  }

  return fehler;
}

export interface ZielPixelRahmen {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ZielPixel {
  gameplay: ZielPixelRahmen;
  kamera: ZielPixelRahmen | null;
  kameraBand: { hoehe: number } | null;
  kameraBox: ZielPixelRahmen | null;
}

function quellPixel(rahmen: HochkantRahmen, quelle: HochkantQuelle): ZielPixelRahmen {
  return {
    x: geradePixel(rahmen.x * quelle.breite),
    y: geradePixel(rahmen.y * quelle.hoehe),
    w: geradePixel(rahmen.w * quelle.breite),
    h: geradePixel(rahmen.h * quelle.hoehe),
  };
}

function zielraumPixel(rahmen: HochkantRahmen, ziel: HochkantZiel): ZielPixelRahmen {
  return {
    x: geradePixel(rahmen.x * ziel.breite),
    y: geradePixel(rahmen.y * ziel.hoehe),
    w: geradePixel(rahmen.w * ziel.breite),
    h: geradePixel(rahmen.h * ziel.hoehe),
  };
}

export function zielPixel(
  layout: HochkantLayout,
  quelle: HochkantQuelle,
  ziel: HochkantZiel,
): ZielPixel {
  const gameplay = quellPixel(layout.gameplay, quelle);
  const kamera =
    layout.modus !== 'nur_gameplay' && layout.kamera ? quellPixel(layout.kamera, quelle) : null;
  const kameraBand =
    layout.modus === 'gestapelt' && layout.kameraBand
      ? { hoehe: geradePixel(layout.kameraBand.hoehe * ziel.hoehe) }
      : null;
  const kameraBox =
    layout.modus === 'bild_im_bild' && layout.kameraBox
      ? zielraumPixel(layout.kameraBox, ziel)
      : null;
  return { gameplay, kamera, kameraBand, kameraBox };
}

export interface VorschauCss {
  backgroundSize: string;
  backgroundPosition: string;
}

export function vorschauAusschnitt(
  rahmen: HochkantRahmen,
  _quelle: HochkantQuelle,
  zielflaeche: { breite: number; hoehe: number },
): VorschauCss {
  const breite = zielflaeche.breite / rahmen.w;
  const hoehe = zielflaeche.hoehe / rahmen.h;
  const links = -rahmen.x * breite;
  const oben = -rahmen.y * hoehe;
  return {
    backgroundSize: `${breite}px ${hoehe}px`,
    backgroundPosition: `${links}px ${oben}px`,
  };
}

function rahmenGleich(a: HochkantRahmen | null, b: HochkantRahmen | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    Math.abs(a.x - b.x) < 1e-9 &&
    Math.abs(a.y - b.y) < 1e-9 &&
    Math.abs(a.w - b.w) < 1e-9 &&
    Math.abs(a.h - b.h) < 1e-9
  );
}

export function hochkantGleich(a: HochkantLayout, b: HochkantLayout): boolean {
  if (a.modus !== b.modus) return false;
  if (!rahmenGleich(a.gameplay, b.gameplay)) return false;
  if (!rahmenGleich(a.kamera, b.kamera)) return false;
  if (!rahmenGleich(a.kameraBox, b.kameraBox)) return false;
  const bandA = a.kameraBand;
  const bandB = b.kameraBand;
  if (bandA === null || bandB === null) return bandA === bandB;
  return Math.abs(bandA.hoehe - bandB.hoehe) < 1e-9 && bandA.lage === bandB.lage;
}

export function naechsterEntwurf(
  entwurf: HochkantLayout,
  basis: HochkantLayout,
  neu: HochkantLayout,
): HochkantLayout {
  return hochkantGleich(entwurf, basis) ? neu : entwurf;
}
