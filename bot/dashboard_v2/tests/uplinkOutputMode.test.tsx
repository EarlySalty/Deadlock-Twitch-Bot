import assert from 'node:assert/strict';
import test from 'node:test';
import React from 'react';
import { QueryClient } from '@tanstack/react-query';
import { renderToStaticMarkup } from 'react-dom/server';
import type { UplinkDestination } from '../src/api/uplink';
import { bestaetigeUplinkSpeichern, twitchOutputFormular, twitchOutputPayload } from '../src/uplinkOutputMode';
import { UplinkOutputMode } from '../src/pages/UplinkOutputMode';

const profil = { width: 1920, height: 1080, fps: 60, bitrate_kbps: 6000 };
const ziel: UplinkDestination = {
  platform: 'twitch', rtmp_url: '', enabled: true,
  requested_output_mode: 'enhanced', active_output_mode: 'single',
  output_state: 'sending', active_profiles: [profil],
  fallback_reason: 'Twitch hat keine zusätzlichen Qualitätsstufen freigegeben.',
};

test('Speicher-ACK ersetzt keinen Zielcache; nur bestätigter GET aktualisiert die Ausgabe', async () => {
  const client = new QueryClient();
  const vorher = { destinations: [{ ...ziel, requested_output_mode: 'single' as const }] };
  client.setQueryData(['uplink-destinations'], vorher);
  let fertig!: (value: { destinations: UplinkDestination[] }) => void;
  const geladen = new Promise<{ destinations: UplinkDestination[] }>((resolve) => { fertig = resolve; });
  const gespeichert = bestaetigeUplinkSpeichern({ ok: true, connection_generations: { twitch: 2 } }, client, () => geladen);
  assert.deepEqual(client.getQueryData(['uplink-destinations']), vorher);
  const nachher = { destinations: [ziel] };
  fertig(nachher);
  assert.deepEqual(await gespeichert, nachher);
  assert.deepEqual(client.getQueryData(['uplink-destinations']), nachher);
  client.clear();
});

test('GET-Fehler nach erfolgreichem ACK bewahrt letzten gültigen Cache und Modusentwurf', async () => {
  const client = new QueryClient();
  const vorher = { destinations: [{ ...ziel, requested_output_mode: 'single' as const }] };
  client.setQueryData(['uplink-destinations'], vorher);
  await assert.rejects(bestaetigeUplinkSpeichern(
    { ok: true, connection_generations: { twitch: 2 } }, client,
    async () => { throw new Error('GET nicht erreichbar'); },
  ), /GET nicht erreichbar/);
  assert.deepEqual(client.getQueryData(['uplink-destinations']), vorher);
  const state = twitchOutputFormular(vorher.destinations[0], 'enhanced');
  assert.equal(state.auswahl, 'enhanced');
  assert.equal(state.geaendert, true);
  assert.equal(state.aktiv, 'single');
  client.clear();
});

test('ein vor dem PUT gestarteter Poll kann den neuen GET nicht ersetzen', async () => {
  const client = new QueryClient();
  const vorher = { destinations: [{ ...ziel, requested_output_mode: 'single' as const }] };
  client.setQueryData(['uplink-destinations'], vorher);
  let alterPollFertig!: (value: typeof vorher) => void;
  const alterPoll = client.fetchQuery({ queryKey: ['uplink-destinations'], staleTime: 0,
    queryFn: () => new Promise<typeof vorher>((resolve) => { alterPollFertig = resolve; }),
  }).catch(() => undefined);
  let neuGeladen = 0;
  const nachher = { destinations: [ziel] };
  await bestaetigeUplinkSpeichern({ ok: true, connection_generations: { twitch: 2 } }, client, async () => {
    neuGeladen++;
    return nachher;
  });
  alterPollFertig(vorher);
  await alterPoll;
  assert.equal(neuGeladen, 1);
  assert.deepEqual(client.getQueryData(['uplink-destinations']), nachher);
  client.clear();
});

test('gespeicherter Enhanced-Wunsch bleibt beim tatsächlichen Einzelstream samt Grund sichtbar', () => {
  const state = twitchOutputFormular(ziel, null);
  assert.equal(state.auswahl, 'enhanced');
  assert.equal(state.aktiv, 'single');
  assert.equal(state.stufen.length, 1);
  assert.equal(state.fallback, ziel.fallback_reason);
  const html = renderToStaticMarkup(<UplinkOutputMode ziel={ziel} entwurf={null} disabled={false} onChange={() => {}} />);
  assert.match(html, /Gespeichert: Enhanced Broadcasting/);
  assert.match(html, /Laufende Ausgabe: Einzelstream/);
  assert.match(html, /Twitch hat keine zusätzlichen Qualitätsstufen freigegeben/);
});

