import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import crypto from 'node:crypto';

const root = path.resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const dist = path.join(root, 'bot/admin_dashboard/dist');
const handlers = path.join(root, 'rust/crates/tb-dashboard-api/src/handlers');
const port = Number(process.env.P2_MOCK_PORT || 4198);
const csrf = 'p2-csrf-token';

const mime = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.json': 'application/json; charset=utf-8',
};

const overlayPages = {
  '/twitch/caster-overlay': 'caster_overlay.html',
  '/twitch/caster-overlay/scoreboard': 'caster_scoreboard.html',
  '/twitch/caster-overlay/versus': 'caster_versus.html',
  '/twitch/caster-overlay/countdown': 'caster_countdown.html',
};

const builtins = {
  'dachlock.png': path.join(handlers, 'caster_assets/dachlock.png'),
  'wochenende.webp': path.join(handlers, 'caster_assets/wochenende.webp'),
  'community.webp': path.join(handlers, 'caster_assets/community.webp'),
  'fun-turnier.webp': path.join(handlers, 'caster_assets/fun_turnier.webp'),
};

const emptyScene = () => ({
  roster: [],
  slots: [null, null],
  layout: 'duo',
  matchContext: {},
  event: { logo: { kind: 'builtin', id: 'wochenende' }, showBrand: true },
  countdown: { title: 'STREAM STARTET IN', target: null },
  sidesSwapped: false,
});

const state = {
  revision: 1,
  scene: emptyScene(),
  library: [],
  assets: [],
};

function send(res, status, body, headers = {}) {
  const payload = typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body);
  res.writeHead(status, { 'cache-control': 'no-store', ...headers });
  res.end(payload);
}

