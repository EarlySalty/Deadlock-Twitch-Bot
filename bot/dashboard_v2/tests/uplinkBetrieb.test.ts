import assert from 'node:assert/strict';
import test from 'node:test';
import { eingangStatus, obsZugang, zielBetrieb } from '../src/uplinkBetrieb';
const browserGlobal = globalThis as typeof globalThis & { window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> } };
browserGlobal.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { plattformVerbindungen } = await import('../src/api/uplink');

test('eingeschaltete Wünsche sind kein Nachweis für laufende Medien', () => {
  const stand = zielBetrieb({ enabled: true });
  assert.equal(stand.state, 'unknown');
  assert.notEqual(stand.tone, 'success');
  assert.equal(stand.activeProfile, null);
});

test('Zugang erneuern bleibt bei eingeschaltetem Ziel sichtbar', () => {
  const stand = zielBetrieb({ enabled: true }, 'neu_verbinden');
  assert.equal(stand.label, 'Zugang erneuern');
  assert.equal(stand.tone, 'warning');
});

test('lokaler Medienfluss behauptet keine öffentliche Plattform-Livebestätigung', () => {
  const stand = zielBetrieb({ enabled: true, output_state: 'sending', publication_confirmed: false });
  assert.equal(stand.label, 'Medien werden gesendet');
  assert.notEqual(stand.tone, 'success');
});

test('fehlendes Chatrecht löscht einen nachgewiesenen Medienfluss nicht', () => {
  const stand = zielBetrieb({ enabled: true, output_state: 'sending' }, 'neu_verbinden');
  assert.equal(stand.state, 'sending');
  assert.equal(stand.tone, 'warning');
  assert.match(stand.reason ?? '', /Zugang/);
});

test('fehlgeschlagene oder beendete Ausgabe zeigt kein aktives Profil', () => {
  for (const output_state of ['failed', 'finished'] as const) {
    const stand = zielBetrieb({ enabled: true, output_state, reason: 'VOD-Ton fehlt.',
      active_profile: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 26000 } });
    assert.equal(stand.activeProfile, null);
    assert.notEqual(stand.tone, 'success');
  }
});

test('gesperrte Zieladresse hat Vorrang vor älterem Sendestatus', () => {
  const stand = zielBetrieb({ enabled: true, blocked: true, output_state: 'sending',
    error: 'Zieladresse ist gesperrt.' });
  assert.equal(stand.state, 'failed');
  assert.equal(stand.reason, 'Zieladresse ist gesperrt.');
});

test('OBS erhält öffentlichen RTMPS-Server und privaten Schlüssel getrennt', () => {
  assert.deepEqual(obsZugang({ service_status: 'ready', public_ingest_url: 'rtmps://uplink.example/live', ingest_key: 'private-test-key' }),
    { server: 'rtmps://uplink.example/live', key: 'private-test-key' });
  for (const public_ingest_url of ['srt://uplink.example:9000?streamid=secret',
    'rtmp://uplink.example/live', 'rtmps://uplink.example/live/private-test-key',
    'rtmps://uplink.example/live?key=secret', 'rtmps://secret@uplink.example/live']) {
    assert.equal(obsZugang({ service_status: 'ready', public_ingest_url, ingest_key: 'private-test-key' }), null);
  }
  assert.equal(obsZugang({ service_status: 'ready', public_ingest_url: 'rtmps://uplink.example/live' }), null);
});

test('Ein Eingang ohne vollständigen Dienst wird nicht als OBS-Einrichtung freigegeben', () => {
  for (const service_status of ['input_only', 'unavailable', undefined]) {
    assert.equal(obsZugang({ service_status, public_ingest_url: 'rtmps://uplink.example/live', ingest_key: 'private-test-key' }), null);
  }
});

test('offene Trennung bleibt sichtbar und über denselben Trennen-Weg wiederholbar', () => {
  const [verbindung] = plattformVerbindungen({ verbindungen: [{ platform: 'twitch', status: 'trennung_offen' }] } as Parameters<typeof plattformVerbindungen>[0]);
  assert.equal(verbindung.statusText, 'Trennung noch nicht bestätigt');
  assert.equal(verbindung.trennenMoeglich, true);
  assert.equal(zielBetrieb({ enabled: true, output_state: 'sending' }, 'trennung_offen').label, 'Trennung noch nicht bestätigt');
  assert.equal(zielBetrieb(undefined, 'trennung_offen').label, 'Trennung noch nicht bestätigt');
});

test('Eingang unterscheidet empfangene Medien, vergangene Session und unbekannten Abruf', () => {
  const session = { active: true, state: 'running', received_events: 12, received_bytes: 1024, error: null };
  assert.equal(eingangStatus(session, false).label, 'Stream wird empfangen');
  assert.equal(eingangStatus({ ...session, active: false }, false).label, 'Eingang beendet');
  assert.equal(eingangStatus(session, true).label, 'Eingangsstatus unbekannt');
  assert.equal(eingangStatus({ ...session, received_events: 0 }, false).label, 'Verbindung aufgebaut, Medien werden erwartet');
  assert.equal(eingangStatus(null, false).label, 'Noch kein Stream empfangen');
});
