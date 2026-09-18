// Run: npm exec --yes --package=playwright -- node tests/community-browser.cjs
// Requires the Vite dev server on loopback:4199. Does not use production APIs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const moduleRoot = process.env.PATH.split(path.delimiter)
  .map(bin => path.resolve(bin, '..', 'playwright'))
  .find(candidate => fs.existsSync(path.join(candidate, 'package.json')));
if (!moduleRoot) throw new Error('Run through npm exec --package=playwright.');
const { chromium } = require(moduleRoot);
const output = path.resolve('../../.tasks/community-coplay');
fs.mkdirSync(output, { recursive: true });
(async () => {
  const executablePath = process.env.COMMUNITY_CHROME_PATH || (fs.existsSync('/usr/local/bin/google-chrome') ? '/usr/local/bin/google-chrome' : undefined);
  const browser = await chromium.launch({ executablePath, headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    // Synthetic fixture; never send requests to Twitch, Discord or production.
    await context.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
    const page = await context.newPage();
    await page.clock.install();
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto('http://127.0.0.1:4199/twitch/dashboard-v2/tests/community-visual.html');
    await page.locator('[data-community-view]').waitFor();
    assert.equal(await page.getByRole('heading', { name: 'test_streamer_a', exact: true }).count(), 1);
    assert.equal(await page.getByRole('heading', { name: 'test_streamer_b', exact: true }).count(), 1);
    await page.getByLabel('Jetzt live', { exact: true }).check();
    assert.equal(await page.getByRole('heading', { name: 'test_streamer_b', exact: true }).count(), 0);
    await page.getByLabel('Jetzt live', { exact: true }).uncheck();
    await page.getByLabel('Nur mit VC-Platz', { exact: true }).uncheck();
    assert.equal(await page.getByRole('button', { name: 'Sprachkanal voll', exact: true }).isDisabled(), true);
    assert.equal(await page.getByRole('link', { name: 'Zum Streamer-VC', exact: true }).getAttribute('href'), 'https://discord.com/channels/1289721245281292288/1326984426906714236');
    await page.getByLabel('Vergleichen mit', { exact: true }).selectOption('test_streamer_b');
    assert.equal(await page.getByRole('button', { name: 'Zeiten vergleichen', exact: true }).nth(1).getAttribute('aria-pressed'), 'true');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), true, 'desktop overflow');
    await page.screenshot({ path: path.join(output, 'community-desktop.png'), fullPage: true });
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), true, 'mobile overflow');
    await page.screenshot({ path: path.join(output, 'community-mobile.png'), fullPage: true });
    await page.clock.fastForward(65_000);
    await page.getByRole('button', { name: 'Aktualisierung erforderlich', exact: true }).first().waitFor();
    assert.equal(await page.getByRole('link', { name: 'Zum Sprachkanal', exact: true }).count(), 0, 'expired lobby links');
    assert.equal(await page.getByText('Die Lobby-Anzeige ist nicht mehr aktuell.', { exact: false }).count(), 1);
    assert.deepEqual(errors, []);
    console.log('PASS: desktop/mobile layout, live filter, full lobby disabled, comparison selection, exact VC URL, stale snapshot closure, no runtime errors.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
