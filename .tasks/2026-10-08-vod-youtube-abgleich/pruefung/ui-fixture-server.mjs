import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const root = '/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008';
const dist = path.join(root, 'bot/analytics/dashboard_v2/dist');
const folder = path.join(root, '.tasks/2026-10-08-vod-youtube-abgleich/pruefung');
const auth = { authenticated: true, level: 'admin', demoMode: false, isAdmin: true, isLocalhost: false, canViewAllStreamers: true, twitchUserId: '42', twitchLogin: 'testkanal', displayName: 'Testkanal', permissions: { viewAllStreamers: true, viewComparison: true, viewChatAnalytics: true, viewOverlap: true }, access: { landing: true, analytics: true }, plan: null };
const server = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://127.0.0.1:4198');
  if (request.method !== 'GET') { response.writeHead(405).end('Read-only synthetic fixture'); return; }
  const json = (value) => { response.writeHead(200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(value)); };
  if (url.pathname.endsWith('/vod-archive') && !url.pathname.includes('/settings/')) {
    const historical = JSON.parse(fs.readFileSync(path.join(folder, 'api-fixture.json'), 'utf8'));
    const referer = new URL(request.headers.referer ?? '/', 'http://127.0.0.1:4198');
    const scenario = request.headers['x-archive-fixture'] ?? referer.searchParams.get('fixture') ?? 'current';
    const filenames = { current: 'youtube-current.json', partial: 'youtube-partial.json', connection: 'youtube-connection.json' };
    const current = JSON.parse(fs.readFileSync(path.join(folder, filenames[scenario] ?? filenames.current), 'utf8'));
    const items = [...current.items, ...historical.items];
    json({ ...historical, items, total: items.length }); return;
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
  if (/^\/brand\/fonts\/(manrope-latin|sora-latin)\.woff2$/.test(url.pathname)) {
    const font = path.join(root, 'website/public/fonts', path.basename(url.pathname));
    if (!fs.existsSync(font)) { response.writeHead(404).end(); return; }
    response.writeHead(200, { 'Content-Type': 'font/woff2' });
    fs.createReadStream(font).pipe(response); return;
  }
  const asset = url.pathname.startsWith('/twitch/dashboard-v2/assets/') ? path.basename(url.pathname) : null;
  const file = asset ? path.join(dist, 'assets', asset) : path.join(dist, 'index.html');
  if (!fs.existsSync(file)) { response.writeHead(404).end(); return; }
  response.writeHead(200, { 'Content-Type': asset?.endsWith('.js') ? 'application/javascript' : asset?.endsWith('.css') ? 'text/css' : 'text/html' });
  fs.createReadStream(file).pipe(response);
});
server.listen(4198, '127.0.0.1', () => console.log('Synthetic SQL handler fixtures with production assets on port 4198. No production login or provider writes.'));
