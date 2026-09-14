import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  ausrichten,
  hochkantAnfang,
  hochkantGleich,
  hochkantPruefen,
  modusWechseln,
  naechsterEntwurf,
  rahmenBegrenzen,
  rahmenZiehen,
  seitenverhaeltnisSperren,
  speichernErlaubt,
  standUebernehmen,
  vorschauAusschnitt,
  zielPixel,
} from '../src/components/uplink/hochkantLayout';
import type { HochkantLayout, HochkantRahmen, HochkantVorrat } from '../src/components/uplink/hochkantLayout';

const ZIEL = { breite: 1080, hoehe: 1920 };
const HD = { breite: 1920, hoehe: 1080 };
const WQHD = { breite: 2560, hoehe: 1440 };

function verhaeltnisQuelle(r: HochkantRahmen, quelle: { breite: number; hoehe: number }): number {
  return (r.w * quelle.breite) / (r.h * quelle.hoehe);
}

function istGerade(n: number): boolean {
  return n % 2 === 0;
}

function rng(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

test('Anfangswert erfuellt die Pruefung fuer 1920x1080', () => {
  assert.deepEqual(hochkantPruefen(hochkantAnfang(HD, ZIEL), ZIEL, HD), []);
});

test('Anfangswert erfuellt die Pruefung fuer 2560x1440', () => {
  assert.deepEqual(hochkantPruefen(hochkantAnfang(WQHD, ZIEL), ZIEL, WQHD), []);
});

test('hochkantAnfang legt Gameplay auf y=0 und volle Hoehe', () => {
  const p = zielPixel(hochkantAnfang(HD, ZIEL), HD, ZIEL);
  assert.equal(p.gameplay.y, 0);
  assert.equal(p.gameplay.h, 1080);
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

test('zielPixel liefert gerade Werte, Rahmen in den Quellmassen und Band unter der Zielhoehe', () => {
  const gestapelt: HochkantLayout = {
    version: 1,
    modus: 'gestapelt',
    gameplay: { x: 0.3, y: 0.05, w: 0.4, h: 0.8 },
    kamera: { x: 0.1, y: 0.1, w: 0.4, h: 0.2 },
    kameraBand: { hoehe: 0.25, lage: 'unten' },
    kameraBox: null,
  };
  const g = zielPixel(gestapelt, HD, ZIEL);
  for (const wert of [g.gameplay.x, g.gameplay.y, g.gameplay.w, g.gameplay.h]) assert.ok(istGerade(wert));
  assert.ok(g.gameplay.x + g.gameplay.w <= HD.breite);
  assert.ok(g.gameplay.y + g.gameplay.h <= HD.hoehe);
  assert.ok(g.kamera);
  for (const wert of [g.kamera.x, g.kamera.y, g.kamera.w, g.kamera.h]) assert.ok(istGerade(wert));
  assert.ok(g.kamera.x + g.kamera.w <= HD.breite);
  assert.ok(g.kamera.y + g.kamera.h <= HD.hoehe);
  assert.ok(g.kameraBand);
  assert.ok(istGerade(g.kameraBand.hoehe));
  assert.ok(g.kameraBand.hoehe < ZIEL.hoehe - 2);
});

test('zielPixel haelt eine linksbuendige breite Kamerabox im Zielbild', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'bild_im_bild',
    gameplay: hochkantAnfang(HD, ZIEL).gameplay,
    kamera: hochkantAnfang(HD, ZIEL).kamera,
    kameraBand: null,
    kameraBox: { x: 0, y: 0.1, w: 1, h: 0.5 },
  };
  const p = zielPixel(layout, HD, ZIEL);
  assert.ok(p.kameraBox);
  assert.ok(p.kameraBox.x + p.kameraBox.w <= ZIEL.breite);
  assert.ok(p.kameraBox.y + p.kameraBox.h <= ZIEL.hoehe);
});

test('zielPixel uebersteht bei keinem gepruefte-gruenen Zufallslayout', () => {
  const zufall = rng(1337);
  let gruen = 0;
  for (let i = 0; i < 500; i += 1) {
    const quelle = zufall() < 0.5 ? HD : WQHD;
    const modusListe = ['nur_gameplay', 'gestapelt', 'bild_im_bild'] as const;
    const modus = modusListe[Math.floor(zufall() * 3)];
    const roh: HochkantLayout = {
      version: 1,
      modus,
      gameplay: { x: zufall() * 0.4, y: zufall() * 0.3, w: 0.2 + zufall() * 0.4, h: 0.2 + zufall() * 0.6 },
      kamera: modus === 'nur_gameplay' ? null : { x: zufall() * 0.4, y: zufall() * 0.4, w: 0.15 + zufall() * 0.3, h: 0.15 + zufall() * 0.3 },
      kameraBand: modus === 'gestapelt' ? { hoehe: 0.1 + zufall() * 0.4, lage: 'unten' } : null,
      kameraBox: modus === 'bild_im_bild' ? { x: zufall() * 0.4, y: zufall() * 0.4, w: 0.2 + zufall() * 0.3, h: 0.2 + zufall() * 0.3 } : null,
    };
    const layout = ausrichten(roh, ZIEL, quelle);
    if (hochkantPruefen(layout, ZIEL, quelle).length > 0) continue;
    gruen += 1;
    const p = zielPixel(layout, quelle, ZIEL);
    assert.ok(p.gameplay.x + p.gameplay.w <= quelle.breite);
    assert.ok(p.gameplay.y + p.gameplay.h <= quelle.hoehe);
    if (p.kamera) {
      assert.ok(p.kamera.x + p.kamera.w <= quelle.breite);
      assert.ok(p.kamera.y + p.kamera.h <= quelle.hoehe);
    }
    if (p.kameraBox) {
      assert.ok(p.kameraBox.x + p.kameraBox.w <= ZIEL.breite);
      assert.ok(p.kameraBox.y + p.kameraBox.h <= ZIEL.hoehe);
    }
    if (p.kameraBand) assert.ok(p.kameraBand.hoehe < ZIEL.hoehe - 2);
  }
  assert.ok(gruen > 50);
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

test('nur_gameplay mit Kameradaten ergibt einen Fehlertext', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const layout: HochkantLayout = {
    version: 1,
    modus: 'nur_gameplay',
    gameplay: anfang.gameplay,
    kamera: anfang.kamera,
    kameraBand: null,
    kameraBox: null,
  };
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('Kamera aus, aber Kameradaten gesetzt')));
});

