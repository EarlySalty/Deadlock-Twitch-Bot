import fs from 'node:fs';
import { chromium } from '/home/nathanael/.worktrees/tb-vod-archiv-status-20261007/bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const folder = '/home/nathanael/.worktrees/tb-vod-archiv-status-20261007/.tasks/2026-10-07-vod-archiv-status/pruefung';
const browser = await chromium.connectOverCDP('http://127.0.0.1:9337');
const context = browser.contexts()[0];
const results = [];
try {
  for (const [name, width, height] of [['desktop', 1440, 1100], ['mobil', 390, 844]]) {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.setViewportSize({ width, height });
    await page.goto('http://127.0.0.1:4197/twitch/social-media?view=archiv');
    await page.locator('#vod-archive-title').waitFor();
    await page.waitForFunction(() => document.querySelectorAll('article').length >= 8);
    await page.evaluate(() => document.fonts.ready);
    const metrics = await page.evaluate(() => ({
      viewport: innerWidth, document: document.documentElement.scrollWidth,
      articles: [...document.querySelectorAll('article')].map((article) => {
        const heading = article.querySelector('h3');
        const status = article.querySelector('h3 + div > span');
        const h = heading.getBoundingClientRect(), s = status.getBoundingClientRect();
        return { title: heading.textContent, status: status.textContent, titleLeft: h.left, statusLeft: s.left, statusRight: s.right, icon: status.querySelector('svg') !== null, time: article.querySelector('time')?.getAttribute('datetime') ?? null, buttons: article.querySelectorAll('button').length };
      }),
      forbiddenAttempt: document.body.innerText.includes('Noch kein Versuch'),
      text: document.body.innerText,
    }));
    if (metrics.document > metrics.viewport || metrics.forbiddenAttempt || metrics.articles.some((item) => !item.icon || item.statusLeft < 0 || item.statusRight > metrics.viewport)) throw new Error(JSON.stringify(metrics));
    await page.screenshot({ path: `${folder}/${name}-oben.png` });
    await page.getByRole('heading', { name: 'Bestätigter älterer Upload', exact: true }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${folder}/${name}-abschluss.png` });
    results.push({ name, width, height, metrics, errors });
    await page.close();
  }
  fs.writeFileSync(`${folder}/moli-layout.json`, JSON.stringify(results, null, 2));
  console.log(JSON.stringify(results.map(({ name, metrics, errors }) => ({ name, viewport: metrics.viewport, document: metrics.document, articles: metrics.articles.length, icons: metrics.articles.every((item) => item.icon), forbiddenAttempt: metrics.forbiddenAttempt, errors }))));
} finally {
  await browser.close();
}
