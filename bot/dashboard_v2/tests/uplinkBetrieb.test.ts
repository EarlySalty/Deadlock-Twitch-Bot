import assert from 'node:assert/strict';
import test from 'node:test';
import { eingangStatus, obsZugang, zielBetrieb } from '../src/uplinkBetrieb';
const browserGlobal = globalThis as typeof globalThis & { window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> } };
browserGlobal.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { plattformVerbindungen, saveUplinkDestination, twitchAudioFormular } = await import('../src/api/uplink');
import type { UplinkDestination } from '../src/api/uplink';

function twitchZiel(felder: Partial<UplinkDestination> = {}): UplinkDestination {
  return { platform: 'twitch', rtmp_url: 'rtmps://example.test/app', enabled: true,
    twitch_audio_mode: null, effective_audio_mode: 'separate_vod', active_audio_mode: null, ...felder };
}

test('Twitch-Altbestand bleibt ohne ausdrückliche Audiowahl unverändert', () => {
  assert.deepEqual(twitchAudioFormular(twitchZiel(), null), {
    auswahl: null, gespeichert: null, naechsterStream: 'separate_vod', aktiv: null, geaendert: false,
  });
  assert.deepEqual(twitchAudioFormular(undefined, null), {
    auswahl: null, gespeichert: null, naechsterStream: null, aktiv: null, geaendert: false,
  });
});

test('Audioentwurf übersteht einen Poll und eine verspätete Bestätigung des vorherigen Stands', () => {
  const alterStand = twitchZiel({ twitch_audio_mode: 'live', effective_audio_mode: 'live',
    active_audio_mode: 'live', output_state: 'sending' });
  assert.deepEqual(twitchAudioFormular(alterStand, 'separate_vod'), {
    auswahl: 'separate_vod', gespeichert: 'live', naechsterStream: 'live', aktiv: 'live', geaendert: true,
  });
  const neuerStand = { ...alterStand, twitch_audio_mode: 'separate_vod' as const,
    effective_audio_mode: 'separate_vod' as const };
  assert.deepEqual(twitchAudioFormular(neuerStand, 'separate_vod'), {
    auswahl: 'separate_vod', gespeichert: 'separate_vod', naechsterStream: 'separate_vod', aktiv: 'live', geaendert: false,
  });
  assert.equal(twitchAudioFormular(neuerStand, null).auswahl, 'separate_vod', 'Neuladen zeigt die gespeicherte Wahl');
});

test('angehaltene oder unbekannte Twitch-Ausgabe zeigt keinen aktiven Ton', () => {
  for (const output_state of ['finished', 'failed', 'starting', undefined]) {
    assert.equal(twitchAudioFormular(twitchZiel({ output_state, active_audio_mode: 'live' }), null).aktiv, null);
  }
  assert.equal(twitchAudioFormular(twitchZiel({ output_state: 'sending', blocked: true, active_audio_mode: 'live' }), null).aktiv, null);
  assert.equal(twitchAudioFormular({ ...twitchZiel(), platform: 'youtube' }, null).auswahl, null);
});

test('Profilspeichern lässt die Audiowahl aus; ausdrückliche Auswahl geht über denselben Proxy', async () => {
  const vorher = globalThis.fetch;
  const bodies: unknown[] = [];
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/twitch/api/v2/uplink/destinations');
    assert.equal(init?.method, 'PUT');
    assert.equal(init?.credentials, 'same-origin');
    bodies.push(JSON.parse(String(init?.body)));
    return new Response(JSON.stringify({ ok: true, live_quality: { status: 'next_stream', message: 'Gespeichert für den nächsten Stream.' } }), { status: 200 });
  };
  try {
    await saveUplinkDestination({ platform: 'twitch', profil: '1080p60' });
    await saveUplinkDestination({ platform: 'twitch', twitch_audio_mode: 'live' });
    await saveUplinkDestination({ platform: 'twitch', twitch_audio_mode: 'separate_vod' });
    assert.deepEqual(bodies, [{ platform: 'twitch', profil: '1080p60' },
      { platform: 'twitch', twitch_audio_mode: 'live' }, { platform: 'twitch', twitch_audio_mode: 'separate_vod' }]);
  } finally {
    globalThis.fetch = vorher;
  }
});

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
