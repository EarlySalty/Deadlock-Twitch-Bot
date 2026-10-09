import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { chromium } from '../../bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const browser = await chromium.connectOverCDP('http://127.0.0.1:9237');
const context = await browser.newContext({ viewport: { width: 1440, height: 1080 } });
const page = await context.newPage();
const evidence = '/home/nathanael/.claude/sichtpruefung/tiktok-dialog-tiefe';
const writes = [];
page.on('request', request => { if (request.method() === 'PUT') writes.push(request.url()); });
try {
  await page.goto('http://127.0.0.1:4197/tiktok-evidence.html');
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('Teamfight'));
  const dialog = page.getByRole('dialog');
  const privacy = dialog.getByLabel('Wer darf den Clip sehen?');
  assert.equal(await privacy.inputValue(), '');
  assert.equal(await dialog.getByLabel('Duett', { exact: true }).isChecked(), false);
  await dialog.getByRole('button', { name: 'Meine Standardwerte übernehmen', exact: true }).evaluate(node => node.click());
  assert.equal(await privacy.inputValue(), 'SELF_ONLY');
  assert.equal(await dialog.getByLabel(/Kommentare/).isChecked(), false);
  assert.equal(await dialog.getByLabel('Duett', { exact: true }).isChecked(), true);
  assert.equal(await dialog.getByRole('checkbox').last().isChecked(), false);
  await dialog.getByRole('button', { name: 'Speichern', exact: true }).evaluate(node => node.click());
  await dialog.getByRole('button', { name: 'Gespeicherten Entwurf laden', exact: true }).waitFor({ state: 'attached' });
  await dialog.getByLabel('Beschreibung für TikTok').evaluate(node => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(node, 'Andere Beschreibung');
    node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await dialog.getByRole('button', { name: 'Gespeicherten Entwurf laden', exact: true }).evaluate(node => node.click());
  assert.equal(await dialog.getByLabel('Beschreibung für TikTok').inputValue(), 'Meine Beschreibung #deadlock');
  assert.equal(await dialog.getByRole('checkbox').last().isChecked(), false);
  const measurements = [];
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1080 });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: `${evidence}/dialog-${width}.png` });
    measurements.push(await page.evaluate(() => ({ width: innerWidth, documentWidth: document.documentElement.scrollWidth, fields: [...document.querySelectorAll('.studio-input, input[type=checkbox]')].map(node => { const s = getComputedStyle(node); return { type: node.type, appearance: s.appearance, border: s.borderColor, background: s.backgroundColor, image: s.backgroundImage, shadow: s.boxShadow }; }) })));
  }
  await fs.writeFile(`${evidence}/moli-dom.json`, JSON.stringify({ measurements, writes }, null, 2));
  assert.ok(measurements.every(value => value.documentWidth <= value.width));
  console.log('Moli: 10 interaction assertions passed; desktop and mobile captured; isolated API fixture, no external upload.');
} finally {
  await context.close();
  await browser.close();
}
