import fs from 'node:fs';

const pid = process.argv[2];
if (!/^[1-9]\d*$/.test(pid ?? '')) throw new Error('Service PID required');
const name = 'TWITCH_INTERNAL_API_TOKEN=';
const credential = fs.readFileSync(`/proc/${pid}/environ`, 'utf8').split('\0')
  .find((entry) => entry.startsWith(name))?.slice(name.length);
if (!credential) throw new Error('Existing internal authentication unavailable');
const response = await fetch('http://127.0.0.1:8769/social-media/api/admin/clips/124589/preview', {
  method: 'POST',
  headers: { 'x-internal-token': credential },
  redirect: 'manual',
});
const type = response.headers.get('content-type');
const body = type?.includes('application/json') ? await response.json() : null;
console.log(JSON.stringify({ status: response.status, type, body }));
if (response.status !== 200 || body?.status !== 'pending') process.exitCode = 1;
