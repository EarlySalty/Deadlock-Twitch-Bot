import assert from 'node:assert/strict';
import { test } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { LanguageProvider } from '../src/context/LanguageContext';
import type { CommunityData, CommunityProfile } from '../src/types/community';

(globalThis as typeof globalThis & { React: typeof React }).React = React;
Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: new URL('https://example.test/?lang=en'), __TWITCH_DASHBOARD_RUNTIME__: {} } });
const { CommunityView, LobbyCard, RecommendationCard } = await import('../src/pages/Community');

const profile: CommunityProfile = { rank_name: 'Archon', rank_tier: 7, subrank: 3, rank_updated_at: 1_758_196_800, mode: 'normal', mode_source: 'steam_history', mode_samples: 12, history_updated_at: null, source_status: 'ok' };
const schedule = { sessions: 4, slots: Array<number>(336).fill(0.5), games: ['Deadlock'] };
const lobby = { channel_id: '1326984426906714236', name: 'Synthetic lobby', member_count: 2, user_limit: 6, slots_free: 4, mode: 'normal' as const, intent: null, rank_average: 7.3, rank_samples: 2, requester_present: false, is_streamer_vc: false, joinable: true };
const person = { login: 'synthetic_partner', live_state: 'live' as const, current_game: 'Deadlock', current_title: null, both_live_same_game: true, profile, schedule, score: 84, schedule_overlap_pct: 25, observed_overlap_minutes: 150, shared_games: ['Deadlock'], shared_windows: [{ weekday: 7, start_minute: 1410, end_minute: 1440, strength: 0.5 }], compatible: true, reasons: [], warnings: [] };
const data: CommunityData = { generated_at: '2026-09-18T12:00:00Z', days: 56, timezone: 'Europe/Berlin', streamer: 'synthetic_owner', own_profile: profile, own_schedule: schedule, recommendations: [person], candidate_count: 1, evaluated_count: 1, discord: { status: 'ok', captured_at: 1000, lobbies: [lobby] } };

function render(language: 'de' | 'en', content: React.ReactNode) {
  window.location = new URL(`https://example.test/?lang=${language}`) as unknown as Location;
  return renderToStaticMarkup(<LanguageProvider>{content}</LanguageProvider>);
}

test('English community view translates populated sections, helper labels and locale-sensitive overlap', () => {
  const html = render('en', <CommunityView data={data} now={1_030_000} onRefresh={() => {}} />);
  for (const text of ['When do your streams overlap?', '2.5 hrs', 'Compare with', 'Steam match history', 'Signal points / 100', 'Shared game', '4 VC slots free', 'about Archon', 'Sun 23:30–24:00', 'Mon 00:00 to 24:00', 'weighted frequency', 'How are the suggestions made?']) assert.ok(html.includes(text), text);
  assert.doesNotMatch(html, /Steam-Rang|ausgewertete Streams|gewichtete Häufigkeit|im Sprachkanal|VC-Plätze frei|Vergleichen mit|Tatsächlich gleichzeitig/);
});

test('German community view keeps the existing labels, weekday boundaries and decimal format', () => {
  const html = render('de', <CommunityView data={data} now={1_030_000} onRefresh={() => {}} />);
  for (const text of ['Wann überschneiden sich eure Streams?', '2,5 Std.', 'Steam-Spielhistorie · 12 Spiele', '4 VC-Plätze frei', 'etwa Archon', 'So 23:30–24:00', 'Mo 00:00 bis 24:00']) assert.ok(html.includes(text), text);
});

test('English empty and unavailable community states distinguish unavailable data from nobody playing', () => {
  for (const [status, expected] of [['link_required', 'Connect your Discord account'], ['membership_unconfirmed', 'Your Discord membership could not be confirmed'], ['unavailable', 'That does not mean nobody is playing'], ['stale', 'The lobby view is out of date']] as const) {
    const html = render('en', <CommunityView data={{ ...data, recommendations: [], discord: { status, captured_at: null, lobbies: [] } }} now={1_030_000} refreshFailed onRefresh={() => {}} />);
    assert.ok(html.includes(expected));
    assert.ok(html.includes('No other active partners'));
    assert.ok(html.includes('Refresh failed.'));
    assert.doesNotMatch(html, /Verbinde dein|Aktualisierung fehlgeschlagen|Noch keine anderen/);
  }
});

test('English stale and full lobby cards preserve disabled entry actions', () => {
  for (const [fresh, value, label] of [[false, lobby, 'Refresh required'], [true, { ...lobby, slots_free: 0, joinable: false }, 'Voice channel full']] as const) {
    const html = render('en', <LobbyCard lobby={value} fresh={fresh} profile={profile} />);
    assert.ok(html.includes(label));
    assert.doesNotMatch(html, /href=/);
    assert.match(html, /disabled/);
  }
});

test('English title-derived mode hints are translated while names remain untouched', () => {
  const html = render('en', <RecommendationCard person={{ ...person, profile: { ...profile, mode_source: 'current_title' } }} selected={false} onSelect={() => {}} />);
  assert.ok(html.includes('Hint from current stream title'));
  assert.ok(html.includes('Title hint'));
  assert.ok(html.includes('synthetic_partner'));
  assert.ok(html.includes('Deadlock'));
});
