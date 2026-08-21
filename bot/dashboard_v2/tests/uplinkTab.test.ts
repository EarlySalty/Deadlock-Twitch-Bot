import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { ApiHttpError } from '../src/api/httpError';

import {
  UPLINK_PLATFORMS,
  UPLINK_TAB_ID,
  UPLINK_TAB_LABEL,
  UPLINK_WAITLIST_FEHLER,
  UPLINK_WAITLIST_TEXT,
  UPLINK_SPEED_HINTERHER,
  UPLINK_SPEED_MITHALTEN,
  UPLINK_KILL_LAEUFT_NOCH,
  UPLINK_LAST_LABEL,
  UPLINK_TWITCH_ALLGEMEINER_FEHLER,
  UPLINK_TWITCH_LOGIN_HINT,
  UPLINK_TWITCH_SCOPE_HINT,
  aktiveSessionId,
  canSaveDestination,
  clampedFields,
  egressJeZiel,
  formatDauer,
  formularAusEinstellungen,
  isUplinkTabVisible,
  killErfolgreich,
  lastProzent,
  speedLage,
  toEingabeZeit,
  toRelayZeit,
  twitchFehlertext,
  uplinkAdminBloeckeSichtbar,
  uplinkAnsicht,
  uplinkStreamerBloeckeSichtbar,
  zielRumpf,
} from '../src/pages/uplinkModel';
import * as uplinkModel from '../src/pages/uplinkModel';

const DASHBOARD_SRC = join(import.meta.dirname, '..', 'src');
const TAB_NAVIGATION_SRC = readFileSync(
  join(DASHBOARD_SRC, 'components', 'layout', 'TabNavigation.tsx'),
  'utf8',
);
const INTERNAL_HOME_SRC = readFileSync(
  join(DASHBOARD_SRC, 'pages', 'InternalHomeLanding.tsx'),
  'utf8',
);
const UPLINK_PAGE_SRC = readFileSync(join(DASHBOARD_SRC, 'pages', 'Uplink.tsx'), 'utf8');

test('ohne Freigabe sieht nur der Admin-Modus den Uplink-Tab', () => {
  assert.equal(isUplinkTabVisible({ publicVisible: false, isAdmin: false }), false);
  assert.equal(isUplinkTabVisible({ publicVisible: false, isAdmin: true }), true);
});

test('mit Freigabe sehen ihn alle', () => {
  assert.equal(isUplinkTabVisible({ publicVisible: true, isAdmin: false }), true);
  assert.equal(isUplinkTabVisible({ publicVisible: true, isAdmin: true }), true);
});

test('ein fehlendes Feld sperrt nichts auf', () => {
  assert.equal(isUplinkTabVisible({ isAdmin: false }), false);
  assert.equal(isUplinkTabVisible({ publicVisible: null, isAdmin: false }), false);
  assert.equal(isUplinkTabVisible({ publicVisible: undefined, isAdmin: null }), false);
});

