import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import test from 'node:test';
const { chromium } = await import(process.env.STUDIO_PLAYWRIGHT_MODULE ?? 'playwright-core');

const dist = path.resolve(import.meta.dirname, '../../analytics/dashboard_v2/dist');
const evidence = path.resolve(import.meta.dirname, '../../../.tasks/2026-09-21-social-studio');
const layout = {
  version: 1,
  source: { width: 1920, height: 1080 },
  game_crop: { x: 420, y: 0, w: 1080, h: 1080 },
  cam_crop: { x: 1500, y: 50, w: 380, h: 380 },
  cam_position: { x: 712, y: 48, w: 320, h: 320 },
  cam_enabled: true,
  mode: 'pip',
};
const platforms = ['youtube', 'tiktok', 'instagram'];
const makePlan = (streamer) => ({
  streamer_login: streamer,
  approval_mode: 'manual',
  approval_modes: ['manual', 'veto_window', 'full_auto'],
  timezone: 'Europe/Berlin',
  subtitles_enabled: true,
  platforms: platforms.map((platform) => ({
    platform,
    auto_post: platform !== 'instagram',
    posts_per_week: 3,
    max_posts_per_day: 1,
    post_times: ['18:00'],
    next_slot: '2026-09-25T16:00:00Z',
  })),
  categories: [
    {
      category_key: 'deadlock',
      display_name: 'Deadlock',
      enrichment_enabled: true,
      auto_post: true,
    },
    {
      category_key: 'other',
      display_name: 'Andere Spiele',
      enrichment_enabled: false,
      auto_post: false,
    },
  ],
  pool: {
    verfuegbare_clips: 25,
    aktive_plattformen: 2,
    reicht_fuer_posts: 50,
    posts_pro_woche: 6,
    reicht_fuer_tage: 58,
    warnung: false,
  },
});
const makeClips = (streamer) =>
  Array.from({ length: 105 }, (_, i) => ({
    clip_db_id: i + 1,
    clip_id: `fixture-${i + 1}`,
    title:
      i === 104
        ? 'Letzte Seite Bebop'
        : i === 0
          ? 'Bebop überlebt den Teamfight'
          : `Community-Clip ${i + 1}`,
    thumbnail_url: null,
    clip_url: null,
    streamer_login: streamer,
    created_at: '2026-09-21T12:00:00Z',
    duration_seconds: 34,
    view_count: 2800,
    game_name: 'Deadlock',
    status: i < 3 ? 'awaiting_approval' : i === 3 ? 'failed' : i === 104 ? 'approved' : 'pending',
    source_kind: 'twitch',
    upload_local_path: null,
    retention_until: '2026-10-01T12:00:00Z',
    discarded_at: null,
    platform_status: { youtube: false, tiktok: false, instagram: false },
    effective_layout: layout,
    layout_override: null,
    approval: {
      state: i < 3 ? 'awaiting_approval' : 'approved',
      approved_platforms: ['youtube', 'tiktok'],
      not_scheduled: [],
    },
    scheduled_at:
      i === 104 ? { youtube: '2026-09-25T16:00:00Z', tiktok: '2026-09-25T18:00:00Z' } : {},
  }));

