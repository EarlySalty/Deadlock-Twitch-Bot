import { chromium } from 'playwright-core';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

const directory = process.argv[2];
if (!directory) throw new Error('Screenshot directory required');
await mkdir(directory, { recursive: true });
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.MOLI_PORT ?? '9247'}`);
const context = browser.contexts()[0] ?? await browser.newContext();
const page = await context.newPage();
const base = `http://127.0.0.1:${process.env.PREVIEW_PORT ?? '4197'}/src/components/socialmedia/ai/preview.html`;
const errors = [];
page.on('pageerror', (error) => errors.push(error.message));

async function stableShot(name, width, height, focus) {
  await page.setViewportSize({ width, height });
  await page.evaluate(() => document.fonts.ready);
  await page.locator('.social-ai').waitFor({ state: 'attached' });
  await page.evaluate(() => window.scrollTo(0, 0));
  if (focus) await page.locator(focus).evaluate((element) => element.scrollIntoView({ block: 'start' }));
  await page.waitForTimeout(120);
  await page.screenshot({ path: `${directory}/${name}.png`, animations: 'disabled', fullPage: name === 'ai-channel-switch-mobile' });
  const metrics = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth, videos: [...document.querySelectorAll('video')].map((video) => ({ controls: video.controls, label: video.getAttribute('aria-label'), readyState: video.readyState, src: video.currentSrc })), overflow: [...document.querySelectorAll('.social-ai *')].filter((element) => element.getBoundingClientRect().right > innerWidth + 1).slice(0, 8).map((element) => ({ className: element.className?.baseVal ?? element.className, tag: element.tagName, right: element.getBoundingClientRect().right, width: element.scrollWidth })) }));
  assert.equal(metrics.viewport, width, `${name}: actual viewport differs`);
  assert.ok(metrics.document <= metrics.viewport, `${name}: horizontal overflow ${JSON.stringify(metrics)}`);
  return metrics;
}

async function go(scenario) {
  await page.goto(`${base}?case=${scenario}`);
  await page.evaluate(() => window.scrollTo(0, 0));
}

async function choose(locator, value) {
  await locator.evaluate((element, selected) => {
    element.value = selected;
    element.dispatchEvent(new Event('change', { bubbles: true }));
  }, value);
  assert.equal(await locator.inputValue(), value);
}

async function settleMutation(kind, success) {
  await page.waitForFunction((key) => window.__aiPending?.some((item) => item.kind === key), kind);
  await page.evaluate(({ key, shouldSucceed }) => {
    const index = window.__aiPending.findIndex((item) => item.kind === key);
    const [item] = window.__aiPending.splice(index, 1);
    if (shouldSucceed) item.succeed(); else item.fail();
  }, { key: kind, shouldSucceed: success });
}

