import { test } from 'node:test';
import assert from 'node:assert/strict';

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
} from '../src/components/uplink/hochkantLayout';
import type { HochkantLayout, HochkantRahmen } from '../src/components/uplink/hochkantLayout';

const ZIEL = { breite: 1080, hoehe: 1920 };
const HD = { breite: 1920, hoehe: 1080 };
const WQHD = { breite: 2560, hoehe: 1440 };

function verhaeltnisQuelle(r: HochkantRahmen, quelle: { breite: number; hoehe: number }): number {
  return (r.w * quelle.breite) / (r.h * quelle.hoehe);
}

function istGerade(n: number): boolean {
  return n % 2 === 0;
}

test('Anfangswert erfuellt die Pruefung fuer 1920x1080', () => {
  assert.deepEqual(hochkantPruefen(hochkantAnfang(HD, ZIEL), ZIEL, HD), []);
});

test('Anfangswert erfuellt die Pruefung fuer 2560x1440', () => {
  assert.deepEqual(hochkantPruefen(hochkantAnfang(WQHD, ZIEL), ZIEL, WQHD), []);
});

test('rahmenBegrenzen haelt an allen vier Kanten', () => {
  assert.deepEqual(rahmenBegrenzen({ x: -0.3, y: 0.5, w: 0.4, h: 0.4 }), { x: 0, y: 0.5, w: 0.4, h: 0.4 });
  assert.deepEqual(rahmenBegrenzen({ x: 0.5, y: -0.2, w: 0.4, h: 0.4 }), { x: 0.5, y: 0, w: 0.4, h: 0.4 });
  assert.deepEqual(rahmenBegrenzen({ x: 0.8, y: 0.3, w: 0.5, h: 0.4 }), { x: 0.5, y: 0.3, w: 0.5, h: 0.4 });
  assert.deepEqual(rahmenBegrenzen({ x: 0.3, y: 0.8, w: 0.4, h: 0.5 }), { x: 0.3, y: 0.5, w: 0.4, h: 0.5 });
});

test('rahmenBegrenzen erzwingt die Mindestkante', () => {
  assert.deepEqual(rahmenBegrenzen({ x: 0.2, y: 0.2, w: 0.01, h: 0.02 }), { x: 0.2, y: 0.2, w: 0.05, h: 0.05 });
});

test('Eckgriff-Drag haelt die gegenueberliegende Ecke und das Verhaeltnis', () => {
  const start: HochkantRahmen = { x: 0.3, y: 0.05, w: 0.3, h: 0.7 };
  const gezogen = rahmenZiehen(start, 'se', 0.05, 0, 0.5625, HD);
  assert.ok(Math.abs(gezogen.x - start.x) < 1e-9);
  assert.ok(Math.abs(gezogen.y - start.y) < 1e-9);
  assert.ok(Math.abs(verhaeltnisQuelle(gezogen, HD) - 0.5625) / 0.5625 <= 0.01);

  const nw = rahmenZiehen(start, 'nw', -0.05, 0, 0.5625, HD);
  assert.ok(Math.abs(nw.x + nw.w - (start.x + start.w)) < 1e-9);
  assert.ok(Math.abs(nw.y + nw.h - (start.y + start.h)) < 1e-9);
  assert.ok(Math.abs(verhaeltnisQuelle(nw, HD) - 0.5625) / 0.5625 <= 0.01);
});

test('seitenverhaeltnisSperren liefert das Verhaeltnis innerhalb von einem Prozent', () => {
  const gesperrt = seitenverhaeltnisSperren({ x: 0.3, y: 0.1, w: 0.3, h: 0.9 }, 0.5625, HD);
  assert.ok(Math.abs(verhaeltnisQuelle(gesperrt, HD) - 0.5625) / 0.5625 <= 0.01);
  assert.ok(gesperrt.x >= 0 && gesperrt.y >= 0 && gesperrt.x + gesperrt.w <= 1 && gesperrt.y + gesperrt.h <= 1);
});

