import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { before, after, test } from 'node:test';
import { build, preview } from 'vite';
import { chromium } from 'playwright-core';

const root = fileURLToPath(new URL('../', import.meta.url));
const evidence = await mkdtemp(path.join(os.tmpdir(), 'd-language-drafts-r2-'));
let server;
let moli;
let browser;
let page;
let base;
const errors = [];

async function freePort() {
  const socket = net.createServer();
  await new Promise((resolve) => socket.listen(0, '127.0.0.1', resolve));
  const { port } = socket.address();
  await new Promise((resolve) => socket.close(resolve));
  return port;
}

before(
  async () => {
    const outDir = path.join(evidence, 'dist');
    await build({
      root,
      mode: 'test',
      base: '/',
      logLevel: 'warn',
      build: {
        outDir,
        emptyOutDir: true,
        rollupOptions: {
          input: { regression: path.join(root, 'tests/browser/languageDrafts.html') },
        },
      },
    });
    await mkdir(path.join(outDir, 'brand/fonts'), { recursive: true });
    for (const name of ['manrope-latin.woff2', 'sora-latin.woff2'])
      await copyFile(
        path.resolve(root, '../../website/public/fonts', name),
        path.join(outDir, 'brand/fonts', name),
      );
    server = await preview({
      root,
      mode: 'test',
      base: '/',
      build: { outDir },
      preview: { host: '127.0.0.1', port: 0, strictPort: false },
    });
    base = `http://127.0.0.1:${server.httpServer.address().port}`;
    const port = await freePort();
    const home = path.join(evidence, 'home');
    const cache = path.join(evidence, 'cache');
    await mkdir(home, { recursive: true });
    await mkdir(cache, { recursive: true });
    const fontconfig = path.join(evidence, 'fonts.conf');
    await writeFile(
      fontconfig,
      `<fontconfig><cachedir>${cache}/fontconfig</cachedir><dir>/usr/share/fonts</dir><dir>/usr/local/share/fonts</dir>` +
        '<alias><family>sans-serif</family><prefer><family>DejaVu Sans</family></prefer></alias>' +
        '<alias><family>serif</family><prefer><family>DejaVu Serif</family></prefer></alias>' +
        '<alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family></prefer></alias>' +
        '<alias><family>cursive</family><prefer><family>DejaVu Sans</family></prefer></alias>' +
        '<alias><family>fantasy</family><prefer><family>DejaVu Sans</family></prefer></alias>' +
        '</fontconfig>',
    );
    moli = spawn(
      '/home/nathanael/.local/bin/moli',
      ['serve', '--host', '127.0.0.1', '--port', String(port), '--layout', '--image', '--font'],
      {
        env: {
          ...process.env,
          FONTCONFIG_FILE: fontconfig,
          HOME: home,
          XDG_CACHE_HOME: cache,
        },
        stdio: 'ignore',
      },
    );
    await new Promise((resolve, reject) => {
      moli.once('error', reject);
      const attempt = async () => {
        if (moli.exitCode !== null) return reject(new Error(`Moli exit ${moli.exitCode}`));
        try {
          const result = await fetch(`http://127.0.0.1:${port}/json/version`);
          if (result.ok) return resolve();
        } catch {}
        setTimeout(attempt, 50);
      };
      void attempt();
    });
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    const context = browser.contexts()[0];
    page = context.pages()[0] ?? (await context.newPage());
    page.on('pageerror', (error) => errors.push(error.message));
    console.log(`EVIDENCE ${evidence}`);
  },
  { timeout: 180_000 },
);

after(async () => {
  await browser?.close();
  moli?.kill('SIGTERM');
  await new Promise((resolve) => (server ? server.httpServer.close(resolve) : resolve()));
});

