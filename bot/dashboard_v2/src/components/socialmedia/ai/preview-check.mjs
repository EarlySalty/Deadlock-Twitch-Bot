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
  await page.screenshot({ path: `${directory}/${name}.png`, animations: 'disabled', fullPage: ['ai-channel-switch-mobile', 'ai-consent-confirm-mobile', 'ai-consent-pending-mobile', 'ai-consent-uncertain-mobile', 'ai-consent-retry-error-mobile', 'ai-consent-retry-allowed-mobile', 'ai-consent-revoke-retry-error-mobile', 'ai-consent-retry-denied-mobile', 'ai-consent-global-retry-error-mobile', 'ai-consent-global-denied-mobile', 'ai-consent-initial-error-mobile', 'ai-consent-initial-recovered-mobile'].includes(name) });
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

async function loseConsentPutResponseAndFirstReload(delayPut = false) {
  await page.evaluate((delayed) => {
    const original = window.fetch;
    window.__aiConsentProbe = { puts: 0, gets: 0, failNextGet: false };
    window.fetch = (input, init) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.origin);
      if (!url.pathname.endsWith('/ai/consent')) return original(input, init);
      const probe = window.__aiConsentProbe;
      if (init?.method === 'PUT') {
        probe.puts += 1;
        probe.failNextGet = true;
        if (delayed) return new Promise((_, reject) => {
          window.__finishConsentPut = () => original(input, init).then(() => reject(new Error('Synthetic response lost after save')), reject);
        });
        return original(input, init).then(() => { throw new Error('Synthetic response lost after save'); });
      }
      probe.gets += 1;
      if (probe.failNextGet) {
        probe.failNextGet = false;
        return Promise.reject(new Error('Synthetic consent reload unavailable'));
      }
      return original(input, init);
    };
  }, delayPut);
}

