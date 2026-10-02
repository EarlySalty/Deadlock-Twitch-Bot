import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { createServer } from 'vite';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const ARTIFACTS = '/home/nathanael/.claude/sichtpruefung/challenges-20261003';
const TEST_AVATAR = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect width="48" height="48" fill="#392519"/><circle cx="24" cy="18" r="9" fill="#c5a059"/><path d="M8 48v-7a16 16 0 0 1 32 0v7" fill="#c5a059"/></svg>');
let questAssignmentStatus = 'assigned';
let ownRank = 14;
let emptyRecruiters = false;
let categoryComplete = true;
let lowLevel = false;

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
  referral_url: 'https://discord.gg/challenges-test-referral',
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
    { key: 'talent_scout', name: 'Talent Scout', progress: 2, tiers: [{ target: 1, unlocked: true }, { target: 3, unlocked: false }] },
    { key: 'clip_hunter', name: 'Clip Hunter', progress: 3, tiers: [{ target: 1, unlocked: true }, { target: 5, unlocked: false }] },
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
    { twitch_user_id: 'v1', avatar_url: TEST_AVATAR, display_name: 'ViewerAlpha', qualified_invites: 7 },
    { twitch_user_id: 'v2', avatar_url: TEST_AVATAR, display_name: 'ViewerBeta', qualified_invites: 4 },
    { twitch_user_id: 'v3', avatar_url: TEST_AVATAR, display_name: 'ViewerGamma', qualified_invites: 2 },
  ],
};

