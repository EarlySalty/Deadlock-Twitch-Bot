import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const DIST = path.join(ROOT, 'dist');

async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  return server.address().port;
}

async function waitFor(check) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.fail('Browser condition did not settle');
}

function guild(guildId) {
  return {
    guildId, name: `Fixture server ${guildId}`, language: 'de',
    announceChannelId: '123456789012345678', alertChannelId: null,
    streamerRoleId: null, pingRoleId: null, promoInviteUrl: null,
    autoPartnerDefault: true, crossServerRaids: true,
    activeStreamers: 1234, inactiveStreamers: 0,
  };
}

test('Server-admin drafts survive translated refresh failures and reconcile acknowledged settings', async (t) => {
  const evidence = await mkdtemp(path.join(os.tmpdir(), 'ttb-sol-ui-r1-'));
  let loggedIn = false;
  let meRequests = 0;
  let meFailure = false;
  let deferPatch = false;
  let releasePatch;
  const missingAssets = new Set();
  const unexpectedApis = [];
  const guilds = [guild('111111111111111111'), guild('222222222222222222')];
  const patches = [];
  const server = http.createServer(async (req, res) => {
    try {
      const pathname = new URL(req.url, 'http://127.0.0.1').pathname;
      let payload;
      let status = 200;
      if (pathname === '/twitch/api/server-admin/me') {
        meRequests += 1;
        status = meFailure ? 503 : loggedIn ? 200 : 401;
        payload = meFailure
          ? { error: 'fixture_transient_failure' }
          : loggedIn
            ? { principal: 'guild_admin', displayName: 'Fixture administrator', csrfToken: 'fixture', guilds, permittedAreas: [] }
            : { error: 'server_admin_login_required' };
      } else if (pathname.endsWith('/settings') && req.method === 'PATCH') {
        let body = '';
        for await (const chunk of req) body += chunk;
        const patch = JSON.parse(body);
        const target = guilds.find((item) => pathname.includes(item.guildId));
        assert.ok(target);
        patches.push({ guildId: target.guildId, patch });
        if (deferPatch) await new Promise((resolve) => { releasePatch = resolve; });
        Object.assign(target, patch);
        if (typeof target.promoInviteUrl === 'string' && target.promoInviteUrl.startsWith('discord.gg/')) {
          target.promoInviteUrl = `https://${target.promoInviteUrl}`;
        }
        payload = target;
      } else if (pathname.startsWith('/twitch/api/server-admin/') && pathname.endsWith('/streamers')) {
        payload = { streamers: [{ twitchUserId: '333333', twitchLogin: 'fixture_streamer', active: true, source: 'fixture', joinedAt: '2026-10-12T12:00:00Z', deactivatedAt: null }] };
      } else if (pathname.startsWith('/twitch/api/')) {
        unexpectedApis.push(pathname);
        status = 404;
        payload = { error: 'fixture_unexpected_api' };
      }
      if (payload) {
        res.writeHead(status, { 'content-type': 'application/json' });
        res.end(JSON.stringify(payload));
        return;
      }
      const relative = pathname.startsWith('/twitch/dashboard-v2/')
        ? pathname.slice('/twitch/dashboard-v2/'.length)
        : pathname === '/twitch/server-admin' ? 'index.html' : pathname.slice(1);
      const file = path.resolve(DIST, relative);
      if (!file.startsWith(`${DIST}${path.sep}`)) throw new Error('Invalid fixture path');
      const data = await readFile(file);
      const contentType = file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : file.endsWith('.html') ? 'text/html' : 'application/octet-stream';
      res.writeHead(200, { 'content-type': contentType });
      res.end(data);
    } catch {
      if (req.url.endsWith('.woff2')) missingAssets.add(req.url);
      res.writeHead(404);
      res.end();
    }
  });
  const port = await listen(server);
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const reservation = net.createServer();
  const driverPort = await listen(reservation);
  await new Promise((resolve) => reservation.close(resolve));
  const driver = spawn('/home/nathanael/.local/bin/moli', ['serve', '--host', '127.0.0.1', '--port', String(driverPort), '--layout', '--image', '--font', '--log-level', 'error'], { stdio: 'inherit' });
  let sessionRoute;
  t.after(async () => {
    try {
      if (releasePatch) releasePatch();
      if (sessionRoute) await request(sessionRoute, undefined, 'DELETE');
    } finally {
      driver.kill('SIGTERM');
    }
  });
  const base = `http://127.0.0.1:${driverPort}`;
  await waitFor(async () => fetch(`${base}/status`).then((response) => response.ok).catch(() => false));
  async function request(route, body, method) {
    const response = await fetch(`${base}${route}`, {
      method: method ?? (body === undefined ? 'GET' : 'POST'),
      headers: body === undefined ? undefined : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const result = await response.json();
    assert.ok(response.ok && !result.value?.error, JSON.stringify(result));
    return result.value;
  }
  const session = await request('/session', { capabilities: { alwaysMatch: {} } });
  const route = `/session/${session.sessionId}`;
  sessionRoute = route;
  const execute = (script) => request(`${route}/execute/sync`, { script, args: [] });
  const switchLanguage = async (language) => {
    const before = meRequests;
    assert.equal(await execute(`document.querySelector('button[lang="${language}"]').click(); return true;`), true);
    await waitFor(() => execute(`return document.documentElement.lang === '${language}'`));
    await waitFor(() => Promise.resolve(meRequests > before));
  };
  const values = () => execute(`return { selected: document.querySelector('#server-admin-guild').value, channel: document.querySelector('#channels input').value, alert: document.querySelectorAll('#channels input')[1].value, language: document.querySelector('#language select').value, stored: localStorage.getItem('dashboard.language') };`);
  const settings = () => execute(`return { invite: document.querySelector('#language input:not([type=checkbox])').value, partner: document.querySelector('#language input[type=checkbox]').checked, raids: document.querySelector('#raids input').checked };`);
  const editInput = (selector, value) => execute(`const input = document.querySelector(${JSON.stringify(selector)}); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(value)}); input.dispatchEvent(new Event('input', { bubbles: true })); return true;`);
  const saved = (section) => execute(`return { disabled: document.querySelector('#${section} button').disabled, status: !!document.querySelector('#${section} [role=status]') };`);
  const snapshot = async (state, language, width, section) => {
    await request(`${route}/window/rect`, { width, height: 1000 });
    await new Promise((resolve) => setTimeout(resolve, 800));
    await request(`${route}/screenshot`);
    await execute(section ? `window.scrollTo(0, document.querySelector('#${section}').getBoundingClientRect().top + window.scrollY); return true;` : 'window.scrollTo(0, 0); return true;');
    await new Promise((resolve) => setTimeout(resolve, 200));
    const png = await request(`${route}/screenshot`);
    const geometry = await execute(`return { viewport: innerWidth, document: document.documentElement.scrollWidth, height: innerHeight, targetTop: ${section ? `document.querySelector('#${section}').getBoundingClientRect().top` : '0'} };`);
    assert.equal(geometry.viewport, width);
    assert.ok(geometry.document <= width, JSON.stringify(geometry));
    assert.ok(geometry.targetTop >= -1 && geometry.targetTop < geometry.height / 2, JSON.stringify(geometry));
    const file = path.join(evidence, `${state}-${language}-${width}.png`);
    await writeFile(file, Buffer.from(png, 'base64'));
    console.log(`SCREENSHOT ${file}`);
  };
  await request(`${route}/url`, { url: `http://127.0.0.1:${port}/twitch/server-admin` });
  await waitFor(() => execute('return !!document.querySelector("button[lang=de]") && !!document.querySelector(\'a[href$="auth/login"]\')'));
  await execute('window.__fixtureErrors = []; window.addEventListener("error", event => window.__fixtureErrors.push(String(event.message))); return true;');
  for (const language of ['en', 'de']) {
    await switchLanguage(language);
    for (const width of [390, 1440]) await snapshot('login', language, width);
  }
  assert.equal(patches.length, 0);
  loggedIn = true;
  meFailure = true;
  await switchLanguage('en');
  await waitFor(() => execute('return !!document.querySelector("[role=alert]") && !document.querySelector("#server-admin-guild")'));
  meFailure = false;
  await execute('document.querySelector("section button:not([lang])").click(); return true;');
  await waitFor(() => execute('return !!document.querySelector("#server-admin-guild")'));
  await execute(`const select = document.querySelector('#server-admin-guild'); select.value = '${guilds[1].guildId}'; select.dispatchEvent(new Event('change', { bubbles: true })); return true;`);
  await waitFor(() => execute(`return document.querySelector('h1').textContent.includes('${guilds[1].guildId}')`));
  await execute(`const input = document.querySelector('#channels input'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, '987654321098765432'); input.dispatchEvent(new Event('input', { bubbles: true })); const select = document.querySelector('#language select'); select.value = 'en'; select.dispatchEvent(new Event('change', { bubbles: true })); return true;`);
  await waitFor(async () => (await values()).channel === '987654321098765432');
  guilds[1].alertChannelId = '555555555555555555';
  await switchLanguage('de');
  await waitFor(async () => (await values()).alert === guilds[1].alertChannelId);
  assert.deepEqual(await values(), { selected: guilds[1].guildId, channel: '987654321098765432', alert: guilds[1].alertChannelId, language: 'en', stored: 'de' });
  assert.equal(guilds[1].language, 'de');
  assert.equal(patches.length, 0);
  for (const language of ['en', 'de']) {
    await switchLanguage(language);
    assert.equal((await values()).channel, '987654321098765432');
    assert.equal((await values()).language, 'en');
    for (const width of [390, 1440]) {
      await snapshot('selected-draft', language, width);
      await snapshot('settings-draft', language, width, 'channels');
      if (width === 390) await snapshot('settings-bottom', language, width, 'language');
    }
  }
  await execute('document.querySelector("#raids input").click(); return true;');
  await waitFor(async () => (await settings()).raids === false);
  meFailure = true;
  for (const language of ['en', 'de']) {
    await switchLanguage(language);
    await waitFor(() => execute('return !!document.querySelector("main > [role=alert]") && !document.querySelector("#overview button").disabled'));
    assert.deepEqual(await values(), { selected: guilds[1].guildId, channel: '987654321098765432', alert: guilds[1].alertChannelId, language: 'en', stored: language });
    assert.equal((await settings()).raids, false);
    assert.equal(patches.length, 0);
    for (const width of [390, 1440]) {
      await snapshot('transient-failure', language, width);
      await snapshot('retained-drafts', language, width, 'channels');
    }
  }
  meFailure = false;
  await execute('document.querySelector("#overview button").click(); return true;');
  await waitFor(() => execute('return !document.querySelector("main > [role=alert]") && !document.querySelector("#overview button").disabled'));
  assert.equal((await values()).language, 'en');
  assert.equal((await settings()).raids, false);
  await execute('document.querySelector("#channels button").click(); return true;');
  await waitFor(() => Promise.resolve(patches.length === 1));
  assert.deepEqual(patches[0], { guildId: guilds[1].guildId, patch: { announceChannelId: '987654321098765432' } });
  await waitFor(() => execute('return document.querySelector("#channels button").disabled'));
  assert.equal((await values()).language, 'en');
  await execute('document.querySelector("#language button").click(); return true;');
  await waitFor(() => Promise.resolve(patches.length === 2));
  assert.deepEqual(patches[1], { guildId: guilds[1].guildId, patch: { language: 'en' } });
  await waitFor(async () => (await saved('language')).status);
  await editInput('#language input:not([type=checkbox])', 'discord.gg/abcd');
  await execute('document.querySelector("#language button").click(); return true;');
  await waitFor(async () => (await saved('language')).status);
  assert.deepEqual(patches[2], { guildId: guilds[1].guildId, patch: { promoInviteUrl: 'discord.gg/abcd' } });
  assert.equal((await settings()).invite, 'https://discord.gg/abcd');
  for (const language of ['en', 'de']) {
    await switchLanguage(language);
    assert.deepEqual(await saved('language'), { disabled: true, status: true });
    assert.equal((await settings()).invite, 'https://discord.gg/abcd');
    for (const width of [390, 1440]) await snapshot('normalized-saved', language, width, 'language');
  }
  deferPatch = true;
  await editInput('#language input:not([type=checkbox])', 'discord.gg/efgh');
  await execute('document.querySelector("#language input[type=checkbox]").click(); document.querySelector("#language button").click(); return true;');
  await waitFor(() => Promise.resolve(!!releasePatch));
  assert.deepEqual(patches[3], { guildId: guilds[1].guildId, patch: { promoInviteUrl: 'discord.gg/efgh', autoPartnerDefault: false } });
  await editInput('#language input:not([type=checkbox])', 'https://discord.gg/abcd');
  await execute('document.querySelector("#language input[type=checkbox]").click(); const select = document.querySelector("#language select"); select.value = "de"; select.dispatchEvent(new Event("change", { bubbles: true })); return true;');
  assert.deepEqual(await saved('language'), { disabled: true, status: false });
  deferPatch = false;
  releasePatch();
  releasePatch = undefined;
  await waitFor(async () => !(await saved('language')).disabled);
  assert.deepEqual(await settings(), { invite: 'https://discord.gg/abcd', partner: true, raids: false });
  assert.equal((await values()).language, 'de');
  assert.equal(guilds[1].promoInviteUrl, 'https://discord.gg/efgh');
  assert.equal(guilds[1].autoPartnerDefault, false);
  assert.equal(guilds[1].language, 'en');
  for (const language of ['en', 'de']) {
    await switchLanguage(language);
    assert.deepEqual(await saved('language'), { disabled: false, status: false });
    assert.deepEqual(await settings(), { invite: 'https://discord.gg/abcd', partner: true, raids: false });
    assert.equal((await values()).language, 'de');
    for (const width of [390, 1440]) await snapshot('concurrent-edit', language, width, 'language');
  }
  await execute('document.querySelector("#language button").click(); return true;');
  await waitFor(async () => (await saved('language')).status);
  assert.deepEqual(patches[4], { guildId: guilds[1].guildId, patch: { language: 'de', promoInviteUrl: 'https://discord.gg/abcd', autoPartnerDefault: true } });
  await execute('document.querySelector("#raids button").click(); return true;');
  await waitFor(async () => (await saved('raids')).status);
  assert.deepEqual(patches[5], { guildId: guilds[1].guildId, patch: { crossServerRaids: false } });
  assert.equal(guilds[0].language, 'de');
  await execute(`const select = document.querySelector('#server-admin-guild'); select.value = '${guilds[0].guildId}'; select.dispatchEvent(new Event('change', { bubbles: true })); return true;`);
  await waitFor(async () => (await values()).channel === guilds[0].announceChannelId);
  assert.equal((await values()).language, 'de');
  assert.deepEqual(await settings(), { invite: '', partner: true, raids: true });
  assert.deepEqual(await execute('return window.__fixtureErrors'), []);
  assert.deepEqual(unexpectedApis, []);
  assert.equal(patches.length, 6);
  assert.ok(patches.every((entry) => entry.guildId === guilds[1].guildId));
  console.log(`FONT_FALLBACK ${JSON.stringify([...missingAssets])}`);
  console.log(`EVIDENCE ${evidence}; me requests ${meRequests}; scoped PATCH requests ${patches.length}`);
});