test('zielPixel liefert gerade Werte, Bandhoehe unter der Zielhoehe und Box im Zielbild', () => {
  const gestapelt: HochkantLayout = {
    version: 1,
    modus: 'gestapelt',
    gameplay: { x: 0.3, y: 0, w: 0.4, h: 0.8 },
    kamera: { x: 0.1, y: 0.1, w: 0.4, h: 0.2 },
    kameraBand: { hoehe: 0.25, lage: 'unten' },
    kameraBox: null,
  };
  const g = zielPixel(gestapelt, HD, ZIEL);
  for (const wert of [g.gameplay.x, g.gameplay.y, g.gameplay.w, g.gameplay.h]) assert.ok(istGerade(wert));
  assert.ok(g.kamera && istGerade(g.kamera.w) && istGerade(g.kamera.h));
  assert.ok(g.kameraBand);
  assert.ok(istGerade(g.kameraBand.hoehe));
  assert.ok(g.kameraBand.hoehe < ZIEL.hoehe - 2);

  const box: HochkantLayout = { ...hochkantAnfang(HD, ZIEL) };
  const p = zielPixel(box, HD, ZIEL);
  assert.ok(p.kameraBox);
  assert.ok(istGerade(p.kameraBox.x) && istGerade(p.kameraBox.y));
  assert.ok(p.kameraBox.x + p.kameraBox.w <= ZIEL.breite);
  assert.ok(p.kameraBox.y + p.kameraBox.h <= ZIEL.hoehe);
});

test('Rahmen ausserhalb des Bildes ergibt einen Fehlertext', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'nur_gameplay',
    gameplay: { x: 0.9, y: 0, w: 0.4, h: 1 },
    kamera: null,
    kameraBand: null,
    kameraBox: null,
  };
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('vollständig im Bild')));
});

test('Kamera null im gestapelten Modus ergibt einen Fehlertext', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'gestapelt',
    gameplay: { x: 0.3, y: 0, w: 0.4, h: 0.75 },
    kamera: null,
    kameraBand: { hoehe: 0.25, lage: 'unten' },
    kameraBox: null,
  };
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('braucht eine Kamera')));
});

test('Bandhoehe ausserhalb von 0.1 bis 0.5 ergibt einen Fehlertext', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'gestapelt',
    gameplay: { x: 0.3, y: 0, w: 0.4, h: 0.4 },
    kamera: { x: 0.1, y: 0.1, w: 0.4, h: 0.6 },
    kameraBand: { hoehe: 0.6, lage: 'unten' },
    kameraBox: null,
  };
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('zehn und fünfzig Prozent')));
});

test('Falsches Seitenverhaeltnis ergibt einen Fehlertext', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'nur_gameplay',
    gameplay: { x: 0.2, y: 0.2, w: 0.6, h: 0.6 },
    kamera: null,
    kameraBand: null,
    kameraBox: null,
  };
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('Gameplay-Seitenverhältnis passt nicht')));
});

test('vorschauAusschnitt fuellt bei formgleichem Rahmen die ganze Flaeche', () => {
  const css = vorschauAusschnitt({ x: 0, y: 0, w: 1, h: 1 }, HD, { breite: 200, hoehe: 356 });
  assert.equal(css.backgroundSize, '200px 356px');
  assert.equal(css.backgroundPosition, '0px 0px');
});

test('vorschauAusschnitt versetzt einen Rahmen rechts der Mitte', () => {
  const css = vorschauAusschnitt({ x: 0.5, y: 0, w: 0.5, h: 1 }, HD, { breite: 200, hoehe: 356 });
  assert.equal(css.backgroundSize, '400px 356px');
  assert.equal(css.backgroundPosition, '-200px 0px');
});

test('naechsterEntwurf uebernimmt neu nur bei unberuehrtem Entwurf', () => {
  const basis = hochkantAnfang(HD, ZIEL);
  const neu = hochkantAnfang(WQHD, ZIEL);
  assert.equal(naechsterEntwurf(basis, basis, neu), neu);

  const beruehrt: HochkantLayout = { ...basis, gameplay: { ...basis.gameplay, x: basis.gameplay.x + 0.1 } };
  assert.equal(naechsterEntwurf(beruehrt, basis, neu), beruehrt);
  assert.ok(!hochkantGleich(beruehrt, basis));
});