async function open(surface, setup) {
  errors.length = 0;
  await page.goto(
    `${base}/tests/browser/languageDrafts.html?surface=${surface}${surface === 'app' ? '&tab=profile&streamer=alice' : ''}`,
  );
  await page.waitForFunction(() => Boolean(window.languageDraftFixture));
  await page.evaluate(() => localStorage.setItem('dashboard.language', 'de'));
  if (setup) await page.evaluate(setup);
  if (surface !== 'app') await click('de');
}

async function click(id) {
  await page.evaluate((id) => document.querySelector(`[data-testid="${id}"]`).click(), id);
}

async function fill(selector, value) {
  await page.evaluate(
    ({ selector, value }) => {
      const input = document.querySelector(selector);
      const prototype =
        input.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, 'value').set.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    },
    { selector, value },
  );
}

async function pressButton(name, index = 0) {
  await page.evaluate(
    ({ name, index }) => {
      const button = [...document.querySelectorAll('button')].filter(
        (button) => button.textContent.trim() === name,
      )[index];
      if (!button || button.disabled) throw new Error(`Button unavailable: ${name}`);
      button.click();
    },
    { name, index },
  );
}

async function settled() {
  await page.waitForFunction(() => window.languageDraftFixture.client.isFetching() === 0);
}

async function language(lang) {
  await click(lang);
  await page.waitForFunction((lang) => document.documentElement.lang === lang, lang);
}

async function snapshot(name, selector) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Page.captureScreenshot', { format: 'png' });
  if (selector)
    await page.evaluate(
      (selector) => document.querySelector(selector).scrollIntoView({ block: 'center' }),
      selector,
    );
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all([...document.images].map((image) => image.decode().catch(() => undefined)));
  });
  await page.waitForFunction((selector) => {
    const style = getComputedStyle(document.querySelector(selector).closest('.panel-card'));
    return Number(style.opacity) === 1 && style.transform === 'none';
  }, selector);
  const { data } = await cdp.send('Page.captureScreenshot', { format: 'png' });
  const geometry = await page.evaluate((selector) => {
    const rect = document.querySelector(selector).getBoundingClientRect();
    return {
      top: rect.top,
      bottom: rect.bottom,
      width: rect.width,
      scrollY,
      height: innerHeight,
      documentWidth: document.documentElement.scrollWidth,
      viewportWidth: innerWidth,
      fonts: document.fonts.status,
      transform: getComputedStyle(document.querySelector(selector).closest('.panel-card'))
        .transform,
    };
  }, selector);
  console.log('VIEW', name, geometry);
  assert.ok(geometry.width > 0 && geometry.top < geometry.height && geometry.bottom > 0, name);
  await writeFile(path.join(evidence, `${name}.png`), Buffer.from(data, 'base64'));
  await cdp.detach();
  assert.equal(
    await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    true,
    name,
  );
}

async function pictures(surface, selector) {
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 1100 });
    for (const lang of ['de', 'en']) {
      await language(lang);
      await snapshot(`${surface}-${lang}-${width}`, selector);
    }
  }
  await language('de');
}

async function translated(key, lang) {
  return page.evaluate(({ key, lang }) => window.languageDraftFixture.translate(lang, key), { key, lang });
}

async function assertErrorRoundtrip(key, selector, wrapper) {
  for (const lang of ['en', 'de', 'en']) {
    await language(lang);
    const expected = await page.evaluate(({ key, lang, wrapper }) => {
      const t = window.languageDraftFixture.translate;
      const error = t(lang, key);
      return wrapper ? t(lang, wrapper, { error }) : error;
    }, { key, lang, wrapper });
    assert.equal(await page.locator(selector).textContent(), expected);
  }
}

const headline = 'input[maxlength="120"]';
const profilePath = '/twitch/api/v2/streamer/profile';

