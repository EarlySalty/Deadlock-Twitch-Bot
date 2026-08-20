import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  UPLINK_PLATFORMS,
  UPLINK_TAB_ID,
  UPLINK_TAB_LABEL,
  UPLINK_WAITLIST_TEXT,
  canSaveDestination,
  clampedFields,
  formatDauer,
  isUplinkTabVisible,
  toEingabeZeit,
  toRelayZeit,
} from '../src/pages/uplinkModel';

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
    canSaveDestination({ rtmpUrl: 'rtmp://live.twitch.tv/app', streamKey: 'k', profileTouched: false }),
    true
  );
  assert.equal(
    canSaveDestination({ rtmpUrl: 'rtmp://live.twitch.tv/app', streamKey: '', profileTouched: true }),
    false
  );
  assert.equal(canSaveDestination({ rtmpUrl: '', streamKey: 'k', profileTouched: true }), false);
  // Nur Profilwerte ändern ist erlaubt, ein leeres Formular nicht.
  assert.equal(canSaveDestination({ rtmpUrl: '', streamKey: '', profileTouched: true }), true);
  assert.equal(canSaveDestination({ rtmpUrl: '', streamKey: '', profileTouched: false }), false);
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
