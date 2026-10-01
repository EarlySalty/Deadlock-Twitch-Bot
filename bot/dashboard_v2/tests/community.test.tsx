import assert from 'node:assert/strict';
import React from 'react';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { PlanProvider } from '../src/context/PlanContext';
Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: new URL('https://example.test/analyse'), __TWITCH_DASHBOARD_RUNTIME__: {} } });
const { Community, CommunityView, LobbyCard, RecommendationCard, StreamerMeeting } = await import('../src/pages/Community');
import { canOpenLobby, directoryFresh, lobbyFit, voiceLink, STREAMER_VC, windowLabel, twitchLink } from '../src/utils/community';
import { resolveTabParam } from '../src/tabAliases';
import { TAB_ENTITLEMENTS } from '../src/types/billing';
import type { CommunityData, CommunityLobby, CommunityProfile, CommunityRecommendation } from '../src/types/community';
(globalThis as typeof globalThis & { React: typeof React }).React = React;

const profile: CommunityProfile = { rank_name: null, rank_tier: null, subrank: null, rank_updated_at: null, mode: null, mode_source: null, mode_samples: 0, history_updated_at: null, source_status: 'not_linked' };
const lobby: CommunityLobby = { channel_id: '1326984426906714236', name: 'Streamer <script>bad()</script>', member_count: 2, user_limit: 6, slots_free: 4, mode: null, intent: null, rank_average: null, rank_samples: 0, requester_present: false, is_streamer_vc: true, joinable: true };
const schedule = { sessions: 0, slots: Array(336).fill(0), games: [] };
const data: CommunityData = { generated_at: '2026-09-18T12:00:00Z', days: 56, timezone: 'Europe/Berlin', streamer: 'example', own_profile: profile, own_schedule: schedule, recommendations: [], candidate_count: 0, evaluated_count: 0, discord: { status: 'ok', captured_at: 1000, lobbies: [lobby] } };
const recommendation: CommunityRecommendation = {
  login: 'mate',
  live_state: 'unknown',
  current_game: null,
  current_title: null,
  both_live_same_game: false,
  profile,
  schedule: { ...schedule, sessions: 4 },
  score: null,
  schedule_overlap_pct: 25,
  observed_overlap_minutes: 150,
  shared_games: [],
  shared_windows: [],
  compatible: true,
  reasons: [],
  warnings: ['Rangvergleich nicht verfügbar', 'Spielmodus nicht auf beiden Seiten bekannt'],
};

test('community tab and aliases are accessible without analytics entitlement', () => {
  assert.deepEqual(resolveTabParam('community'), { tab: 'community' });
  assert.deepEqual(resolveTabParam('lobbys'), { tab: 'community' });
  assert.equal(TAB_ENTITLEMENTS.community, undefined);
});
test('voice links stay on the configured guild and reject untrusted IDs', () => {
  assert.equal(voiceLink(STREAMER_VC), 'https://discord.com/channels/1289721245281292288/1326984426906714236');
  for (const id of ['javascript:alert(1)', '123/456', '0', '1?guild=2', 'abc']) assert.equal(voiceLink(id), null);
  assert.equal(twitchLink('../malicious'), null);
});
test('lobby freshness expires in the browser and rejects future snapshots', () => {
  assert.equal(directoryFresh(data.discord, 1_030_000), true);
  assert.equal(directoryFresh(data.discord, 1_061_000), false);
  assert.equal(directoryFresh(data.discord, 900_000), false);
  assert.equal(directoryFresh({ ...data.discord, status: 'unavailable' }, 1_030_000), false);
});
test('recommendation cards hide unknown values and non actionable warnings', () => {
  const html = renderToStaticMarkup(<RecommendationCard person={recommendation} selected={false} onSelect={() => {}} />);
  assert.doesNotMatch(html, /Rang unbekannt/);
  assert.doesNotMatch(html, /Modus unbekannt/);
  assert.doesNotMatch(html, /Livestatus unbekannt/);
  assert.doesNotMatch(html, /Rangvergleich nicht verfügbar/);
  assert.doesNotMatch(html, /Spielmodus nicht auf beiden Seiten bekannt/);
  assert.doesNotMatch(html, /noch keine Wertung/);
});