test('profile PUT conflict retains the precise error, revision, CSRF and draft through language roundtrips', async () => {
  await open('profile');
  await page.waitForSelector(headline, { state: 'attached' });
  await language('en');
  await fill(headline, 'Conflict Grüße');
  const key = 'Das Profil wurde inzwischen geändert. Bitte den aktuellen Stand laden, bevor du erneut speicherst.';
  await page.evaluate(({ path, key }) => {
    window.languageDraftFixture.failures.set(`PUT ${path}`, {
      status: 409, body: { code: 'profile_conflict', message: key },
    });
    window.keptEditor = document.querySelector('input[maxlength="120"]');
  }, { path: profilePath, key });
  await pressButton(await translated('Änderungen speichern', 'en'));
  await page.waitForSelector('[role="alert"]', { state: 'attached' });
  await assertErrorRoundtrip(key, '[role="alert"] > p', '{error} Deine Eingaben bleiben hier erhalten.');
  assert.equal(await page.locator(headline).inputValue(), 'Conflict Grüße');
  assert.equal(await page.evaluate(() => window.keptEditor === document.querySelector('input[maxlength="120"]')), true);
  const puts = await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method === 'PUT'));
  assert.equal(puts.length, 1);
  assert.equal(puts[0].owner, 'alice');
  assert.equal(puts[0].csrf, 'synthetic-csrf');
  assert.equal(puts[0].body.revision, 1);
  assert.equal(puts[0].body.published, false);
  assert.equal(puts[0].body.profile.headline, 'Conflict Grüße');
  assert.deepEqual(errors, []);
});

for (const [start, end, key] of [
  ['', '2026-10-25T04:00', 'Bitte Datum und Uhrzeit vollständig eintragen.'],
  ['2026-03-29T02:30', '2026-03-29T04:00', 'Diese Uhrzeit existiert in Berlin wegen der Zeitumstellung nicht. Bitte eine andere Uhrzeit wählen.'],
  ['2026-10-25T02:30', '2026-10-25T04:00', 'Diese Uhrzeit kommt bei der Zeitumstellung zweimal vor. Bitte eine eindeutige Uhrzeit außerhalb der doppelten Stunde wählen.'],
]) {
 test(`profile calendar preserves rejected Berlin time ${start || 'incomplete'} across language changes`, async () => {
    await open('profile');
    await page.waitForSelector(headline, { state: 'attached' });
    await page.evaluate(() => document.querySelector('button[aria-label^="Termin am"]').click());
    await fill('input[type="datetime-local"]', start);
    await page.evaluate((end) => {
      const input = document.querySelectorAll('input[type="datetime-local"]')[1];
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, end);
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    }, end);
    await pressButton(await translated('Termin übernehmen', 'de'));
    await page.waitForSelector('[role="alert"]', { state: 'attached' });
    await assertErrorRoundtrip(key, '[role="alert"]');
    assert.equal(await page.locator('input[type="datetime-local"]').first().inputValue(), start);
    assert.equal(await page.locator('input[type="datetime-local"]').last().inputValue(), end);
    assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method !== 'GET').length), 0);
    assert.deepEqual(errors, []);
  });
}

for (const action of ['save', 'import']) {
 test(`late old-owner profile ${action} cannot replace a new owner's draft`, async () => {
    await open('profile');
    await page.waitForSelector(headline, { state: 'attached' });
    await fill(headline, 'Alice pending Grüße');
    await page.evaluate((path) => window.languageDraftFixture.hold.add(path), profilePath);
    await pressButton(action === 'save' ? 'Änderungen speichern' : 'Aus Twitch importieren');
    await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
    await page.evaluate(() => window.languageDraftFixture.hold.clear());
    await click('owner');
    await page.waitForFunction(() => document.querySelector('input[maxlength="120"]')?.value === 'bob headline');
    await fill(headline, 'Bob protected Grüße');
    await page.evaluate(() => window.languageDraftFixture.release());
    await language('en');
    await language('de');
    assert.equal(await page.locator(headline).inputValue(), 'Bob protected Grüße');
    assert.deepEqual(errors, []);
  });
}