test('Moduswechsel bleibt bei Refetch Entwurf und wechselt keine aktive Anzeige', () => {
  const state = twitchOutputFormular(ziel, 'single');
  assert.equal(state.auswahl, 'single');
  assert.equal(state.gespeichert, 'enhanced');
  assert.equal(state.geaendert, true);
  const refreshed = twitchOutputFormular({ ...ziel, active_output_mode: 'enhanced' }, 'single');
  assert.equal(refreshed.auswahl, 'single');
  assert.equal(refreshed.aktiv, 'enhanced');
});

test('Teiländerungen ohne explizite Moduswahl setzen keinen Standard zurück', () => {
  assert.deepEqual(twitchOutputPayload('twitch', null), {});
  assert.deepEqual(twitchOutputPayload('youtube', 'enhanced'), {});
  assert.deepEqual(twitchOutputPayload('twitch', 'enhanced'), { twitch_output_mode: 'enhanced' });
  assert.deepEqual(twitchOutputPayload('twitch', 'single'), { twitch_output_mode: 'single' });
});

test('beendete, fehlgeschlagene oder blockierte Ausgabe zeigt keine veralteten aktiven Stufen', () => {
  for (const patch of [{ output_state: 'finished' as const }, { output_state: 'failed' as const }, { blocked: true }]) {
    const state = twitchOutputFormular({ ...ziel, ...patch }, null);
    assert.equal(state.aktiv, null);
    assert.deepEqual(state.stufen, []);
    assert.equal(state.gespeichert, 'enhanced');
  }
});

test('fehlende oder unvollständige aktive Profile werden nicht aus Wünschen ergänzt', () => {
  const state = twitchOutputFormular({ ...ziel, active_profiles: undefined, active_profile: profil, requested: profil }, null);
  assert.deepEqual(state.stufen, []);
  assert.equal(twitchOutputFormular(undefined, null).aktiv, null);
  assert.deepEqual(twitchOutputFormular({ ...ziel, active_profiles: [{ ...profil, height: 0 }] }, null).stufen, []);
});

test('unbekannte Serverwerte bestätigen keine Betriebsart und beschädigen die Karte nicht', () => {
  const unbekannt = { ...ziel, active_output_mode: 'auto', requested_output_mode: 'auto', active_profiles: {}, fallback_reason: {} } as unknown as UplinkDestination;
  const state = twitchOutputFormular(unbekannt, null);
  assert.equal(state.aktiv, null);
  assert.equal(state.gespeichert, null);
  assert.deepEqual(state.stufen, []);
  assert.equal(state.fallback, null);
});

test('echte Enhanced-Leiter wird angezeigt, einzelnes Profil bestätigt kein Enhanced', () => {
  const html = (profiles: typeof profil[]) => renderToStaticMarkup(<UplinkOutputMode
    ziel={{ ...ziel, active_output_mode: 'enhanced', active_profiles: profiles, fallback_reason: null }}
    entwurf={null} disabled={false} onChange={() => {}} />);
  assert.match(html([profil]), /Mehrere Qualitätsstufen noch nicht bestätigt/);
  const enhanced = html([profil, { ...profil, width: 1280, height: 720, bitrate_kbps: 3000 }]);
  assert.match(enhanced, /Laufende Ausgabe: Enhanced Broadcasting/);
  assert.match(enhanced, /1920×1080/);
  assert.match(enhanced, /1280×720/);
});

test('Betriebsart besitzt native Radiogruppe, Beschriftungen und vorlesbaren Status', () => {
  const html = renderToStaticMarkup(<UplinkOutputMode ziel={ziel} entwurf={'single'} disabled={true} onChange={() => {}} />);
  assert.match(html, /<fieldset disabled=""/);
  assert.match(html, /<legend[^>]*>Twitch-Betriebsart<\/legend>/);
  assert.equal((html.match(/type="radio"/g) ?? []).length, 2);
  assert.match(html, /checked="" value="single"/);
  assert.match(html, /role="status" aria-atomic="true"/);
  assert.match(html, /aria-describedby=/);
});
