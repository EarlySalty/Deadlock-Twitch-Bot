import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from '/tmp/tb-gold-tools/node_modules/playwright-core/index.mjs';

const destination = new URL('./screenshots-v3/', import.meta.url).pathname;
await mkdir(destination, { recursive: true });
const browser = await chromium.launch({
  executablePath: '/home/nathanael/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome',
  headless: true,
  timeout: 15000,
});
const results = [];
const baselines = new Map();
try {
  for (const route of ['analyse', 'dashboard']) {
    for (const variant of ['original', 'polished', 'antique']) {
      const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
      const errors = [];
      const apiRequests = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => {
        if (new URL(request.url()).pathname.includes('/api/')) apiRequests.push(request.url());
      });
      await page.goto(`http://100.117.29.112:4187/${route}?gold=${variant}`, { waitUntil: 'networkidle', timeout: 20000 });
      await page.getByRole('button', { name: 'Hilfe bekommen', exact: true }).waitFor({ timeout: 10000 });
      if (route === 'analyse') await page.getByText('Platz 7', { exact: true }).waitFor();
      else await page.getByRole('link', { name: 'Zur Analyse', exact: true }).waitFor();
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(2000);
      assert.deepEqual(errors, [], `${route}/${variant}: Browserfehler`);
      assert.deepEqual(apiRequests, [], `${route}/${variant}: Backend-Aufruf`);
      const appearance = await page.evaluate(() => {
        const root = getComputedStyle(document.documentElement);
        const tokenNames = ['success', 'warning', 'danger', 'info', 'chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5', 'primary', 'accent', 'accent-hover', 'bg', 'card'];
        return {
          variant: document.documentElement.dataset.gold ?? 'original',
          tokens: tokenNames.map(name => root.getPropertyValue(`--color-${name}`).trim()),
          bodyBackground: getComputedStyle(document.body).backgroundImage,
          charts: Array.from(document.querySelectorAll('.recharts-area-curve, .recharts-radar-polygon')).map(element => getComputedStyle(element).stroke),
          helpBackground: getComputedStyle(document.querySelector('.assistent-knopf')).backgroundImage,
          helpShadow: getComputedStyle(document.querySelector('.assistent-knopf')).boxShadow,
          horizontalOverflow: document.documentElement.scrollWidth > window.innerWidth,
        };
      });
      assert.equal(appearance.variant, variant);
      assert.equal(appearance.horizontalOverflow, false);
      if (variant === 'original') baselines.set(route, appearance);
      else {
        const original = baselines.get(route);
        assert.deepEqual(appearance.tokens, original.tokens);
        assert.deepEqual(appearance.charts, original.charts);
        assert.equal(appearance.bodyBackground, original.bodyBackground);
        assert.match(appearance.helpBackground, /linear-gradient/);
        assert.ok(appearance.helpShadow.split(', rgba').every(shadow => shadow.includes('inset')));
      }
      await page.screenshot({ path: `${destination}${route}-${variant}.png` });
      await page.screenshot({ path: `${destination}${route}-${variant}-full.png`, fullPage: true });
      results.push(`${route}/${variant}: OK, keine API-Aufrufe, keine Browserfehler, kein horizontaler Überlauf`);
      console.log(results.at(-1));
      await page.close();
    }
  }
  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 } });
  for (const route of ['analyse', 'dashboard']) {
    await mobile.goto(`http://100.117.29.112:4187/${route}?gold=polished`, { waitUntil: 'networkidle' });
    await mobile.waitForTimeout(2000);
    assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false);
    await mobile.screenshot({ path: `${destination}${route}-polished-mobile.png`, fullPage: true });
    results.push(`${route}/polished-mobile: OK, kein horizontaler Überlauf`);
  }
  await writeFile(new URL('./browser-checks-v3.txt', import.meta.url), `${results.join('\n')}\nStatusfarben, Diagrammfarben, Markentokens und Seitenhintergrund in allen Varianten identisch.\n`);
} finally {
  await browser.close();
}
