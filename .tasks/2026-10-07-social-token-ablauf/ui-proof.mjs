import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import { chromium } from '../../bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const root = path.resolve(import.meta.dirname, '../..');
const dist = path.join(root, 'bot/analytics/dashboard_v2/dist');
const now = Date.parse('2026-10-07T12:00:00Z');
const iso = days => new Date(now + days * 86400000).toISOString();
let statuses;
const failures = [];
const server = http.createServer(async (req, res) => {
  const p = new URL(req.url, 'http://localhost').pathname;
  let data;
  if (p === '/twitch/api/v2/auth-status') data = {
    authenticated: true, level: 'admin', authLevel: 'admin', isAdmin: true,
    adminEligible: true, adminMode: true, isLocalhost: false,
    canViewAllStreamers: true, canAccessAnalyticsDashboard: true,
    plan: { planId: 'analysis_dashboard', planName: 'Admin', tier: 'extended', isExtended: true, entitlements: [] },
  };
  else if (p === '/twitch/api/v2/streamers') data = [{ login: 'fixture', twitchUserId: '11' }];
  else if (p.endsWith('/platforms/status')) {
    if (statuses === null) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify({ error: 'platform_status_failed' }));
    }
    data = { platforms: statuses };
  }
  else if (p.endsWith('/access/me')) data = { allowed: true, streamer: 'fixture', isAdmin: true };
  else if (p.endsWith('/access')) data = { items: [{ streamer_login: 'fixture', granted: true }] };
  else if (p.includes('vod-archive')) data = { streamer_login: 'fixture', enabled: false, privacy: 'private', privacy_options: ['private'] };
  else if (p.includes('/clips')) data = { items: [], total: 0, page: 1, page_size: 100 };
  else if (p.includes('/posting-plan')) data = { platforms: [], categories: [], pool: { aktive_plattformen: 0 }, timezone: 'Europe/Berlin' };
  else if (p.includes('/api/')) data = {};
  if (data !== undefined) {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify(data));
  }
  const relative = p.startsWith('/twitch/dashboard-v2/') ? p.slice('/twitch/dashboard-v2/'.length) : 'index.html';
  const file = path.resolve(dist, relative);
  if (!file.startsWith(dist + '/')) { res.writeHead(400); return res.end(); }
  try {
    const body = await fs.readFile(file);
    const type = file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html';
    res.writeHead(200, { 'Content-Type': type }); res.end(body);
  } catch { res.writeHead(404); res.end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ executablePath: '/home/nathanael/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome', headless: true, args: ['--no-sandbox'] });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, locale: 'de-DE', reducedMotion: 'reduce' });
  page.on('pageerror', error => failures.push(error.message));
  await page.route('**/*', route => route.request().url().startsWith(base) ? route.continue() : route.abort());
  await page.addInitScript(fixed => {
    const Original = Date;
    globalThis.Date = class extends Original {
      constructor(...args) { super(...(args.length ? args : [fixed])); }
      static now() { return fixed; }
    };
    localStorage.setItem('ddc-language', 'de');
  }, now);
  const status = (platform, overrides) => ({ platform, connected: true, username: 'fixture', expired: false, needs_reauth: false, automatically_renewed: true, expires_at: iso(-1), refresh_expires_at: null, ...overrides });
  const cases = [
    ['metadata-read-error', null, [], 0],
    ['instagram-20-days', [status('youtube', {}), status('tiktok', { refresh_expires_at: iso(365) }), status('instagram', { expires_at: iso(20) })], ['27.10.2026'], 0],
    ['nonrenewable-6-days', [status('youtube', { automatically_renewed: false, expires_at: iso(6) }), status('tiktok', { automatically_renewed: false, expires_at: iso(6) }), status('instagram', { expires_at: iso(30) })], ['13.10.2026', '13.10.2026'], 0],
    ['renewable-access-ended', [status('youtube', {}), status('tiktok', { refresh_expires_at: iso(365) }), status('instagram', { expires_at: iso(60) })], [], 0],
    ['nonrenewable-ended', [status('youtube', { automatically_renewed: false, expired: true, needs_reauth: true }), status('tiktok', { automatically_renewed: false, expired: true, needs_reauth: true }), status('instagram', { expires_at: iso(60) })], [], 2],
  ];
  const evidence = [];
  for (const [name, platforms, dates, reconnects] of cases) {
    statuses = platforms;
    await page.goto(base + '/social-media-admin?twitch_user_id=11&oauth=probe');
    await page.getByRole('heading', { name: /Verbindungen/ }).waitFor().catch(async error => {
      console.log(JSON.stringify({ body: await page.locator('body').innerText(), failures }));
      await page.screenshot({ path: path.join(import.meta.dirname, 'ui-proof-failure.png') });
      throw error;
    });
    await page.evaluate(() => document.fonts.ready);
    const card = page.getByRole('heading', { name: /Verbindungen/ }).locator('..').locator('..');
    await page.waitForFunction(() => !document.querySelector('.animate-spin'));
    if (platforms === null) {
      await card.getByText('Zustand unbekannt', { exact: true }).first().waitFor();
      assert.equal(await card.getByText('Zustand unbekannt', { exact: true }).count(), 3);
      assert.equal(await card.getByRole('link').count(), 0);
      assert.equal(await card.getByRole('button', { name: 'Trennen', exact: true }).count(), 0);
    }
    const text = await card.innerText();
    const shownDates = text.match(/\d{2}\.\d{2}\.\d{4}/g) ?? [];
    assert.deepEqual(shownDates, dates, name);
    assert.equal(await card.getByRole('link', { name: 'Neu verbinden', exact: true }).count(), reconnects, name);
    const geometry = await page.evaluate(() => ({ document: document.documentElement.scrollWidth, viewport: innerWidth }));
    assert.ok(geometry.document <= geometry.viewport, name);
    await page.screenshot({ path: path.join(import.meta.dirname, `${name}.png`) });
    evidence.push({ name, text, dates: shownDates, reconnects, geometry });
  }
  assert.deepEqual(failures, []);
  await fs.writeFile(path.join(import.meta.dirname, 'ui-proof.json'), JSON.stringify({ now: new Date(now).toISOString(), dist, evidence }, null, 2));
  console.log(JSON.stringify({ cases: evidence.length, pageErrors: failures.length, dist }));
} finally {
  await browser.close();
  await new Promise(resolve => server.close(resolve));
}
