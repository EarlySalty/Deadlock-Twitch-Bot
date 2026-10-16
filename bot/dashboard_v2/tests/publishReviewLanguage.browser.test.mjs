import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { createServer as createPortProbe } from 'node:net';
import { createWriteStream } from 'node:fs';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { build } from 'vite';
import { chromium } from 'playwright-core';
import { translate, LOCALES } from '../src/i18n/dictionary.ts';

const frontend = path.resolve(import.meta.dirname, '..');
const artifacts = await mkdtemp('/tmp/ttb-publishing-language-');
const dist = path.join(artifacts, 'built-fixture');
const fixturePath = 'src/components/socialmedia/autopilot/publish-review/fixture.html';
const fontDirectory = '/home/nathanael/repos/Website/dl-brand/fonts';
const fontsConfig = path.join(artifacts, 'fonts.conf');
const images = [];
const requests = [];

async function language(page, value) {
  await page.evaluate((next) => window.publishReviewFixture.setLanguage(next), value);
  await page.waitForFunction((next) => document.documentElement.lang === next, value);
}

async function calls(page) {
  return page.evaluate(() => window.publishReviewFixture.calls);
}

async function settled(page) {
  await page.waitForFunction(() => document.querySelector('.publish-video') && document.querySelector('.publish-review')?.getAttribute('aria-busy') === 'false');
}

