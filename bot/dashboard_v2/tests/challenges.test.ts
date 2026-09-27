/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (relativePath: string) => readFileSync(join(SRC, relativePath), 'utf8');

const APP = read('App.tsx');
const SIDEBAR = read('components/layout/DashboardSidebar.tsx');
const PAGE = read('pages/Challenges.tsx');
const API = read('api/challenges.ts');

test('Challenges nutzt die gemeinsame Dashboard Route und Sidebar', () => {
  assert.match(APP, /path === '\/twitch\/challenges'/);
  assert.match(APP, /<DashboardShell activeRoute="challenges"><Challenges \/><\/DashboardShell>/);
  assert.match(SIDEBAR, /href: '\/twitch\/challenges', label: 'Challenges'/);
});

test('Sidebar zeigt Challenges nur für aktive Partner und Admins', () => {
  assert.match(
    SIDEBAR,
    /authStatus\?\.authenticated && \(authStatus\.isAdmin \|\| authStatus\.partnerStatus === 'active'\)/,
  );
  assert.match(SIDEBAR, /canSeeChallenges/);
});

test('neue Partner sehen drei Startquests', () => {
  for (const key of ['active_discord_invite', 'stream_together', 'community_match']) {
    assert.match(PAGE, new RegExp(`key: '${key}'`));
  }
  assert.match(PAGE, /if \(current\.length >= 3\) return current\.slice\(0, 3\);/);
  assert.match(PAGE, /STARTER_QUESTS\.filter\(quest => !keys\.has\(quest\.key\)\)/);
});

test('Challenges zeigt Ziel, Wochenfortschritt, Erfolge und Werber', () => {
  for (const text of ['Diese Woche', 'Mit uns erreicht', 'Deine Werber', 'Einsatz (Monat)']) {
    assert.ok(PAGE.includes(text), `${text} fehlt`);
  }
  assert.match(PAGE, /data\.next_goal\.fastest_route/);
  assert.match(PAGE, /data\.streak\.current/);
  assert.match(PAGE, /achievementBadges\(data\.achievements\)/);
  assert.match(PAGE, /recruiter\.qualified_invites/);
});

test('Rangliste hebt Platz eins und Raid Boost hervor und hält die eigene Position sichtbar', () => {
  assert.match(PAGE, /rank === 1/);
  assert.match(PAGE, /<Crown /);
  assert.match(PAGE, /Raid Boost/);
  assert.match(PAGE, /viewerOwn && !viewerOwnVisible/);
  assert.match(PAGE, /effortOwn && !effortOwnVisible/);
});

test('Challenges nutzt exakt die API aus Prompt 2 plus Effort-Leaderboard', () => {
  assert.match(API, /fetchApi<ChallengesMe>\('\/challenges\/me'\)/);
  assert.match(API, /fetchApi<ChallengeViewers>\('\/challenges\/viewers'\)/);
  assert.match(API, /fetchApi<EffortLeaderboard>\('\/leaderboard\/effort'\)/);
  assert.match(API, /fetchApi<ViewerLeaderboard>\('\/leaderboard', \{ limit \}\)/);
  assert.doesNotMatch(PAGE, /quest_done|quest_all_three_bonus|points_for\(/);
});

test('Challenges fügt keinen goldenen Kartenleuchteffekt hinzu', () => {
  assert.doesNotMatch(PAGE, /card-glow|shadow-primary/);
});

test('neue Challenges Texte enthalten keinen Gedankenstrich', () => {
  assert.doesNotMatch(PAGE, /\u2014/);
  assert.doesNotMatch(API, /\u2014/);
});