for (const [index, variant] of [[0, 'compact'], [1, 'full']]) {
 test(`${variant} rating rollback retains precise save errors and pending content`, async () => {
    await open('reports');
    await page.waitForSelector('textarea', { state: 'attached' });
    await settled();
    await page.evaluate(({ index }) => {
      const input = document.querySelectorAll('textarea')[index];
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(input, 'Rating rejected Grüße');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }, { index });
    const key = 'Bewertung konnte nicht gespeichert werden.';
    await page.evaluate((key) => window.languageDraftFixture.failures.set('POST /twitch/api/v2/stream-report/rate', { status: 422, body: { message: key } }), key);
    await pressButton('Speichern', index);
    await page.waitForSelector('.text-danger', { state: 'attached' });
    await assertErrorRoundtrip(key, '.text-danger');
    assert.equal(await page.locator('textarea').nth(index).inputValue(), 'Rating rejected Grüße');
    const calls = await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.path.endsWith('/rate')));
    assert.equal(calls.length, 1);
    assert.deepEqual(calls[0].body, { session_id: 101, streamer: 'alice', variant, rating: 'neutral', comment: 'Rating rejected Grüße' });
    assert.deepEqual(errors, []);
  });
}

test('AB vote error roundtrip does not replay a save, and success retains returned totals', async () => {
  await open('reports');
  await page.waitForSelector('textarea', { state: 'attached' });
  await settled();
  await fill('textarea[placeholder^="Optional:"]', 'Vote rejected Grüße');
  const key = 'Abstimmung konnte nicht gespeichert werden.';
  await page.evaluate((key) => window.languageDraftFixture.failures.set('POST /twitch/api/v2/stream-report/ab-vote', { status: 409, body: { error: key } }), key);
  await pressButton('Abstimmen');
  await page.waitForSelector('.text-danger', { state: 'attached' });
  await assertErrorRoundtrip(key, '.text-danger');
  assert.equal(await page.locator('textarea').last().inputValue(), 'Vote rejected Grüße');
  assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method === 'POST').length), 1);
  await page.evaluate(() => window.languageDraftFixture.failures.clear());
  await language('de');
  await pressButton('Abstimmen');
  await page.waitForFunction(() => window.languageDraftFixture.client.getQueryData(['stream-report-ab-vote', 'alice', 101])?.totals.compact === 4);
  await settled();
  for (const lang of ['en', 'de', 'en']) await language(lang);
  assert.deepEqual(await page.evaluate(() => window.languageDraftFixture.client.getQueryData(['stream-report-ab-vote', 'alice', 101]).totals), { compact: 4, full: 2, gleich: 1 });
  assert.equal(await page.locator('textarea').last().inputValue(), 'Vote rejected Grüße');
  const posts = await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method === 'POST'));
  assert.equal(posts.length, 2);
  assert.deepEqual(posts[1].body, { session_id: 101, streamer: 'alice', winner: 'gleich', comment: 'Vote rejected Grüße' });
  assert.deepEqual(errors, []);
});

test('late old-owner AB vote cannot overwrite the new owner or query cache', async () => {
  await open('reports');
  await page.waitForSelector('textarea', { state: 'attached' });
  await settled();
  await fill('textarea[placeholder^="Optional:"]', 'Alice vote Grüße');
  await page.evaluate(() => window.languageDraftFixture.hold.add('/twitch/api/v2/stream-report/ab-vote'));
  await pressButton('Abstimmen');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
  await page.evaluate(() => window.languageDraftFixture.hold.clear());
  await click('owner');
  await settled();
  await page.waitForFunction(() => document.querySelectorAll('textarea')[2]?.value === 'baseline');
  await fill('textarea[placeholder^="Optional:"]', 'Bob vote Grüße');
  await page.evaluate(() => window.languageDraftFixture.release());
  await language('en');
  await language('de');
  assert.equal(await page.locator('textarea').last().inputValue(), 'Bob vote Grüße');
  assert.deepEqual(await page.evaluate(() => window.languageDraftFixture.client.getQueryData(['stream-report-ab-vote', 'bob', 101]).totals), { compact: 1, full: 1, gleich: 1 });
  assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method === 'POST').length), 1);
  assert.deepEqual(errors, []);
});

