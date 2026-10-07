import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const root = '/home/nathanael/.worktrees/tb-vod-archiv-status-20261007';
const dist = path.join(root, 'bot/analytics/dashboard_v2/dist');
const fixturePath = path.join(root, '.tasks/2026-10-07-vod-archiv-status/pruefung/api-fixture.json');
const auth = { authenticated: true, level: 'admin', demoMode: false, isAdmin: true, isLocalhost: false, canViewAllStreamers: true, twitchUserId: '42', twitchLogin: 'testkanal', displayName: 'Testkanal', permissions: { viewAllStreamers: true, viewComparison: true, viewChatAnalytics: true, viewOverlap: true }, access: { landing: true, analytics: true }, plan: null };
const server = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://127.0.0.1:4197');
  if (request.method !== 'GET') {
    response.writeHead(405).end('Read-only fixture');
    return;
  }
  const json = (value) => { response.writeHead(200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(value)); };
  if (url.pathname.endsWith('/vod-archive') && !url.pathname.includes('/settings/')) {
    if (!fs.existsSync(fixturePath)) { response.writeHead(503).end('API fixture not ready'); return; }
    json(JSON.parse(fs.readFileSync(fixturePath, 'utf8')));
    return;
  }
  if (url.pathname.endsWith('/auth-status')) { json(auth); return; }
  if (url.pathname.endsWith('/streamers')) { json([{ login: 'testkanal', twitchUserId: '42', displayName: 'Testkanal' }]); return; }
  if (url.pathname.endsWith('/access/me')) { json({ allowed: true, streamer: 'testkanal', isAdmin: true }); return; }
  if (url.pathname.endsWith('/access')) { json([{ twitch_user_id: '42', streamer_login: 'testkanal', granted: true }]); return; }
  if (url.pathname.endsWith('/settings/vod-archive')) { json({ enabled: true, privacy: 'unlisted' }); return; }
  if (url.pathname.endsWith('/platforms/status')) { json({ platforms: [] }); return; }
  if (url.pathname.endsWith('/settings/posting-plan')) { json({ platforms: [], categories: [], approval_mode: 'manual', timezone: 'Europe/Berlin', subtitles_enabled: false }); return; }
  if (url.pathname.endsWith('/clips')) { json({ items: [], total: 0, page: 1, page_size: 50 }); return; }
  if (url.pathname.includes('/api/')) { json({}); return; }
  const asset = url.pathname.startsWith('/twitch/dashboard-v2/assets/') ? path.basename(url.pathname) : null;
  const file = asset ? path.join(dist, 'assets', asset) : path.join(dist, 'index.html');
  if (!fs.existsSync(file)) { response.writeHead(404).end(); return; }
  response.writeHead(200, { 'Content-Type': asset?.endsWith('.js') ? 'application/javascript' : asset?.endsWith('.css') ? 'text/css' : 'text/html' });
  fs.createReadStream(file).pipe(response);
});
server.listen(4197, '127.0.0.1', () => console.log('Isolated read-only fixture, production bundle, port 4197'));
