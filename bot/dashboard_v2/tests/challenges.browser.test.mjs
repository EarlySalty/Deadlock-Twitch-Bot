import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { createServer } from 'vite';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const REPO = fileURLToPath(new URL('../../../', import.meta.url));
const ARTIFACTS = process.env.CHALLENGES_ARTIFACTS ?? path.join(REPO, 'docs', 'screenshots', 'challenges');
let questAssignmentStatus = 'assigned';
let ownRank = 14;
let emptyRecruiters = false;

const AUTH = {
  authenticated: true,
  level: 'partner',
  demoMode: false,
  isAdmin: false,
  isLocalhost: false,
  adminEligible: false,
  adminMode: false,
  canViewAllStreamers: false,
  twitchUserId: '123456',
  twitchLogin: 'test_partner',
  displayName: 'Test Partner',
  partnerStatus: 'active',
  technicalPauseReason: null,
  operationalState: 'active',
  canAccessAnalyticsDashboard: true,
  tokenErrorGraceExpiresAt: null,
  csrfToken: 'test-csrf',
  csrf_token: 'test-csrf',
  plan: {
    planId: 'pro',
    planName: 'Creator Pro',
    tier: 'pro',
    isExtended: true,
    expiresAt: null,
    source: 'test',
    entitlements: ['analytics'],
  },
  access: { landing: true, analytics: true },
  permissions: {
    viewAllStreamers: false,
    viewComparison: true,
    viewChatAnalytics: true,
    viewOverlap: true,
  },
};

const CHALLENGES_ME = {
  twitch_user_id: '123456',
  next_reset_at: '2026-10-04T22:00:00Z',
  generated_at: '2026-09-27T00:15:00Z',
  timezone: 'Europe/Berlin',
  streamer: 'test_partner',
  quests: [
    { key: 'active_discord_invite', text: 'Bringe 1 neue aktive Person in den Discord', progress: 0, goal: 1, completed: false },
    { key: 'stream_together', text: 'Streame mindestens 30 Minuten per Stream Together mit einem Partner', progress: 1, goal: 1, completed: true },
    { key: 'stream_above_average', text: 'Streame diese Woche mindestens 30 Minuten', progress: 0, goal: 30, completed: false },
  ],
  streak: { current: 4, longest: 7, freeze_used_this_month: false },
  level: {
    level: 4,
    total_points: 740,
    current_threshold: 400,
    next_threshold: 1000,
  },
  next_goal: {
    missing_points: 260,
    fastest_route: 'Noch 6 weitere aktive Einladungen',
  },
  achievements: [
    { key: 'recruiter', name: 'Recruiter', progress: 18, tiers: [{ target: 5, unlocked: true }, { target: 25, unlocked: false }, { target: 100, unlocked: false }] },
    { key: 'team_player', name: 'Teamplayer', progress: 12, tiers: [{ target: 10, unlocked: true }, { target: 50, unlocked: false }, { target: 150, unlocked: false }] },
    { key: 'duo', name: 'Duo', progress: 6, tiers: [{ target: 5, unlocked: true }, { target: 25, unlocked: false }] },
    { key: 'stamina', name: 'Ausdauer', progress: 7, tiers: [{ target: 4, unlocked: true }, { target: 12, unlocked: false }, { target: 26, unlocked: false }] },
  ],
  with_us: {
    people_brought_in_who_stayed: 18,
    community_hours: 42.5,
    received_raids: 11,
  },
  season: {
    next_reset_at: '2026-09-30T22:00:00Z',
    month: '2026-09',
    points: 62,
    rank: 14,
    active_partners: 28,
  },
};

const CHALLENGE_VIEWERS = {
  generated_at: '2026-09-27T00:15:00Z',
  streamer: 'test_partner',
  recruiters: [
    { twitch_user_id: 'v1', display_name: 'ViewerAlpha', qualified_invites: 7 },
    { twitch_user_id: 'v2', display_name: 'ViewerBeta', qualified_invites: 4 },
    { twitch_user_id: 'v3', display_name: 'ViewerGamma', qualified_invites: 2 },
  ],
};

const EFFORT_LEADERBOARD = {
  month: '2026-09',
  entries: Array.from({ length: 10 }, (_, index) => ({
    rank: index + 1,
    twitch_login: index === 0 ? 'partner_gold' : `partner_${index + 1}`,
    points: 186 - index * 11,
    raid_boost: index === 0,
    is_self: false,
  })),
  own_position: {
    rank: 14,
    twitch_login: 'test_partner',
    points: 62,
    raid_boost: false,
    is_self: true,
  },
};

const VIEWER_LEADERBOARD = {
  window: { days: 30 },
  categories: [
    {
      key: 'tracked',
      title: 'Top Tracked',
      count: 10,
      entries: Array.from({ length: 10 }, (_, index) => ({
        rank: index + 1,
        streamer: `partner_${index + 1}`,
        avg_viewers: 220 - index * 12,
        max_viewers: 300 - index * 10,
        samples: 80,
      })),
      own_position: {
        rank: 14,
        streamer: 'test_partner',
        avg_viewers: 74.3,
        max_viewers: 121,
        samples: 70,
      },
    },
    { key: 'category', title: 'Top Kategorie', count: 0, entries: [] },
  ],
};

