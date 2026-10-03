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
  assert.match(SIDEBAR, /href: '\/twitch\/challenges', label: 'Rangliste & Erfolge'/);
});

test('Sidebar zeigt Challenges nur für aktive Partner und Admins', () => {
  assert.match(
    SIDEBAR,
    /authStatus\?\.authenticated && authStatus\.twitchUserId && \(authStatus\.isAdmin \|\| authStatus\.partnerStatus === 'active'\)/,
  );
  assert.match(SIDEBAR, /canSeeChallenges/);
});

test('Wochenaufgaben kommen vollständig aus der Engine ohne erfundene Ergänzungen', () => {
  assert.doesNotMatch(PAGE, /STARTER_QUESTS/);
  assert.match(PAGE, /const quests = me.data\?\.quests \?\? \[\];/);
  assert.match(PAGE, /me.data\?\.next_reset_at/);
  assert.match(PAGE, /freeze_used_this_month/);
});

test('leerer erreichbarer Quest-Pool wird als Partnerstatus angezeigt', () => {
  assert.match(API, /'pending' \| 'assigned' \| 'no_reachable_quests'/);
  assert.match(API, /quest_assignment_status: ChallengeAssignmentStatus/);
  assert.match(PAGE, /quest_assignment_status === 'pending'/);
  assert.match(PAGE, /quest_assignment_status === 'no_reachable_quests'/);
  assert.ok(PAGE.includes('Deine Wochenaufgaben werden gerade vorbereitet.'));
  assert.ok(PAGE.includes('Sobald die aktuelle Woche ausgewertet ist, erscheinen sie hier.'));
  assert.ok(PAGE.includes('Diese Woche ist gerade keine Aufgabe für dich erreichbar.'));
  assert.ok(PAGE.includes('Die Monatswertung läuft unabhängig davon weiter.'));
});

test('Challenges zeigt Ziel, Wochenfortschritt, Erfolge und Werber', () => {
  for (const text of ['Diese Woche', 'Mit uns erreicht', 'Deine Werber', 'Einsatz (Monat)']) {
    assert.ok(PAGE.includes(text), `${text} fehlt`);
  }
  assert.match(PAGE, /data\.next_goal\.fastest_route/);
  assert.match(PAGE, /data\.streak\.current/);
  assert.match(PAGE, /data\.achievements\.map\(achievement => <AchievementCard/);
  assert.match(PAGE, /recruiter\.qualified_invites/);
  assert.doesNotMatch(PAGE, /Deine Punkte und Erfolge bleiben erhalten|Wegen einer Datenlücke|data\.category_data_complete/);
  assert.doesNotMatch(PAGE, /achievementBadges|LockKeyhole|max-w-/);
  for (const category of ['Werber', 'Teamspieler', 'Duo', 'Ausdauer', 'Talentscout', 'Clipjäger']) {
    assert.ok(PAGE.includes(category), category);
  }
  assert.match(PAGE, /achievement\.tiers\.map/);
  assert.match(PAGE, /achievement\.progress/);
  assert.match(PAGE, /navigator\.clipboard\.writeText\(data\.referral_url\)/);
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