for (const [switchIndex, saveIndex, endpoint] of [
  [0, 0, '/twitch/api/v2/streamer/moderation/settings'],
  [4, 1, '/twitch/api/v2/streamer/scam-guard/settings'],
]) {
 test(`settings save ${saveIndex} keeps unknown server details raw and preserves dirty controls`, async () => {
    await open('settings');
    await page.waitForFunction(() => document.querySelectorAll('[role="switch"]').length === 5);
    await page.locator('[role="switch"]').nth(switchIndex).click();
    const detail = 'Synthetic backend detail Grüße #409';
    await page.evaluate(({ endpoint, detail }) => window.languageDraftFixture.failures.set(`POST ${endpoint}`, { status: 409, body: { error: detail } }), { endpoint, detail });
    await pressButton('Einstellungen speichern', saveIndex);
    await page.waitForSelector('[role="alert"]', { state: 'attached' });
    for (const lang of ['en', 'de', 'en']) {
      await language(lang);
      assert.equal(await page.locator('[role="alert"]').textContent(), detail);
    }
    assert.equal(await page.locator('[data-unsaved="true"]').count(), 1);
    assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method === 'POST').length), 1);
    assert.deepEqual(errors, []);
  });
}


test('cached profile survives background failure and language roundtrips without replaying saves', async () => {
  await open('profile');
  await page.waitForSelector(headline, { state: 'attached' });
  await fill(headline, 'Untranslated dirty Grüße');
  await page.evaluate(() => {
    window.keptEditor = document.querySelector('input[maxlength="120"]');
    window.languageDraftFixture.fail.add('/twitch/api/v2/streamer/profile');
  });
  await click('refresh');
  await settled();
  assert.equal(
    await page.evaluate(
      () =>
        window.languageDraftFixture.client
          .getQueryCache()
          .findAll({ queryKey: ['partner-profile'] })[0].state.status,
    ),
    'error',
  );
  assert.equal(await page.locator('[role="alert"]').count(), 1);
  assert.equal(await page.locator(headline).count(), 1);
  assert.equal(await page.locator(headline).inputValue(), 'Untranslated dirty Grüße');
  await pictures('profile-error', headline);
  await pictures('profile-read-error', '[role="alert"]');
  assert.equal(await page.locator(headline).inputValue(), 'Untranslated dirty Grüße');
  assert.equal(
    await page.evaluate(
      () => window.keptEditor === document.querySelector('input[maxlength="120"]'),
    ),
    true,
  );
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.method !== 'GET').length,
    ),
    0,
  );
  await page.evaluate(() => window.languageDraftFixture.fail.clear());
  await pressButton('Änderungen speichern');
  await page.waitForFunction(() =>
    window.languageDraftFixture.calls.some((call) => call.method === 'PUT'),
  );
  await language('en');
  await language('de');
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.method === 'PUT').length,
    ),
    1,
  );
  assert.equal(
    await page.evaluate(
      () =>
        window.languageDraftFixture.calls.find((call) => call.method === 'PUT').body.profile
          .headline,
    ),
    'Untranslated dirty Grüße',
  );
  assert.deepEqual(errors, []);
});

test('profile resets on account change and ignores a late old-owner query', async () => {
  await open('profile');
  await page.waitForSelector(headline, { state: 'attached' });
  await fill(headline, 'Old owner draft');
  await page.evaluate((path) => window.languageDraftFixture.hold.add(path), profilePath);
  await click('refresh');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
  await click('owner');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 2);
  assert.equal(await page.locator(headline).count(), 0);
  await page.evaluate(() => {
    const fixture = window.languageDraftFixture;
    fixture.hold.clear();
    fixture.held.pop()();
  });
  await page.waitForSelector(headline, { state: 'attached' });
  assert.equal(await page.locator(headline).inputValue(), 'bob headline');
  await page.evaluate(() => window.languageDraftFixture.release());
  await settled();
  assert.equal(await page.locator(headline).inputValue(), 'bob headline');
  assert.deepEqual(errors, []);
});