function payloadFor(pathname) {
  if (pathname.endsWith('/auth-status')) return AUTH;
  if (pathname.endsWith('/internal-home')) {
    return { twitchLogin: 'test_partner', displayName: 'Test Partner', avatarUrl: null };
  }
  if (pathname.endsWith('/challenges/me')) {
    return questAssignmentStatus !== 'assigned'
      ? { ...CHALLENGES_ME, quests: [], quest_assignment_status: questAssignmentStatus }
      : { ...CHALLENGES_ME, quest_assignment_status: 'assigned' };
  }
  if (pathname.endsWith('/challenges/viewers')) {
    return emptyRecruiters ? { ...CHALLENGE_VIEWERS, recruiters: [] } : CHALLENGE_VIEWERS;
  }
  if (pathname.endsWith('/leaderboard/effort')) {
    const own = { ...EFFORT_LEADERBOARD.own_position, rank: ownRank };
    return {
      ...EFFORT_LEADERBOARD,
      entries: EFFORT_LEADERBOARD.entries.map(entry => entry.rank === ownRank ? own : entry),
      own_position: own,
    };
  }
  if (pathname.endsWith('/leaderboard')) {
    return {
      ...VIEWER_LEADERBOARD,
      categories: VIEWER_LEADERBOARD.categories.map(category => {
        if (category.key !== 'tracked') return category;
        const own = { ...category.own_position, rank: ownRank };
        return {
          ...category,
          entries: category.entries.map(entry => entry.rank === ownRank ? own : entry),
          own_position: own,
        };
      }),
    };
  }
  if (pathname.endsWith('/streamer/onboarding')) {
    return {
      current_step: 0,
      completed: true,
      active_step: 'bookmark',
      completed_step_ids: [],
      paused: true,
      discord_linked: true,
      steam_linked: true,
      discord_status: 'connected',
      steam_status: 'connected',
    };
  }
  if (pathname.endsWith('/feedback/counts')) return { total: 0, unread: 0 };
  return {};
}

