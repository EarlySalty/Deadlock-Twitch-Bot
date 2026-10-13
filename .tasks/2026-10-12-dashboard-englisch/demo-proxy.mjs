import http from 'node:http';

const port = Number(process.argv[2] ?? 8765);
const origin = 'https://deutsche-deadlock-community.de';

http.createServer(async (req, res) => {
  if (req.method !== 'GET') {
    res.writeHead(405).end();
    return;
  }
  const target = req.url.startsWith('/twitch/api/v2') ? req.url.replace('/twitch/api/v2', '/twitch/demo/api/v2') : req.url;
  if (!target.startsWith('/twitch/demo/api/v2')) {
    res.writeHead(404).end();
    return;
  }
  try {
    const headers = { accept: req.headers.accept ?? 'application/json' };
    if (req.headers['x-dashboard-language']) headers['x-dashboard-language'] = req.headers['x-dashboard-language'];
    const upstream = await fetch(`${origin}${target}`, { headers });
    const body = Buffer.from(await upstream.arrayBuffer());
    res.writeHead(upstream.status, { 'content-type': upstream.headers.get('content-type') ?? 'application/json' });
    res.end(body);
  } catch {
    res.writeHead(502).end();
  }
}).listen(port, '127.0.0.1');
