import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  OBS_BITRATE_START,
  normalisiereCaps,
  obsBitrateEmpfehlung,
} from '../src/uplinkEmpfehlung';
import type { UplinkDestination } from '../src/api/uplink';

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const ZIEL_KARTE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'UplinkZiel.tsx'), 'utf8');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');
const UPLINK_API = readFileSync(join(DASHBOARD_ROOT, 'src', 'api', 'uplink.ts'), 'utf8');

function ziel(
  platform: string,
  bitrate_kbps: number | undefined,
  enabled = true,
): UplinkDestination {
  return {
    platform,
    rtmp_url: `rtmp://${platform}`,
    enabled,
    requested:
      bitrate_kbps === undefined
        ? undefined
        : { width: 1920, height: 1080, fps: 60, bitrate_kbps },
  };
}

test('ohne Ziele nennt die Anleitung einen Startwert statt einer leeren Zahl', () => {
  assert.equal(obsBitrateEmpfehlung([]).kbps, OBS_BITRATE_START);
  assert.equal(obsBitrateEmpfehlung(undefined).kbps, OBS_BITRATE_START);
  assert.equal(obsBitrateEmpfehlung([]).staerkstesZiel, null);
  // Der Startwert ist die Standardstufe plus Reserve, keine ausgedachte Zahl.
  assert.equal(OBS_BITRATE_START, 7500);
});

test('die Empfehlung nimmt das staerkste eingeschaltete Ziel plus rund 20 Prozent', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 6000), ziel('youtube', 8000)]);
  // 8000 plus 20 Prozent sind 9600, aufgerundet auf volle 500 also 10000.
  assert.equal(ergebnis.kbps, 10000);
  assert.equal(ergebnis.staerkstesZiel, 8000);
});

test('die Empfehlung ist immer ein Vielfaches von 500', () => {
  for (const bitrate of [1500, 4500, 6000, 8000, 12000, 16000, 18000, 24000]) {
    const kbps = obsBitrateEmpfehlung([ziel('twitch', bitrate)]).kbps;
    assert.equal(kbps % 500, 0, `${bitrate} ergibt ${kbps}`);
    assert.ok(kbps >= bitrate * 1.2, `${kbps} liegt unter der Reserve auf ${bitrate}`);
    assert.ok(kbps < bitrate * 1.2 + 500, `${kbps} rundet zu weit ueber ${bitrate}`);
  }
});

test('ein glatter Wert bekommt keine zusaetzliche Stufe aufgeschlagen', () => {
  // 5000 plus 20 Prozent sind exakt 6000, das ist schon ein Vielfaches von 500.
  assert.equal(obsBitrateEmpfehlung([ziel('twitch', 5000)]).kbps, 6000);
});

test('pausierte Ziele zaehlen nicht, solange ein Ziel eingeschaltet ist', () => {
  const ergebnis = obsBitrateEmpfehlung([
    ziel('twitch', 6000, true),
    ziel('youtube', 24000, false),
  ]);
  assert.equal(ergebnis.staerkstesZiel, 6000);
  assert.equal(ergebnis.kbps, 7500);
});

test('sind alle Ziele pausiert, zaehlen sie trotzdem statt des Startwerts', () => {
  // Sonst stuende in der Anleitung eine Zahl aus dem Nichts, obwohl die
  // eingestellten Werte direkt danebenstehen.
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 12000, false)]);
  assert.equal(ergebnis.staerkstesZiel, 12000);
  assert.equal(ergebnis.kbps, 14500);
});

test('Ziele ohne eingestellte Qualitaet fallen aus der Rechnung', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', undefined), ziel('kick', 4500)]);
  assert.equal(ergebnis.staerkstesZiel, 4500);
  assert.equal(ergebnis.kbps, 5500);
});

test('normalisiereCaps liest die neuen Empfehlungsfelder', () => {
  const caps = normalisiereCaps({
    platform: 'twitch',
    recommended_width: 2560,
    recommended_height: 1440,
    recommended_fps: 60,
    recommended_bitrate_kbps: 12000,
    force_cbr: true,
  });
  assert.deepEqual(caps, {
    platform: 'twitch',
    recommended_width: 2560,
    recommended_height: 1440,
    recommended_fps: 60,
    recommended_bitrate_kbps: 12000,
    force_cbr: true,
  });
});

test('normalisiereCaps versteht auch die alten max-Felder', () => {
  // Dashboard und Relay werden nicht in derselben Sekunde ausgerollt. Ohne
  // diesen Zweig stuenden dazwischen gar keine Empfehlungen an den Feldern.
  const caps = normalisiereCaps({
    platform: 'kick',
    max_width: 1920,
    max_height: 1080,
    max_fps: 60,
    max_bitrate_kbps: 8000,
    force_cbr: true,
  });
  assert.equal(caps.recommended_width, 1920);
  assert.equal(caps.recommended_bitrate_kbps, 8000);
  assert.equal(caps.force_cbr, true);
});

