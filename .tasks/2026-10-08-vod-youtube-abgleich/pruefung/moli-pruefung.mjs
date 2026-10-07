import fs from 'node:fs';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chromium } from '/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const root = '/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008';
const folder = `${root}/.tasks/2026-10-08-vod-youtube-abgleich/pruefung`;
const dist = `${root}/bot/analytics/dashboard_v2/dist`;
const browser = await chromium.connectOverCDP('http://127.0.0.1:9338');
const context = browser.contexts()[0];
const sha = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
const dirty = execFileSync('git', ['-C', root, 'status', '--porcelain'], { encoding: 'utf8' }).length > 0;
const files = [`${dist}/index.html`, ...fs.readdirSync(`${dist}/assets`).map(name => `${dist}/assets/${name}`)];
const hashes = Object.fromEntries(files.map(file => [file.slice(dist.length + 1), crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')]));
const results = [];
try {
  for (const scenario of ['current', 'partial', 'connection']) {
  for (const [sizeName, width, height] of [['desktop', 1440, 1100], ['mobil', 390, 844]]) {
    const name = `${sizeName}-${scenario}`;
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.setViewportSize({ width, height });
    await page.setExtraHTTPHeaders({ 'X-Archive-Fixture': scenario });
    await page.goto(`http://127.0.0.1:4198/twitch/social-media?view=archiv&fixture=${scenario}`);
    await page.locator('#vod-archive-title').waitFor();
    await page.waitForFunction(() => document.querySelectorAll('article').length >= 9);
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: `${folder}/${name}-oben.png` });
    const metrics = await page.evaluate(() => ({
      viewport: innerWidth,
      document: document.documentElement.scrollWidth,
      fonts: { status: document.fonts.status, manrope: document.fonts.check('16px Manrope'), sora: document.fonts.check('16px Sora') },
      images: [...document.images].map(image => ({ src: image.getAttribute('src'), complete: image.complete, width: image.naturalWidth })),
      articles: [...document.querySelectorAll('article')].map(article => {
        const heading = article.querySelector('h3');
        const status = article.querySelector('h3 + div > span');
        const rect = status.getBoundingClientRect();
        return { title: heading.textContent, status: status.textContent, left: rect.left, right: rect.right, icon: status.querySelector('svg') !== null, times: [...article.querySelectorAll('time')].map(time => time.getAttribute('datetime')), links: [...article.querySelectorAll('a')].map(link => link.getAttribute('href')), buttons: [...article.querySelectorAll('button')].map(button => ({ text: button.textContent, disabled: button.disabled })), text: article.innerText };
      }),
    }));
    const proof = metrics.articles.find(article => article.title === 'Fremder Stream');
    const expected = JSON.parse(fs.readFileSync(`${folder}/youtube-${scenario === 'current' ? 'current' : scenario === 'connection' ? 'connection' : 'partial'}.json`, 'utf8')).items[0];
    if (metrics.document > metrics.viewport || metrics.articles.some(article => !article.icon || article.left < 0 || article.right > metrics.viewport || article.right <= article.left) || !proof?.times.length || !proof.text.includes(expected.status_label) || !proof.buttons.some(button => button.disabled) || errors.length) throw new Error(JSON.stringify({ metrics, errors, scenario }));
    if (scenario === 'connection' && !proof.links.some(link => link.includes('oauth/start/youtube'))) throw new Error('Existing reconnect path missing from connection fixture');
    await page.getByRole('heading', { name: 'Fremder Stream', exact: true }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${folder}/${name}-youtube.png` });
    await page.getByRole('heading', { name: 'Unklarer älterer Abschluss', exact: true }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${folder}/${name}-unklar.png` });
    results.push({ name, width, height, metrics, errors });
    await page.close();
  }
  }
  fs.writeFileSync(`${folder}/moli-layout.json`, JSON.stringify({ fixtureOnly: true, sha, dirty, hashes, results }, null, 2));
  console.log(JSON.stringify({ fixtureOnly: true, sha, dirty, results: results.map(({ name, metrics, errors }) => ({ name, viewport: metrics.viewport, document: metrics.document, articles: metrics.articles.length, errors })) }));
} finally {
  await browser.close();
}
