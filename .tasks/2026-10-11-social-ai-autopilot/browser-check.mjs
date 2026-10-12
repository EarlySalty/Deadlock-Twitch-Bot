import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from '../../bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const directory = '/home/nathanael/.claude/sichtpruefung/track-b-wf-fac1dca2';
await mkdir(directory, { recursive: true });
const browser = await chromium.connectOverCDP('http://127.0.0.1:9258');
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();
const consoleErrors = [];
const requestFailures = [];
page.on('pageerror', error => consoleErrors.push(error.message));
page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()); });
page.on('requestfailed', request => requestFailures.push({ url: request.url(), failure: request.failure()?.errorText }));
const url = 'http://127.0.0.1:4188/src/components/socialmedia/autopilot/schedule-queue/fixture.html';
const measurements = [];
try {
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('.sq-jobs li');
  await page.evaluate(() => document.fonts.ready);
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    await page.waitForSelector('.sq-jobs li');
    await page.evaluate(() => document.fonts.ready);
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.waitForFunction(() => document.querySelectorAll('.sq-jobs li').length === 4);
    const measurement = await page.evaluate(() => ({ width: innerWidth, documentWidth: document.documentElement.scrollWidth, bodyWidth: document.body.scrollWidth, buttons: document.querySelectorAll('button').length }));
    assert.ok(measurement.documentWidth <= width, JSON.stringify(measurement));
    assert.ok(measurement.bodyWidth <= width, JSON.stringify(measurement));
    measurements.push(measurement);
    if (!process.argv.includes('--controls-only')) {
      await page.screenshot({ path: `${directory}/schedule-${width}.png` });
      if (width === 390) {
        await page.locator('.sq-queue').scrollIntoViewIfNeeded();
        await page.screenshot({ path: `${directory}/queue-390.png` });
      }
    }
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('.sq-jobs li');
  await page.getByLabel('Vorlauf in Stunden').fill('12');
  await page.locator('.sq-toggles label').nth(1).click();
  await page.getByRole('button', { name: 'Änderungen speichern' }).click();
  await page.waitForFunction(() => window.scheduleQueueFixture.snapshot().settings.paused);
  const saved = await page.evaluate(() => window.scheduleQueueFixture.requests.findLast(request => request.method === 'PUT' && request.path === '/autopilot/schedule'));
  assert.deepEqual(saved.body, { twitch_user_id: '42', lead_time_hours: 12, paused: true });
  await page.getByRole('button', { name: 'Als Nächstes', exact: true }).nth(1).click();
  await page.waitForFunction(() => window.scheduleQueueFixture.snapshot().jobs.filter(job => job.status === 'queued')[0].id === 103);
  await page.getByRole('button', { name: 'Entfernen', exact: true }).first().click();
  const confirmationGeometry = await page.evaluate(() => {
    const button = document.querySelector('.sq-confirm button');
    if (!button) throw new Error('Missing cancellation confirmation');
    const rect = button.getBoundingClientRect().toJSON();
    button.click();
    return rect;
  });
  await page.waitForFunction(() => window.scheduleQueueFixture.snapshot().jobs.find(job => job.id === 103).status === 'cancelled');
  const state = await page.evaluate(() => window.scheduleQueueFixture.snapshot());
  assert.equal(state.jobs.find(job => job.id === 101).status, 'running');
  assert.equal(state.jobs.find(job => job.id === 101).mode, 'static');
  assert.equal(state.jobs.filter(job => job.status === 'queued').length, 1);
  const requests = await page.evaluate(() => window.scheduleQueueFixture.requests);
  await page.getByLabel('Vorlauf in Stunden').fill('30');
  await page.getByRole('button', { name: 'Aktualisieren', exact: true }).click();
  assert.equal(await page.getByLabel('Vorlauf in Stunden').inputValue(), '30');
  await page.evaluate(() => window.setScheduleQueueOwner('43'));
  await page.waitForFunction(() => document.querySelector('input[type="number"]')?.value === '24');
  await page.evaluate(() => window.setScheduleQueueOwner('42'));
  await page.waitForFunction(() => document.querySelector('input[type="number"]')?.value === '12');
  const ownerIsolation = { dirtyDraftSurvivedRefresh: true, owner43Lead: 24, owner42SavedLead: 12 };
  await writeFile(`${directory}/browser-evidence.json`, JSON.stringify({ browser: 'moli 1.1.14', url, measurements, consoleErrors, requestFailures, confirmationGeometry, cancellationActivation: 'native DOM click after unsupported Moli geometry for the inserted button', requests, state }, null, 2));
  assert.deepEqual(consoleErrors, []);
  assert.deepEqual(requestFailures, []);
  console.log(JSON.stringify({ directory, measurements, consoleErrors, requestFailures, mutations: requests.filter(request => request.method !== 'GET').length }));
} finally {
  await context.close();
  await browser.close();
}