test('initial profile failure stays distinct from a cached editor', async () => {
  await open('profile&initialFailure=1');
  await settled();
  assert.equal(await page.locator(headline).count(), 0);
  assert.equal(await page.locator('[role="alert"]').count(), 1);
});

test('rating and vote drafts survive cached GET failures and language changes', async () => {
  await open('reports');
  await page.waitForSelector('textarea', { state: 'attached' });
  await settled();
  await fill('textarea', 'Rating Grüße');
  await fill('textarea[placeholder*="Optional:"]', 'Vote Grüße');
  await page.evaluate(() => {
    window.keptRating = document.querySelector('textarea');
    for (const path of ['/twitch/api/v2/stream-report', '/twitch/api/v2/stream-report/ab-vote'])
      window.languageDraftFixture.fail.add(path);
  });
  await click('refresh');
  await settled();
  assert.equal(await page.locator('[role="alert"]').count(), 3);
  await pictures('rating-error', 'textarea');
  assert.equal(await page.locator('textarea').first().inputValue(), 'Rating Grüße');
  assert.equal(await page.locator('textarea').last().inputValue(), 'Vote Grüße');
  assert.equal(
    await page.evaluate(() => window.keptRating === document.querySelector('textarea')),
    true,
  );
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.method !== 'GET').length,
    ),
    0,
  );
  await page.evaluate(() => window.languageDraftFixture.fail.clear());
  await pressButton('Speichern');
  await page.waitForFunction(() =>
    window.languageDraftFixture.calls.some((call) => call.path.endsWith('/rate')),
  );
  await language('en');
  await language('de');
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.path.endsWith('/rate')).length,
    ),
    1,
  );
  assert.deepEqual(errors, []);
});

test('old-owner rating mutation cannot populate a new owner with the same session', async () => {
  await open('reports');
  await page.waitForSelector('textarea', { state: 'attached' });
  await settled();
  await fill('textarea', 'Alice mutation');
  await page.evaluate(() => {
    const fixture = window.languageDraftFixture;
    window.keptRating = document.querySelector('textarea');
    for (const query of fixture.client.getQueryCache().getAll()) {
      if (query.queryKey[1] !== 'alice') continue;
      const key = [...query.queryKey];
      key[1] = 'bob';
      fixture.client.setQueryData(key, structuredClone(query.state.data));
      if (key[0] === 'stream-report') {
        key[2] = null;
        fixture.client.setQueryData(key, structuredClone(query.state.data));
      }
    }
    fixture.hold.add('/twitch/api/v2/stream-report/rate');
  });
  await pressButton('Speichern');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
  await click('owner');
  await settled();
  await page.waitForSelector('textarea', { state: 'attached' });
  assert.equal(await page.locator('textarea').first().inputValue(), 'baseline');
  assert.equal(
    await page.evaluate(() => window.keptRating === document.querySelector('textarea')),
    true,
  );
  await fill('textarea', 'Bob draft');
  await page.evaluate(() => window.languageDraftFixture.release());
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 0);
  await language('en');
  await language('de');
  assert.equal(await page.locator('textarea').first().inputValue(), 'Bob draft');
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.path.endsWith('/rate')).length,
    ),
    1,
  );
  assert.deepEqual(errors, []);
});