test('recommendation cards keep useful live game rank and match badges', () => {
  const known: CommunityRecommendation = {
    ...recommendation,
    live_state: 'live',
    current_game: 'Deadlock',
    both_live_same_game: true,
    score: 84,
    shared_games: ['Deadlock'],
    profile: {
      ...profile,
      rank_name: 'Archon',
      rank_tier: 7,
      subrank: 3,
      rank_updated_at: 1_758_196_800,
      mode: 'normal',
      mode_source: 'steam_history',
      mode_samples: 12,
    },
  };
  const html = renderToStaticMarkup(<RecommendationCard person={known} selected onSelect={() => {}} />);
  assert.match(html, /Gemeinsames Spiel/);
  assert.match(html, /Deadlock/);
  assert.match(html, /Archon 3/);
  assert.match(html, /Standard/);
  assert.match(html, /Signalpunkte \/ 100/);
});

test('community view promotes real overlap as a KPI and keeps Discord sticky', () => {
  const html = renderToStaticMarkup(<CommunityView data={{ ...data, recommendations: [recommendation] }} now={1_030_000} onRefresh={() => {}} />);
  assert.match(html, /Tatsächlich gleichzeitig/);
  assert.match(html, /2,5 Std\./);
  assert.match(html, /xl:sticky/);
});

test('full, stale or unauthorized lobbies have no join action', () => {
  assert.equal(canOpenLobby(lobby, true), true);
  assert.equal(canOpenLobby(lobby, false), false);
  assert.equal(canOpenLobby({ ...lobby, joinable: false }, true), false);
  const full = { ...lobby, member_count: 6, slots_free: 0, joinable: false };
  const fullHtml = renderToStaticMarkup(<LobbyCard lobby={full} fresh profile={profile} />);
  assert.match(fullHtml, /Sprachkanal voll/);
  assert.doesNotMatch(fullHtml, /href=/);
  const staleHtml = renderToStaticMarkup(<LobbyCard lobby={lobby} fresh={false} profile={profile} />);
  assert.match(staleHtml, /Aktualisierung erforderlich/);
  assert.doesNotMatch(staleHtml, /href=/);
});
test('lobby name is escaped and capacities are clearly voice, not game slots', () => {
  const html = renderToStaticMarkup(<LobbyCard lobby={lobby} fresh profile={profile} />);
  assert.match(html, /4 VC-Plätze frei/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;/);
});
test('known rank and mode conflicts are visible, unknown data earns no points', () => {
  const normal = { ...lobby, is_streamer_vc: false, mode: 'normal' as const, rank_average: 8.3 };
  assert.equal(lobbyFit(normal, profile).points, 0);
  assert.equal(lobbyFit(normal, { ...profile, rank_tier: 2 }).conflict, true);
  assert.equal(lobbyFit(normal, { ...profile, mode: 'street_brawl' }).conflict, true);
  assert.equal(lobbyFit(normal, { ...profile, rank_tier: 8, mode: 'normal' }).conflict, false);
});
test('streamer meeting has exact VC link and channel-scoped moderation guidance', () => {
  const html = renderToStaticMarkup(<StreamerMeeting />);
  assert.match(html, /1289721245281292288\/1326984426906714236/);
  assert.match(html, /kein serverweiter Bann/);
  assert.match(html, /Mod-Team oder der Serverleitung/);
  assert.doesNotMatch(html, /iframe/);
});
test('empty network and missing Discord link are distinct from nobody online', () => {
  const html = renderToStaticMarkup(<CommunityView data={{ ...data, discord: { status: 'link_required', captured_at: null, lobbies: [] } }} now={1_030_000} onRefresh={() => {}} />);
  assert.match(html, /Noch keine anderen aktiven Partner/);
  assert.match(html, /Verbinde dein Discord-Konto/);
  assert.doesNotMatch(html, /Gerade keine belegte/);
});
test('historical unavailable source does not claim everyone is offline', () => {
  const html = renderToStaticMarkup(<CommunityView data={{ ...data, discord: { status: 'unavailable', captured_at: null, lobbies: [] } }} now={1_030_000} onRefresh={() => {}} />);
  assert.match(html, /bedeutet nicht, dass niemand spielt/);
});
test('demo mode does not enable a live community query', () => {
  const client = new QueryClient();
  const html = renderToStaticMarkup(<QueryClientProvider client={client}><PlanProvider plan={null} isAdmin={false} isLocalhost={false} isDemoMode><Community streamer="demo" days={56} /></PlanProvider></QueryClientProvider>);
  assert.match(html, /Demo liest keine persönlichen/);
  const query = client.getQueryCache().getAll()[0];
  assert.equal(query.state.fetchStatus, 'idle');
  client.clear();
});
test('local midnight window labels preserve 24:00 end boundary', () => {
  assert.equal(windowLabel({ weekday: 7, start_minute: 1410, end_minute: 1440, strength: 0.5 }), 'So 23:30–24:00');
});