async function retryFailedConsentReload(expectedSwitch, expectedChecked, startEnabled, failureShot, successShot) {
  await page.getByRole('status').filter({ hasText: 'Ob die Änderung übernommen wurde, ist unklar.' }).waitFor({ state: 'attached' });
  await page.getByRole('button', { name: 'Freigabe neu laden' }).focus();
  await page.keyboard.press('Enter');
  await page.getByRole('button', { name: 'Erneut laden' }).waitFor({ state: 'attached' });
  assert.equal(await page.getByRole('switch').count(), 0);
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  assert.deepEqual(await page.evaluate(() => window.__aiConsentProbe), { puts: 1, gets: 1, failNextGet: false });
  await stableShot(failureShot, 390, 844, '.social-ai-consent');
  await page.getByRole('button', { name: 'Erneut laden' }).focus();
  await page.keyboard.press('Enter');
  await page.getByRole('switch', { name: expectedSwitch }).waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'switch');
  assert.equal(await page.getByRole('switch').getAttribute('aria-checked'), String(expectedChecked));
  assert.ok(await page.getByRole('switch').isEnabled());
  assert.equal(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isEnabled(), startEnabled);
  assert.equal(await page.getByText('Ob die Änderung übernommen wurde, ist unklar.', { exact: false }).count(), 0);
  assert.deepEqual(await page.evaluate(() => window.__aiConsentProbe), { puts: 1, gets: 2, failNextGet: false });
  await stableShot(successShot, 390, 844, '.social-ai-consent');
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
  assert.equal(await page.locator('dl.social-ai-facts dt').count(), 8);
  const timing = await page.locator('dl.social-ai-facts > div').evaluateAll((rows) => Object.fromEntries(rows.map((row) => [row.querySelector('dt')?.textContent, row.querySelector('dd')?.textContent])));
  assert.equal(timing['Gesamte Bearbeitungszeit'], '183 s');
  assert.equal(timing['KI-Antwortzeit'], '78,2 s');
  assert.equal(timing['FFmpeg-Rechenzeit'], '42,9 s');
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
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).focus();
  await page.keyboard.press('Space');
  await page.getByRole('group', { name: 'Freigabe bestätigen' }).waitFor({ state: 'attached' });
  const opening = await page.evaluate(() => ({ focusedType: document.activeElement?.getAttribute('type'), focusedDescription: document.activeElement?.getAttribute('aria-describedby'), switchChecked: document.querySelector('[role="switch"]')?.getAttribute('aria-checked'), controls: document.querySelector('[role="switch"]')?.getAttribute('aria-controls'), groupId: document.querySelector('[role="group"][aria-label="Freigabe bestätigen"]')?.id }));
  assert.deepEqual(opening, { focusedType: 'checkbox', focusedDescription: 'social-ai-consent-description', switchChecked: 'false', controls: 'social-ai-consent-confirm', groupId: 'social-ai-consent-confirm' });
  await page.keyboard.press('Tab');
  const tabFocus = await page.evaluate(() => ({ tag: document.activeElement?.tagName, type: document.activeElement?.getAttribute('type'), text: document.activeElement?.textContent?.slice(0, 80) }));
  assert.equal(tabFocus.text, 'Abbrechen', JSON.stringify(tabFocus));
  assert.ok(await page.getByRole('button', { name: 'Zustimmung erteilen' }).isDisabled());
  const consentMetrics = await stableShot('ai-consent-confirm-mobile', 390, 844, '.social-ai-confirm');
  const consentBounds = await page.locator('.social-ai-confirm').evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { x: rect.left, y: rect.top + scrollY, width: rect.width, height: rect.height, pageHeight: document.documentElement.scrollHeight };
  });
  assert.ok(consentBounds.x >= 0 && consentBounds.x + consentBounds.width <= 390 && consentBounds.width > 290);
  assert.ok(consentBounds.y >= 0 && consentBounds.y + consentBounds.height <= consentBounds.pageHeight);
  await page.getByRole('button', { name: 'Abbrechen' }).evaluate((element) => element.click());
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('role')), 'switch');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'switch');
  const savedStatus = page.getByRole('status').filter({ hasText: 'Freigabe gespeichert.' });
  await savedStatus.waitFor({ state: 'attached' });
  assert.ok(await savedStatus.evaluate((element) => getComputedStyle(element).display !== 'none' && getComputedStyle(element).visibility !== 'hidden'));
  await page.locator('#social-ai-clip').selectOption('502');
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('status').filter({ hasText: 'Freigabe widerrufen.' }).waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'switch');
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await go('global-off');
  await page.getByText('Ein Admin muss die externe KI erst freigeben.').waitFor();
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await stableShot('ai-global-off-mobile', 390, 844, '.social-ai-global');
  await go('failed');
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).waitFor({ state: 'attached' });
  assert.ok(await page.getByRole('alert').filter({ hasText: 'Das bearbeitete Video konnte nicht erstellt werden.' }).isVisible());
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
      const kind = channel === '123456789' && init?.method === 'PUT' && url.pathname.endsWith('/consent') ? 'consent' : channel === '123456789' && init?.method === 'POST' && url.pathname.endsWith('/jobs') ? 'start' : channel === '123456789' && init?.method === 'POST' && url.pathname.endsWith('/retry') ? 'retry' : channel === '987654321' && init?.method === 'POST' && url.pathname.endsWith('/jobs') ? 'start-secondary' : channel === '987654321' && init?.method === 'POST' && url.pathname.endsWith('/retry') ? 'retry-secondary' : null;
      return kind ? new Promise((resolve, reject) => window.__aiPending.push({ kind, succeed: () => original(input, init).then(resolve, reject), fail: () => reject(new Error('Synthetic delayed failure')) })) : original(input, init);
    };
  });
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  assert.ok(await page.getByRole('button', { name: 'Abbrechen' }).isDisabled());
  assert.ok(await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).isDisabled());
  const pendingStatus = page.getByRole('status').filter({ hasText: 'Zustimmung wird gespeichert…' });
  await pendingStatus.waitFor({ state: 'attached' });
  assert.equal(await pendingStatus.getAttribute('aria-live'), 'polite');
  const pendingMetrics = await stableShot('ai-consent-pending-mobile', 390, 844, '.social-ai-confirm');
  await settleMutation('consent', false);
  const uncertainStatus = page.getByRole('status').filter({ hasText: 'Ob die Änderung übernommen wurde, ist unklar.' });
  await uncertainStatus.waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.textContent?.includes('Freigabe neu laden'));
  assert.ok(await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).isDisabled());
  assert.ok(await page.getByRole('button', { name: 'Zustimmung erteilen' }).isDisabled());
  assert.ok(await page.getByRole('button', { name: 'Freigabe neu laden' }).isEnabled());
  await page.getByRole('button', { name: 'Freigabe neu laden' }).evaluate((element) => element.click());
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'switch');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  assert.ok(await page.getByRole('button', { name: 'Abbrechen' }).isDisabled());
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).waitFor({ state: 'attached' });
  await settleMutation('consent', true);
  assert.equal(await page.getByRole('button', { name: 'Zustimmung erteilen' }).count(), 1);
  assert.equal(await page.getByText('Ob die Änderung übernommen wurde, ist unklar.', { exact: false }).count(), 0);
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
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1202');
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).evaluate((element) => element.click());
  await settleMutation('retry-secondary', false);
  await page.getByRole('alert').filter({ hasText: 'Der erneute Versuch konnte nicht gestartet werden.' }).waitFor({ state: 'attached' });
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1201');
  assert.equal(await page.getByText('Der erneute Versuch konnte nicht gestartet werden.').count(), 0);
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1202');
  assert.equal(await page.getByText('Der erneute Versuch konnte nicht gestartet werden.').count(), 0);
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).evaluate((element) => element.click());
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1201');
  await settleMutation('retry-secondary', false);
  assert.equal(await page.getByText('Der erneute Versuch konnte nicht gestartet werden.').count(), 0);
  await choose(page.locator('#social-ai-clip'), '601');
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await settleMutation('start-secondary', false);
  await page.getByRole('alert').filter({ hasText: 'Die Bearbeitung konnte nicht gestartet werden.' }).waitFor({ state: 'attached' });
  await choose(page.locator('#social-ai-clip'), '602');
  assert.equal(await page.getByText('Die Bearbeitung konnte nicht gestartet werden.').count(), 0);
  await choose(page.locator('#social-ai-clip'), '601');
  assert.equal(await page.getByText('Die Bearbeitung konnte nicht gestartet werden.').count(), 0);
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).evaluate((element) => element.click());
  await choose(page.locator('#social-ai-clip'), '602');
  await choose(page.getByRole('combobox', { name: 'Auftrag ansehen' }), '1202');
  await settleMutation('start-secondary', true);
  assert.equal(await page.getByRole('combobox', { name: 'Auftrag ansehen' }).inputValue(), '1202');
  assert.equal(await page.getByText('Die Bearbeitung konnte nicht gestartet werden.').count(), 0);
  await go('consent-off');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  await loseConsentPutResponseAndFirstReload();
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await page.getByRole('status').filter({ hasText: 'Ob die Änderung übernommen wurde, ist unklar.' }).waitFor({ state: 'attached' });
  assert.equal(await page.getByRole('switch').getAttribute('aria-checked'), 'false');
  assert.ok(await page.getByRole('switch').isDisabled());
  const uncertainMetrics = await stableShot('ai-consent-uncertain-mobile', 390, 844, '.social-ai-consent-feedback');
  await retryFailedConsentReload('Externe KI: zugestimmt', true, true, 'ai-consent-retry-error-mobile', 'ai-consent-retry-allowed-mobile');
  await go('review');
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor();
  await loseConsentPutResponseAndFirstReload(true);
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('status').filter({ hasText: 'Widerruf wird gespeichert…' }).waitFor({ state: 'attached' });
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await page.evaluate(() => window.__finishConsentPut());
  await page.getByRole('status').filter({ hasText: 'Ob die Änderung übernommen wurde, ist unklar.' }).waitFor({ state: 'attached' });
  assert.equal(await page.getByRole('switch').getAttribute('aria-checked'), 'true');
  assert.ok(await page.getByRole('switch').isDisabled());
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await retryFailedConsentReload('Externe KI: nicht zugestimmt', false, false, 'ai-consent-revoke-retry-error-mobile', 'ai-consent-retry-denied-mobile');
  await go('global-off');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  await loseConsentPutResponseAndFirstReload();
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).evaluate((element) => element.click());
  await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).evaluate((element) => element.click());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).evaluate((element) => element.click());
  await retryFailedConsentReload('Externe KI: zugestimmt', true, false, 'ai-consent-global-retry-error-mobile', 'ai-consent-global-denied-mobile');
  assert.ok(await page.getByText('Ein Admin muss die externe KI erst freigeben.').isVisible());
  await go('race');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  await page.evaluate(() => {
    const original = window.fetch;
    window.__aiInitialConsentReads = 0;
    window.__allowInitialConsentRetry = false;
    window.fetch = (input, init) => {
      const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.origin);
      if (url.pathname.endsWith('/ai/consent') && url.searchParams.get('twitch_user_id') === '987654321' && init?.method !== 'PUT') {
        window.__aiInitialConsentReads += 1;
        if (!window.__allowInitialConsentRetry) return Promise.reject(new Error('Synthetic initial consent load unavailable'));
      }
      return original(input, init);
    };
  });
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByRole('button', { name: 'Erneut laden' }).waitFor({ state: 'attached' });
  assert.equal(await page.getByRole('switch').count(), 0);
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await stableShot('ai-consent-initial-error-mobile', 390, 844, '.social-ai-consent');
  await page.evaluate(() => { window.__allowInitialConsentRetry = true; });
  await page.getByRole('button', { name: 'Erneut laden' }).focus();
  await page.keyboard.press('Enter');
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'switch');
  assert.ok(await page.getByRole('switch').isEnabled());
  assert.ok(await page.evaluate(() => window.__aiInitialConsentReads) >= 2);
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await stableShot('ai-consent-initial-recovered-mobile', 390, 844, '.social-ai-consent');
  assert.deepEqual(errors, []);
  const evidence = { environment: 'synthetic-local-preview', desktop, mobile, opening, consentMetrics, consentBounds, pendingMetrics, uncertainMetrics, playback, playbackReady: playback.every((video) => video.currentTime > 0 && video.readyState >= 2 && !video.error), channelRacePassed: true, sameChannelActionScopePassed: true, acceptedButResponseLostReloadPassed: true, failedReloadRetryAllowedPassed: true, failedReloadRetryDeniedPassed: true, failedReloadGlobalDeniedPassed: true, initialConsentGetRetryPassed: true, revokePendingAndUncertainGuardPassed: true, screenshots: 20, errors };
  await writeFile(`${directory}/run.json`, `${JSON.stringify(evidence, null, 2)}\n`);
  assert.ok(evidence.playbackReady, `synthetic media did not play: ${JSON.stringify(playback)}`);
  console.log(JSON.stringify(evidence, null, 2));
} finally {
  await page.close();
  await browser.close();
}
