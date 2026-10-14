import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { createServer as createNetServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { chromium } from 'playwright-core';
import { build, preview } from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitFor(check, label) {
  for (let attempt = 0; attempt < 150; attempt += 1) {
    if (await check()) return;
    await sleep(100);
  }
  throw new Error(label);
}

async function freePort() {
  const server = createNetServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

test('Uplink-Hilfe wechselt EN DE EN ohne Entwurfsverlust und rendert alle Kapitel', { timeout: 180_000 }, async (t) => {
  const artifacts = await mkdtemp(join(tmpdir(), 'ttb-sol-uplink-r1-'));
  const outDir = join(artifacts, 'app');
  await build({
    root, mode: 'preview', base: '/', logLevel: 'error',
    build: { outDir, rollupOptions: { input: join(root, 'tests/browser/uplinkHelp.html') } },
  });
  const server = await preview({
    root, mode: 'preview', base: '/', logLevel: 'error', build: { outDir },
    preview: { host: '127.0.0.1', port: await freePort(), strictPort: true },
  });
  t.after(() => new Promise((resolve, reject) => server.httpServer.close((error) => error ? reject(error) : resolve())));
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const port = await freePort();
  const moli = spawn('/home/nathanael/.local/bin/moli', [
    'serve', '--host', '127.0.0.1', '--port', String(port), '--layout', '--image', '--font',
  ], { stdio: 'ignore' });
  t.after(async () => {
    if (moli.exitCode === null && moli.signalCode === null) {
      moli.kill('SIGTERM');
      await once(moli, 'exit');
    }
  });
  await waitFor(() => fetch(`http://127.0.0.1:${port}/json/version`).then((response) => response.ok).catch(() => false), 'Moli startet nicht');
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  t.after(() => browser.close());
  const context = await browser.newContext({ viewport: { width: 390, height: 900 } });
  t.after(() => context.close());
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin === origin || url.protocol === 'data:' ? route.continue() : route.abort();
  });
  const page = await context.newPage();
  t.after(async () => { console.log(JSON.stringify({ artifacts, errors, dom: await page.evaluate(() => document.body?.textContent?.slice(-2500)).catch(() => null) })); });
  const errors = [];
  const requests = [];
  const reports = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('request', (request) => {
    if (/\/uplink\/(?:en\/)?(?:was-ist|obs|stoerungen)\.html$/.test(request.url())) requests.push(request.url());
  });
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
  await page.goto(`${origin}/tests/browser/uplinkHelp.html`, { waitUntil: 'domcontentloaded' });
  await waitFor(() => page.evaluate(() => document.querySelectorAll('.uplink-doc').length === 3), 'Kapitel fehlen').catch(async (error) => {
    console.log(JSON.stringify({ errors, dom: await page.evaluate(() => document.documentElement.outerHTML.slice(-4000)) }));
    throw error;
  });
  await page.evaluate(() => {
    const card = document.querySelector('details[data-platform="youtube"]');
    if (!card) throw new Error('YouTube fixture fehlt');
    card.open = true;
  });
  await waitFor(() => page.evaluate(() => [...document.querySelectorAll('details[data-platform="youtube"] label')].some((label) => /Bitrate/i.test(label.textContent) && label.querySelector('input'))), 'Bearbeitbares Bitratenfeld fehlt');
  await page.evaluate(() => {
    const label = [...document.querySelectorAll('details[data-platform="youtube"] label')].find((element) => /Bitrate/i.test(element.textContent) && element.querySelector('input'));
    const input = label.querySelector('input');
    window.uplinkDraft = input;
    input.setAttribute('data-uplink-draft', '');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, '13000');
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await waitFor(() => page.evaluate(() => /13000|13[.,]000/.test(document.querySelector('details[data-platform="youtube"] summary').textContent)), 'Entwurf wurde nicht im React-State übernommen');
  for (const language of ['en', 'de', 'en']) {
    if (reports.length > 0) {
      assert.equal(await page.evaluate((locale) => {
        const button = document.querySelector(`button[lang="${locale}"]`);
        if (!button) return false;
        button.click();
        return true;
      }, language), true);
    }
    const source = await readFile(join(root, 'public/uplink', language === 'en' ? 'en' : '', 'was-ist.html'), 'utf8');
    const heading = source.match(/<h2>([^<]+)<\/h2>/)[1];
    await waitFor(() => page.evaluate(({ locale, heading }) => {
      const docs = [...document.querySelectorAll('.uplink-doc')];
      const english = locale === 'en';
      return document.documentElement.lang === locale && docs.length === 3
        && docs.every((doc) => doc.textContent.length > 200)
        && [...document.querySelectorAll('.uplink-doc img')].every((image) => image.getAttribute('src').startsWith(english ? '/uplink/en/' : '/uplink/bilder/'))
        && docs[0].querySelector('h3')?.textContent === heading;
    }, { locale: language, heading }), `Kapitel wechseln nicht nach ${language}`);
    assert.deepEqual(await page.evaluate(() => ({ same: window.uplinkDraft === document.querySelector('[data-uplink-draft]'), value: document.querySelector('[data-uplink-draft]').value })), { same: true, value: '13000' });
    const prefix = language === 'en' ? '/uplink/en/' : '/uplink/';
    const links = await page.evaluate(() => [...document.querySelectorAll('a[href$="obs.html#vod"]')].map((link) => link.getAttribute('href')));
    assert.ok(links.length >= 2);
    assert.ok(links.every((href) => href === `${prefix}obs.html#vod`));
    await page.evaluate(() => {
      document.querySelector('[data-section="uplink-help"]').open = true;
      for (const chapter of document.querySelectorAll('.uplink-help-shell')) chapter.open = true;
      document.querySelector('details[data-platform="twitch"]').open = true;
    });
    await waitFor(() => page.evaluate(() => [...document.querySelectorAll('.uplink-doc img')].every((image) => image.complete && image.naturalWidth > 0)), 'Kapitelbilder fehlen');
    await page.evaluate(() => document.fonts.ready);
    if (reports.some((report) => report.language === language)) continue;
    for (const width of [390, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(() => window.scrollTo(0, 0));
      await page.screenshot();
      for (const chapter of ['was-ist', 'obs', 'stoerungen']) {
        await page.evaluate((name) => document.querySelector(`.uplink-doc[data-doc="${name}"]`).parentElement.scrollIntoView({ block: 'start' }), chapter);
        await sleep(600);
        const path = join(artifacts, `${language}-${width}-${chapter}.png`);
        await page.screenshot({ path });
        const report = await page.evaluate((name) => ({
          language: document.documentElement.lang,
          chapterTop: document.querySelector(`.uplink-doc[data-doc="${name}"]`).getBoundingClientRect().top,
          width: innerWidth,
          documentWidth: document.documentElement.scrollWidth,
          fonts: document.fonts.status,
          chapters: [...document.querySelectorAll('.uplink-help-shell')].map((element) => ({ open: element.open, title: element.querySelector('summary').textContent })),
          images: [...document.querySelectorAll('.uplink-doc img')].map((image) => ({ src: image.getAttribute('src'), width: image.naturalWidth })),
        }), chapter);
        assert.equal(report.language, language);
        assert.ok(report.chapterTop >= -20 && report.chapterTop < 900);
        assert.ok(report.documentWidth <= report.width);
        assert.ok(report.chapters.every((item) => item.open));
        reports.push({ path, chapter, ...report });
      }
      await page.evaluate(() => document.querySelector('.uplink-doc[data-doc="obs"] figure').scrollIntoView({ block: 'start' }));
      await sleep(600);
      const path = join(artifacts, `${language}-${width}-obs-image.png`);
      await page.screenshot({ path });
      reports.push({ path, language, width, chapter: 'obs-image' });
    }
  }
  assert.deepEqual(errors, []);
  const counts = Object.fromEntries([...new Set(requests)].map((url) => [url, requests.filter((item) => item === url).length]));
  assert.equal(Object.keys(counts).length, 6);
  for (const [url, count] of Object.entries(counts)) {
    assert.equal(count, url.includes('/uplink/en/') ? 3 : 2);
  }
  await writeFile(join(artifacts, 'checks.json'), JSON.stringify({ reports, requests: counts, errors, browser: 'Moli 1.1.14', app: origin }, null, 2));
  console.log(JSON.stringify({ artifacts, captures: reports.length, requests: counts, errors }));
});
