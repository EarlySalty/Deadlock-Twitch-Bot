import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const root = '/home/nathanael/.worktrees/ttb-opus-r1-20261015/bot/admin_dashboard/dist';
let guilds = [{guild_id: '1', name: 'Deutsche Community', cross_server_raids: true}, {guild_id: '2', name: 'English Community', cross_server_raids: false}];
http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  if (url.pathname.startsWith('/twitch/api/')) {
    res.setHeader('Content-Type', 'application/json');
    if (url.pathname.endsWith('/auth-status')) return res.end(JSON.stringify({isAdmin: true, csrfToken: 'isolated-fixture', displayName: 'Test Admin'}));
    if (url.pathname.endsWith('/config/guild-raids')) {
      if (req.method === 'POST') {
        let raw = ''; for await (const chunk of req) raw += chunk;
        const body = JSON.parse(raw);
        if (req.headers['x-csrf-token'] !== 'isolated-fixture') {res.statusCode = 403; return res.end('{}');}
        guilds = guilds.map(g => g.guild_id === body.guild_id ? {...g, cross_server_raids: body.cross_server_raids} : g);
        return res.end(JSON.stringify(guilds.find(g => g.guild_id === body.guild_id)));
      }
      return res.end(JSON.stringify(guilds));
    }
    if (url.pathname.endsWith('/config/overview')) return res.end(JSON.stringify({raids: {totalManagedStreamers: 2, raidBotEnabledCount: 2, livePingEnabledCount: 2, allRaidBotEnabled: true, allLivePingEnabled: true}}));
    return res.end(JSON.stringify({streamers: []}));
  }
  const relative = url.pathname.replace(/^\/twitch\/admin\/?/, '');
  let file = path.join(root, relative);
  if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) file = path.join(root, 'index.html');
  res.setHeader('Content-Type', file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html');
  fs.createReadStream(file).pipe(res);
}).listen(19865, '127.0.0.1', () => console.log('Isolated fixture ready on 19865'));
