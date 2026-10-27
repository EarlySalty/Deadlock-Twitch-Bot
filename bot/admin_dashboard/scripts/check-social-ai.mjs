import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { limits as limitFixture, usage } from '../src/api/socialAiFixtures.mjs';

const [playwrightPath, cdpUrl, origin, evidenceDir] = process.argv.slice(2);
if (!playwrightPath || !cdpUrl || !origin || !evidenceDir) throw new Error('Playwright-Modul, eigener Moli-CDP-Anschluss, lokale Vorschau und Nachweisverzeichnis erforderlich.');
const { chromium } = await import(pathToFileURL(playwrightPath).href);
const browser = await chromium.connectOverCDP(cdpUrl);
const results = [];
const groupTable = '.social-ai-table[aria-label^="Verbrauch nach Gruppierung"]';
await mkdir(evidenceDir, { recursive: true });

async function run(width) {
  const context = await browser.newContext({ viewport: { width, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  let limits = structuredClone(limitFixture);
  let mode = 'normal';
  let conflict = false;
  let writes = 0;
  let releaseStale;
  let heldStale;
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    const originalFetch = window.fetch;
    window.syntheticRequestBodies = [];
    window.fetch = (input, init) => {
      if (init?.method === 'POST') window.syntheticRequestBodies.push(init.body);
      return originalFetch.call(window, input, init);
    };
  });
  await context.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== origin) return route.abort();
    if (!url.pathname.startsWith('/twitch/api/')) return route.continue();
    const send = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    if (url.pathname.endsWith('/auth-status')) return send({ authenticated: true, isAdmin: true, csrfToken: 'synthetic-csrf', loginUrl: '/synthetic-login', discordLoginUrl: '/synthetic-login', displayName: 'Synthetische Testdaten, kein Livebetrieb' });
    if (!url.pathname.includes('/social-ai/')) return send([]);
    if (mode === 'unavailable') return send({ error: 'synthetic-unavailable' }, 404);
    if (url.pathname.endsWith('/external-consent')) return send({ enabled: false });
    if (request.method() === 'POST') {
      writes++;
      const body = request.postDataJSON() ?? await page.evaluate(() => JSON.parse(window.syntheticRequestBodies.at(-1)));
      assert.equal(request.headers()['x-csrf-token'], 'synthetic-csrf');
      assert.equal(body.csrf_token, 'synthetic-csrf');
      assert.equal('initial_total_cap_micro_usd' in body, false);
      if (conflict) { conflict = false; return send({ error: 'synthetic-conflict' }, 409); }
      if (url.pathname.endsWith('/global')) {
        assert.equal(body.expected_revision, limits.global.revision);
        limits.global = { ...limits.global, revision: limits.global.revision + 1, paused: body.paused, reason: body.reason };
      }
      if (url.pathname.endsWith('/user')) {
        const existing = limits.users.find(user => user.twitch_user_id === body.twitch_user_id);
        assert.equal(body.expected_revision, existing?.revision ?? 0);
        const user = { ...limitFixture.users[0], ...body, revision: (existing?.revision ?? 0) + 1 };
        limits.users = [...limits.users.filter(user => user.twitch_user_id !== body.twitch_user_id), user];
      }
      if (url.pathname.endsWith('/plan')) {
        assert.equal(body.expected_revision, limits.plans[0].revision);
        limits.plans[0] = { ...limits.plans[0], ...body, revision: limits.plans[0].revision + 1 };
        assert.equal(limits.plan_policies_active, false);
      }
      return send(limits);
    }
    if (url.pathname.endsWith('/limits')) return send(limits);
    const grouping = url.searchParams.get('group_by');
    const offset = Number(url.searchParams.get('offset'));
    const report = usage(grouping, offset);
    if (mode === 'empty') { report.groups = []; report.totals = { ...report.totals, attempts: 0, estimated_micro_usd: null, held_micro_usd: null }; report.has_more = false; }
    if (mode === 'unknown') { report.totals.incomplete_attempts = 1; report.totals.unknown_cost_attempts = 1; report.groups[0].incomplete_attempts = 1; report.groups[0].unknown_cost_attempts = 1; }
    if (mode === 'stale' && grouping === 'user') {
      report.groups[0].provider = 'Veraltete Antwort';
      await new Promise(resolve => { releaseStale = resolve; heldStale?.(); });
      try { return await send(report); } catch { return; }
    }
    return send(report);
  });

  async function clickButton(name, role = 'button') {
    const button = page.getByRole(role, { name, exact: true });
    await page.screenshot();
    await button.scrollIntoViewIfNeeded();
    await page.screenshot();
    await button.click();
    await page.screenshot();
  }

  async function checkPause() {
    const checkbox = page.getByLabel('Neue KI-Anfragen global pausieren', { exact: true });
    await page.screenshot();
    await checkbox.scrollIntoViewIfNeeded();
    await page.screenshot();
    await checkbox.check();
    await page.screenshot();
  }

  async function setGrouping(value) {
    const select = page.getByRole('combobox', { name: /^Gruppierung/ });
    await select.selectOption(value);
    await select.evaluate(element => element.dispatchEvent(new Event('change', { bubbles: true })));
    await page.screenshot();
  }

  async function capture(name, selector) {
    if (selector) await page.locator(selector).scrollIntoViewIfNeeded();
    else await page.evaluate(() => window.scrollTo({ top: 0, behavior: 'instant' }));
    await page.evaluate(async () => {
      await document.fonts.ready;
      if (typeof document.getAnimations === 'function') await Promise.all(document.getAnimations().map(animation => animation.finished.catch(() => {})));
      else await new Promise(resolve => setTimeout(resolve, 250));
    });
    await page.screenshot();
    const geometry = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth }));
    assert.ok(geometry.document <= geometry.viewport + 1, `Horizontaler Überlauf: ${JSON.stringify(geometry)}`);
    await page.screenshot({ path: `${evidenceDir}/${name}-${width}.png`, fullPage: false });
    results.push({ name, width, geometry });
  }

  try {
    await page.goto(`${origin}/twitch/admin/content/social-ai`, { waitUntil: 'networkidle' });
    assert.ok(page.url().endsWith('/content/social-ai/usage'));
    assert.equal(await page.locator('.social-ai nav a').count(), 3);
    assert.equal(await page.locator('a[href*="social-ai/skills"]').count(), 1);
    await page.locator(groupTable).waitFor({ state: 'attached' });
    await page.screenshot();
    await capture('usage');
    await setGrouping('job');
    await page.waitForFunction(() => document.querySelector('.social-ai-table[aria-label^="Verbrauch nach Gruppierung"]')?.textContent.includes('Gesamtlaufzeit'));
    const table = await page.locator(groupTable).textContent();
    assert.ok(table.includes('18,4 s') && table.includes('6,8 s') && table.includes('8,4 s'));
    await page.getByLabel('Anbieter', { exact: true }).fill('synthetic-filter');
    await clickButton('Filter anwenden');
    await clickButton('Weiter');
    await page.waitForFunction(() => document.querySelector('.social-ai')?.textContent.includes('Seite 2'));
    assert.equal(await page.getByRole('button', { name: 'Weiter', exact: true }).isDisabled(), true);
    mode = 'unknown';
    await clickButton('Stand neu laden');
    await page.waitForFunction(() => document.querySelector('.social-ai')?.textContent.includes('Vollständige Kostensummen sind unbekannt'));
    const costCells = await page.locator('.social-ai-summary dd').allTextContents();
    assert.equal(costCells[1], costCells[2]);
    assert.equal(costCells[2], costCells[3]);
    await capture('usage-unknown', '.social-ai-summary');
    mode = 'stale';
    const held = new Promise(resolve => { heldStale = resolve; });
    await setGrouping('user');
    await held;
    await setGrouping('provider');
    await page.locator(groupTable).waitFor({ state: 'attached' });
    await page.screenshot();
    releaseStale();
    assert.equal(await page.locator(groupTable).textContent().then(text => text.includes('Veraltete Antwort')), false);
    mode = 'empty';
    await clickButton('Stand neu laden');
    await page.locator('.social-ai .empty-state').waitFor({ state: 'attached' });
    await page.screenshot();
    mode = 'unavailable';
    await clickButton('Stand neu laden');
    await page.getByRole('alert').waitFor({ state: 'attached' });
    await page.screenshot();
    assert.equal(await page.locator(groupTable).count(), 0);

    mode = 'normal';
    await clickButton('Verarbeitungsgrenzen', 'link');
    await page.getByRole('heading', { name: 'Globale Verarbeitung', exact: true }).waitFor({ state: 'attached' });
    await page.screenshot();
    await capture('limits');
    await checkPause();
    await page.getByLabel('Begründung für die globale Änderung', { exact: true }).fill('Synthetischer Pausentest');
    conflict = true;
    await clickButton('Globale Grenzen speichern');
    await page.getByRole('alert').waitFor({ state: 'attached' });
    await page.screenshot();
    assert.equal(await page.getByRole('button', { name: 'Globale Grenzen speichern', exact: true }).isDisabled(), true);
    await clickButton('Stand neu laden');
    await checkPause();
    await page.getByLabel('Begründung für die globale Änderung', { exact: true }).fill('Synthetischer Pausentest nach Konflikt');
    await clickButton('Globale Grenzen speichern');
    await page.waitForFunction(() => document.querySelector('.social-ai')?.textContent.includes('Revision 2'));
    await page.getByLabel('Twitch-Nutzer-ID', { exact: true }).fill('100000002');
    await page.getByLabel(/^Monatsbudget in USD/).fill('20,000001');
    await page.getByLabel('Begründung für die Nutzergrenze', { exact: true }).fill('Synthetische Nutzergrenze');
    assert.equal(await page.getByRole('button', { name: 'Nutzergrenze speichern', exact: true }).isDisabled(), true);
    await page.getByLabel(/^Monatsbudget in USD/).fill('0');
    await clickButton('Nutzergrenze speichern');
    await page.getByRole('button', { name: 'Nutzer 100000002 bearbeiten', exact: true }).waitFor({ state: 'attached' });
    await page.screenshot();
    assert.equal(limits.users.find(user => user.twitch_user_id === '100000002').monthly_cap_micro_usd, 0);
    await page.getByLabel('Vorgemerkte KI-Aufträge pro Monat', { exact: true }).fill('3');
    await page.getByLabel('Begründung für die Planregel', { exact: true }).fill('Synthetische inaktive Planregel');
    await clickButton('Planregel inaktiv vormerken');
    await page.waitForFunction(() => document.querySelector('.social-ai')?.textContent.includes('Vorgemerkte Revision 1'));
    assert.equal(limits.plan_policies_active, false);
    await capture('plan', '.social-ai form:last-child');
    limits.enforcement_available = false;
    limits.plan_configuration_available = false;
    await clickButton('Stand neu laden');
    await page.waitForFunction(() => document.querySelector('.social-ai')?.textContent.includes('Die serverseitige Durchsetzung ist noch nicht angebunden'));
    assert.equal(await page.getByLabel('Neue KI-Anfragen global pausieren', { exact: true }).isDisabled(), true);
    assert.equal(await page.getByRole('button', { name: 'Planregel inaktiv vormerken', exact: true }).isDisabled(), true);
    assert.deepEqual(errors, []);
    results.push({ width, functional: 'passed', fixtureWrites: writes, pageErrors: errors, live: false });
  } finally { await context.close(); }
}

try {
  for (const width of [1440, 390]) await run(width);
  await writeFile(`${evidenceDir}/checks.json`, JSON.stringify(results, null, 2));
  console.log(JSON.stringify(results));
} finally { await browser.close(); }
