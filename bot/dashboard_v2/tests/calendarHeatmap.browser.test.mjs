import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { createServer } from 'vite';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const fixture = `<!doctype html><html lang="de"><head><meta charset="utf-8"></head>
<body><div id="root"></div><script type="module">
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { CalendarHeatmap } from '/src/components/heatmaps/CalendarHeatmap.tsx';
import '/src/index.css';
const today = new Date().toISOString().slice(0, 10);
const lastYear = String(Number(today.slice(0, 4)) - 1) + '-09-01';
const data = [
  { date: today, streamCount: 70, hoursWatched: 250, value: 250 },
  { date: lastYear, streamCount: 89, hoursWatched: 450, value: 450 },
];
createRoot(document.getElementById('root')).render(
  createElement('main', { style: { padding: 16 } },
    createElement('div', { className: 'grid grid-cols-1 lg:grid-cols-2 gap-6' },
      createElement('div', { className: 'bg-card rounded-xl border border-border p-5' }, 'Vergleichskarte'),
      createElement(CalendarHeatmap, { data, days: Number(new URLSearchParams(location.search).get('days')) }),
    ),
  ),
);
</script></body></html>`;

test('Kalender bleibt bei 30 bis 3650 Tagen auf Desktop und Mobil sichtbar, lesbar und bedienbar', async t => {
  const server = await createServer({
    root: ROOT,
    base: '/',
    server: { host: '127.0.0.1', port: 0, strictPort: false, open: false },
    plugins: [{
      name: 'calendar-test-fixture',
      configureServer(vite) {
        vite.middlewares.use('/__calendar-fixture', async (_req, res, next) => {
          try {
            res.setHeader('Content-Type', 'text/html');
            res.end(await vite.transformIndexHtml('/__calendar-fixture', fixture));
          } catch (error) { next(error); }
        });
      },
    }],
  });
  t.after(() => server.close());
  await server.listen();
  const { port } = server.httpServer.address();
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const page = await browser.newPage({ reducedMotion: 'reduce' });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  // No real analytics or user data, and no requests to external services.
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1'
    ? route.continue() : route.abort());

  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    for (const days of [30, 365, 730, 3650]) {
      await page.goto(`http://127.0.0.1:${port}/__calendar-fixture?days=${days}`);
      const card = page.locator('[data-calendar-heatmap]');
      await card.waitFor({ state: 'visible' });
      const resolution = days > 366 ? 'month' : 'day';
      assert.equal(await card.getAttribute('data-resolution'), resolution);
      const geometry = await card.evaluate(element => {
        const cells = [...element.querySelectorAll('[data-calendar-cell]')];
        const scroll = element.querySelector('[data-calendar-scroll]');
        const labels = [...scroll.firstElementChild.firstElementChild.children];
        const labelBounds = labels.map(label => {
          const range = document.createRange();
          range.selectNodeContents(label);
          const rect = range.getBoundingClientRect();
          return { left: rect.left, right: rect.right };
        });
        return {
          cells: cells.length,
          minWidth: Math.min(...cells.map(cell => cell.getBoundingClientRect().width)),
          minHeight: Math.min(...cells.map(cell => cell.getBoundingClientRect().height)),
          pageOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          visible: cells.every(cell => getComputedStyle(cell).opacity === '1'),
          labelOverlap: labelBounds.some((rect, index) => index > 0 && rect.left < labelBounds[index - 1].right),
        };
      });
      assert.ok(geometry.minWidth >= (resolution === 'month' ? 24 : 12), JSON.stringify({ width, days, geometry }));
      assert.ok(geometry.minHeight >= 24);
      assert.ok(geometry.pageOverflow <= 1, JSON.stringify({ width, days, geometry }));
      assert.ok(geometry.visible);
      assert.ok(!geometry.labelOverlap, JSON.stringify({ width, days, geometry }));
      if (resolution === 'month') assert.ok(geometry.cells <= 132);
      else assert.equal(geometry.cells, days);

      if (resolution === 'day') {
        assert.ok(await card.evaluate(element => {
          const scroll = element.querySelector('[data-calendar-scroll]');
          const last = [...element.querySelectorAll('[data-calendar-cell]')].at(-1).getBoundingClientRect();
          const viewport = scroll.getBoundingClientRect();
          return last.left >= viewport.left && last.right <= viewport.right + 1;
        }), 'Aktuelle Tage müssen beim Öffnen sichtbar sein, nicht außerhalb des Scrollbereichs');
      }
      const active = card.locator('[data-calendar-cell]:not([data-stream-count="0"])').first();
      const label = await active.getAttribute('aria-label');
      await active.click();
      assert.equal(await card.locator('[data-calendar-details]').textContent(), label);
      await active.press('Escape');
      assert.match(await card.locator('[data-calendar-details]').textContent(), /Für Details/);
      await active.focus();
      // Move focus away and back to exercise keyboard activation even after Escape.
      await active.press('Tab');
      await active.focus();
      assert.equal(await card.locator('[data-calendar-details]').textContent(), label);
      assert.deepEqual(errors, []);
      console.log(JSON.stringify({ width, days, ...geometry }));

      if (process.env.CALENDAR_SCREENSHOT_DIR && (days === 3650 || days === 365)) {
        await mkdir(process.env.CALENDAR_SCREENSHOT_DIR, { recursive: true });
        await card.screenshot({ path: path.join(process.env.CALENDAR_SCREENSHOT_DIR, `${days}-${width}.png`) });
      }
    }
  }
});