test('die Freigabe filtert Uplink in beiden Hauptnavigationen', () => {
  assert.equal(isUplinkTabVisible({ publicVisible: false, isAdmin: false }), false);
  assert.match(TAB_NAVIGATION_SRC, /if \(tab\.id === UPLINK_TAB_ID\) return uplinkVisible/);
  assert.match(INTERNAL_HOME_SRC, /const uplinkVisible = useUplinkVisibility\(isAdminView\)/);
  assert.match(INTERNAL_HOME_SRC, /\.\.\.\(uplinkVisible\s*\?\s*\[/);
});

test('Admin ohne Twitch-Identität rendert Verwaltung statt Fehlerkarte', () => {
  const ansicht = uplinkAnsicht({ isAdmin: true, twitchLogin: null });

  assert.equal(ansicht, 'admin-ohne-twitch');
  assert.equal(uplinkAdminBloeckeSichtbar(ansicht), true);
  assert.equal(uplinkStreamerBloeckeSichtbar(ansicht), false);
  assert.equal(UPLINK_TWITCH_LOGIN_HINT, 'Für die persönliche Ansicht ist ein Twitch-Login nötig.');
  assert.equal(UPLINK_TWITCH_LOGIN_HINT.includes('—'), false);
});

test('der Tab heißt in der Navigation Uplink und trägt die Kennung restream', () => {
  assert.equal(UPLINK_TAB_ID, 'restream');
  assert.equal(UPLINK_TAB_LABEL, 'Uplink');
});

test('die Warteliste-Karte nennt keinen Tarif', () => {
  assert.match(UPLINK_WAITLIST_TEXT, /Warteliste/);
  assert.equal(UPLINK_WAITLIST_TEXT.includes('Add-on'), false);
  assert.equal(UPLINK_WAITLIST_TEXT.includes('bezahlt'), false);
  // Ohne echte Umlaute liest sich der Text im Dashboard falsch.
  assert.equal(/ae|oe|ue|ss/.test(UPLINK_WAITLIST_TEXT), false);
  assert.equal(UPLINK_WAITLIST_TEXT.includes('—'), false);
});

test('alle vier Plattformen aus der Spezifikation sind da', () => {
  assert.deepEqual(
    UPLINK_PLATFORMS.map((p) => p.id),
    ['twitch', 'kick', 'youtube', 'tiktok']
  );
});

test('geklemmte Werte werden benannt, unveränderte nicht', () => {
  const gewuenscht = { width: 2560, height: 1440, fps: 60, bitrate_kbps: 12000 };
  const wirksam = { width: 1920, height: 1080, fps: 60, bitrate_kbps: 6000 };
  const treffer = clampedFields(gewuenscht, wirksam);
  assert.deepEqual(
    treffer.map((t) => t.label),
    ['Breite', 'Höhe', 'Datenrate']
  );
  assert.equal(treffer[0].effective, 1920);
  assert.deepEqual(clampedFields(gewuenscht, gewuenscht), []);
  assert.deepEqual(clampedFields(null, wirksam), []);
});

test('Adresse und Schlüssel gehen nur zusammen weg', () => {
  assert.equal(
    canSaveDestination({
      rtmpUrl: 'rtmp://live.twitch.tv/app',
      streamKey: 'k',
      profileTouched: false,
      verbunden: false,
    }),
    true
  );
  assert.equal(
    canSaveDestination({
      rtmpUrl: 'rtmp://live.twitch.tv/app',
      streamKey: '',
      profileTouched: true,
      verbunden: false,
    }),
    false
  );
  assert.equal(
    canSaveDestination({ rtmpUrl: '', streamKey: 'k', profileTouched: true, verbunden: false }),
    false
  );
  // Ein Schlüssel ohne Adresse geht auch bei verbundenem Ziel nicht weg.
  assert.equal(
    canSaveDestination({ rtmpUrl: '', streamKey: 'k', profileTouched: true, verbunden: true }),
    false
  );
  // Profilwerte ohne gespeichertes Ziel dürfen nicht an das Relay gehen.
  assert.equal(
    canSaveDestination({ rtmpUrl: '', streamKey: '', profileTouched: true, verbunden: false }),
    false
  );
  assert.equal(
    canSaveDestination({ rtmpUrl: '', streamKey: '', profileTouched: false, verbunden: false }),
    false
  );
});

test('nach dem automatischen Verbinden reicht die Adresse ohne Schlüssel', () => {
  // Zustand nach dem Twitch-Knopf: Adresse steht im Feld, der Schlüssel liegt
  // beim Server, nur die Profilwerte wurden angefasst.
  assert.equal(
    canSaveDestination({
      rtmpUrl: 'rtmp://live.twitch.tv/app',
      streamKey: '',
      profileTouched: true,
      verbunden: true,
    }),
    true
  );
  // Ohne geänderte Profilwerte gibt es nichts zu speichern.
  assert.equal(
    canSaveDestination({
      rtmpUrl: 'rtmp://live.twitch.tv/app',
      streamKey: '',
      profileTouched: false,
      verbunden: true,
    }),
    false
  );
});

test('der Rumpf schickt Adresse und Schlüssel nur zusammen', () => {
  const nurProfil = zielRumpf({
    platform: 'twitch',
    rtmpUrl: 'rtmp://live.twitch.tv/app',
    streamKey: '',
    width: '1920',
    height: '1080',
    fps: '60',
    bitrate: '6000',
  });
  assert.deepEqual(nurProfil, {
    platform: 'twitch',
    enabled: true,
    width: 1920,
    height: 1080,
    fps: 60,
    bitrate_kbps: 6000,
  });
  assert.equal('rtmp_url' in nurProfil, false);
  assert.equal('stream_key' in nurProfil, false);

  assert.deepEqual(
    zielRumpf({
      platform: 'kick',
      rtmpUrl: 'rtmp://kick.example/app',
      streamKey: 'geheim',
      width: '',
      height: '',
      fps: '',
      bitrate: '',
    }),
    { platform: 'kick', enabled: true, rtmp_url: 'rtmp://kick.example/app', stream_key: 'geheim' }
  );

  assert.deepEqual(
    zielRumpf({
      platform: 'kick',
      rtmpUrl: '',
      streamKey: 'geheim',
      width: '',
      height: '',
      fps: '',
      bitrate: '',
    }),
    { platform: 'kick', enabled: true }
  );
});

test('manuelles Speichern schaltet jedes zuvor abgeschaltete Ziel wieder ein', () => {
  for (const platform of UPLINK_PLATFORMS) {
    const body = zielRumpf({
      platform: platform.id,
      rtmpUrl: platform.defaultRtmpUrl || `rtmp://${platform.id}.example/app`,
      streamKey: 'neuer-schluessel',
      width: '',
      height: '',
      fps: '',
      bitrate: '',
    });
    assert.equal(body.enabled, true, `${platform.id} muss manuell wieder einschaltbar sein`);
  }
});

test('die Verwaltung listet aktive Sessions einzeln und trifft beim Beenden ihre ID', async () => {
  const globalState = globalThis as typeof globalThis & {
    window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
  };
  const vorherigesFenster = globalState.window;
  globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
  const { fetchUplinkAdminOverview, killUplinkSession } = await import('../src/api/uplink');
  const vorher = globalThis.fetch;
  const anfragen: string[] = [];
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    anfragen.push(`${init?.method ?? 'GET'} ${url}`);
    if (url.endsWith('/admin/overview')) {
      return new Response(
        JSON.stringify({
          loadavg: 0.4,
          max_points: 12,
          used_points: 4,
          active_sessions: [
            { session_id: 7, streamer_id: 101, started_at: '2026-08-21T10:00:00Z' },
            { session_id: 8, streamer_id: 202, started_at: '2026-08-21T10:01:00Z' },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    }
    return new Response(
      JSON.stringify({ session_id: 8, ended: true, stopped: true }),
      { status: 200, headers: { 'Content-Type': 'application/json' } },
    );
  };
  try {
    const overview = await fetchUplinkAdminOverview();
    assert.deepEqual(overview.active_sessions.map((session) => session.session_id), [7, 8]);
    await killUplinkSession(overview.active_sessions[1].session_id);
    assert.match(anfragen[1], /\/admin\/sessions\/8\/kill\?confirm=true$/);
    assert.match(UPLINK_PAGE_SRC, /active_sessions/);
    assert.match(UPLINK_PAGE_SRC, /beenden\.mutate\(eintrag\.session_id\)/);
  } finally {
    globalThis.fetch = vorher;
    globalState.window = vorherigesFenster;
  }
});

test('die Nummer des laufenden Streams kommt aus dem Feld session', () => {
  assert.equal(
    aktiveSessionId({
      session: {
        id: 7,
        started_at: '2026-08-20T10:00:00Z',
        ingest_protocol: 'srt',
        ingest_codec: 'hevc',
      },
    }),
    7
  );
  assert.equal(aktiveSessionId({ session: null }), null);
  assert.equal(aktiveSessionId({}), null);
  assert.equal(aktiveSessionId(undefined), null);
  assert.equal(aktiveSessionId(null), null);
});

test('ein Sessionwechsel baut einen neuen Messwert-Schlüssel und wird abgefragt', () => {
  assert.deepEqual(uplinkModel.uplinkMetricsQueryKey(7), ['uplink-metrics', 7]);
  assert.notDeepEqual(
    uplinkModel.uplinkMetricsQueryKey(7),
    uplinkModel.uplinkMetricsQueryKey(8),
  );
  assert.equal(uplinkModel.UPLINK_ME_REFETCH_INTERVAL_MS, 15_000);
  assert.match(UPLINK_PAGE_SRC, /refetchInterval: UPLINK_ME_REFETCH_INTERVAL_MS/);
});

test('Zeitplan-Speichern schützt ungeladene Abrufe und unvollständige Zeilen', () => {
  assert.deepEqual(
    uplinkModel.scheduleSavePlan([], { loaded: false, failed: false }),
    {
      entries: null,
      error: 'Der Zeitplan ist noch nicht geladen.',
    },
  );
  assert.deepEqual(
    uplinkModel.scheduleSavePlan([], { loaded: false, failed: true }),
    {
      entries: null,
      error: 'Der Zeitplan konnte nicht geladen werden.',
    },
  );
  const unvollstaendig = uplinkModel.scheduleSavePlan(
    [{ von: '2026-08-20T18:30', bis: '' }],
    { loaded: true, failed: false },
  );
  assert.equal(unvollstaendig.entries, null);
  assert.equal(unvollstaendig.error, 'Zeit 1: Beginn und Ende müssen ausgefüllt sein.');
  assert.match(UPLINK_PAGE_SRC, /disabled={isLoading \|\| isError}/);
});

test('Zielkarten behaupten bei Lade- oder Fehlerzustand keine Verbindung', () => {
  assert.equal(uplinkModel.zielVerbindungsLabel('loading', undefined), 'Wird geladen');
  assert.equal(
    uplinkModel.zielVerbindungsLabel('error', undefined),
    'Status nicht verfügbar',
  );
  assert.equal(uplinkModel.zielVerbindungsLabel('ready', false), 'Nicht verbunden');
});

test('die Warteliste unterscheidet Laden, Fehler und leere Antwort', () => {
  assert.equal(
    uplinkModel.wartelistenAnzeige({ isLoading: true, isError: false, hasData: false, entryCount: 0 }),
    'loading',
  );
  assert.equal(
    uplinkModel.wartelistenAnzeige({ isLoading: false, isError: true, hasData: false, entryCount: 0 }),
    'error',
  );
  assert.equal(
    uplinkModel.wartelistenAnzeige({ isLoading: false, isError: false, hasData: true, entryCount: 0 }),
    'empty',
  );
});

test('die Statuskarte sagt in Worten, ob der Server mitkommt', () => {
  assert.equal(speedLage(0.82), UPLINK_SPEED_HINTERHER);
  assert.equal(speedLage(1), UPLINK_SPEED_MITHALTEN);
  assert.equal(speedLage(1.04), UPLINK_SPEED_MITHALTEN);
  assert.equal(speedLage(null), null);
  assert.equal(speedLage(undefined), null);
  assert.equal(speedLage(Number.NaN), null);
  assert.equal(speedLage(-1), null);
  for (const satz of [UPLINK_SPEED_HINTERHER, UPLINK_SPEED_MITHALTEN]) {
    // Keine Zahl und kein Fachwort im Satz, sonst liest ihn niemand.
    assert.equal(/\d/.test(satz), false);
    assert.equal(/speed|encoder|degraded/i.test(satz), false);
    assert.equal(satz.includes('—'), false);
  }
  assert.notEqual(UPLINK_SPEED_HINTERHER, UPLINK_SPEED_MITHALTEN);
});

test('die Auslastung steht als lesbarer Prozentwert da oder gar nicht', () => {
  assert.equal(lastProzent(41.4), '41 %');
  assert.equal(lastProzent(0), '0 %');
  assert.equal(lastProzent(99.6), '100 %');
  assert.equal(lastProzent(null), null);
  assert.equal(lastProzent(undefined), null);
  assert.equal(lastProzent(Number.NaN), null);
  assert.equal(lastProzent(-3), null);
});

test('die Auslastung ist die dieses Streams, nicht die der ganzen Maschine', () => {
  // `cpu_pct` kommt aus den Werten der Session. Stünde "Server" davor, läse
  // der Streamer die Last aller anderen mit und hielte sie für seine.
  assert.match(UPLINK_LAST_LABEL, /Stream/);
  assert.equal(/Server|Maschine|CPU/i.test(UPLINK_LAST_LABEL), false);
  assert.equal(UPLINK_LAST_LABEL.includes('—'), false);
});

test('die Datenrate je Ziel trägt den Namen der Plattform', () => {
  assert.deepEqual(egressJeZiel({ twitch: 6000, eigener: 2500 }), [
    { ziel: 'Twitch', kbps: 6000 },
    { ziel: 'eigener', kbps: 2500 },
  ]);
  assert.deepEqual(egressJeZiel({ kick: null, youtube: 4500 }), [
    { ziel: 'YouTube', kbps: 4500 },
  ]);
  assert.deepEqual(egressJeZiel({}), []);
  assert.deepEqual(egressJeZiel(null), []);
  assert.deepEqual(egressJeZiel(undefined), []);
});

test('nur eine verweigerte Freigabe schickt den Streamer zum Neuverbinden', () => {
  assert.equal(twitchFehlertext(new ApiHttpError('nope', 401)), UPLINK_TWITCH_SCOPE_HINT);
  assert.equal(twitchFehlertext(new ApiHttpError('nope', 403)), UPLINK_TWITCH_SCOPE_HINT);
  // Ein Serverfehler hat nichts mit der Freigabe zu tun.
  assert.equal(twitchFehlertext(new ApiHttpError('kaputt', 500)), UPLINK_TWITCH_ALLGEMEINER_FEHLER);
  assert.notEqual(twitchFehlertext(new ApiHttpError('kaputt', 500)), UPLINK_TWITCH_SCOPE_HINT);
  assert.equal(twitchFehlertext(new Error('irgendwas')), UPLINK_TWITCH_ALLGEMEINER_FEHLER);
  assert.equal(twitchFehlertext(undefined), UPLINK_TWITCH_ALLGEMEINER_FEHLER);
  assert.equal(/\bScope\b|Token|Session/.test(UPLINK_TWITCH_ALLGEMEINER_FEHLER), false);
  assert.equal(UPLINK_TWITCH_ALLGEMEINER_FEHLER.includes('—'), false);
});

test('beendet heißt beendet, nicht nur angesagt', () => {
  assert.equal(
    killErfolgreich({ session_id: 7, ended: true, end_reason: 'admin', stopped: true }),
    true
  );
  // Der Server hat den Stream nur vorgemerkt, er läuft weiter.
  assert.equal(
    killErfolgreich({ session_id: 7, ended: true, end_reason: 'admin', stopped: false }),
    false
  );
  assert.equal(killErfolgreich({ session_id: 7, ended: true }), false);
  assert.equal(killErfolgreich(undefined), false);
  assert.equal(killErfolgreich(null), false);
  assert.match(UPLINK_KILL_LAEUFT_NOCH, /läuft noch/);
  assert.equal(UPLINK_KILL_LAEUFT_NOCH.includes('—'), false);
});

test('ein misslungener Eintrag auf die Warteliste sagt das auch', () => {
  assert.match(UPLINK_WAITLIST_FEHLER, /Warteliste/);
  assert.equal(UPLINK_WAITLIST_FEHLER.includes('—'), false);
  assert.equal(/ae|oe|ue|ss/.test(UPLINK_WAITLIST_FEHLER), false);
  assert.equal(/Token|Session|Scope/.test(UPLINK_WAITLIST_FEHLER), false);
});

test('die gespeicherten Grenzen kommen aus der Antwort ins Formular zurück', () => {
  assert.deepEqual(formularAusEinstellungen({ max_points: 12, load_reject_threshold: 6.5 }), {
    plaetze: '12',
    lastgrenze: '6.5',
  });
  assert.deepEqual(formularAusEinstellungen({ max_points: 12, load_reject_threshold: 6 }), {
    plaetze: '12',
    lastgrenze: '6',
  });
  assert.deepEqual(formularAusEinstellungen(undefined), { plaetze: '', lastgrenze: '' });
  assert.deepEqual(formularAusEinstellungen({}), { plaetze: '', lastgrenze: '' });
});

test('max_points = 0 wird als 0 an das Relay geschickt', async () => {
  const globalState = globalThis as typeof globalThis & {
    window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
  };
  const vorherigesFenster = globalState.window;
  globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
  const { saveUplinkAdminSettings } = await import('../src/api/uplink');
  const vorher = globalThis.fetch;
  let gesendeterRumpf: unknown;
  globalThis.fetch = async (_input, init) => {
    gesendeterRumpf = JSON.parse(String(init?.body));
    return new Response(JSON.stringify({ max_points: 0, load_reject_threshold: 6 }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    await saveUplinkAdminSettings({ max_points: uplinkModel.zahlOderUndefined('0', true) });
    assert.deepEqual(gesendeterRumpf, { max_points: 0 });
  } finally {
    globalThis.fetch = vorher;
    globalState.window = vorherigesFenster;
  }
});

test('der Zeitplan verteilt Erwartungen und verspricht keinen reservierten Platz', () => {
  assert.equal(
    uplinkModel.UPLINK_SCHEDULE_TEXT,
    'Trag deine geplanten Zeiten ein. Der Zeitplan hilft dir, Erwartungen zu verteilen und Konflikte sichtbar zu machen. Ob dein Stream starten kann, hängt von der aktuellen Auslastung ab.',
  );
  assert.match(UPLINK_PAGE_SRC, /UPLINK_SCHEDULE_TEXT/);
  assert.doesNotMatch(UPLINK_PAGE_SRC, /Wir halten dir dann einen Platz frei\./);
  assert.doesNotMatch(UPLINK_PAGE_SRC, /reservierten Platz|Kapazität reserviert/);
});

test('Zeitangaben gehen als UTC raus und kommen als Ortszeit zurück', () => {
  const eingabe = '2026-08-20T18:30';
  const raus = toRelayZeit(eingabe);
  assert.ok(raus, 'die Eingabe muss umgerechnet werden');
  assert.match(raus as string, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
  assert.equal((raus as string).length, 20);
  assert.equal(toEingabeZeit(raus as string), eingabe);
  assert.equal(toRelayZeit(''), null);
  assert.equal(toRelayZeit('kein datum'), null);
});

test('die Laufzeit rechnet gegen einen übergebenen Zeitpunkt, nicht gegen die Wanduhr', () => {
  const start = '2026-08-20T10:00:00Z';
  const jetzt = Date.parse('2026-08-20T12:35:00Z');
  assert.equal(formatDauer(start, null, jetzt), '2 h 35 min');
  assert.equal(formatDauer(start, '2026-08-20T10:42:00Z', jetzt), '42 min');
  assert.equal(formatDauer(start, '2026-08-20T09:00:00Z', jetzt), '');
});
