import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// `uplink.ts` zieht die Laufzeitkonfiguration nach, die `window` erwartet.
// Wie in uplinkReconnectWait.test.ts: Fenster stubben, dann erst laden.
const globalState = globalThis as typeof globalThis & {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { dockAdressen, plattformVerbindungen, uplinkConnectUrl, verbindenAktiv } = await import(
  '../src/api/uplink'
);
type DockUrls = import('../src/api/uplink').DockUrls;
type UplinkMe = import('../src/api/uplink').UplinkMe;

const WURZEL = join(import.meta.dirname, '..');
const UPLINK_API = readFileSync(join(WURZEL, 'src/api/uplink.ts'), 'utf8');
const UPLINK_SEITE = readFileSync(join(WURZEL, 'src/pages/Uplink.tsx'), 'utf8');
const OBS_HILFE = readFileSync(join(WURZEL, 'public/uplink/obs.html'), 'utf8');
const OBS_WISSEN = readFileSync(join(WURZEL, '../../rust/knowledge/bot/uplink-obs.md'), 'utf8');

const BASIS: UplinkMe = {
  enabled: true,
  waitlisted: false,
  ingest_key: 'rsr_test',
  rtmp_url: '',
  srt_hint: '',
  reconnect_wait_s: 90,
  reconnect_wait_max_s: 300,
};

const VIER: DockUrls = {
  chat: 'https://relay.test/dock/chat?t=abc',
  activity: 'https://relay.test/dock/activity?t=abc',
  stream_info: 'https://relay.test/dock/stream-info?t=abc',
  points: 'https://relay.test/dock/points?t=abc',
};

test('dockAdressen_liefert_genau_vier_eigene', () => {
  // Vier Fenster, feste Reihenfolge, die Namen wie sie in OBS eingetragen
  // werden. Nichts anderes steht in der Liste.
  const liste = dockAdressen({ ...BASIS, dock_url_vorhanden: true, dock_urls: VIER });
  assert.equal(liste.length, 4);
  assert.deepEqual(
    liste.map((d) => [d.titel, d.url]),
    [
      ['Chat', VIER.chat],
      ['Aktivität', VIER.activity],
      ['Stream-Infos', VIER.stream_info],
      ['Kanalpunkte', VIER.points],
    ],
  );

  // Die Adressen kommen jetzt dauerhaft aus `me`, nicht mehr nur einmalig aus
  // der Antwort des Erzeugens. Genau das war der Fehler vorher: nach einem
  // Neuladen der Seite war die Karte leer.
  const nurAusMe = dockAdressen({ ...BASIS, dock_urls: VIER });
  assert.equal(nurAusMe.length, 4);
  assert.equal(nurAusMe[0].url, VIER.chat);

  // Ohne Adressen bleibt die Liste leer, statt auf Platzhalter zu zeigen.
  assert.deepEqual(dockAdressen(BASIS), []);
  assert.deepEqual(dockAdressen({ ...BASIS, dock_urls: null, dock_url_vorhanden: true }), []);

  // Eine leere Adresse faellt weg, statt eine Kopierzeile ohne Ziel anzubieten.
  const halb = dockAdressen({ ...BASIS, dock_urls: { ...VIER, activity: '', points: '  ' } });
  assert.deepEqual(
    halb.map((d) => d.titel),
    ['Chat', 'Stream-Infos'],
  );
});

test('keine_twitch_popouts_mehr', () => {
  // Die fertigen Twitch-Fenster sind ersatzlos weg: sie zeigten nur Twitch und
  // brauchten eine eigene Anmeldung im OBS-Browser.
  const liste = dockAdressen({ ...BASIS, dock_urls: VIER });
  assert.ok(liste.every((d) => !d.url.includes('twitch.tv')));

  for (const [name, quelle] of [
    ['src/api/uplink.ts', UPLINK_API],
    ['src/pages/Uplink.tsx', UPLINK_SEITE],
    ['public/uplink/obs.html', OBS_HILFE],
  ] as const) {
    assert.ok(!quelle.includes('twitch.tv/popout'), `${name} nennt noch ein Twitch-Popout`);
    assert.ok(
      !quelle.includes('popout/stream-manager'),
      `${name} nennt noch ein Twitch-Popout`,
    );
  }
  assert.ok(!UPLINK_API.includes('OBS_DOCKS'), 'OBS_DOCKS lebt noch');
  assert.ok(
    !UPLINK_SEITE.includes('Einmal bei Twitch anmelden'),
    'der Anmelde-Satz steht noch in der Karte',
  );
});

test('verbindenButton_nur_fuer_twitch_aktiv', () => {
  assert.equal(verbindenAktiv('twitch'), true);
  assert.equal(verbindenAktiv('kick'), false);
  assert.equal(verbindenAktiv('youtube'), false);
  assert.equal(verbindenAktiv('tiktok'), false);

  const zeilen = plattformVerbindungen({
    ...BASIS,
    verbindungen: [
      { platform: 'twitch', status: 'verbunden', stream_key_vorhanden: true },
      { platform: 'kick', status: 'getrennt' },
    ],
  });
  assert.equal(zeilen.length, 4);
  assert.equal(zeilen[0].id, 'twitch');
  assert.equal(zeilen[0].aktiv, true);
  assert.equal(zeilen[0].status, 'verbunden');
  assert.equal(zeilen[0].statusText, 'Verbunden');
  assert.equal(zeilen[0].knopfText, 'Neu verbinden');
  assert.equal(zeilen[0].streamKeyVorhanden, true);
  assert.equal(zeilen[0].trennenMoeglich, true);

  // Der Grant steht, aber im Uplink liegt kein Schluessel: es geht noch kein
  // Bild raus. Ein blankes "Verbunden" waere hier die Falschaussage, die den
  // Streamer am Sendetag suchen laesst.
  const ohneKey = plattformVerbindungen({
    ...BASIS,
    verbindungen: [{ platform: 'twitch', status: 'verbunden', stream_key_vorhanden: false }],
  });
  assert.equal(ohneKey[0].statusText, 'Verbunden, Schlüssel fehlt');
  assert.equal(ohneKey[0].streamKeyVorhanden, false);
  assert.equal(ohneKey[0].trennenMoeglich, true);
  assert.ok(zeilen.slice(1).every((z) => !z.aktiv));
  assert.ok(zeilen.slice(1).every((z) => z.status === 'getrennt'));
  assert.ok(zeilen.slice(1).every((z) => z.statusText === 'Folgt später'));
  assert.ok(zeilen.slice(1).every((z) => z.knopfText === null));
  assert.ok(zeilen.slice(1).every((z) => !z.trennenMoeglich));

  const neu = plattformVerbindungen({
    ...BASIS,
    verbindungen: [{ platform: 'twitch', status: 'neu_verbinden' }],
  });
  assert.equal(neu[0].statusText, 'Zugang abgelaufen');
  // Status und Knopf sagen Verschiedenes: Zustand und Handlung.
  assert.notEqual(neu[0].statusText, neu[0].knopfText);
  assert.equal(neu[0].knopfText, 'Neu verbinden');
  // Ein abgelaufener Zugang laesst sich trennen: die Tokens liegen noch da.
  assert.equal(neu[0].trennenMoeglich, true);
  assert.equal(neu[0].streamKeyVorhanden, false);

  assert.equal(plattformVerbindungen(BASIS)[0].statusText, 'Nicht verbunden');
  assert.equal(plattformVerbindungen(BASIS)[0].knopfText, 'Mit Twitch verbinden');
  // Was nie verbunden war, hat auch nichts zu trennen.
  assert.equal(plattformVerbindungen(BASIS)[0].trennenMoeglich, false);

  // Verbinden laeuft ueber den bestehenden Streamer-OAuth mit dem
  // Uplink-Scope-Profil; einen eigenen Connect-Pfad gibt es nicht mehr.
  assert.equal(uplinkConnectUrl('twitch'), '/twitch/raid/auth?scope_profile=uplink');
  assert.equal(uplinkConnectUrl('kick'), '');
});

test('hilfe_und_wissensbasis_zeigen_auf_schritt_5', () => {
  // Beide Fassungen schicken den Streamer an die Stelle, an der die Adressen
  // wirklich stehen. Zeigen sie auf eine Karte, die es nicht mehr gibt, sucht
  // er im Dashboard nach etwas, das dort nie auftaucht.
  for (const [name, quelle] of [
    ['public/uplink/obs.html', OBS_HILFE],
    ['rust/knowledge/bot/uplink-obs.md', OBS_WISSEN],
    ['src/pages/Uplink.tsx', UPLINK_SEITE],
  ] as const) {
    assert.ok(
      !quelle.includes('Chat und OBS-Fenster'),
      `${name} nennt noch die abgeschaffte Karte`,
    );
  }
  for (const [name, quelle] of [
    ['public/uplink/obs.html', OBS_HILFE],
    ['rust/knowledge/bot/uplink-obs.md', OBS_WISSEN],
  ] as const) {
    assert.ok(quelle.includes('Fenster einrichten'), `${name} nennt Schritt 5 nicht beim Namen`);
    assert.ok(quelle.includes('Schritt 5'), `${name} nennt die Schrittnummer nicht`);
  }
});
