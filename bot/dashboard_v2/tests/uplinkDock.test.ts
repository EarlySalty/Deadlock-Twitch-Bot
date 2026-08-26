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

test('dockAdressen_setzt_eigene_adresse_zuerst', () => {
  const liste = dockAdressen({
    ...BASIS,
    dock_url: 'https://relay.test/dock/chat?t=abc',
    dock_url_vorhanden: true,
  });
  assert.equal(liste[0].eigene, true);
  assert.equal(liste[0].url, 'https://relay.test/dock/chat?t=abc');
  assert.equal(liste.length, 5);
  assert.deepEqual(
    liste.slice(1).map((d) => d.eigene),
    [false, false, false, false],
  );
  assert.equal(liste[1].url, 'https://www.twitch.tv/popout/earlysalty/chat?darkpopout');
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
});

test('verbindenButton_nur_fuer_twitch_aktiv', () => {
  assert.equal(verbindenAktiv('twitch'), true);
  assert.equal(verbindenAktiv('kick'), false);
  assert.equal(verbindenAktiv('youtube'), false);
  assert.equal(verbindenAktiv('tiktok'), false);

  const zeilen = plattformVerbindungen({
    ...BASIS,
    verbindungen: [
      { platform: 'twitch', status: 'verbunden' },
      { platform: 'kick', status: 'getrennt' },
    ],
  });
  assert.equal(zeilen.length, 4);
  assert.equal(zeilen[0].id, 'twitch');
  assert.equal(zeilen[0].aktiv, true);
  assert.equal(zeilen[0].status, 'verbunden');
  assert.equal(zeilen[0].statusText, 'Chat verbunden');
  assert.equal(zeilen[0].knopfText, 'Chat neu verbinden');
  assert.ok(zeilen.slice(1).every((z) => !z.aktiv));
  assert.ok(zeilen.slice(1).every((z) => z.status === 'getrennt'));
  assert.ok(zeilen.slice(1).every((z) => z.statusText === 'Chat folgt'));
  assert.ok(zeilen.slice(1).every((z) => z.knopfText === null));

  const neu = plattformVerbindungen({
    ...BASIS,
    verbindungen: [{ platform: 'twitch', status: 'neu_verbinden' }],
  });
  assert.equal(neu[0].statusText, 'Chat abgelaufen');
  assert.equal(neu[0].knopfText, 'Chat neu verbinden');
  assert.equal(plattformVerbindungen(BASIS)[0].statusText, 'Chat nicht verbunden');
  assert.equal(plattformVerbindungen(BASIS)[0].knopfText, 'Chat von Twitch verbinden');

  assert.equal(uplinkConnectUrl('twitch'), '/twitch/api/v2/uplink/connect/twitch');
});
