import fs from 'node:fs';

const pid = process.argv[2];
if (!/^[1-9]\d*$/.test(pid ?? '')) throw new Error('Service PID required');
const allowed = new Set([
  '/social-media/api/access/me',
  '/social-media/api/clips/124589/tiktok/creator-info',
  '/social-media/api/admin/clips/124589/preview',
]);
const paths = process.argv.slice(3);
if (!paths.length || paths.some((path) => !allowed.has(path))) throw new Error('Read-only path required');
const name = 'TWITCH_INTERNAL_API_TOKEN=';
const credential = fs.readFileSync(`/proc/${pid}/environ`, 'utf8').split('\0')
  .find((entry) => entry.startsWith(name))?.slice(name.length);
if (!credential) throw new Error('Existing internal authentication unavailable');
for (const path of paths) {
  const response = await fetch(`http://127.0.0.1:8769${path}`, {
    headers: { 'x-internal-token': credential },
    redirect: 'manual',
  });
  const type = response.headers.get('content-type');
  const body = type?.includes('application/json') ? await response.json() : null;
  console.log(JSON.stringify({ path, status: response.status, type, body }));
  if (response.status !== 200 || !body) process.exitCode = 1;
}
