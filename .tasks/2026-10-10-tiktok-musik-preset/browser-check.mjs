import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { chromium } from '../../bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const browser = await chromium.connectOverCDP('http://127.0.0.1:9338');
const context = await browser.newContext({ viewport: { width: 1440, height: 1080 } });
const page = await context.newPage();
const evidence = '/home/nathanael/.claude/sichtpruefung/tiktok-musik-preset';
await fs.mkdir(evidence, { recursive: true });
const click = locator => locator.evaluate(node => node.click());
const postCheckbox = () => page.getByRole('checkbox', { name: /Mit der Veröffentlichung/ });
const saveCheckbox = () => page.getByRole('checkbox', { name: 'Der Musiknutzung von TikTok für alle meine Clips zustimmen' });
const apply = () => click(page.getByRole('button', { name: 'Meine Standardwerte übernehmen', exact: true }));
const saveDefaults = () => click(page.getByRole('button', { name: 'Als Standard speichern', exact: true }));
const saved = () => page.waitForFunction(() => document.body.textContent.includes('Deine Standardwerte sind gespeichert.'));
const writes = () => page.evaluate(() => JSON.parse(document.body.dataset.tiktokEditorWrites ?? '[]'));
try {
  await page.goto('http://127.0.0.1:4198/twitch/dashboard-v2/tiktok-evidence.html');
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('Teamfight'));
  assert.equal(await saveCheckbox().isChecked(), false);
  assert.equal(await postCheckbox().isChecked(), false);
  await apply();
  assert.equal(await postCheckbox().isChecked(), false);
  await click(saveCheckbox());
  await saveDefaults();
  await saved();
  assert.equal(await saveCheckbox().isChecked(), false);
  assert.equal(await postCheckbox().count(), 0);
  assert.ok((await page.locator('body').textContent()).includes('Deine Musikzustimmung ist im Standard gespeichert.'));
  await page.getByLabel('Beschreibung für TikTok').evaluate(node => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(node, 'Andere Beschreibung');
    node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await page.getByLabel('Wer darf den Clip sehen?').evaluate(node => {
    node.value = 'PUBLIC_TO_EVERYONE';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await click(page.getByRole('checkbox', { name: 'Duett', exact: true }));
  assert.equal(await postCheckbox().count(), 0);
  const schedule = page.getByRole('button', { name: 'Clip mit TikTok einplanen', exact: true });
  assert.equal(await schedule.isDisabled(), false);
  await schedule.evaluate(node => node.form.requestSubmit());
  const options = await page.evaluate(() => JSON.parse(document.body.dataset.tiktokPostOptions));
  assert.equal(options.consent, true);
  assert.equal(options.caption, 'Andere Beschreibung');
  await click(page.getByRole('checkbox', { name: 'Dieser Clip enthält Werbung' }));
  await click(page.getByRole('checkbox', { name: 'Markenpartner', exact: true }));
  assert.equal(await postCheckbox().isChecked(), false);
  assert.equal(await schedule.isDisabled(), true);
  assert.equal(await page.getByRole('link', { name: 'Richtlinie für Markeninhalte' }).count(), 1);
  await click(postCheckbox());
  assert.equal(await schedule.isDisabled(), false);
  await click(page.getByRole('button', { name: 'Speichern', exact: true }));
  await page.waitForFunction(() => document.body.textContent.includes('Gespeichert. Der Clip wurde nicht eingeplant.'));
  assert.ok(!('music_consent' in (await writes()).at(-1).body));
  await click(page.getByRole('button', { name: 'Gespeicherten Entwurf laden', exact: true }));
  assert.equal(await postCheckbox().isChecked(), false);
  await apply();
  assert.equal(await postCheckbox().count(), 0);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('tiktok-fixture-account', { detail: 'different-account' })));
  await page.waitForFunction(() => [...document.querySelectorAll('input[type=checkbox]')].some(node => node.parentElement.textContent.includes('Mit der Veröffentlichung')));
  await apply();
  assert.equal(await postCheckbox().isChecked(), false);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('tiktok-fixture-account', { detail: 'fixture-only' })));
  await page.waitForFunction(() => !document.body.textContent.includes('Aktuelle TikTok-Einstellungen werden geladen.'));
  await apply();
  assert.equal(await postCheckbox().count(), 0);
  const measurements = [];
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1080 });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: `${evidence}/dialog-${width}.png` });
    if (width === 390) {
      await page.evaluate(() => { const node = document.querySelector('.studio-dialog-content'); node.scrollTop = node.scrollHeight; });
      await page.screenshot({ path: `${evidence}/dialog-390-footer.png` });
    }
    measurements.push(await page.evaluate(() => ({ width: innerWidth, documentWidth: document.documentElement.scrollWidth, notice: document.body.textContent.includes('Mit der Veröffentlichung stimmst du TikToks'), checkboxes: [...document.querySelectorAll('input[type=checkbox]')].map(node => ({ checked: node.checked, text: node.parentElement.textContent })) })));
  }
  assert.ok(measurements.every(value => value.documentWidth <= value.width && value.notice));
  await saveDefaults();
  await saved();
  assert.equal(await postCheckbox().isChecked(), false);
  await apply();
  assert.equal(await postCheckbox().isChecked(), false);
  await fs.writeFile(`${evidence}/moli-dom.json`, JSON.stringify({ measurements, writes: await writes(), options }, null, 2));
  console.log('Moli: preset grant, edits, post consent, branded consent, draft isolation, account switch, revocation and desktop/mobile layout passed. Synthetic fixture, no upload.');
} finally {
  await context.close();
  await browser.close();
}