test('successful settings loads and dirty controls are independent of provider language', async () => {
  await open('settings');
  await page.waitForSelector('[role="switch"]');
  await page.waitForFunction(() => document.querySelectorAll('[role="switch"]').length === 5);
  const initialGets = await page.evaluate(
    () =>
      window.languageDraftFixture.calls.filter(
        (call) => call.path.endsWith('/settings') && call.method === 'GET',
      ).length,
  );
  await page.locator('[role="switch"]').first().click();
  await fill('input[type="range"]', '89');
  await pictures('settings-moderation', '[data-tour-id="onboarding-bot"]');
  await pictures('settings-scam', 'input[type="range"]');
  assert.equal(await page.locator('[role="switch"]').first().getAttribute('aria-checked'), 'false');
  assert.equal(await page.locator('input[type="range"]').first().inputValue(), '89');
  assert.equal(
    await page.evaluate(
      () =>
        window.languageDraftFixture.calls.filter(
          (call) => call.path.endsWith('/settings') && call.method === 'GET',
        ).length,
    ),
    initialGets,
  );
  assert.equal(await page.locator('[data-unsaved="true"]').count(), 2);
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.method === 'POST').length,
    ),
    0,
  );
  await pressButton('Einstellungen speichern');
  await page.waitForFunction(() => document.querySelectorAll('[data-unsaved="true"]').length === 1);
  await language('en');
  await language('de');
  assert.equal(
    await page.evaluate(
      () => window.languageDraftFixture.calls.filter((call) => call.method === 'POST').length,
    ),
    1,
  );
  assert.equal(
    await page.evaluate(
      () =>
        window.languageDraftFixture.calls.find((call) => call.method === 'POST').body
          .global_ban_enabled,
    ),
    false,
  );
  assert.deepEqual(errors, []);
});

test('pending settings loads finish after language change and old-account replies are ignored', async () => {
  await open('settings');
  await page.waitForFunction(() => document.querySelectorAll('[role="switch"]').length === 5);
  await page.evaluate(() => {
    const fixture = window.languageDraftFixture;
    fixture.hold.add('/twitch/api/v2/streamer/moderation/settings');
    fixture.hold.add('/twitch/api/v2/streamer/scam-guard/settings');
  });
  await click('owner');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 2);
  await language('en');
  await click('owner');
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 4);
  assert.equal(await page.locator('[role="switch"]').count(), 0);
  await page.evaluate(() => {
    const fixture = window.languageDraftFixture;
    fixture.hold.clear();
    fixture.held.pop()();
    fixture.held.pop()();
  });
  await page.waitForFunction(() => document.querySelectorAll('[role="switch"]').length === 5);
  await page.locator('[role="switch"]').first().click();
  await page.evaluate(() => window.languageDraftFixture.release());
  await language('de');
  assert.equal(await page.locator('[role="switch"]').first().getAttribute('aria-checked'), 'false');
  assert.equal(await page.locator('[data-unsaved="true"]').count(), 1);
  assert.deepEqual(errors, []);
});

test('initial settings failures show errors instead of an endless loading state', async () => {
  await open('settings&initialFailure=settings');
  await page.waitForFunction(() => window.languageDraftFixture.completed === 2);
  await page.waitForSelector('[role="alert"]', { state: 'attached' });
  await language('en');
  await language('de');
  assert.equal(await page.locator('[role="switch"]').count(), 0);
  assert.equal(await page.locator('[role="alert"]').count(), 2);
  assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.length), 2);
});

for (const [name, switchIndex, saveIndex, path] of [
  ['moderation', 0, 0, '/twitch/api/v2/streamer/moderation/settings'],
  ['scam guard', 4, 1, '/twitch/api/v2/streamer/scam-guard/settings'],
]) {
 test(`${name} ignores a late old-account save while preserving the new draft`, async () => {
    await open('settings');
    await page.waitForFunction(() => document.querySelectorAll('[role="switch"]').length === 5);
    await page.evaluate(
      (index) => document.querySelectorAll('[role="switch"]')[index].click(),
      switchIndex,
    );
    await page.evaluate((path) => window.languageDraftFixture.hold.add(path), path);
    await pressButton('Einstellungen speichern', saveIndex);
    await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
    await page.evaluate(() => window.languageDraftFixture.hold.clear());
    await click('owner');
    await page.waitForFunction(
      (index) =>
        document.querySelectorAll('[role="switch"]')[index]?.getAttribute('aria-checked') ===
        'false',
      switchIndex,
    );
    await page.evaluate(
      (index) => document.querySelectorAll('[role="switch"]')[index].click(),
      switchIndex,
    );
    await page.evaluate(() => {
      window.languageDraftFixture.hold.clear();
      window.languageDraftFixture.release();
    });
    await language('en');
    await language('de');
    assert.equal(
      await page.locator('[role="switch"]').nth(switchIndex).getAttribute('aria-checked'),
      'true',
    );
    assert.equal(await page.locator('[data-unsaved="true"]').count(), 1);
    assert.equal(
      await page.evaluate(
        () => window.languageDraftFixture.calls.filter((call) => call.method === 'POST').length,
      ),
      1,
    );
    assert.deepEqual(errors, []);
  });
}

