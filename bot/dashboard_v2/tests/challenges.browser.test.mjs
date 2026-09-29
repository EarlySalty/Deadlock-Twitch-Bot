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
const ARTIFACTS = path.join(REPO, 'docs', 'screenshots', 'challenges');

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
    { key: 'community_match', text: 'Spiele ein Match mit jemandem aus der Community', progress: 0, goal: 1, completed: false },
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
  if (pathname.endsWith('/challenges/me')) return CHALLENGES_ME;
  if (pathname.endsWith('/challenges/viewers')) return CHALLENGE_VIEWERS;
  if (pathname.endsWith('/leaderboard/effort')) return EFFORT_LEADERBOARD;
  if (pathname.endsWith('/leaderboard')) return VIEWER_LEADERBOARD;
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

  const executablePath = process.env.BROWSER_BIN
    || ['/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/brave-browser'].find(existsSync);
  assert.ok(executablePath, 'Chromium installieren oder BROWSER_BIN setzen.');

  const browser = await chromium.launch({
    executablePath,
    headless: true,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking'],
  });
  t.after(async () => browser.close());

  const context = await browser.newContext({ viewport: { width: 1440, height: 1200 } });
  const page = await context.newPage();
  const consoleErrors = [];
  page.on('console', message => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });
  page.on('pageerror', error => consoleErrors.push(error.message));

  const port = server.httpServer.address().port;
  await page.goto(`http://127.0.0.1:${port}/twitch/challenges`, { waitUntil: 'networkidle' });
  await page.getByText('Noch 6 weitere aktive Einladungen bis Level 5.').waitFor();
  await page.getByText('Diese Woche').waitFor();
  await page.getByText('Deine Werber').waitFor();

  const sidebar = page.locator('a[href="/twitch/challenges"]');
  await expectVisible(sidebar);
  await sidebar.click();
  await page.waitForLoadState('networkidle');

  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();
  await page.getByText('Raid Boost').waitFor();
  await page.getByText('Deine Position').waitFor();
  await page.getByRole('main').getByText('test_partner', { exact: true }).waitFor();

  await mkdir(ARTIFACTS, { recursive: true });
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-desktop.png'), fullPage: true });

  await page.getByRole('button', { name: 'Zuschauer (30 Tage)' }).click();
  await page.getByText('74,3 Ø').waitFor();
  await page.getByRole('button', { name: 'Einsatz (Monat)' }).click();

  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: path.join(ARTIFACTS, 'challenges-mobile.png'), fullPage: true });

  assert.deepEqual(consoleErrors, []);
  await context.close();
});

async function expectVisible(locator) {
  await locator.waitFor({ state: 'visible' });
  assert.equal(await locator.isVisible(), true);
}
