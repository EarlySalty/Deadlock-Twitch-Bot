import { chromium } from 'playwright-core';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

const directory = process.argv[2];
if (!directory) throw new Error('Evidence directory required');
await mkdir(directory, { recursive: true });
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.MOLI_PORT ?? '9257'}`);
const page = await (browser.contexts()[0] ?? await browser.newContext()).newPage();
const base = `http://127.0.0.1:${process.env.PREVIEW_PORT ?? '4217'}/src/components/socialmedia/ai/preview.html?case=revisions`;
const errors = [];
const shots = [];
page.on('pageerror', (error) => errors.push(error.message));

async function shot(name, width, target) {
  await page.setViewportSize({width,height:width === 390 ? 844 : 900});
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({animations:'disabled'});
  await page.locator(target).evaluate((element) => element.scrollIntoView({block:'start'}));
  await page.waitForTimeout(120);
  await page.screenshot({path:`${directory}/${name}.png`,animations:'disabled'});
  const dimensions = await page.evaluate(() => ({viewport:innerWidth,document:document.documentElement.scrollWidth}));
  assert.equal(dimensions.viewport,width);
  assert.ok(dimensions.document <= dimensions.viewport,JSON.stringify(dimensions));
  shots.push({name,...dimensions});
}

async function click(name) {
  const button = page.getByRole('button',{name,exact:true});
  assert.ok(await button.isEnabled(),`${name} is disabled`);
  await button.evaluate((element) => element.click());
}

try {
  await page.goto(base);
  await page.getByRole('button',{name:'Änderung anfordern'}).waitFor();
  const multi = page.getByRole('checkbox',{name:/^Mehrere Videos aus einem Clip erlauben/});
  await multi.waitFor();
  assert.equal(await multi.isChecked(),false);
  assert.equal(await page.getByRole('checkbox',{name:/^Experimentelles KI-Layout/}).isChecked(),false);
  await multi.evaluate((element) => element.click());
  await page.getByText('Optionen gespeichert.').waitFor({state:'attached'});
  assert.equal(await multi.isChecked(),true);
  await shot('options-desktop',1440,'.social-ai-options');
  await shot('options-mobile',390,'.social-ai-options');
  assert.equal(await page.getByRole('combobox',{name:'Videofassung'}).inputValue(),'2203');
  assert.ok(await page.getByRole('button',{name:'In den Zeitplan übernehmen'}).isDisabled());
  await page.locator('details summary').evaluate((element) => element.click());
  assert.equal(await page.getByRole('combobox',{name:'Vergleichsfassung'}).inputValue(),'2201');
  await shot('compare-desktop',1440,'.social-ai-compare');
  await shot('controls-desktop',1440,'.social-ai-revisions');
  await shot('controls-mobile',390,'.social-ai-revisions');
  await page.getByRole('checkbox',{name:/YouTube/}).evaluate((element) => element.click());
  assert.ok(await page.getByRole('button',{name:'In den Zeitplan übernehmen'}).isDisabled());
  await click('Ausgewählte Plattformen freigeben');
  await page.waitForFunction(() => document.querySelector('.social-ai-platforms label small')?.textContent === 'Video freigegeben');
  assert.ok(await page.getByRole('button',{name:'In den Zeitplan übernehmen'}).isEnabled());
  await click('In den Zeitplan übernehmen');
  await page.getByRole('status').filter({hasText:'Ohne erneutes Rendern eingeplant'}).waitFor({state:'attached'});
  await page.getByRole('textbox',{name:'Was soll sich an dieser Fassung ändern?'}).fill('Kürze den Einstieg, behalte die ursprüngliche Reaktion.');
  await click('Änderung anfordern');
  await page.getByRole('group',{name:'Aufwendige Änderung bestätigen'}).waitFor({state:'attached'});
  assert.ok(await page.getByRole('button',{name:'Änderung bestätigen'}).isDisabled());
  await shot('confirmation-desktop',1440,'.social-ai-revisions');
  await shot('confirmation-mobile',390,'.social-ai-revisions');
  await page.getByRole('checkbox',{name:/Ich bestätige den vorgeschlagenen/}).focus();
  await page.keyboard.press('Space');
  assert.ok(await page.getByRole('button',{name:'Änderung bestätigen'}).isEnabled());
  await click('Änderung verwerfen');
  await page.locator('.social-ai-status--rejected').waitFor({state:'attached'});
  assert.ok(await page.getByRole('textbox',{name:'Was soll sich an dieser Fassung ändern?'}).isEnabled());
  await page.getByRole('textbox',{name:'Was soll sich an dieser Fassung ändern?'}).fill('Behalte diese Fassung als Grundlage.');
  assert.ok(await page.getByRole('button',{name:'Änderung anfordern'}).isEnabled());
  assert.ok(await page.locator('video[aria-label="Bearbeitetes Video abspielen"]').getAttribute('src'));
  assert.deepEqual(errors,[]);
  await writeFile(`${directory}/run.json`,JSON.stringify({environment:'synthetic-local-moli',passed:9,errors,shots},null,2));
  console.log(JSON.stringify({passed:9,errors,shots}));
} finally {
  await page.close();
  await browser.close();
}