test('real App rejects a late previous-language internal-home response', async () => {
  await open('app');
  await page.waitForSelector(headline, { state: 'attached' });
  await fill(headline, 'App language race Grüße');
  await page.evaluate(() => document.querySelector('button[lang="en"]').click());
  await page.waitForFunction(() => document.body.textContent.includes('Home EN'));
  await page.evaluate(() => window.languageDraftFixture.hold.add('/twitch/api/v2/internal-home'));
  await page.evaluate(() => document.querySelector('button[lang="de"]').click());
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 1);
  await page.evaluate(() => document.querySelector('button[lang="en"]').click());
  await page.waitForFunction(() => window.languageDraftFixture.held.length === 2);
  await page.evaluate(() => {
    window.languageDraftFixture.hold.clear();
    window.languageDraftFixture.held.pop()();
  });
  await page.waitForFunction(() => window.languageDraftFixture.completed === window.languageDraftFixture.calls.length - 1);
  await page.evaluate(() => window.languageDraftFixture.release());
  await page.waitForFunction(() => window.languageDraftFixture.completed === window.languageDraftFixture.calls.length);
  assert.equal(await page.evaluate(() => document.body.textContent.includes('Home EN')), true);
  assert.equal(await page.evaluate(() => document.body.textContent.includes('Home DE')), false);
  assert.equal(await page.locator(headline).inputValue(), 'App language race Grüße');
  assert.equal(await page.evaluate(() => window.languageDraftFixture.calls.filter(call => call.method !== 'GET').length), 0);
  assert.deepEqual(errors, []);
});

test('real App language selector does not refresh profile or account data', async () => {
  await open('app');
  await page.waitForSelector(headline, { state: 'attached' });
  await fill(headline, 'Actual App draft');
  const before = await page.evaluate(() => window.languageDraftFixture.calls.length);
  await page.evaluate(() => document.querySelector('button[lang="en"]').click());
  await page.waitForFunction(() => document.documentElement.lang === 'en');
  await page.waitForFunction(
    (before) =>
      window.languageDraftFixture.calls
        .slice(before)
        .some((call) => call.path.endsWith('/internal-home') && call.language === 'en'),
    before,
  );
  await page.evaluate(() => document.querySelector('button[lang="de"]').click());
  await page.waitForFunction(() => document.documentElement.lang === 'de');
  await page.waitForFunction(
    (before) =>
      window.languageDraftFixture.calls
        .slice(before)
        .some((call) => call.path.endsWith('/internal-home') && call.language === 'de'),
    before,
  );
  assert.equal(await page.locator(headline).inputValue(), 'Actual App draft');
  assert.equal(
    await page.evaluate(
      () =>
        window.languageDraftFixture.calls.filter((call) => call.path.endsWith('/profile')).length,
    ),
    1,
  );
  assert.equal(
    await page.evaluate(
      (before) =>
        window.languageDraftFixture.calls
          .slice(before)
          .every((call) => call.path.endsWith('/internal-home')),
      before,
    ),
    true,
  );
  assert.deepEqual(errors, []);
});