async function screenshot(page, name) {
  await page.evaluate(async () => { await document.fonts.ready; });
  const file = path.join(artifacts, `${name}.png`);
  await page.screenshot({ path: file });
  const bytes = await readFile(file);
  const dimensions = { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
  assert.deepEqual(dimensions, page.viewportSize());
  const measured = await page.evaluate(() => ({ width: innerWidth, documentWidth: document.documentElement.scrollWidth, documentHeight: document.documentElement.scrollHeight, scrollY, stylesheets: document.styleSheets.length, fonts: document.fonts.status, dialog: (() => { const dialog = document.querySelector('dialog'); return dialog ? { open: dialog.open, display: getComputedStyle(dialog).display, rect: dialog.getBoundingClientRect().toJSON() } : null; })() }));
  assert.ok(measured.documentWidth <= measured.width, JSON.stringify(measured));
  assert.ok(measured.stylesheets > 0);
  const buttons = await page.locator('.publish-review button').evaluateAll((elements) => elements.map((button) => {
    const style = getComputedStyle(button);
    const range = document.createRange();
    range.selectNodeContents(button);
    return {
      text: button.textContent,
      className: button.className,
      rect: button.getBoundingClientRect().toJSON(),
      textRect: range.getBoundingClientRect().toJSON(),
      textLines: Array.from(range.getClientRects(), (rect) => rect.toJSON()),
      clientWidth: button.clientWidth,
      scrollWidth: button.scrollWidth,
      clientHeight: button.clientHeight,
      scrollHeight: button.scrollHeight,
      style: Object.fromEntries(['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'whiteSpace', 'display', 'height', 'minHeight', 'maxHeight', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'flex', 'boxSizing'].map((key) => [key, style[key]])),
    };
  }));
  images.push({ file, ...dimensions, ...measured, buttons });
}

async function expectDateAndDuration(page, currentLanguage) {
  const expected = await page.evaluate((locale) => {
    const video = window.publishReviewFixture.snapshot().videos[0];
    return {
      date: new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(video.expires_at)),
      duration: new Intl.NumberFormat(locale, { style: 'unit', unit: 'second', unitDisplay: 'long' }).format(video.duration_ms / 1000),
      timezone: new Intl.DateTimeFormat().resolvedOptions().timeZone,
    };
  }, LOCALES[currentLanguage]);
  assert.ok((await page.locator('.publish-video').first().locator('.publish-expiry').textContent()).includes(expected.date));
  assert.ok((await page.locator('.publish-video').first().locator('header p').textContent()).includes(expected.duration));
  return expected.timezone;
}

test('built publication panel preserves state and effects across language changes in Moli', { timeout: 240000 }, async (t) => {
  await writeFile(fontsConfig, `<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>/usr/share/fonts</dir>
  <dir>/usr/local/share/fonts</dir>
  <cachedir>${artifacts}/font-cache</cachedir>
  <alias><family>sans-serif</family><prefer><family>DejaVu Sans</family></prefer></alias>
  <alias><family>serif</family><prefer><family>DejaVu Serif</family></prefer></alias>
  <alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family></prefer></alias>
  <alias><family>cursive</family><prefer><family>DejaVu Sans</family></prefer></alias>
  <alias><family>fantasy</family><prefer><family>DejaVu Sans</family></prefer></alias>
</fontconfig>
`);
  await build({ root: frontend, configFile: path.join(frontend, 'vite.config.ts'), mode: 'preview', build: { outDir: dist, emptyOutDir: true, rollupOptions: { input: path.join(frontend, fixturePath) } } });
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    requests.push({ method: request.method, path: url.pathname });
    const isFont = url.pathname.startsWith('/brand/fonts/');
    const root = isFont ? fontDirectory : dist;
    const file = path.resolve(root, isFont ? path.basename(url.pathname) : `.${decodeURIComponent(url.pathname)}`);
    if (!file.startsWith(`${root}/`)) { response.writeHead(403).end(); return; }
    try {
      const body = await readFile(file);
      const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png' };
      response.writeHead(200, { 'content-type': types[path.extname(file)] ?? 'application/octet-stream' });
      response.end(body);
    } catch { response.writeHead(404).end(); }
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())));
  const probe = createPortProbe();
  probe.listen(0, '127.0.0.1');
  await once(probe, 'listening');
  const port = probe.address().port;
  await new Promise((resolve) => probe.close(resolve));
  const log = createWriteStream(path.join(artifacts, 'moli.log'));
  const moli = spawn('/home/nathanael/.local/bin/moli', ['serve', '--host', '127.0.0.1', '--port', String(port), '--timeout', '60', '--layout', '--image', '--font'], { env: { PATH: '/usr/local/bin:/usr/bin:/bin', HOME: artifacts, XDG_CACHE_HOME: artifacts, FONTCONFIG_FILE: fontsConfig, FONTCONFIG_PATH: artifacts }, stdio: ['ignore', 'pipe', 'pipe'] });
  moli.stdout.pipe(log);
  moli.stderr.pipe(log);
  t.after(async () => { if (moli.exitCode === null) { moli.kill('SIGTERM'); await once(moli, 'exit'); } log.end(); });
  let ready = false;
  for (let attempt = 0; attempt < 100 && !ready; attempt++) {
    if (moli.exitCode !== null) throw new Error(`Moli exited with ${moli.exitCode}`);
    try { ready = (await fetch(`http://127.0.0.1:${port}/json/version`)).ok; } catch { await new Promise((resolve) => setTimeout(resolve, 50)); }
  }
  assert.ok(ready, 'Moli CDP startup');
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  t.after(() => browser.close());
  const failures = [];
  async function open() {
    const page = await browser.contexts()[0].newPage();
    page.on('pageerror', (error) => failures.push(error.message));
    page.on('requestfailed', (request) => failures.push(request.url()));
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto(`http://127.0.0.1:${server.address().port}/${fixturePath}`, { waitUntil: 'load' });
    await settled(page);
    await language(page, 'de');
    return page;
  }

  await t.test('locale changes preserve dirty preferences, selected destinations and explicit pending choice without loads', async () => {
    const page = await open();
    try {
      const video = page.locator('.publish-video').first();
      await video.locator('[aria-label="YouTube"] input').evaluate((element) => element.click());
      await page.locator('.publish-preferences summary').evaluate((element) => element.click());
      await page.locator('.publish-preferences input[type="number"]').fill('36');
      await page.evaluate(() => { window.originalPanel = document.querySelector('.publish-review'); window.originalVideo = document.querySelector('.publish-video'); });
      const initialCalls = await calls(page);
      const timezones = [];
      const caption = await page.locator('textarea').inputValue();
      for (const width of [390, 1440]) {
        await page.setViewportSize({ width, height: 1000 });
        for (const currentLanguage of ['de', 'en']) {
          await language(page, currentLanguage);
          timezones.push(await expectDateAndDuration(page, currentLanguage));
          assert.equal(await video.locator('[aria-label="YouTube"] input').isChecked(), true);
          assert.equal(await page.locator('.publish-preferences input[type="number"]').inputValue(), '36');
          assert.equal(await page.locator('.publish-review').getAttribute('data-unsaved'), 'true');
          assert.equal(await page.locator('textarea').inputValue(), caption);
          await page.evaluate(() => scrollTo(0, 0));
          await screenshot(page, `draft-${currentLanguage}-${width}`);
          await page.locator('.publish-preferences').scrollIntoViewIfNeeded();
          await screenshot(page, `preferences-${currentLanguage}-${width}`);
        }
      }
      await language(page, 'de');
      await page.locator('.publish-mode-row button').first().evaluate((element) => element.click());
      assert.equal(await page.locator('dialog .publish-primary').isDisabled(), true);
      await page.locator('dialog input[type="radio"]').nth(1).evaluate((element) => element.click());
      for (const width of [390, 1440]) {
        await page.setViewportSize({ width, height: 1000 });
        for (const currentLanguage of ['de', 'en', 'de']) {
          await language(page, currentLanguage);
          assert.equal(await page.locator('dialog input[type="radio"]').nth(1).isChecked(), true);
          assert.equal(await page.locator('dialog .publish-primary').isEnabled(), true);
          assert.equal(await page.locator('dialog').evaluate((dialog) => dialog.open), true);
          if (currentLanguage === 'en' || images.filter((image) => image.file.endsWith(`dialog-de-${width}.png`)).length === 0) await screenshot(page, `dialog-${currentLanguage}-${width}`);
        }
      }
      assert.equal(await page.evaluate(() => window.originalPanel === document.querySelector('.publish-review') && window.originalVideo === document.querySelector('.publish-video')), true);
      assert.deepEqual(await calls(page), initialCalls);
      assert.equal(new Set(timezones).size, 1);
      await page.locator('dialog .publish-primary').evaluate((element) => element.click());
      await page.waitForFunction(() => !document.querySelector('dialog'));
      assert.deepEqual((await calls(page)).filter((call) => call.method === 'transition'), [{ method: 'transition', owner: 'synthetic-owner', payload: { mode: 'full_auto', confirmed: true, pending: 'transfer_eligible' } }]);
      assert.equal(await page.locator('.publish-preferences input[type="number"]').inputValue(), '36');
      assert.equal(await video.locator('[aria-label="YouTube"] input').isChecked(), true);
      await page.locator('.publish-preferences button[type="submit"]').evaluate((element) => element.click());
      await settled(page);
      assert.equal((await calls(page)).find((call) => call.method === 'preferences').payload.reminder_hours, 36);
      assert.equal(await page.locator('.publish-review').getAttribute('data-unsaved'), null);
    } finally { await page.close(); }
  });

  await t.test('transient template keys rerender while raw backend errors and stored notifications stay unchanged', async () => {
    const page = await open();
    try {
      const originalNotifications = await page.locator('.publish-notifications p').allTextContents();
      await page.locator('.publish-video').nth(1).locator('[aria-label="Instagram"] button').evaluate((element) => element.click());
      await page.waitForFunction(() => document.querySelector('.publish-message'));
      const savedCalls = await calls(page);
      for (const currentLanguage of ['de', 'en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('.publish-message').textContent(), translate(currentLanguage, '{platform} wird mit derselben Datei erneut geprüft.', { platform: 'Instagram' }));
        assert.deepEqual(await page.locator('.publish-notifications p').allTextContents(), originalNotifications);
        for (const width of [390, 1440]) {
          await page.setViewportSize({ width, height: 1000 });
          await screenshot(page, `success-${currentLanguage}-${width}`);
        }
      }
      assert.deepEqual(await calls(page), savedCalls);
      assert.deepEqual(savedCalls.filter((call) => call.method === 'retry'), [{ method: 'retry', owner: 'synthetic-owner', payload: { id: 102, platform: 'instagram' } }]);
      await page.evaluate(() => window.publishReviewFixture.failNext('pause', 'http_500'));
      await page.locator('.publish-mode-row button').nth(1).evaluate((element) => element.click());
      await page.waitForFunction(() => document.querySelector('.publish-review > .publish-error'));
      const failedCalls = await calls(page);
      for (const currentLanguage of ['en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('.publish-review > .publish-error').textContent(), translate(currentLanguage, 'Die Änderung konnte nicht gespeichert werden. Bitte prüfe die Verbindung und versuche es erneut.'));
        for (const width of [390, 1440]) {
          await page.setViewportSize({ width, height: 1000 });
          await screenshot(page, `error-${currentLanguage}-${width}`);
        }
      }
      assert.deepEqual(await calls(page), failedCalls);
      await page.evaluate(() => window.publishReviewFixture.failNext('pause', 'Verbindung'));
      await page.locator('.publish-mode-row button').nth(1).evaluate((element) => element.click());
      await page.waitForFunction(() => document.querySelector('.publish-review > .publish-error')?.textContent === 'Verbindung');
      await language(page, 'en');
      assert.equal(await page.locator('.publish-review > .publish-error').textContent(), 'Verbindung');
    } finally { await page.close(); }
  });

  await t.test('TikTok cancellation stays local and approval preserves raw caption, consent and account/hash options', async () => {
    const page = await open();
    try {
      const video = page.locator('.publish-video').first();
      const caption = 'Verbindung #Überraschung';
      await page.locator('textarea').fill(caption);
      await video.locator('[aria-label="TikTok"] input').evaluate((element) => element.click());
      await page.evaluate(() => window.publishReviewFixture.setTikTokCancelled(true));
      await video.locator('.publish-primary').evaluate((element) => element.click());
      await page.waitForFunction(() => document.querySelector('.publish-review > .publish-error'));
      const cancelledCalls = await calls(page);
      assert.equal(cancelledCalls.filter((call) => call.method === 'approve').length, 0);
      for (const currentLanguage of ['en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('.publish-review > .publish-error').textContent(), translate(currentLanguage, 'Bitte prüfe die TikTok-Einstellungen dieser Videoversion.'));
        assert.equal(await page.locator('textarea').inputValue(), caption);
        assert.equal(await video.locator('[aria-label="TikTok"] input').isChecked(), true);
        for (const width of [390, 1440]) {
          await page.setViewportSize({ width, height: 1000 });
          await screenshot(page, `caption-${currentLanguage}-${width}`);
        }
      }
      assert.deepEqual(await calls(page), cancelledCalls);
      await page.evaluate(() => window.publishReviewFixture.setTikTokCancelled(false));
      await video.locator('[aria-label="YouTube"] input').evaluate((element) => element.click());
      await video.locator('.publish-primary').evaluate((element) => element.click());
      await page.waitForFunction(() => document.querySelector('.publish-message'));
      const approval = (await calls(page)).find((call) => call.method === 'approve');
      assert.deepEqual(approval, {
        method: 'approve', owner: 'synthetic-owner', payload: {
          id: 101, decisions: [
            { platform: 'tiktok', tiktok_options: { caption, privacy_level: 'SELF_ONLY', allow_comment: false, allow_duet: false, allow_stitch: false, commercial_content: false, brand_organic_toggle: false, brand_content_toggle: false, consent: true, creator_username: 'synthetic-creator', credential_id: 7, platform_user_id: 'synthetic-tiktok', approved_video_sha256: 'synthetic-hash', video_path: 'synthetic-video.mp4' } },
            { platform: 'youtube' },
          ]
        }
      });
      assert.equal(await video.locator('[aria-label="YouTube"] input').isChecked(), false);
      assert.equal(await video.locator('[aria-label="TikTok"] input').isChecked(), false);
      await settled(page);
      assert.deepEqual((await calls(page)).find((call) => call.method === 'schedule'), { method: 'schedule', owner: 'synthetic-owner', payload: { id: 101, platforms: ['youtube', 'instagram', 'tiktok'] } });
      await video.locator('header button').evaluate((element) => element.click());
      assert.deepEqual(await page.evaluate(() => window.publishReviewFixture.callbacks), [{ method: 'configureTikTok', id: 101 }, { method: 'configureTikTok', id: 101 }, { method: 'open', id: 101 }]);
    } finally { await page.close(); }
  });

  await t.test('saving fence survives language changes and actual owner changes reset drafts and reject late results', async () => {
    const page = await open();
    try {
      await page.locator('.publish-video').first().locator('[aria-label="YouTube"] input').evaluate((element) => element.click());
      await page.locator('.publish-preferences summary').evaluate((element) => element.click());
      await page.locator('.publish-preferences input[type="number"]').fill('48');
      await page.evaluate(() => { window.releasePause = window.publishReviewFixture.holdNext('pause'); });
      await page.locator('.publish-mode-row button').nth(1).evaluate((element) => element.click());
      await language(page, 'en');
      assert.equal(await page.locator('.publish-review').getAttribute('data-saving'), 'true');
      assert.equal(await page.locator('.publish-mode-row button').nth(1).isDisabled(), true);
      await page.locator('.publish-mode-row button').nth(1).evaluate((button) => { button.click(); button.click(); });
      await page.evaluate(() => { window.oldOwnerPanel = document.querySelector('.publish-review'); window.publishReviewFixture.setOwner('synthetic-owner-b'); });
      await settled(page);
      assert.equal(await page.evaluate(() => window.oldOwnerPanel === document.querySelector('.publish-review')), false);
      assert.equal(await page.locator('.publish-video').first().locator('[aria-label="YouTube"] input').isChecked(), false);
      assert.equal(await page.locator('.publish-preferences input[type="number"]').inputValue(), '24');
      assert.equal(await page.locator('.publish-review').getAttribute('data-unsaved'), null);
      await page.locator('.publish-preferences summary').evaluate((element) => element.click());
      for (const width of [390, 1440]) {
        await page.setViewportSize({ width, height: 1000 });
        for (const currentLanguage of ['de', 'en']) {
          await language(page, currentLanguage);
          assert.equal(await page.locator('.publish-preferences input[type="number"]').inputValue(), '24');
          await page.evaluate(() => scrollTo(0, 0));
          await screenshot(page, `owner-reset-${currentLanguage}-${width}`);
          await page.locator('.publish-preferences').scrollIntoViewIfNeeded();
          await screenshot(page, `owner-preferences-${currentLanguage}-${width}`);
        }
      }
      await page.evaluate(() => window.releasePause());
      await page.waitForFunction(() => window.publishReviewFixture.calls.filter((call) => call.method === 'load' && call.owner === 'synthetic-owner').length === 2);
      assert.equal(await page.locator('.publish-message').count(), 0);
      assert.equal(await page.locator('.publish-mode-row button').nth(1).textContent(), translate('en', 'Pausieren'));
      assert.equal((await calls(page)).filter((call) => call.method === 'pause').length, 1);
      await page.evaluate(() => { window.releaseOwnerLoad = window.publishReviewFixture.holdNext('load'); window.publishReviewFixture.setOwner('synthetic-owner-c'); });
      await page.waitForFunction(() => !document.querySelector('.publish-video'));
      await language(page, 'de');
      await page.evaluate(() => window.publishReviewFixture.setOwner('synthetic-owner-d'));
      await settled(page);
      await page.evaluate(() => window.releaseOwnerLoad());
      assert.equal(await page.locator('.publish-review > .publish-error').count(), 0);
      assert.equal((await calls(page)).filter((call) => call.owner === 'synthetic-owner-c').length, 0);
    } finally { await page.close(); }
  });

  await t.test('owner changes reset open dialog and pending choice while held and uncertain destinations keep restrictions', async () => {
    const page = await open();
    try {
      await page.evaluate(() => {
        const fixture = window.publishReviewFixture;
        const snapshot = fixture.snapshot();
        snapshot.videos[0].event = { id: 701, produced_video_id: 101, scheduled_at: '2026-10-18T16:00:00Z', executed_at: null, completed_at: null, destinations: ['account_changed', 'exact_file_changed', 'tiktok_effect_boundary_pending'].map((reason, index) => ({ platform: ['youtube', 'instagram', 'tiktok'][index], state: 'held', reason, attempts: 0, next_retry_at: null, external_id: null, account_id: null, visibility: null })) };
        snapshot.videos[2].cleanup_error = 'Die Videodatei konnte nicht gelöscht werden. Wir versuchen es nicht automatisch erneut.';
        fixture.replace(snapshot);
      });
      await page.locator('.publish-review-header button').evaluate((element) => element.click());
      await settled(page);
      assert.equal(await page.locator('.publish-video').first().locator('.publish-destination button').count(), 0);
      assert.equal(await page.locator('.publish-video').nth(3).locator('.publish-destination button').count(), 0);
      assert.equal(await page.locator('.publish-video').nth(1).locator('[aria-label="YouTube"] input').isDisabled(), true);
      assert.equal(await page.locator('.publish-video').nth(1).locator('[aria-label="TikTok"] input').isDisabled(), true);
      await language(page, 'en');
      assert.equal(await page.locator('.publish-video').nth(2).locator('.publish-error').textContent(), translate('en', 'Die Videodatei konnte nicht gelöscht werden. Wir versuchen es nicht automatisch erneut.'));
      assert.equal((await page.evaluate(() => window.publishReviewFixture.snapshot())).videos[2].cleanup_error, 'Die Videodatei konnte nicht gelöscht werden. Wir versuchen es nicht automatisch erneut.');
      await page.locator('.publish-mode-row button').first().evaluate((element) => element.click());
      await page.locator('dialog input[type="radio"]').first().evaluate((element) => element.click());
      await language(page, 'de');
      await page.evaluate(() => window.publishReviewFixture.setOwner('synthetic-owner-next'));
      await settled(page);
      assert.equal(await page.locator('dialog').count(), 0);
      assert.equal(await page.locator('.publish-review').getAttribute('data-unsaved'), null);
      await page.locator('.publish-mode-row button').first().evaluate((element) => element.click());
      assert.equal(await page.locator('dialog input:checked').count(), 0);
      assert.equal(await page.locator('dialog .publish-primary').isDisabled(), true);
      assert.equal((await calls(page)).filter((call) => call.method === 'transition').length, 0);
    } finally { await page.close(); }
  });

  await t.test('initial loading and failed-load presentation changes language without restarting the owner load', async () => {
    const page = await open();
    try {
      await page.evaluate(() => { window.releaseInitialLoad = window.publishReviewFixture.holdNext('load'); window.publishReviewFixture.setOwner('synthetic-loading-owner'); });
      await page.waitForFunction(() => !document.querySelector('.publish-video'));
      const loadingCalls = await calls(page);
      for (const currentLanguage of ['en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('.publish-review > [role="status"]').textContent(), translate(currentLanguage, 'Veröffentlichungsdaten werden geladen.'));
        assert.equal(await page.locator('.publish-video').count(), 0);
      }
      assert.deepEqual(await calls(page), loadingCalls);
      await page.evaluate(() => window.releaseInitialLoad());
      await settled(page);
      await page.evaluate(() => { window.publishReviewFixture.failNext('load', 'synthetic failure'); window.publishReviewFixture.setOwner('synthetic-failed-owner'); });
      await page.waitForFunction(() => document.querySelector('.publish-review > .publish-error'));
      const failedCalls = await calls(page);
      for (const currentLanguage of ['en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('.publish-review > .publish-error').textContent(), translate(currentLanguage, 'Die Veröffentlichungsdaten konnten nicht geladen werden. Bitte versuche es erneut.'));
        assert.equal(await page.locator('.publish-video').count(), 0);
      }
      assert.deepEqual(await calls(page), failedCalls);
    } finally { await page.close(); }
  });

  await t.test('busy mode confirmation keeps the explicit choice and cancel fence across language changes', async () => {
    const page = await open();
    try {
      await page.locator('.publish-mode-row button').first().evaluate((element) => element.click());
      await page.locator('dialog input[type="radio"]').first().evaluate((element) => element.click());
      await page.evaluate(() => { window.releaseTransition = window.publishReviewFixture.holdNext('transition'); });
      await page.locator('dialog .publish-primary').evaluate((element) => element.click());
      for (const currentLanguage of ['en', 'de']) {
        await language(page, currentLanguage);
        assert.equal(await page.locator('dialog .publish-primary').isDisabled(), true);
        assert.equal(await page.locator('dialog input[type="radio"]').first().isChecked(), true);
        assert.equal(await page.locator('dialog').evaluate((dialog) => dialog.dispatchEvent(new Event('cancel', { cancelable: true }))), false);
        await page.locator('dialog .publish-primary').evaluate((button) => { button.click(); button.click(); });
      }
      assert.equal((await calls(page)).filter((call) => call.method === 'transition').length, 0);
      await page.evaluate(() => window.releaseTransition());
      await page.waitForFunction(() => !document.querySelector('dialog'));
      assert.deepEqual((await calls(page)).filter((call) => call.method === 'transition'), [{ method: 'transition', owner: 'synthetic-owner', payload: { mode: 'full_auto', confirmed: true, pending: 'keep_review' } }]);
    } finally { await page.close(); }
  });

  assert.deepEqual(failures, []);
  assert.equal(requests.filter((request) => request.method !== 'GET' || request.path.includes('/api/')).length, 0);
  assert.ok(requests.some((request) => request.path.endsWith('.css')));
  assert.ok(requests.some((request) => request.path.endsWith('.woff2')));
  await writeFile(path.join(artifacts, 'evidence.json'), JSON.stringify({ images, requests, failures, moli: '/home/nathanael/.local/bin/moli', fontsConfig }, null, 2));
  t.diagnostic(`Publication panel artifacts: ${artifacts}`);
});