try {
  await go('review');
  await page.getByText('Original und bearbeitete Fassung').waitFor();
  await page.locator('video').first().waitFor();
  await stableShot('ai-review-overview-desktop', 1440, 900);
  const desktop = await stableShot('ai-review-desktop', 1440, 900, '.social-ai-video-grid');
  await stableShot('ai-review-overview-mobile', 390, 844);
  const mobile = await stableShot('ai-review-mobile-source', 390, 844, '.social-ai-video-grid figure:first-child');
  await stableShot('ai-review-mobile-output', 390, 844, '.social-ai-video-grid figure:last-child');
  assert.equal(desktop.videos.length, 2);
  assert.equal(mobile.videos.length, 2);
  assert.ok(desktop.videos.every((video) => video.controls && video.label));
  assert.equal(await page.locator('dl.social-ai-facts dt').count(), 7);
  assert.ok(await page.getByText('Die neueste Fassung ist nicht abspielbar.').isVisible());
  const playback = await page.locator('video').evaluateAll(async (videos) => {
    const results = [];
    for (const video of videos) {
      video.muted = true;
      try {
        await video.play();
        await new Promise((resolve, reject) => {
          const deadline = performance.now() + 4000;
          const poll = () => video.currentTime > 0 && !video.paused ? resolve() : performance.now() > deadline ? reject(new Error('Playback did not advance')) : setTimeout(poll, 50);
          poll();
        });
        results.push({ label: video.getAttribute('aria-label'), currentTime: video.currentTime, readyState: video.readyState, paused: video.paused, error: video.error?.message ?? null });
      } catch (error) {
        results.push({ label: video.getAttribute('aria-label'), currentTime: video.currentTime, readyState: video.readyState, paused: video.paused, error: String(error) });
      }
      video.pause();
    }
    return results;
  });
  await go('consent-off');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.scrollIntoView({ block: 'center' }));
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  assert.ok(await page.getByRole('button', { name: 'Zustimmung erteilen' }).isDisabled());
  const consentMetrics = await stableShot('ai-consent-confirm-mobile', 390, 844, '.social-ai-confirm');
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor({ state: 'attached' });
  await page.locator('#social-ai-clip').selectOption('502');
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  await go('global-off');
  await page.getByText('Die globale Freigabe ist gesperrt.').waitFor();
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await stableShot('ai-global-off-mobile', 390, 844, '.social-ai-global');
  await go('failed');
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).waitFor({ state: 'attached' });
  assert.ok(await page.getByRole('alert').filter({ hasText: 'Der Renderversuch ist fehlgeschlagen.' }).isVisible());
  await stableShot('ai-failed-mobile', 390, 844, '.social-ai-failure');
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).evaluate((element) => element.click());
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  await go('handoff');
  await page.locator('#social-ai-clip option[value="135"]').waitFor({ state: 'attached' });
  assert.equal(await page.locator('#social-ai-clip').inputValue(), '135');
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isEnabled());
  await stableShot('ai-handoff-mobile', 390, 844, '.social-ai-pick');
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  assert.ok(await page.getByText('Clip 135', { exact: false }).count() > 0);
  await go('race');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  assert.ok(await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).isDisabled());
  await page.evaluate(() => {
    const original = window.fetch;
    window.__aiPending = [];
    window.fetch = (input, init) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.origin);
      const channel = typeof init?.body === 'string' ? JSON.parse(init.body).twitch_user_id : null;
      const kind = channel === '123456789' && init?.method === 'PUT' && url.pathname.endsWith('/consent') ? 'consent' : channel === '123456789' && init?.method === 'POST' && url.pathname.endsWith('/jobs') ? 'start' : channel === '123456789' && init?.method === 'POST' && url.pathname.endsWith('/retry') ? 'retry' : null;
      return kind ? new Promise((resolve, reject) => window.__aiPending.push({ kind, succeed: () => original(input, init).then(resolve, reject), fail: () => reject(new Error('Synthetic delayed failure')) })) : original(input, init);
    };
  });
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).waitFor({ state: 'attached' });
  await settleMutation('consent', true);
  assert.equal(await page.getByRole('button', { name: 'Zustimmung erteilen' }).count(), 1);
  assert.equal(await page.getByText('Die Freigabe konnte nicht gespeichert werden.').count(), 0);
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor({ state: 'attached' });
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await page.getByText('Der übernommene Clip wurde im geprüften Kanalbestand nicht als Twitch-Clip gefunden.').waitFor({ state: 'attached' });
  await choose(page.locator('#social-ai-clip'), '601');
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isEnabled());
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor({ state: 'attached' });
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await settleMutation('retry', false);
  assert.equal(await page.getByText('Der erneute Versuch konnte nicht gestartet werden.').count(), 0);
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1202');
  await page.getByText('Auftrag 1.202').waitFor({ state: 'attached' });
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1202');
  await page.getByText('Auftrag 1.202').waitFor({ state: 'attached' });
  await settleMutation('start', true);
  assert.equal(await page.getByRole('combobox', { name: 'Auftrag ansehen' }).inputValue(), '1202');
  assert.equal(await page.getByText('Auftrag 1.202').count(), 1);
  assert.equal(await page.getByText('Die Bearbeitung konnte nicht gestartet werden.').count(), 0);
  await stableShot('ai-channel-switch-mobile', 390, 844, '.social-ai-history');
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  assert.equal(await page.getByRole('combobox', { name: 'Auftrag ansehen' }).inputValue(), '1201');
  assert.deepEqual(errors, []);
  const evidence = { environment: 'synthetic-local-preview', desktop, mobile, consentMetrics, playback, playbackReady: playback.every((video) => video.currentTime > 0 && video.readyState >= 2 && !video.error), channelRacePassed: true, screenshots: 10, errors };
  await writeFile(`${directory}/run.json`, `${JSON.stringify(evidence, null, 2)}\n`);
  assert.ok(evidence.playbackReady, `synthetic media did not play: ${JSON.stringify(playback)}`);
  console.log(JSON.stringify(evidence, null, 2));
} finally {
  await page.close();
  await browser.close();
}