test('Kamera oben wird abgelehnt', () => {
  const roh: HochkantLayout = {
    version: 1,
    modus: 'gestapelt',
    gameplay: { x: 0.3, y: 0, w: 0.4, h: 0.7 },
    kamera: { x: 0.2, y: 0.1, w: 0.4, h: 0.2 },
    kameraBand: { hoehe: 0.25, lage: 'oben' },
    kameraBox: null,
  };
  const layout = ausrichten(roh, ZIEL, HD);
  assert.ok(hochkantPruefen(layout, ZIEL, HD).some((f) => f.includes('Kamera oben unterstützt Uplink noch nicht')));
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

test('Kamera aus und wieder an behaelt den zuletzt bearbeiteten Kamerarahmen', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const bearbeitet: HochkantLayout = {
    ...anfang,
    kamera: { x: 0.1, y: 0.12, w: 0.25, h: 0.25 * (HD.breite / HD.hoehe) },
  };
  const merker: HochkantVorrat = { kamera: bearbeitet.kamera, kameraBand: null, kameraBox: bearbeitet.kameraBox };
  const aus = modusWechseln(bearbeitet, 'nur_gameplay', ZIEL, HD, merker);
  assert.equal(aus.kamera, null);
  const an = modusWechseln(aus, 'bild_im_bild', ZIEL, HD, merker);
  assert.ok(an.kamera);
  assert.ok(Math.abs(an.kamera.x - bearbeitet.kamera!.x) < 1e-9);
  assert.ok(Math.abs(an.kamera.y - bearbeitet.kamera!.y) < 1e-9);
  assert.ok(Math.abs(an.kamera.w - bearbeitet.kamera!.w) < 1e-9);
  assert.ok(Math.abs(an.kamera.h - bearbeitet.kamera!.h) < 1e-9);
});

test('naechsterEntwurf durch bearbeiten, speichern, Bestaetigung und zuruecksetzen', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const neu: HochkantLayout = { ...anfang, modus: 'nur_gameplay', kamera: null, kameraBand: null, kameraBox: null };

  assert.equal(naechsterEntwurf(anfang, null, neu, anfang), neu);

  const bearbeitet: HochkantLayout = { ...anfang, gameplay: { ...anfang.gameplay, x: anfang.gameplay.x + 0.05 } };
  assert.equal(naechsterEntwurf(bearbeitet, null, neu, anfang), bearbeitet);
  assert.ok(!hochkantGleich(bearbeitet, anfang));

  const bestaetigung: HochkantLayout = { ...bearbeitet, gameplay: { ...bearbeitet.gameplay } };
  assert.equal(naechsterEntwurf(bearbeitet, bearbeitet, bestaetigung, anfang), bestaetigung);

  assert.equal(naechsterEntwurf(bearbeitet, null, null, anfang), bearbeitet);
});

test('Speichern, Fehler und erneutes Speichern bleiben erlaubt, bis der Serverstand gleicht', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const bearbeitet: HochkantLayout = { ...anfang, gameplay: { ...anfang.gameplay } };
  assert.equal(speichernErlaubt(bearbeitet, null, [], false), true);
  assert.equal(speichernErlaubt(bearbeitet, null, [], false), true);
  assert.equal(speichernErlaubt(bearbeitet, bearbeitet, [], false), false);
  assert.equal(speichernErlaubt(bearbeitet, null, ['Fehler'], false), false);
  assert.equal(speichernErlaubt(bearbeitet, null, [], true), false);
});

test('standUebernehmen laesst den Entwurf stehen und laesst die Basis wandern', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const bearbeitet: HochkantLayout = { ...anfang, gameplay: { ...anfang.gameplay, x: anfang.gameplay.x + 0.05 } };
  const neuerStand: HochkantLayout = { ...anfang, modus: 'nur_gameplay', kamera: null, kameraBand: null, kameraBox: null };

  const neu = standUebernehmen(bearbeitet, null, neuerStand, anfang);
  assert.equal(neu.entwurf, bearbeitet);
  assert.equal(neu.basis, neuerStand);
  assert.equal(speichernErlaubt(neu.entwurf, neu.basis, [], false), true);

  const unberuehrt = standUebernehmen(anfang, null, neuerStand, anfang);
  assert.equal(unberuehrt.entwurf, neuerStand);
  assert.equal(unberuehrt.basis, neuerStand);
});

test('standUebernehmen setzt die Basis auf null, wenn der Stand geloescht wird', () => {
  const anfang = hochkantAnfang(HD, ZIEL);
  const bearbeitet: HochkantLayout = { ...anfang, gameplay: { ...anfang.gameplay, x: anfang.gameplay.x + 0.05 } };
  const gestrichen = standUebernehmen(bearbeitet, anfang, null, anfang);
  assert.equal(gestrichen.entwurf, bearbeitet);
  assert.equal(gestrichen.basis, null);
  assert.equal(speichernErlaubt(gestrichen.entwurf, gestrichen.basis, [], false), true);
});