test('Challenges Seite ist auf Desktop und Mobil bedienbar', { timeout: 120_000 }, async t => {
  const server = await createServer({
    root: ROOT,
    mode: 'test',
    base: '/',
    logLevel: 'error',
    server: { host: '127.0.0.1', port: 0, strictPort: false },
    plugins: [{
      name: 'challenges-test-api',
      configureServer(vite) {
        vite.middlewares.use((request, response, next) => {
          const url = new URL(request.url, 'http://127.0.0.1');
          if (!url.pathname.startsWith('/twitch/api/v2/')) return next();
          response.writeHead(200, { 'content-type': 'application/json' });
          response.end(JSON.stringify(payloadFor(url.pathname)));
        });
      },
    }],
  });
  await server.listen();
  t.after(async () => server.close());

  const executablePath = ['/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/brave-browser'].find(existsSync);
  assert.ok(executablePath, 'Chromium installieren.');

  const browser = await chromium.launch({
    executablePath,
    headless: true,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking'],
  });
  t.after(async () => browser.close());

  const context = await browser.newContext({ viewport: { width: 1440, height: 1200 } });
  // Layout checks must not depend on an external font stylesheet being reachable.
  await context.route('https://fonts.googleapis.com/**', route => route.fulfill({
    status: 200, contentType: 'text/css', body: '',
  }));
  const page = await context.newPage();
  const consoleErrors = [];
  page.on('console', message => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });
  page.on('pageerror', error => consoleErrors.push(error.message));

  const port = server.httpServer.address().port;
  await page.goto(`http://127.0.0.1:${port}/twitch/challenges`, { waitUntil: 'networkidle' });
  await page.getByText('Noch 6 weitere aktive Einladungen bis Level 5.').waitFor();
  await page.getByRole('heading', { name: 'Diese Woche', exact: true }).waitFor();
  await page.getByText('Deine Werber').waitFor();

  const sidebar = page.locator('a[href="/twitch/challenges"]');
  await expectVisible(sidebar);
  await sidebar.click();
  await page.waitForLoadState('networkidle');

  const main = page.getByRole('main');
  const banner = main.locator('section').first();
  assert.ok((await banner.boundingBox()).height <= 100, 'Zielbanner bleibt kompakt.');
  const locked = main.locator('article').filter({ has: page.getByRole('heading', { name: 'Recruiter 2', exact: true }) });
  for (const text of [locked.locator('h3'), locked.locator('p')]) {
    const rgb = await text.evaluate(el => {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 1;
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#161616';
      ctx.fillRect(0, 0, 1, 1);
      ctx.fillStyle = getComputedStyle(el).color;
      ctx.fillRect(0, 0, 1, 1);
      return [...ctx.getImageData(0, 0, 1, 1).data].slice(0, 3);
    });
    assert.ok(rgb.every(channel => channel >= 160 && channel <= 185), 'Gesperrter Text bleibt gut lesbares, neutrales Grau.');
  }
  assert.ok(await locked.evaluate(el => el.classList.contains('border-white/10')));
  assert.equal(await locked.locator('svg.lucide-lock-keyhole').count(), 1);
  const stats = main.locator('article').filter({ hasText: /Leute, die geblieben sind|Stunden mit der Community|Raids erhalten/ });
  assert.equal(await stats.count(), 3);
  for (const card of await stats.all()) {
    assert.ok((await card.boundingBox()).height <= 80, 'Kennzahlkachel ohne unnötigen Leerraum.');
  }
  const level = main.locator('article').filter({ has: page.getByRole('heading', { name: '740 Punkte' }) });
  const levelBox = await level.boundingBox();
  const lastStatBox = await stats.last().boundingBox();
  assert.ok(Math.abs(levelBox.y + levelBox.height - lastStatBox.y - lastStatBox.height) < 2);
  const streamQuest = main.locator('article').filter({ hasText: 'Streame diese Woche mindestens 30 Minuten' });
  assert.equal(await streamQuest.getByText('0 / 30', { exact: true }).evaluate(el => getComputedStyle(el).color), 'rgb(197, 160, 89)');

  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();
  await page.getByText('Raid Boost').waitFor();
  await page.getByText('Deine Position').waitFor();
  await page.getByRole('main').getByText('test_partner', { exact: true }).waitFor();
  await expectOwnRow(page, 14);

  await mkdir(ARTIFACTS, { recursive: true });
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-desktop.png'), fullPage: true });

  await page.getByRole('button', { name: 'Zuschauer (30 Tage)' }).click();
  await page.getByText('74,3 Ø').waitFor();
  await expectOwnRow(page, 14);
  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();

  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-mobile.png'), fullPage: true });
  for (const width of [320, 390, 768, 1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, `Kein horizontaler Overflow bei ${width}px.`);
  }

  emptyRecruiters = true;
  for (const rank of [4, 1]) {
    ownRank = rank;
    await page.goto(`http://127.0.0.1:${port}/twitch/challenges`, { waitUntil: 'networkidle' });
    await main.getByText('test_partner', { exact: true }).waitFor();
    await expectOwnRow(page, rank);
    assert.equal(await main.getByText('Deine Position', { exact: true }).count(), 0, 'Eigener Eintrag in den Top 10 wird nicht doppelt angeheftet.');
    const emptyState = main.getByText('Noch niemand hat über deinen Link aktive Leute gebracht.', { exact: true }).locator('..');
    assert.ok((await emptyState.boundingBox()).height <= 76, 'Leerer Werber-Bereich bleibt kompakt.');
    await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();
    await expectOwnRow(page, rank);
    await page.screenshot({ path: path.join(ARTIFACTS, `challenges-own-rank-${rank}.png`), fullPage: true });
  }
  ownRank = 14;
  emptyRecruiters = false;

  questAssignmentStatus = 'no_reachable_quests';
  await page.setViewportSize({ width: 1440, height: 1200 });
  await page.goto(`http://127.0.0.1:${port}/twitch/challenges`, { waitUntil: 'networkidle' });
  await page.getByRole('status').getByText('Diese Woche ist gerade keine Aufgabe für dich erreichbar.').waitFor();
  await page.getByText('Bringe 1 neue aktive Person in den Discord', { exact: true }).waitFor({ state: 'detached' });
  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();
  await page.getByText('Deine Position').waitFor();
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-no-quests-desktop.png'), fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-no-quests-mobile.png'), fullPage: true });

  questAssignmentStatus = 'pending';
  await page.setViewportSize({ width: 1440, height: 1200 });
  await page.goto(`http://127.0.0.1:${port}/twitch/challenges`, { waitUntil: 'networkidle' });
  await page.getByRole('status').getByText('Deine Wochenaufgaben werden gerade vorbereitet.').waitFor();
  await page.getByText('Diese Woche ist gerade keine Aufgabe für dich erreichbar.', { exact: true }).waitFor({ state: 'detached' });
  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();
  await page.getByText('Deine Position').waitFor();
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-pending-desktop.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-pending-mobile.png'), fullPage: true });

  assert.deepEqual(consoleErrors, []);
  await context.close();
});

async function expectVisible(locator) {
  await locator.waitFor({ state: 'visible' });
  assert.equal(await locator.isVisible(), true);
}

async function expectOwnRow(page, rank) {
  const ownBadge = page.getByRole('main').getByText('Du', { exact: true });
  assert.equal(await ownBadge.count(), 1);
  const row = ownBadge.locator('xpath=../../..');
  assert.ok(await row.evaluate(el => el.classList.contains('border-primary/40')));
  assert.ok(await row.evaluate(el => el.classList.contains('bg-primary/10')));
  assert.equal(await row.evaluate(el => getComputedStyle(el).paddingTop), '10px');
  assert.equal(await row.evaluate(el => getComputedStyle(el).paddingBottom), '10px');
  if (rank === 1) {
    assert.equal(await row.locator('svg[aria-label="Platz 1"]').count(), 1);
  } else {
    assert.equal(await row.getByText(`#${rank}`, { exact: true }).count(), 1);
  }
}