const EFFORT_LEADERBOARD = {
  month: '2026-09',
  entries: Array.from({ length: 10 }, (_, index) => ({
    rank: index + 1,
    avatar_url: TEST_AVATAR,
    twitch_login: index === 0 ? 'partner_gold' : `partner_${index + 1}`,
    points: 186 - index * 11,
    raid_boost: index === 0,
    is_self: false,
  })),
  own_position: {
    rank: 14,
    avatar_url: TEST_AVATAR,
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
        avatar_url: TEST_AVATAR,
        streamer: `partner_${index + 1}`,
        avg_viewers: 220 - index * 12,
        max_viewers: 300 - index * 10,
        samples: 80,
      })),
      own_position: {
        rank: 14,
        avatar_url: TEST_AVATAR,
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
    const me = lowLevel ? { ...CHALLENGES_ME, level: { level: 1, total_points: 5, current_threshold: 0, next_threshold: 50 }, next_goal: { missing_points: 45, fastest_route: 'Noch 1 geworbener Partner' } } : CHALLENGES_ME;
    return questAssignmentStatus !== 'assigned'
      ? { ...me, quests: [], quest_assignment_status: questAssignmentStatus }
      : { ...me, category_data_complete: categoryComplete,
          streak: { ...CHALLENGES_ME.streak, data_complete: categoryComplete },
          quests: CHALLENGES_ME.quests.map(quest => ({ ...quest,
            data_complete: quest.key !== 'stream_above_average' || categoryComplete })),
          quest_assignment_status: 'assigned' };
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

  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, permissions: ['clipboard-read', 'clipboard-write'] });
  // Die Layoutprüfung benötigt keine externe Schriftdatei.
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
  const pageUrl = `http://127.0.0.1:${port}/twitch/challenges`;
  const main = page.getByRole('main');
  const reload = async () => {
    await page.goto(pageUrl, { waitUntil: 'networkidle' });
    await main.getByRole('heading', { name: 'Diese Woche', exact: true }).waitFor();
  };
  await mkdir(ARTIFACTS, { recursive: true });
  await reload();

  for (const [width, height] of [[1920, 1080], [2560, 1440]]) {
    await page.setViewportSize({ width, height });
    for (const name of ['Werber', 'Teamspieler', 'Duo', 'Ausdauer', 'Talentscout', 'Clipjäger']) {
      const card = main.locator('article').filter({ has: page.getByRole('heading', { name, exact: true }) });
      assert.equal(await card.count(), 1, `Eine Karte für ${name}.`);
      assert.equal(await card.getByText(/Stufe [123]/).count(), ['Duo', 'Talentscout', 'Clipjäger'].includes(name) ? 2 : 3, 'Alle vom Server gelieferten Stufen bleiben sichtbar.');
    }
    assert.equal(await main.getByText(/Wegen einer Datenlücke/).count(), 0);
    const clips = main.locator('article').filter({ has: page.getByRole('heading', { name: 'Clipjäger', exact: true }) });
    await clips.getByText('3 / 5', { exact: true }).waitFor();
    const achievements = main.locator('article[data-achievement]');
    const positions = await achievements.evaluateAll(cards => cards.map(card => card.getBoundingClientRect().y));
    assert.equal(positions[3], positions[4], 'Die letzte Erfolgsreihe ist gefüllt.');
    assert.equal(positions[4], positions[5], 'Sechs Kategorien bilden zwei vollständige Reihen.');
    const levelProgress = main.getByRole('progressbar', { name: 'Fortschritt zum nächsten Level', exact: true });
    assert.equal(await levelProgress.getAttribute('aria-valuenow'), '340');
    assert.equal(await levelProgress.getAttribute('aria-valuemax'), '600');
    const level = main.locator('article').filter({ has: page.getByRole('heading', { name: '740 Punkte', exact: true }) });
    assert.ok((await level.boundingBox()).height < 220, 'Level-Karte bleibt kompakt.');
    await expectLeaderboard(page, 14);
    for (const avatar of await main.locator('img').all()) { await avatar.scrollIntoViewIfNeeded(); await avatar.evaluate(image => image.decode()); }
    assert.equal(await main.locator('img').evaluateAll(images => images.every(image => image.complete && image.naturalWidth > 0)), true, 'Avatare sind geladen.');
    const viewersTab = main.getByRole('button', { name: 'Zuschauer (30 Tage)', exact: true });
    const effortTab = main.getByRole('button', { name: 'Einsatz (Monat)', exact: true });
    assert.ok(await viewersTab.getAttribute('title'), 'Zuschauer-Tab erklärt den Zeitraum.');
    assert.ok(await effortTab.getAttribute('title'), 'Einsatz-Tab erklärt die Wertung.');
    assert.ok(await main.getByText('Ø Zuschauer pro Stream', { exact: true }).count(), 'Zuschauer-Wert ist erklärt.');
    await assertNoOverflow(page, width);
    await page.evaluate(() => window.scrollTo(0, 0));
    await capture(page, { path: path.join(ARTIFACTS, `challenges-${width}-zuschauer.png`), fullPage: true });
    await capture(page, { path: path.join(ARTIFACTS, `challenges-${width}-oben.png`) });
    await main.getByRole('heading', { name: 'Deine Werber', exact: true }).scrollIntoViewIfNeeded();
    await capture(page, { path: path.join(ARTIFACTS, `challenges-${width}-werber-ausschnitt.png`) });
    const content = main.locator('.challenges-page');
    assert.equal(await content.evaluate(element => getComputedStyle(element).maxWidth), 'none', 'Inhalt erhält keine maximale Breite.');
    assert.ok(await content.evaluate(element => parseFloat(getComputedStyle(element).paddingBottom) >= 96), 'Unterer Abstand lässt Platz für die Hilfe.');
    await effortTab.click();
    await main.getByText('Raid Boost', { exact: true }).waitFor();
    await expectLeaderboard(page, 14);
    await page.evaluate(() => window.scrollTo(0, 0));
    await capture(page, { path: path.join(ARTIFACTS, `challenges-${width}-einsatz.png`), fullPage: true });
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    await capture(page, { path: path.join(ARTIFACTS, `challenges-${width}-unten.png`) });
    const lastRow = main.locator('[data-rank]').last();
    const help = page.getByRole('button', { name: 'Hilfe bekommen', exact: true });
    const rowBox = await lastRow.boundingBox();
    const helpBox = await help.boundingBox();
    assert.ok(rowBox.y + rowBox.height <= helpBox.y, 'Hilfe-Button überdeckt die letzte Ranglistenzeile nicht.');
    await viewersTab.click();
  }

  emptyRecruiters = true;
  for (const rank of [4, 1]) {
    ownRank = rank;
    await reload();
    await expectLeaderboard(page, rank);
    assert.equal(await main.getByText('Deine Position', { exact: true }).count(), 0, 'Eigener Eintrag wird in den Top 10 nicht doppelt angeheftet.');
    const copy = main.getByRole('button', { name: 'Kopieren', exact: true });
    await copy.click();
    const clipboard = await page.evaluate(() => navigator.clipboard.readText());
    assert.ok(clipboard.startsWith('https://'), 'Kopieren liefert einen vollständigen Empfehlungslink.');
    assert.equal(clipboard, CHALLENGES_ME.referral_url, 'Kopieren erhält den vom Server gelieferten Link unverändert.');
    await capture(page, { path: path.join(ARTIFACTS, `challenges-leere-werber-rang-${rank}.png`), fullPage: true });
    await main.getByRole('button', { name: 'Einsatz (Monat)', exact: true }).click();
    await expectLeaderboard(page, rank);
  }
  ownRank = 14;
  emptyRecruiters = false;
  await reload();
  for (const width of [320, 390, 768, 1024, 1920, 2560]) {
    await page.setViewportSize({ width, height: 900 });
    await assertNoOverflow(page, width);
  }

  for (const [status, message] of [
    ['no_reachable_quests', 'Diese Woche ist gerade keine Aufgabe für dich erreichbar.'],
    ['pending', 'Deine Wochenaufgaben werden gerade vorbereitet.'],
  ]) {
    questAssignmentStatus = status;
    await page.setViewportSize({ width: 1920, height: 1080 });
    await reload();
    await main.getByRole('status').getByText(message, { exact: true }).waitFor();
    assert.equal(await main.getByText('Bringe 1 neue aktive Person in den Discord', { exact: true }).count(), 0);
  }
  categoryComplete = false;
  questAssignmentStatus = 'assigned';
  await reload();
  assert.equal(await main.getByText(/Wegen einer Datenlücke|Deine Punkte und Erfolge bleiben erhalten/).count(), 0, 'Der Datenlückenbanner bleibt auch bei unvollständigen Daten entfernt.');
  await main.getByText('Wertung ausgesetzt', { exact: true }).waitFor();
  await expectLeaderboard(page, 14);
  lowLevel = true;
  categoryComplete = true;
  await reload();
  await main.getByRole('heading', { name: '5 Punkte', exact: true }).waitFor();
  await main.getByText('Noch 1 geworbener Partner bis Level 2.', { exact: true }).waitFor();
  await main.getByText('Punkte für Level 2', { exact: true }).waitFor();
  const lowProgress = main.getByRole('progressbar', { name: 'Fortschritt zum nächsten Level', exact: true });
  assert.equal(await lowProgress.getAttribute('aria-valuenow'), '5');
  assert.equal(await lowProgress.getAttribute('aria-valuemax'), '50');
  await capture(page, { path: path.join(ARTIFACTS, 'challenges-level-1.png'), fullPage: true, animations: 'disabled' });
  assert.deepEqual(consoleErrors, []);
  await context.close();
});

async function assertNoOverflow(page, width) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, `Kein horizontaler Overflow bei ${width}px.`);
}

async function expectLeaderboard(page, rank) {
  const main = page.getByRole('main');
  const ownBadge = main.getByText('DU', { exact: true });
  assert.equal(await ownBadge.count(), 1);
  const row = ownBadge.locator('xpath=ancestor::*[@data-rank][1]');
  assert.equal(await row.getAttribute('data-rank'), String(rank));
  for (const place of [1, 2, 3]) {
    assert.equal(await main.locator(`svg[aria-label="Platz ${place}"]`).count(), 1, `Platz ${place} ist ausgezeichnet.`);
  }
}

async function capture(page, options) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.screenshot({ ...options, animations: 'disabled' });
}
