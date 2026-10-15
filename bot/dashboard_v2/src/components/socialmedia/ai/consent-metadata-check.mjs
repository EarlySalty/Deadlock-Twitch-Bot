import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { limits } from '../../../../../admin_dashboard/src/api/socialAiFixtures.mjs';

const directory = process.argv[2];
if (!directory) throw new Error('Screenshot directory required');
await mkdir(directory, { recursive: true });
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.MOLI_PORT ?? '9247'}`);
const context = browser.contexts()[0] ?? await browser.newContext();
const page = await context.newPage();
const dashboard = `http://127.0.0.1:${process.env.PREVIEW_PORT ?? '4197'}/src/components/socialmedia/ai/preview.html`;

async function shot(name, width, locator) {
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.setViewportSize({ width, height: 1000 });
  await page.evaluate(async () => { await document.fonts.ready; await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))); });
  await locator.evaluate(element => element.scrollIntoView({ block: 'start' }));
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const size = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth }));
  await page.screenshot({ path: `${directory}/${name}-${width}.png` });
  assert.ok(size.document <= size.viewport, `${name}: horizontal overflow ${JSON.stringify(size)}`);
}

try {
  await page.setViewportSize({ width: 390, height: 1000 });
  await page.goto(`${dashboard}?case=review`);
  const panel = page.getByRole('form', { name: 'Titel, Beschreibung und Hashtags' });
  await panel.waitFor({ state: 'attached' });
  const shared = panel.getByRole('radio', { name: 'Für alle Plattformen gleich' });
  const separate = panel.getByRole('radio', { name: 'Je Plattform getrennt' });
  assert.ok(await shared.isChecked());
  assert.equal(await panel.getByLabel('Titel', { exact: true }).count(), 1);
  await page.evaluate(() => {
    const original = window.fetch;
    window.__metadataSaves = [];
    window.fetch = (input, init) => {
      if (String(input).includes('/metadata') && init?.method === 'PUT') window.__metadataSaves.push(JSON.parse(init.body));
      return original(input, init);
    };
  });
  await panel.getByLabel('Titel', { exact: true }).fill('Gemeinsamer Titel', { force: true });
  await panel.getByLabel('Beschreibung', { exact: true }).fill('Gemeinsame Beschreibung', { force: true });
  await panel.getByLabel('Hashtags', { exact: true }).fill('#Deadlock #Test', { force: true });
  await separate.focus();
  await separate.press('Space');
  assert.equal(await panel.getByLabel('Beschreibung', { exact: true }).count(), 3);
  for (const description of await panel.getByLabel('Beschreibung', { exact: true }).all()) assert.equal(await description.inputValue(), 'Gemeinsame Beschreibung');
  await shared.focus();
  await shared.press('Space');
  await panel.getByLabel('Titel', { exact: true }).fill('x'.repeat(101), { force: true });
  assert.ok(await panel.getByRole('button', { name: 'Texte speichern', exact: true }).isDisabled());
  await panel.getByLabel('Titel', { exact: true }).fill('Gemeinsamer Titel', { force: true });
  await panel.getByLabel('Beschreibung', { exact: true }).fill('x'.repeat(2201), { force: true });
  assert.ok(await panel.getByRole('button', { name: 'Texte speichern', exact: true }).isDisabled());
  await panel.getByLabel('Beschreibung', { exact: true }).fill('Gemeinsame Beschreibung', { force: true });
  await panel.getByRole('button', { name: 'Texte speichern', exact: true }).focus();
  await page.keyboard.press('Enter');
  await panel.getByRole('status').filter({ hasText: 'sind gespeichert' }).waitFor({ state: 'attached' });
  const saved = await page.evaluate(() => window.__metadataSaves.at(-1).platforms);
  assert.deepEqual(saved.youtube, saved.tiktok);
  assert.deepEqual(saved.youtube, saved.instagram);
  for (const width of [390]) await shot('metadata-shared', width, panel);
  await separate.focus();
  await separate.press('Space');
  await panel.getByLabel('Beschreibung', { exact: true }).nth(1).fill('Nur TikTok', { force: true });
  await panel.getByRole('button', { name: 'Texte speichern', exact: true }).focus();
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.__metadataSaves.length === 2);
  await panel.getByRole('status').filter({ hasText: 'sind gespeichert' }).waitFor({ state: 'attached' });
  const distinct = await page.evaluate(() => window.__metadataSaves.at(-1).platforms);
  assert.equal(distinct.tiktok.description, 'Nur TikTok');
  assert.equal(distinct.youtube.description, 'Gemeinsame Beschreibung');
  for (const width of [390]) await shot('metadata-separate', width, panel);
  await page.goto(`${dashboard}?case=global-off`);
  const message = page.getByText('Ein Admin muss die externe KI erst freigeben. Bis dahin kannst du keine KI-Bearbeitung starten.');
  await message.waitFor({ state: 'attached' });
  for (const width of [390]) await shot('streamer-admin-required', width, message);
  await page.addInitScript(({ fixture }) => {
    let enabled = false;
    window.__adminPuts = [];
    const original = window.fetch.bind(window);
    window.fetch = async (input, init) => {
      const url = new URL(String(input), location.href);
      if (!url.pathname.startsWith('/twitch/api/')) return original(input, init);
      let data = {};
      if (url.pathname.endsWith('/auth-status')) data = { authenticated: true, isAdmin: true, csrfToken: 'synthetic-csrf', displayName: 'Testadmin' };
      if (url.pathname.endsWith('/social-ai/limits')) data = fixture;
      if (url.pathname.endsWith('/social-ai/external-consent')) {
        if (init?.method === 'PUT') {
          const body = JSON.parse(init.body);
          window.__adminPuts.push({ body, headers: init.headers });
          enabled = body.enabled;
          if (window.__failConsentPut) return new Response('{}', { status: 503, headers: { 'Content-Type': 'application/json' } });
        }
        data = { enabled };
      }
      return new Response(JSON.stringify(data), { headers: { 'Content-Type': 'application/json' } });
    };
  }, { fixture: limits });
  await page.goto(`http://127.0.0.1:${process.env.ADMIN_PORT ?? '4198'}/twitch/admin/content/social-ai/limits`);
  const toggle = page.getByRole('switch', { name: 'Externe KI global freigeben' });
  await toggle.waitFor({ state: 'attached' });
  assert.ok(!await toggle.isChecked());
  await toggle.focus();
  await toggle.press('Space');
  await page.waitForFunction(() => document.querySelector('[role="switch"]')?.checked === true);
  const requests = await page.evaluate(() => window.__adminPuts);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].body.enabled, true);
  assert.equal(requests[0].body.csrf_token, 'synthetic-csrf');
  assert.equal(requests[0].headers['X-CSRF-Token'], 'synthetic-csrf');
  for (const width of [390]) await shot('admin-consent', width, toggle.locator('xpath=ancestor::section'));
  await page.evaluate(() => { window.__failConsentPut = true; });
  await toggle.focus();
  await toggle.press('Space');
  await page.getByRole('alert').filter({ hasText: 'Freigabe neu' }).waitFor({ state: 'attached' });
  assert.ok(await toggle.isDisabled());
  await page.getByRole('button', { name: 'Freigabe neu laden' }).focus();
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.querySelector('[role="switch"]')?.disabled === false);
  assert.ok(!await toggle.isChecked());
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`http://127.0.0.1:${process.env.ADMIN_PORT ?? '4198'}/twitch/admin/content/social-ai/limits`);
  await toggle.waitFor({ state: 'attached' });
  await shot('admin-consent', 1440, toggle.locator('xpath=ancestor::section'));
  await page.goto(`${dashboard}?case=review`);
  await panel.waitFor({ state: 'attached' });
  await shot('metadata-shared', 1440, panel);
  await separate.focus();
  await separate.press('Space');
  await shot('metadata-separate', 1440, panel);
  await page.goto(`${dashboard}?case=global-off`);
  await message.waitFor({ state: 'attached' });
  await shot('streamer-admin-required', 1440, message);
  console.log('Consent and metadata browser contracts passed at 390 and 1440 pixels.');
} finally {
  await page.close();
  await browser.close();
}
