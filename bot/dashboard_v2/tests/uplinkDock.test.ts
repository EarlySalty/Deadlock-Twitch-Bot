import assert from 'node:assert/strict';
import test from 'node:test';

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

const BASIS: UplinkMe = {
  enabled: true,
  waitlisted: false,
  ingest_key: 'rsr_test',
  rtmp_url: '',
  srt_hint: '',
  twitch_login: 'earlysalty',
  reconnect_wait_s: 90,
  reconnect_wait_max_s: 300,
};

const VIER: DockUrls = {
  chat: 'https://relay.test/dock/chat?t=abc',
  activity: 'https://relay.test/dock/activity?t=abc',
  stream_info: 'https://relay.test/dock/stream-info?t=abc',
  points: 'https://relay.test/dock/points?t=abc',
};

test('dockAdressen_liefert_vier_eigene_in_fester_reihenfolge', () => {
  const liste = dockAdressen({ ...BASIS, dock_url_vorhanden: true }, VIER);
  assert.deepEqual(
    liste.slice(0, 4).map((d) => [d.titel, d.url, d.eigene]),
    [
      ['Chat', VIER.chat, true],
      ['Aktivität', VIER.activity, true],
      ['Stream-Infos', VIER.stream_info, true],
      ['Kanalpunkte', VIER.points, true],
    ],
  );
  // Danach die vier Twitch-Fenster, unveraendert als Zusatz.
  assert.equal(liste.length, 8);
  assert.ok(liste.slice(4).every((d) => !d.eigene));
  assert.equal(liste[4].url, 'https://www.twitch.tv/popout/earlysalty/chat?darkpopout');
});

test('ohne_dock_urls_faellt_auf_dock_url_zurueck', () => {
  // Aeltere Server liefern nur die eine Adresse. Dann gilt sie als
  // Chat-Fenster; die drei anderen fehlen, statt ins Leere zu zeigen.
  const liste = dockAdressen({
    ...BASIS,
    dock_url: 'https://relay.test/dock/chat?t=abc',
    dock_url_vorhanden: true,
  });
  assert.equal(liste[0].eigene, true);
  assert.equal(liste[0].titel, 'Chat');
  assert.equal(liste[0].url, 'https://relay.test/dock/chat?t=abc');
  assert.equal(liste.length, 5);
  assert.ok(liste.slice(1).every((d) => !d.eigene));
});

test('dockAdressen_ohne_dock_url_liefert_nur_twitch_popouts', () => {
  const liste = dockAdressen({ ...BASIS, dock_url_vorhanden: false });
  assert.equal(liste.length, 4);
  assert.ok(liste.every((d) => !d.eigene));
  assert.ok(liste.every((d) => d.url.includes('twitch.tv')));

  const leer = dockAdressen({ ...BASIS, dock_url: '   ' });
  assert.ok(leer.every((d) => !d.eigene));

  const ohneLogin = dockAdressen({ ...BASIS, twitch_login: undefined });
  assert.equal(ohneLogin.length, 3);
  assert.ok(ohneLogin.every((d) => d.titel !== 'Chat'));

  // Eine leere Adresse im Viererpaket faellt weg, statt eine Kopierzeile
  // ohne Ziel anzubieten.
  const halb = dockAdressen({ ...BASIS }, { ...VIER, activity: '', points: '  ' });
  assert.deepEqual(
    halb.filter((d) => d.eigene).map((d) => d.titel),
    ['Chat', 'Stream-Infos'],
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
