import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { chromium } from '../../bot/dashboard_v2/node_modules/playwright-core/index.mjs';

const output = '/home/nathanael/.claude/sichtpruefung/social-aufraeumen-1';
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ executablePath: '/usr/local/bin/google-chrome', headless: true });
try {
  for (const state of ['loading', 'missing-id', 'ready']) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    const page = await context.newPage();
    let release;
    let granted = true;
    const writes = [];
    const pending = new Promise((resolve) => { release = resolve; });
    await page.route('**/twitch/api/**', async (route) => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      let response = {};
      if (path.endsWith('/auth-status')) {
        response = { authenticated: true, isAdmin: true, authLevel: 'admin', csrfToken: 'visual-only', loginUrl: '/login', discordLoginUrl: '/login' };
      } else if (path.endsWith('/partner-access')) {
        if (request.method() === 'PUT') {
          const body = request.postDataJSON();
          writes.push(body);
          granted = body.granted;
          response = { success: true };
        } else {
          if (state === 'loading') await pending;
          response = { items: [{ twitch_user_id: '123456789', streamer_login: 'alter_name', granted, granted_by: 'Sichtprüfung', granted_at: '2026-10-07T08:00:00Z' }] };
        }
      } else if (path.endsWith('/streamers/sichtpruefung')) {
        response = { login: 'sichtpruefung', display_name: 'Sichtprüfung', twitch_user_id: state === 'missing-id' ? null : '123456789', verified: true, archived: false, partner_status: 'active', sessions: [], settings: {}, stats: {} };
      } else if (path.includes('engagement')) {
        response = { enabled: false };
      }
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(response) });
    });
    await page.goto('http://localhost:4182/twitch/admin/community/streamers/sichtpruefung');
    const toggle = page.getByRole('switch', { name: /Partner-Freigabe/ });
    await toggle.waitFor({ state: 'visible' });
    await toggle.scrollIntoViewIfNeeded();
    await page.evaluate(async () => {
      await document.fonts.ready;
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
    assert.equal(await toggle.isDisabled(), state !== 'ready');
    const dimensions = await page.evaluate(() => ({ document: document.documentElement.scrollWidth, viewport: innerWidth }));
    assert.ok(dimensions.document <= dimensions.viewport);
    await page.screenshot({ path: `${output}/${state}.png`, fullPage: false });
    if (state === 'ready') {
      assert.equal(await toggle.getAttribute('aria-checked'), 'true');
      await toggle.click();
      await page.waitForFunction(() => document.querySelector('[aria-label="Partner-Freigabe erteilen"]')?.getAttribute('aria-checked') === 'false');
      assert.deepEqual(writes, [{ twitch_user_id: '123456789', granted: false }]);
    }
    release();
    console.log(JSON.stringify({ state, disabled: state !== 'ready', dimensions, writes, screenshot: `${output}/${state}.png`, fixture: true }));
    await context.close();
  }
} finally {
  await browser.close();
}
