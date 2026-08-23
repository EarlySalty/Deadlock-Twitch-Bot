import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  OBS_STUFE_2K,
  OBS_STUFE_STANDARD,
  normalisiereCaps,
  obsBitrateEmpfehlung,
} from '../src/uplinkEmpfehlung';
import type { UplinkDestination } from '../src/api/uplink';

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const ZIEL_KARTE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'UplinkZiel.tsx'), 'utf8');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');
const UPLINK_API = readFileSync(join(DASHBOARD_ROOT, 'src', 'api', 'uplink.ts'), 'utf8');
// Die Hilfeseite ist auf derselben Dashboard-Seite eingebettet. Zwei
// Empfehlungen, die sich widersprechen, waren der Befund; deshalb ist sie
// hier die Belegquelle und nicht nur Deko.
const OBS_HILFE = readFileSync(
  join(DASHBOARD_ROOT, 'public', 'uplink', 'obs.html'),
  'utf8',
);

function ziel(
  platform: string,
  hoehe: number | undefined,
  enabled = true,
  bitrate_kbps = 6000,
): UplinkDestination {
  return {
    platform,
    rtmp_url: `rtmp://${platform}`,
    enabled,
    requested:
      hoehe === undefined
        ? undefined
        : { width: Math.round((hoehe * 16) / 9), height: hoehe, fps: 60, bitrate_kbps },
  };
}

test('beide Stufen stehen woertlich so in der eingebetteten OBS-Hilfeseite', () => {
  // Der eigentliche Befund war nicht die Zahl 22000, sondern dass sie der
  // Hilfeseite auf derselben Seite widersprochen hat. Dieser Test ist die
  // Klammer dagegen: eine Empfehlung, die dort nicht steht, faellt auf.
  for (const stufe of [OBS_STUFE_STANDARD, OBS_STUFE_2K]) {
    assert.match(
      OBS_HILFE,
      new RegExp(`VBR ${stufe.kbps} / max ${stufe.maxKbps}`),
      `VBR ${stufe.kbps} / max ${stufe.maxKbps} fehlt in obs.html`,
    );
  }
});

test('ohne Ziele nennt die Anleitung die Standardstufe', () => {
  for (const leer of [[], undefined]) {
    const ergebnis = obsBitrateEmpfehlung(leer);
    assert.equal(ergebnis.herkunft, 'start');
    assert.equal(ergebnis.hoehe, null);
    assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
    assert.equal(ergebnis.maxKbps, OBS_STUFE_STANDARD.maxKbps);
  }
});

test('ein fehlgeschlagener Abruf ist nicht dasselbe wie kein Ziel', () => {
  // Der Abruf laeuft mit `retry: false`. Faellt er aus, hat der Streamer
  // trotzdem Ziele, und der Text darf ihm nicht erzaehlen, er habe keine.
  const ergebnis = obsBitrateEmpfehlung([], true);
  assert.equal(ergebnis.herkunft, 'unbekannt');
  assert.equal(ergebnis.hoehe, null);
  assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
});

test('der Ladefehler schlaegt vorhandene Ziele, statt aus ihnen zu rechnen', () => {
  // Sonst stuende eine Zahl da, die aus einem halben Abruf stammt.
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 1440)], true);
  assert.equal(ergebnis.herkunft, 'unbekannt');
  assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
});

test('Ziele bis 1080p bekommen die Standardstufe', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 1080), ziel('kick', 720)]);
  assert.equal(ergebnis.herkunft, 'ziele');
  assert.equal(ergebnis.hoehe, 1080);
  assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
  assert.equal(ergebnis.maxKbps, OBS_STUFE_STANDARD.maxKbps);
});

test('geht irgendwo 2K raus, gilt die groessere Stufe', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 1080), ziel('youtube', 1440)]);
  assert.equal(ergebnis.hoehe, 1440);
  assert.equal(ergebnis.kbps, OBS_STUFE_2K.kbps);
  assert.equal(ergebnis.maxKbps, OBS_STUFE_2K.maxKbps);
});

test('keine Zielbitrate treibt die Empfehlung ueber die Hilfeseite hinaus', () => {
  // Die Vorgaengerfassung war staerkste Zielbitrate mal 1,2. Ein
  // YouTube-Ziel auf 24000 kbps ergab daraus 29000, die Fixture-Ziele 22000,
  // und kein gewoehnlicher Heimanschluss traegt das. Genau deshalb gibt es
  // Uplink: einmal hochladen statt viermal.
  for (const bitrate of [4500, 6000, 12000, 24000, 50000]) {
    for (const hoehe of [480, 720, 1080, 1440, 2160]) {
      const ergebnis = obsBitrateEmpfehlung([ziel('youtube', hoehe, true, bitrate)]);
      assert.ok(
        ergebnis.maxKbps <= OBS_STUFE_2K.maxKbps,
        `${hoehe}p bei ${bitrate} kbps ergibt ${ergebnis.maxKbps}`,
      );
    }
  }
});

test('pausierte Ziele zaehlen nicht, solange ein Ziel eingeschaltet ist', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 1080, true), ziel('youtube', 1440, false)]);
  assert.equal(ergebnis.hoehe, 1080);
  assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
});

test('sind alle Ziele pausiert, zaehlen sie trotzdem statt des Startwerts', () => {
  // Sonst stuende in der Anleitung eine Zahl aus dem Nichts, obwohl die
  // eingestellten Werte direkt danebenstehen.
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', 1440, false)]);
  assert.equal(ergebnis.herkunft, 'ziele');
  assert.equal(ergebnis.hoehe, 1440);
  assert.equal(ergebnis.kbps, OBS_STUFE_2K.kbps);
});

test('Ziele ohne eingestellte Qualitaet fallen aus der Rechnung', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', undefined), ziel('kick', 720)]);
  assert.equal(ergebnis.hoehe, 720);
  assert.equal(ergebnis.kbps, OBS_STUFE_STANDARD.kbps);
});

test('gar keine brauchbare Hoehe faellt auf den Startwert zurueck', () => {
  const ergebnis = obsBitrateEmpfehlung([ziel('twitch', undefined)]);
  assert.equal(ergebnis.herkunft, 'start');
  assert.equal(ergebnis.hoehe, null);
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

test('die Anleitung reicht den Ladefehler an die Empfehlung durch', () => {
  // Ohne das zweite Argument ist ein ausgefallener Abruf von einem leeren
  // Konto nicht zu unterscheiden, und genau das war der Befund.
  assert.match(UPLINK_PAGE, /obsBitrateEmpfehlung\(gespeicherteZiele, zieleFehler\)/);
});

test('die Anleitung sagt beim Ladefehler, dass die Zahl nicht passen muss', () => {
  assert.match(UPLINK_PAGE, /Deine Ziele konnten wir gerade nicht laden/);
});

test('kein Kommentar verspricht einen Ersatzwert, den es nicht gibt', () => {
  // Der Ingest-Deckel als Rueckfallwert ist mit der Klemmung weggefallen.
  assert.doesNotMatch(UPLINK_PAGE, /Ingest-Deckel zurueck/);
  assert.doesNotMatch(UPLINK_PAGE, /INGEST_FALLBACK/);
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