test(
  'Social Studio: Produktionsbundle mit isoliertem API-Vertrag',
  { timeout: 120000 },
  async (t) => {
    const plans = { earlysalty: makePlan('earlysalty'), partner2: makePlan('partner2') };
    const clips = { earlysalty: makeClips('earlysalty'), partner2: makeClips('partner2') };
    const writes = [];
    const requests = [];
    const errors = [];
    let failTarget = '';
    let fremdAenderung = null;
    let failQueue = false;
    const server = http.createServer(async (req, res) => {
      const url = new URL(req.url, 'http://localhost');
      const p = url.pathname;
      requests.push(req.method + ' ' + req.url);
      const json = (body, status = 200) => {
        res.writeHead(status, { 'content-type': 'application/json' });
        res.end(JSON.stringify(body));
      };
      const streamer =
        url.searchParams.get('streamer') ?? url.searchParams.get('streamer_login') ?? 'earlysalty';
      let input = {};
      if (req.method !== 'GET') {
        let body = '';
        for await (const chunk of req) body += chunk;
        try {
          input = JSON.parse(body);
        } catch {}
        writes.push({ p, streamer, input });
      }
      if (failTarget && req.method !== 'GET' && p.includes(failTarget))
        return json({ error: 'save_failed' }, 503);
      if (p === '/twitch/api/v2/auth-status')
        return json({
          authenticated: true,
          level: 'admin',
          authLevel: 'admin',
          isAdmin: true,
          adminEligible: true,
          adminMode: true,
          isLocalhost: false,
          twitchLogin: null,
          canViewAllStreamers: true,
          canAccessAnalyticsDashboard: true,
          plan: {
            planId: 'analysis_dashboard',
            planName: 'Admin',
            tier: 'extended',
            isExtended: true,
            entitlements: [],
          },
        });
      if (p === '/twitch/api/v2/streamers')
        return json([{ login: 'earlysalty' }, { login: 'partner2' }]);
      if (p === '/social-media/api/access/me')
        return json({ allowed: true, streamer: 'earlysalty', isAdmin: true });
      if (p === '/social-media/api/access')
        return json({ items: [{ streamer_login: 'earlysalty', granted: true }] });
      if (p.endsWith('/streamer-layout'))
        return json({
          streamer_login: streamer,
          layout,
          cam_enabled: true,
          mode: 'pip',
          is_default: false,
        });
      if (p === '/social-media/api/admin/clips') {
        if (failQueue) return json({ error: 'offline' }, 503);
        const page = Number(url.searchParams.get('page') || 1),
          size = Number(url.searchParams.get('page_size') || 100);
        return json({
          items: clips[streamer].slice((page - 1) * size, page * size),
          total: clips[streamer].length,
          page,
          page_size: size,
        });
      }
      if (p.includes('/settings/posting-plan')) {
        const plan = plans[streamer];
        if (req.method !== 'GET') {
          if (p.includes('/platform/'))
            Object.assign(
              plan.platforms.find((item) => item.platform === p.split('/').at(-1)),
              input,
            );
          else if (p.includes('/category/'))
            Object.assign(
              plan.categories.find((item) => item.category_key === p.split('/').at(-1)),
              input,
            );
          else Object.assign(plan, input);
          if (fremdAenderung) {
            fremdAenderung();
            fremdAenderung = null;
          }
        }
        return json(plan);
      }
      if (p.includes('vod-archive'))
        return json({
          streamer_login: streamer,
          enabled: false,
          privacy: 'private',
          privacy_options: ['private'],
          privacy_forced: true,
        });
      if (p.includes('/platforms/status'))
        return json({
          platforms: platforms.map((platform) => ({
            platform,
            connected: platform !== 'instagram',
            username: streamer,
            expired: false,
          })),
        });
      if (/\/approval\/\d+\/decision$/.test(p)) {
        const id = Number(p.split('/').at(-2));
        const clip = clips.earlysalty.find((c) => c.clip_db_id === id);
        clip.status = input.decision === 'approve' ? 'approved' : 'skipped';
        clip.approval.state = clip.status;
        clip.approval.approved_platforms = input.platforms;
        return json({ clip_db_id: id, clip, approval: clip.approval });
      }
      if (p.endsWith('/preview')) return json({ clip_db_id: 1, status: 'ready', ready: true });
      if (p.endsWith('/enrichment'))
        return json({
          clip_db_id: 1,
          transcript_raw: 'Bebop!',
          transcript_corrected: 'Bebop!',
          status: 'done',
          detected_terms: [],
          hashtags_youtube: [],
          hashtags_tiktok: [],
          hashtags_instagram: [],
          transcript_segments: [],
        });
      if (p.includes('/api/')) return json({ items: [], total: 0 });
      try {
        let file;
        if (p.startsWith('/brand/fonts/'))
          file = '/home/nathanael/repos/Website/dl-brand/fonts/' + path.basename(p);
        else if (p.startsWith('/twitch/dashboard-v2/'))
          file = path.join(dist, p.slice('/twitch/dashboard-v2/'.length));
        else file = path.join(dist, 'index.html');
        const body = await fs.readFile(file);
        const ext = path.extname(file);
        res.writeHead(200, {
          'content-type':
            {
              '.js': 'text/javascript',
              '.css': 'text/css',
              '.png': 'image/png',
              '.woff2': 'font/woff2',
              '.html': 'text/html',
            }[ext] ?? 'application/octet-stream',
        });
        res.end(body);
      } catch {
        res.writeHead(404);
        res.end();
      }
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const base = `http://127.0.0.1:${server.address().port}`;
    const browser = await chromium.launch({
      executablePath:
        process.env.STUDIO_BROWSER ??
        '/home/nathanael/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome',
      headless: true,
      args: ['--no-sandbox', '--disable-dev-shm-usage'],
    });
    t.after(async () => {
      await browser.close();
      await new Promise((resolve) => server.close(resolve));
    });
    const page = await browser.newPage({
      viewport: { width: 1440, height: 1080 },
      reducedMotion: 'reduce',
    });
    page.setDefaultTimeout(6000);
    t.after(async () => {
      await fs.mkdir(evidence, { recursive: true });
      await fs.writeFile(
        path.join(evidence, 'browser-state.json'),
        JSON.stringify({ writes, requests, errors }, null, 2),
      );
    });
    page.on('pageerror', (error) => errors.push(error.message));
    await page.route('**/*', (route) =>
      route.request().url().startsWith(base) ? route.continue() : route.abort(),
    );
    await page.goto(base + '/social-media-admin?streamer=earlysalty');
    await page.locator('.studio-clip').first().waitFor();
    const tab = (name) => page.getByRole('tab', { name, exact: true });
    await t.test(
      'vollständige Kennzahlen, keine Vorschau-Requests beim Laden und echtes Logo',
      async () => {
        assert.equal(await page.locator('.studio-clip').count(), 24);
        assert.ok(requests.some((r) => r.includes('page=2')));
        assert.ok(!requests.some((r) => r.includes('/preview')));
        assert.ok((await page.locator('.studio-metrics').innerText()).includes('2'));
        assert.ok(
          await page
            .locator('.studio-brand img')
            .evaluate((img) => img.complete && img.naturalWidth > 0),
        );
        assert.ok(await page.evaluate(() => document.fonts.check('14px "Studio Manrope"')));
      },
    );
    await t.test('Suche erreicht Seite 2 und Freigabe sendet Plattformen', async () => {
      await page.getByRole('searchbox').fill('Letzte Seite');
      assert.equal(await page.locator('.studio-clip').count(), 1);
      await page.getByRole('searchbox').fill('Bebop überlebt');
      await page
        .locator('.studio-clip')
        .getByRole('button', { name: 'Clip freigeben', exact: true })
        .click();
      await page.waitForFunction(
        () => !document.querySelector('.studio-clip button.studio-primary'),
      );
      assert.deepEqual(writes.find((w) => w.p.endsWith('/decision')).input.platforms, [
        'youtube',
        'tiktok',
      ]);
      await page.getByRole('searchbox').fill('');
    });
    await t.test('Dialog hat Fokus, Escape schließt, Layoutfehler bleibt sichtbar', async () => {
      await page.locator('.studio-clip').nth(1).getByLabel('Weitere Aktionen').click();
      await page.getByRole('button', { name: 'Layout anpassen', exact: true }).click();
      await page.getByRole('dialog').waitFor();
      for (let i = 0; i < 35; i++) await page.keyboard.press('Tab');
      assert.ok(await page.getByRole('dialog').evaluate((d) => d.contains(document.activeElement)));
      await page.getByRole('button', { name: 'Cam an', exact: true }).click();
      failTarget = '/layout';
      await page.getByRole('button', { name: 'Override speichern', exact: true }).click();
      await page.getByRole('dialog').getByRole('alert').waitFor();
      assert.equal(await page.getByRole('dialog').count(), 1);
      failTarget = '';
      await page.keyboard.press('Escape');
      assert.equal(await page.getByRole('dialog').count(), 0);
      assert.ok(await page.evaluate(() => document.activeElement?.tagName === 'SUMMARY'));
    });
    await t.test(
      'Zeitplan sendet vor Speichern nichts und behält Entwurf bei Teilfehler',
      async () => {
        await tab('Auto-Pilot & Zeitplan').click();
        const count = writes.length;
        await page.getByLabel('Posts pro Woche', { exact: true }).nth(0).fill('4');
        await page.getByLabel('Posts pro Woche', { exact: true }).nth(1).fill('5');
        await page.getByRole('heading', { name: 'Dein Posting-Rhythmus' }).click();
        assert.equal(writes.length, count);
        failTarget = '/platform/tiktok';
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText(/Teilweise gespeichert/).waitFor();
        assert.equal(
          await page.getByLabel('Posts pro Woche', { exact: true }).nth(1).inputValue(),
          '5',
        );
        assert.equal(plans.earlysalty.platforms[0].posts_per_week, 4);
        assert.equal(plans.earlysalty.platforms[1].posts_per_week, 3);
        failTarget = '';
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText('Änderungen gespeichert.', { exact: true }).waitFor();
        assert.equal(plans.earlysalty.platforms[1].posts_per_week, 5);
      },
    );
    await t.test('Enter übernimmt auch das gerade bearbeitete Zeitplanfeld', async () => {
      const field = page.getByLabel('Höchstens pro Tag', { exact: true }).first();
      await field.fill('2');
      await field.press('Enter');
      await page.getByText('Änderungen gespeichert.', { exact: true }).waitFor();
      assert.equal(plans.earlysalty.platforms[0].max_posts_per_day, 2);
    });
    await t.test('ungültige Zeiten und Zahlen lösen keinen Schreibaufruf aus', async () => {
      const count = writes.length;
      await page.getByLabel('Posts pro Woche', { exact: true }).first().fill('999');
      await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
      assert.equal(writes.length, count);
      await page.getByRole('button', { name: 'Verwerfen', exact: true }).click();
      await page.getByLabel('Uhrzeiten, mit Komma getrennt', { exact: true }).first().fill('25:99');
      await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
      assert.equal(writes.length, count);
      await page.getByRole('button', { name: 'Verwerfen', exact: true }).click();
    });
    await t.test('Fremdänderung zwischen Laden und Speichern wird nicht zurückgeschrieben', async () => {
      await page.getByLabel('Höchstens pro Tag', { exact: true }).nth(1).fill('2');
      fremdAenderung = () => {
        plans.earlysalty.platforms[0].max_posts_per_day = 3;
      };
      const vor = writes.length;
      await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
      await page.getByText('Änderungen gespeichert.', { exact: true }).waitFor();
      const zielSchreibungen = writes
        .slice(vor)
        .filter((w) => /\/settings\/posting-plan\/platform\//.test(w.p))
        .map((w) => w.p.split('/').at(-1));
      assert.deepEqual(zielSchreibungen, ['tiktok']);
      assert.equal(plans.earlysalty.platforms[0].max_posts_per_day, 3);
      assert.equal(plans.earlysalty.platforms[1].max_posts_per_day, 2);
    });
    await t.test(
      'Retry nach fehlgeschlagenem Schreiben lässt fremde Felder unberührt',
      async () => {
        await page.getByLabel('Posts pro Woche', { exact: true }).nth(1).fill('6');
        // Fremder Akteur ändert ein Feld, das der Entwurf nicht berührt, bevor
        // der Fehlerpfad neu liest.
        plans.earlysalty.platforms[0].max_posts_per_day = 7;
        failTarget = '/platform/tiktok';
        const vor = writes.length;
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText(/Speichern fehlgeschlagen/).waitFor();
        failTarget = '';
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText('Änderungen gespeichert.', { exact: true }).waitFor();
        const zielSchreibungen = writes
          .slice(vor)
          .filter((w) => /\/settings\/posting-plan\/platform\//.test(w.p))
          .map((w) => w.p.split('/').at(-1));
        assert.deepEqual(zielSchreibungen, ['tiktok', 'tiktok']);
        assert.deepEqual(writes.findLast((w) => w.p.endsWith('/platform/tiktok')).input, {
          posts_per_week: 6,
        });
        assert.equal(plans.earlysalty.platforms[0].max_posts_per_day, 7);
        assert.equal(plans.earlysalty.platforms[1].posts_per_week, 6);
      },
    );
    await t.test(
      'Abschalten der Vollautomatik wird vor den Zielen wirksam und übersteht Teilfehler',
      async () => {
        plans.earlysalty.approval_mode = 'full_auto';
        await page.reload();
        await tab('Auto-Pilot & Zeitplan').click();
        await page.getByRole('button', { name: /Vollautomatik/ }).waitFor();
        await page.getByRole('button', { name: /Nur nach Freigabe/ }).click();
        await page.getByLabel('Höchstens pro Tag', { exact: true }).nth(0).fill('4');
        failTarget = '/platform/youtube';
        const vor = writes.length;
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText(/Teilweise gespeichert/).waitFor();
        const neu = writes.slice(vor);
        const einstellungen = neu.findIndex((w) => w.p.endsWith('/settings/posting-plan'));
        const ziel = neu.findIndex((w) => w.p.endsWith('/platform/youtube'));
        assert.ok(einstellungen !== -1 && ziel !== -1 && einstellungen < ziel);
        assert.equal(neu[einstellungen].input.approval_mode, 'manual');
        assert.equal(plans.earlysalty.approval_mode, 'manual');
        failTarget = '';
        await page.getByRole('button', { name: 'Änderungen speichern', exact: true }).click();
        await page.getByText('Änderungen gespeichert.', { exact: true }).waitFor();
        assert.equal(plans.earlysalty.platforms[0].max_posts_per_day, 4);
        assert.equal(plans.earlysalty.approval_mode, 'manual');
      },
    );
    await t.test('Template-Auswahl lässt sich ohne zusätzliche Änderung speichern', async () => {
      await tab('Templates & Layouts').click();
      await page.getByRole('button', { name: 'Gameplay mit Hintergrund Layout anpassen' }).click();
      await page.getByRole('dialog').getByRole('button', { name: 'Layout für earlysalty speichern', exact: true }).click();
      await page.getByRole('dialog').waitFor({ state: 'detached' });
      const payload = writes.findLast(write => write.p.endsWith('/streamer-layout')).input;
      assert.equal(payload.layout.mode, 'blur_pad');
      assert.equal(payload.layout.cam_enabled, false);
      await tab('Auto-Pilot & Zeitplan').click();
    });
    await t.test(
      'Kanalwechsel verwirft nach Bestätigung alten Entwurf und Editorzustand',
      async () => {
        await page.getByLabel('Posts pro Woche', { exact: true }).first().fill('8');
        await page.getByRole('heading', { name: 'Dein Posting-Rhythmus' }).click();
        page.once('dialog', (d) => d.accept());
        await page.getByLabel('Streamer wählen', { exact: true }).selectOption('partner2');
        await tab('Auto-Pilot & Zeitplan').click();
        assert.equal(
          await page.getByLabel('Posts pro Woche', { exact: true }).first().inputValue(),
          '3',
        );
        assert.equal(plans.earlysalty.platforms[0].posts_per_week, 4);
      },
    );
    await t.test(
      'Bereiche bleiben bei 320, 390, 768 und 1440 Pixel ohne Seitenüberlauf',
      async () => {
        await fs.mkdir(evidence, { recursive: true });
        for (const width of [320, 390, 768, 1440]) {
          await page.setViewportSize({ width, height: 1080 });
          for (const name of [
            'Pipeline',
            'Auto-Pilot & Zeitplan',
            'Templates & Layouts',
            'Konten & Einstellungen',
          ]) {
            await tab(name).click();
            await page.waitForTimeout(100);
            const size = await page.evaluate(() => ({
              width: document.documentElement.clientWidth,
              doc: document.documentElement.scrollWidth,
            }));
            assert.ok(size.doc <= size.width, `${name} @ ${width}: ${JSON.stringify(size)}`);
          }
          await tab('Pipeline').click();
          await page.screenshot({ path: path.join(evidence, `studio-${width}.png`) });
        }
        await tab('Auto-Pilot & Zeitplan').click();
        await page.screenshot({ path: path.join(evidence, 'studio-autopilot.png') });
      },
    );
    await t.test('Fehlgeschlagene Pipeline wird nicht als leerer Bestand angezeigt', async () => {
      failQueue = true;
      await page.reload();
      await page
        .getByText('Die Pipeline konnte nicht vollständig geladen werden.', { exact: true })
        .waitFor({ timeout: 15000 });
      assert.equal(
        await page.getByText('Keine Clips für diesen Filter', { exact: true }).count(),
        0,
      );
    });
    assert.deepEqual(errors, []);
  },
);