function json(res, body, status = 200) {
  send(res, status, body, { 'content-type': 'application/json; charset=utf-8' });
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

function publicTeam(team) {
  if (!team) return null;
  return {
    name: team.name,
    short: team.short ?? null,
    logoUrl: team.logoAssetId ? `/twitch/caster-overlay/assets/${team.logoAssetId}` : null,
    players: team.players ?? [],
    coach: team.coach?.name || null,
  };
}

function publicOverlay() {
  const scene = state.scene;
  const teamA = publicTeam(scene.matchContext.teamA);
  const teamB = publicTeam(scene.matchContext.teamB);
  return {
    revision: state.revision,
    layout: scene.layout,
    slots: scene.slots,
    matchContext: scene.matchContext,
    teams: scene.sidesSwapped ? { left: teamB, right: teamA } : { left: teamA, right: teamB },
    sidesSwapped: scene.sidesSwapped,
    event: {
      logoUrl: '/twitch/caster-overlay/builtin/wochenende.webp',
      showBrand: scene.event?.showBrand !== false,
      brandUrl: '/twitch/caster-overlay/builtin/dachlock.png',
    },
    countdown: scene.countdown,
  };
}

function serveFile(res, file, type) {
  if (!fs.existsSync(file)) {
    send(res, 404, 'not found');
    return;
  }
  send(res, 200, fs.readFileSync(file), { 'content-type': type || mime[path.extname(file)] || 'application/octet-stream' });
}

function serveDist(res, urlPath) {
  const rel = urlPath.replace(/^\/twitch\/admin\/?/, '') || 'index.html';
  const file = path.join(dist, rel);
  if (file.startsWith(dist) && fs.existsSync(file) && fs.statSync(file).isFile()) {
    serveFile(res, file);
    return;
  }
  serveFile(res, path.join(dist, 'index.html'), 'text/html; charset=utf-8');
}

async function handleApi(req, res, url) {
  const pathname = url.pathname;
  if (pathname === '/twitch/api/v2/auth-status') {
    json(res, {
      authenticated: true,
      isAdmin: true,
      isLocalhost: true,
      authLevel: 'admin',
      csrfToken: csrf,
      displayName: 'Admin',
      twitchLogin: 'admin',
      twitchUserId: '1',
    });
    return;
  }
  if (pathname === '/twitch/api/v2/public/caster-overlay') {
    json(res, publicOverlay());
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay' && req.method === 'GET') {
    json(res, { revision: state.revision, scene: state.scene });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay' && req.method === 'POST') {
    const body = JSON.parse((await readBody(req)).toString('utf8') || '{}');
    state.scene = body.scene ?? state.scene;
    state.revision += 1;
    json(res, { revision: state.revision, scene: state.scene });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/context') {
    json(res, { available: false, reason: 'kein Turnier-abo', teams: [] });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/casters') {
    json(res, { available: true, complete: true, items: [] });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-cameras') {
    json(res, { items: [] });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/coach-members') {
    const q = (url.searchParams.get('q') || '').trim().toLowerCase();
    const items = q.length < 2
      ? []
      : [{ discordId: '426770757989957633', displayName: 'Coach Max', username: 'coachmax' }]
        .filter((item) => item.displayName.toLowerCase().includes(q) || item.username.includes(q));
    json(res, { available: true, items });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/teams' && req.method === 'GET') {
    json(res, { items: state.library });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/teams' && req.method === 'POST') {
    const team = JSON.parse((await readBody(req)).toString('utf8') || '{}');
    team.libraryId = crypto.randomUUID();
    state.library.push(team);
    json(res, { team });
    return;
  }
  const teamMatch = pathname.match(/^\/twitch\/api\/admin\/caster-overlay\/teams\/([^/]+)$/);
  if (teamMatch && req.method === 'PUT') {
    const team = JSON.parse((await readBody(req)).toString('utf8') || '{}');
    team.libraryId = teamMatch[1];
    state.library = state.library.map((item) => (item.libraryId === team.libraryId ? team : item));
    json(res, { team });
    return;
  }
  if (teamMatch && req.method === 'DELETE') {
    state.library = state.library.filter((item) => item.libraryId !== teamMatch[1]);
    json(res, { deleted: teamMatch[1] });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/assets' && req.method === 'GET') {
    const kind = url.searchParams.get('kind');
    json(res, { assets: state.assets.filter((item) => !kind || item.kind === kind) });
    return;
  }
  if (pathname === '/twitch/api/admin/caster-overlay/assets' && req.method === 'POST') {
    const body = await readBody(req);
    const id = crypto.randomUUID();
    const kind = url.searchParams.get('kind') || 'team_logo';
    const name = url.searchParams.get('name') || 'Logo';
    const asset = {
      id,
      kind,
      name,
      mime: req.headers['content-type'] || 'image/png',
      size: body.length,
      createdAt: new Date().toISOString(),
      url: `/twitch/caster-overlay/assets/${id}`,
      bytes: body,
    };
    state.assets.unshift(asset);
    json(res, { ...asset, bytes: undefined });
    return;
  }
  if (pathname.startsWith('/twitch/api/admin/')) {
    json(res, { items: [], streamers: [] });
    return;
  }
  send(res, 404, 'not found');
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url || '/', `http://127.0.0.1:${port}`);
    const pathname = url.pathname;
    if (pathname.startsWith('/twitch/api/')) {
      process.stdout.write(`${req.method} ${pathname}\n`);
      await handleApi(req, res, url);
      return;
    }
    if (overlayPages[pathname]) {
      serveFile(res, path.join(handlers, overlayPages[pathname]), 'text/html; charset=utf-8');
      return;
    }
    if (pathname === '/twitch/caster-overlay/background.png') {
      serveFile(res, path.join(handlers, 'caster_background.png'), 'image/png');
      return;
    }
    const builtin = pathname.match(/^\/twitch\/caster-overlay\/builtin\/([^/]+)$/);
    if (builtin && builtins[builtin[1]]) {
      serveFile(res, builtins[builtin[1]]);
      return;
    }
    const asset = pathname.match(/^\/twitch\/caster-overlay\/assets\/([^/]+)$/);
    if (asset) {
      const found = state.assets.find((item) => item.id === asset[1]);
      if (!found) {
        send(res, 404, 'not found');
        return;
      }
      send(res, 200, found.bytes, { 'content-type': found.mime });
      return;
    }
    if (pathname.startsWith('/twitch/admin')) {
      serveDist(res, pathname);
      return;
    }
    send(res, 404, 'not found');
  } catch (error) {
    send(res, 500, String(error));
  }
});

server.listen(port, '127.0.0.1', () => {
  process.stdout.write(`p2-mock ${port}\n`);
});
