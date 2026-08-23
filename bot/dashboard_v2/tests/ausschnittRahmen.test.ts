import { test } from 'node:test';
import assert from 'node:assert/strict';

import { ausschnittRahmen, grossesStandbild } from '../src/utils/socialMediaLayout';

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

/**
 * Faellt die Regel auf eine fremde URL nicht an, laedt der Editor still das
 * kleine, unscharfe Bild. Das ist kein Fehler, muss aber die Eingabe
 * unveraendert zurueckgeben, sonst zeigt der Rahmen eine tote Grafik.
 */
test('grossesStandbild hebt Twitch-Standbilder auf 1920x1080', () => {
  const klein =
    'https://static-cdn.jtvnw.net/twitch-video-assets/prod/abc/landscape/thumb/thumb-0000000000-480x272.jpg';
  assert.equal(
    grossesStandbild(klein),
    'https://static-cdn.jtvnw.net/twitch-video-assets/prod/abc/landscape/thumb/thumb-0000000000-1920x1080.jpg',
  );
  assert.equal(
    grossesStandbild('https://clips-media-assets2.twitch.tv/AT-cm%7Cx-preview-480x272.jpg'),
    'https://clips-media-assets2.twitch.tv/AT-cm%7Cx-preview-1920x1080.jpg',
  );
});

test('grossesStandbild laesst URLs ohne Groessensuffix unveraendert', () => {
  for (const url of [
    '/social-media/api/clips/12/thumb.jpg',
    'https://beispiel.de/bild.png?w=480x272',
    'https://beispiel.de/1920x1080/bild.webp',
    'https://beispiel.de/bild.jpg',
  ]) {
    assert.equal(grossesStandbild(url), url, `unerwartet umgeschrieben: ${url}`);
  }
});
