import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';

const directory = process.argv[2];
if (!directory) throw new Error('Screenshot directory required');
await mkdir(directory, { recursive: true });
const browser = await chromium.connectOverCDP('http://127.0.0.1:9237');
const context = browser.contexts()[0] ?? await browser.newContext();
const page = await context.newPage();
const base = 'http://localhost:4189/src/components/socialmedia/ai/preview.html';
const errors = [];
page.on('pageerror', (error) => errors.push(error.message));

async function stableShot(name, width, height, focus) {
  await page.setViewportSize({ width, height });
  await page.evaluate(() => document.fonts.ready);
  await page.locator('.social-ai').waitFor({ state: 'visible' });
  if (focus) await page.locator(focus).evaluate((element) => element.scrollIntoView({ block: 'start' }));
  await page.screenshot({ path: `${directory}/${name}.png`, animations: 'disabled' });
  const metrics = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth, videos: [...document.querySelectorAll('video')].map((video) => ({ controls: video.controls, label: video.getAttribute('aria-label'), readyState: video.readyState, src: video.currentSrc })) }));
  assert.ok(metrics.document <= metrics.viewport, `${name}: horizontal overflow ${JSON.stringify(metrics)}`);
  return metrics;
}

try {
  await page.goto(`${base}?case=review`);
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
  assert.ok(await page.getByText('Die neueste Fassung ist nicht abspielbar.').isVisible());
  assert.ok(await page.getByText('Noch unbekannt').count() > 0);
  await page.goto(`${base}?case=consent-off`);
  await page.setViewportSize({ width: 1000, height: 2000 });
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).waitFor();
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  await page.getByRole('switch', { name: 'Externe KI: nicht zugestimmt' }).click();
  assert.ok(await page.getByRole('button', { name: 'Zustimmung erteilen' }).isDisabled());
  await page.locator('.social-ai-confirm label').evaluate((element) => element.scrollIntoView({ block: 'center' }));
  await page.screenshot({ path: `${directory}/ai-consent-confirm-mobile.png`, animations: 'disabled' });
  await page.locator('.social-ai-confirm label').click();
  assert.ok(await page.getByRole('checkbox', { name: /Ich stimme der Übertragung/ }).isChecked());
  await page.getByRole('button', { name: 'Zustimmung erteilen' }).click();
  await page.getByRole('switch', { name: 'Externe KI: zugestimmt' }).waitFor();
  await page.locator('#social-ai-clip').selectOption('502');
  await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).click();
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  const consent = await page.locator('.social-ai-status--rendering').textContent();
  await page.goto(`${base}?case=global-off`);
  await page.getByText('Die globale Freigabe ist gesperrt.').waitFor();
  assert.ok(await page.getByRole('button', { name: 'KI-Bearbeitung manuell starten' }).isDisabled());
  const global = await stableShot('ai-global-off', 390, 844, '.social-ai-global');
  await page.goto(`${base}?case=failed`);
  await page.setViewportSize({ width: 1440, height: 2000 });
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).waitFor();
  await stableShot('ai-failed-render', 1440, 900, '.social-ai-failure');
  await page.getByRole('button', { name: 'Fehlgeschlagene Phase erneut versuchen' }).click();
  await page.locator('.social-ai-status--rendering').waitFor({ state: 'attached' });
  const retry = await page.locator('.social-ai-status--rendering').textContent();
  await page.goto(`${base}?case=review`);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('button', { name: 'Vorschaukanal wechseln' }).click();
  await page.getByText('Die Aufträge konnten nicht geladen werden.').waitFor({ state: 'attached' });
  assert.equal(await page.getByText('Auftrag 1.101').count(), 0);
  await stableShot('ai-other-channel-error', 1440, 900, '.social-ai-history');
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ desktop, mobile, consent, global, retry, errors }, null, 2));
} finally {
  await page.close();
  await browser.close();
}
