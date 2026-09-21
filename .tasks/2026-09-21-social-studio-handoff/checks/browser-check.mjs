import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { existsSync, readdirSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

// Runs against the offline reference, without product credentials or APIs.
const here = path.dirname(fileURLToPath(import.meta.url));
const reference = path.resolve(here, '../reference');
const screenshots = path.join(here, 'screenshots');
const cache = path.join(os.homedir(), '.cache/ms-playwright');
const cached = existsSync(cache)
  ? readdirSync(cache).filter(name => name.startsWith('chromium-')).sort()
      .flatMap(name => ['chrome-linux64/chrome', 'chrome-linux/chrome'].map(exe => path.join(cache, name, exe)))
  : [];
const executablePath = [process.env.BROWSER_PATH, '/usr/bin/chromium', '/usr/bin/chromium-browser', ...cached]
  .filter(Boolean).find(candidate => existsSync(candidate));
if (!executablePath) throw new Error('Set BROWSER_PATH to an installed Chromium executable.');
await fs.mkdir(screenshots, { recursive: true });
const html = await fs.readFile(path.join(reference, 'preview.html'), 'utf8');
const passed = [];
const widths = [];
const mark = name => { passed.push(name); console.log('PASS ' + name); };
const browser = await chromium.launch({ executablePath, headless: true, args: ['--no-sandbox'] });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1080 } });
  page.setDefaultTimeout(5000);
  const errors = [];
  let networkRequests = 0;
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', () => { networkRequests += 1; });
  await page.route('**/*', route => route.abort());
  const settle = async () => {
    await page.evaluate(async () => {
      await document.fonts.ready;
      await Promise.all([...document.images].map(img => img.complete ? Promise.resolve() : new Promise(resolve => {
        img.addEventListener('load', resolve, { once: true });
        img.addEventListener('error', resolve, { once: true });
      })));
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
  };
  const fit = async view => {
    const measurement = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth }));
    assert.ok(measurement.document <= measurement.viewport, view + ': horizontal overflow');
    widths.push({ view, ...measurement });
  };
  const shot = async name => {
    await settle();
    await page.screenshot({ path: path.join(screenshots, name + '.png'), fullPage: false, animations: 'disabled' });
  };
  const count = async expected => page.waitForFunction(n => document.querySelectorAll('article').length === n, expected);
  const textIncludes = async (selector, text) => page.waitForFunction(
    ({ selector, text }) => document.querySelector(selector)?.textContent.includes(text), { selector, text });

  await page.setContent(html, { waitUntil: 'load' });
  await count(3);
  await fit('desktop-queue');
  assert.equal(await page.locator('.studio-shell').evaluate(el => getComputedStyle(el).backgroundColor), 'rgb(13, 13, 13)');
  assert.match(await page.locator('.btn-primary').first().evaluate(el => getComputedStyle(el).backgroundImage), /rgb\(197, 160, 89\)/);
  assert.equal(await page.locator('.btn-primary').first().evaluate(el => getComputedStyle(el).color), 'rgb(36, 26, 18)');
  assert.ok(await page.locator('aside .brand-logo').evaluate(img => img.complete && img.naturalWidth === 96));
  await shot('preview-desktop');
  mark('Desktop: three review cards, brand colors, dark text on gold and loaded community logo');

  for (const [tab, name] of [['autopilot', 'preview-autopilot'], ['templates', 'preview-templates'], ['accounts', 'preview-accounts']]) {
    await page.locator('#tab-' + tab).click();
    await page.evaluate(() => scrollTo(0, 0));
    await fit('desktop-' + tab);
    await shot(name);
    if (tab === 'autopilot') {
      await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
      await shot('preview-autopilot-bottom');
      await page.evaluate(() => scrollTo(0, 0));
    }
  }
  mark('Settled viewport screenshots for the four work areas');

  await page.locator('#tab-queue').click();
  await page.locator('[data-action="approve"][data-id="1"]').click();
  await count(2);
  await page.locator('[data-filter="scheduled"]').click();
  await count(3);
  mark('Approval moves one demo clip to the scheduled filter after successful save');

  await page.locator('[data-filter="all"]').click();
  await page.locator('#search').fill('Hook');
  await count(1);
  await page.locator('#search').fill('no such clip 92347');
  await page.getByText('Keine Clips gefunden', { exact: true }).waitFor({ state: 'visible' });
  await page.locator('#search').fill('');
  mark('Search and empty-state handling');
  await page.locator('[data-layout="grid"]').click();
  assert.equal(await page.locator('#queue-list').getAttribute('class'), 'grid-view');
  await page.locator('[data-layout="list"]').click();
  mark('Grid and list switch');

  await page.locator('[data-action="menu"][data-id="2"]').click();
  await page.getByRole('button', { name: 'Transkript bearbeiten', exact: true }).click();
  await page.locator('#dialog').waitFor({ state: 'visible' });
  for (let i = 0; i < 9; i += 1) {
    assert.ok(await page.evaluate(() => document.querySelector('#dialog').contains(document.activeElement)));
    await page.keyboard.press('Tab');
  }
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#dialog').isVisible(), false);
  assert.ok(await page.locator('#clip-more-2').evaluate(el => el === document.activeElement));
  mark('Dialog focus containment, Escape and return to trigger');

  await page.locator('#tab-accounts').click();
  await page.locator('[data-action="fail-next"]').click();
  await page.locator('#tab-queue').click();
  await page.locator('[data-filter="review"]').click();
  await page.locator('[data-action="approve"][data-id="2"]').click();
  await textIncludes('[data-clip="2"] [role="alert"]', 'nicht gespeichert');
  assert.equal(await page.locator('[data-clip="2"]').getAttribute('data-status'), 'review');
  mark('Failed approval retains the clip and shows a local error');

  await page.locator('#tab-autopilot').click();
  const weekly = page.locator('[name="youtube.posts_per_week"]');
  await weekly.fill('71');
  await page.locator('#schedule-form button[type="submit"]').click();
  assert.equal(await weekly.getAttribute('aria-invalid'), 'true');
  assert.ok(await weekly.evaluate(el => el === document.activeElement));
  await weekly.fill('5');
  await page.locator('#schedule-form button[type="submit"]').click();
  await textIncludes('#save-status', 'Gespeichert');
  mark('Invalid schedule values rejected, valid draft saved');

  await page.locator('#tab-accounts').click();
  await page.locator('[data-action="fail-next"]').click();
  await page.locator('#tab-autopilot').click();
  await page.locator('[name="youtube.posts_per_week"]').fill('6');
  await page.locator('#schedule-form button[type="submit"]').click();
  await textIncludes('#save-status', 'nicht gespeichert');
  assert.equal(await page.locator('[name="youtube.posts_per_week"]').inputValue(), '6');
  await page.locator('[data-action="reset-schedule"]').click();
  assert.equal(await page.locator('[name="youtube.posts_per_week"]').inputValue(), '5');
  mark('Failed save preserves draft; reset restores last successful demo state');

  await page.locator('#tab-queue').focus();
  for (const [key, id] of [['ArrowRight', 'tab-autopilot'], ['End', 'tab-accounts'], ['Home', 'tab-queue']]) {
    await page.keyboard.press(key);
    assert.ok(await page.locator('#' + id).evaluate(el => el === document.activeElement));
  }
  mark('Tabs support arrow keys, Home and End');

  await page.locator('#tab-templates').click();
  await page.locator('[data-action="template"]').first().click();
  await page.locator('#position-slider').fill('70');
  await page.locator('#position-slider').dispatchEvent('input');
  await shot('preview-editor');
  await page.locator('#layout-form button[type="submit"]').click();
  await page.locator('#dialog').waitFor({ state: 'hidden' });
  await page.locator('[data-action="template"]').first().click();
  assert.equal(await page.locator('#position-slider').inputValue(), '70');
  await page.keyboard.press('Escape');
  mark('Schematic layout demo saves and restores local settings');

  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    assert.ok(await page.locator('header .brand-logo').isVisible());
    for (const tab of ['queue', 'autopilot', 'templates', 'accounts']) {
      await page.locator('#tab-' + tab).click();
      await fit(width + '-' + tab);
    }
    await page.locator('#tab-queue').click();
    if (width === 390) {
      await page.evaluate(() => scrollTo(0, 0));
      await shot('preview-mobile');
      await page.locator('article').first().scrollIntoViewIfNeeded();
      await shot('preview-mobile-cards');
    }
  }
  mark('Four mobile work areas at 390px and 320px, loaded logo, no document overflow');
  assert.deepEqual(errors, []);
  assert.equal(networkRequests, 0);
  mark('No JavaScript errors or network requests; no product APIs used');

  await fs.writeFile(path.join(here, 'browser-results.json'), JSON.stringify({
    passed: passed.length, checks: passed, browser: browser.version(),
    target: '../reference/preview.html only', font_rendering: 'system fallback; not proof of product fonts',
    screenshots: 'new viewport captures on the project host, not original chat PNGs',
    js_errors: errors.length, network_requests: networkRequests, widths,
  }, null, 2) + '\n');
  console.log('Reference browser checks passed: ' + passed.length);
} finally {
  await browser.close();
}