test('fehlende oder unbrauchbare Werte werden zu null statt zu 0', () => {
  const caps = normalisiereCaps({ platform: 'tiktok', recommended_bitrate_kbps: 0 });
  assert.equal(caps.recommended_width, null);
  assert.equal(caps.recommended_bitrate_kbps, null);
  assert.equal(caps.force_cbr, false);
});

test('die Oberflaeche droht nirgends mehr mit Herunterrechnen', () => {
  for (const [datei, inhalt] of [
    ['UplinkZiel.tsx', ZIEL_KARTE],
    ['Uplink.tsx', UPLINK_PAGE],
  ] as const) {
    // Kommentare duerfen die alte Mechanik erklaeren, der sichtbare Text nicht.
    const sichtbar = inhalt.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
    assert.doesNotMatch(sichtbar, /rechnen wir .*herunter/, datei);
    assert.doesNotMatch(sichtbar, /geht 1:1 raus/, datei);
    assert.doesNotMatch(sichtbar, /nehmen wir nicht an/, datei);
    assert.doesNotMatch(sichtbar, /nehmen wir nicht entgegen/, datei);
  }
});

test('die Zielkarte zeigt keine geklemmten Werte mehr an', () => {
  // Kopfzeile, Bestaetigungssatz und Eingabefeld muessen denselben Wert zeigen.
  // `effective` war die einzige Quelle, aus der ein abweichender Wert kam.
  assert.doesNotMatch(ZIEL_KARTE, /ziel[?.]*\.effective\./);
  assert.doesNotMatch(ZIEL_KARTE, /\{ziel\?\.effective/);
});

test('die OBS-Anleitung nennt keine feste Bitrate-Spanne mehr', () => {
  assert.doesNotMatch(UPLINK_PAGE, /15000 bis 25000/);
  assert.doesNotMatch(UPLINK_PAGE, /20000 kbps/);
  assert.match(UPLINK_PAGE, /obsBitrateEmpfehlung/);
});

/**
 * Schneidet den Warntext der 1440p-Stufe aus `api/uplink.ts`.
 *
 * Die Tests darunter pruefen alle denselben Satz, und ein `assert.match` gegen
 * die ganze Datei wuerde auch anschlagen, wenn die Stelle in einem Kommentar
 * oder in einer anderen Stufe steht.
 */
function warnung1440(): string {
  const stufe = UPLINK_API.slice(UPLINK_API.indexOf("name: '1440p60'"));
  const start = stufe.indexOf('warnung:');
  return stufe.slice(start, stufe.indexOf("',", start));
}

test('der 2K-Hinweis nennt Enhanced Broadcasting und seine Folgen', () => {
  const warnung = warnung1440();
  assert.match(warnung, /Enhanced Broadcasting/);
  // Die Folge fuer die Zuschauer ist der Teil, den man beim Ueberfliegen
  // uebersieht und der hinterher im Chat steht.
  assert.match(warnung, /Qualitätsstufen/);
  assert.match(warnung, /puffert/);
});

test('die 1440p-Warnung nennt keine Bitrate', () => {
  // Hier stand einmal "die 20 Mbit/s, die Twitch dafuer nennt, senden wir".
  // Die Zahl stammt aus Twitchs Hilfeartikel und gilt fuer den direkten Weg
  // an Twitch. Ueber Uplink gehen 12000 kbps raus, und was der Streamer zu
  // uns hochlaedt, ist noch einmal etwas anderes. Keine der drei Zahlen
  // gehoert in diese Warnung, deshalb steht dort gar keine.
  // Aufloesungen wie "1440p" und "720p" duerfen bleiben, die sind belegt.
  // Gemeint ist jede Datenrate.
  const warnung = warnung1440();
  assert.doesNotMatch(warnung, /Mbit|Mbps|kbps|Kbps|kBit|MBit/);
});

test('die 1440p-Warnung bleibt lesbar kurz und wird nicht ausgehoehlt', () => {
  // Eine Warnung, die niemand zu Ende liest, warnt nicht. Die erste Fassung
  // hatte acht Saetze und fuehrte jede Einzelheit aus dem Twitch-Hilfeartikel
  // auf, auch die Partner-und-Affiliate-Schranke, die nur fuer Enhanced
  // Broadcasting gilt und damit fuer einen Weg, den wir gar nicht gehen.
  // Die Untergrenze haelt die Gegenrichtung offen: der Satz darf beim Kuerzen
  // nicht auf einen Halbsatz zusammenfallen, der nichts mehr erklaert.
  const warnung = warnung1440();
  assert.ok(warnung.length < 500, `1440p-Warnung ist ${warnung.length} Zeichen lang`);
  assert.ok(warnung.length > 200, `1440p-Warnung ist nur ${warnung.length} Zeichen lang`);
});

test('kein Em-Dash im Uplink-Text', () => {
  for (const [datei, inhalt] of [
    ['UplinkZiel.tsx', ZIEL_KARTE],
    ['Uplink.tsx', UPLINK_PAGE],
    ['api/uplink.ts', UPLINK_API],
  ] as const) {
    assert.doesNotMatch(inhalt, /[—–]/, datei);
  }
});
