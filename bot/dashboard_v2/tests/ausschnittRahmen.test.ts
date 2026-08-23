import { test } from 'node:test';
import assert from 'node:assert/strict';

import { ausschnittRahmen } from '../src/utils/socialMediaLayout';

const QUELLE = { width: 1920, height: 1080 };

/**
 * Die Vorschau im Layout-Editor zeigt denselben Ausschnitt, den ffmpeg spaeter
 * rendert. Stimmt die Rechnung nicht, stellt der Streamer gegen ein falsches
 * Bild ein und merkt es erst am fertigen Video.
 */
test('Ausschnitt gleicher Form fuellt das Ziel genau', () => {
  // 540x960 aus dem Quellbild in einen 1080x1920-Rahmen: exakt Faktor 2.
  const r = ausschnittRahmen(QUELLE, { x: 0, y: 0, w: 540, h: 960 }, 1080, 1920)!;
  assert.equal(Math.round(r.breite), 356); // 1920*2 / 1080
  assert.equal(Math.round(r.hoehe), 113); // 1080*2 / 1920
  assert.equal(Math.round(r.links), 0);
  assert.equal(Math.round(r.oben), 0);
});

test('Versatz des Ausschnitts verschiebt das Bild gegenlaeufig', () => {
  const links = ausschnittRahmen(QUELLE, { x: 0, y: 0, w: 540, h: 960 }, 1080, 1920)!;
  const rechts = ausschnittRahmen(QUELLE, { x: 200, y: 0, w: 540, h: 960 }, 1080, 1920)!;
  // Der Ausschnitt wandert nach rechts, also muss das Bild nach links rutschen.
  assert.ok(rechts.links < links.links, `${rechts.links} nicht kleiner als ${links.links}`);
  // 200 Quellpixel bei Faktor 2 sind 400 Zielpixel, das sind 37,04% von 1080.
  assert.equal(Math.round((links.links - rechts.links) * 100) / 100, 37.04);
});

test('Breiterer Ausschnitt als das Ziel wird mittig nachgeschnitten', () => {
  // 16:9-Ausschnitt in ein 9:16-Ziel: ffmpeg skaliert auf die Hoehe und
  // schneidet links und rechts gleich viel weg. Also bleibt oben 0 und das
  // Bild ragt symmetrisch ueber die Seiten hinaus.
  const r = ausschnittRahmen(QUELLE, { x: 0, y: 0, w: 1920, h: 1080 }, 1080, 1920)!;
  assert.equal(Math.round(r.oben), 0);
  const ueberstand = r.breite - 100;
  assert.ok(ueberstand > 0, 'kein Ueberstand, es wurde nicht auf die Hoehe skaliert');
  assert.equal(Math.round(r.links * 100) / 100, Math.round((-ueberstand / 2) * 100) / 100);
});

test('Ausschnitt ohne Flaeche liefert nichts statt Division durch null', () => {
  assert.equal(ausschnittRahmen(QUELLE, { x: 0, y: 0, w: 0, h: 100 }, 1080, 1920), null);
  assert.equal(ausschnittRahmen(QUELLE, { x: 0, y: 0, w: 100, h: 100 }, 0, 1920), null);
});
