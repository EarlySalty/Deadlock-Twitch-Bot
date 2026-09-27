// Browser acceptance test against the built page and an explicitly synthetic API.
// Usage: node tests/clips-browser.mjs /path/to/playwright/index.mjs /path/to/Website/dl-brand [browser-executable]
import { createServer } from 'node:http'
import { readFile, mkdir } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import assert from 'node:assert/strict'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const dist = path.join(root, 'dist')
const brand = process.argv[3] && path.resolve(process.argv[3])
const output = path.resolve(root, '../docs/screenshots/clips')
const { chromium } = await import(process.argv[2] ? pathToFileURL(path.resolve(process.argv[2])).href : 'playwright')
let mode = 'voting'
let admin = false
let loggedIn = true
let votesUsed = 1
let submitted = 0
let tooYoung = false
let hideState = new Set()
let voted = new Set([2])
const requests = []
const titles = ['Der letzte Treffer dreht den Fight', 'Vier Gegner. Ein perfekter Moment.', 'Die Rettung mit zwölf Lebenspunkten', 'Der Steal war nicht geplant', 'Dieser Haken trifft immer', 'Knapp daneben ist auch vorbei']
const posters = ['johnnyblazedx', 'miracleghost9', 'whysolowkey', 'coolysdl', 'duzzel', 'kdenos']
const clips = titles.map((title, index) => ({
  id: index + 1, clip_id: `SyntheticContestClip${index + 1}`, clip_url: '#', title,
  thumbnail_url: `/streamer/clips/poster/${posters[index]}.jpg`, channel: `testpartner${index + 1}`, channel_name: `Testpartner ${index + 1}`,
  votes: [84, 67, 51, 38, 22, 14][index], my_vote: false, my_own: index === 5,
}))
const json = (response, value, status = 200) => {
  response.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' })
  response.end(JSON.stringify(value))
}
const current = () => {
  const visible = clips.filter(clip => !hideState.has(clip.id)).map(clip => ({ ...clip, my_vote: voted.has(clip.id), votes: clip.votes + (voted.has(clip.id) && clip.id !== 2 ? 1 : 0) }))
  return { month: '2026-09-01', month_label: 'September 2026', phase: mode, phase_ends_at: mode === 'submission' ? '2026-09-21T22:00:00Z' : '2026-09-30T22:00:00Z',
    submissions: visible, top3: visible.slice(0, 3), total: visible.length, next_offset: null }
}
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost')
  if (url.pathname.startsWith('/clips/api') || url.pathname === '/clips/auth/logout') {
    requests.push([request.method, url.pathname])
    if (mode === 'unavailable') return json(response, { message: 'Der Wettbewerb ist gerade nicht erreichbar.' }, 503)
    if (url.pathname === '/clips/api/current') return json(response, current())
    if (url.pathname === '/clips/api/session') return json(response, {
      authenticated: loggedIn, provider: loggedIn ? 'discord' : null, display_name: 'Prüfkonto', discord_authenticated: loggedIn,
      is_admin: admin, can_submit: loggedIn && mode === 'submission' && submitted < 3, submissions_used: submitted, submissions_limit: 3,
      can_vote: loggedIn && !tooYoung && mode === 'voting' && votesUsed < 5, votes_used: votesUsed, votes_limit: 5,
      eligibility_unavailable: false, discord_eligibility: { account_age_ok: !tooYoung, member_age_ok: true, present: true },
    })
    if (url.pathname === '/clips/api/archive') return json(response, { months: [
      { month: '2026-08-01', month_label: 'August 2026', winners: clips.slice(0, 3).map((clip, index) => ({ ...clip, rank: index + 1 })) },
      { month: '2026-07-01', month_label: 'Juli 2026', winners: [ { ...clips[3], rank: 1 } ] },
    ], next_before: null })
    if (url.pathname === '/clips/api/admin/submissions') return json(response, { submissions: clips.filter(clip => hideState.has(clip.id)) })
    let raw = ''; for await (const chunk of request) raw += chunk
    const body = raw ? JSON.parse(raw) : {}
    if (url.pathname === '/clips/api/vote') { voted.add(body.submission_id); votesUsed++; return json(response, { ok: true }) }
    if (url.pathname === '/clips/api/submit') { assert.ok(body.clip_url.startsWith('https://clips.twitch.tv/')); submitted++; return json(response, { ok: true }) }
    if (url.pathname === '/clips/api/admin/hide') { body.hidden ? hideState.add(body.submission_id) : hideState.delete(body.submission_id); return json(response, { ok: true }) }
    if (url.pathname === '/clips/auth/logout') { loggedIn = false; return json(response, { ok: true }) }
    return json(response, { message: 'Not found in fixture' }, 404)
  }
  try {
    let file
    if (url.pathname === '/clips' || url.pathname === '/clips/') file = path.join(dist, 'clips/index.html')
    else if (url.pathname.startsWith('/streamer/')) file = path.resolve(dist, `.${url.pathname.slice('/streamer'.length)}`)
    else if (brand && url.pathname.startsWith('/brand/')) file = path.resolve(brand, `.${url.pathname.slice('/brand'.length)}`)
    else { response.writeHead(404); return response.end() }
    if (!file.startsWith(`${dist}${path.sep}`) && !(brand && file.startsWith(`${brand}${path.sep}`))) { response.writeHead(403); return response.end() }
    const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.jpg': 'image/jpeg', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' }[path.extname(file)] || 'application/octet-stream'
    response.writeHead(200, { 'content-type': mime }); response.end(await readFile(file))
  } catch { response.writeHead(404); response.end() }
})
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
const origin = `http://127.0.0.1:${server.address().port}`
let browser
try {
  await mkdir(output, { recursive: true })
  browser = await chromium.launch({ headless: true, executablePath: process.argv[4] || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu'] })
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'de-DE', timezoneId: 'Europe/Berlin' })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  page.on('dialog', dialog => dialog.accept())
  // External playback is deliberately isolated; no third-party requests or real votes.
  await page.route('https://clips.twitch.tv/**', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Test-Player</title><body style="background:#111;color:#ddd;font:16px sans-serif">Twitch-Player im Browsertest</body>' }))
  await page.goto(`${origin}/clips`)
  await page.getByRole('heading', { name: 'September 2026', exact: true }).waitFor()
  await page.getByText('August 2026', { exact: true }).last().waitFor()
  assert.equal(await page.locator('.top-card .vote-button').count(), 3, 'all top-three cards must have voting controls')
  assert.equal(await page.locator('.top-card .vote-button').first().isEnabled(), true)
  await page.locator('.top-card .vote-button').first().click()
  await page.getByRole('status').filter({ hasText: 'Stimme gespeichert.' }).waitFor()
  assert.ok(requests.some(([method, path]) => method === 'POST' && path === '/clips/api/vote'))
  assert.equal(await page.locator('.top-card .vote-button').first().isDisabled(), true)
  assert.equal(await page.evaluate(() => localStorage.length + sessionStorage.length), 0)
  await page.locator('.top-card .clip-preview').first().click()
  const frame = page.locator('.top-card iframe').first()
  await frame.waitFor()
  const src = new URL(await frame.getAttribute('src'))
  assert.equal(src.searchParams.get('parent'), '127.0.0.1')
  const box = await frame.boundingBox(); assert.ok(box.width >= 400 && box.height >= 300)
  // Capture the ordinary page, not the synthetic embedded-player response.
  await page.reload(); await page.locator('.archive-card').first().waitFor(); await page.evaluate(() => document.fonts.ready)
  await page.screenshot({ path: path.join(output, 'desktop.jpg'), type: 'jpeg', quality: 88, fullPage: true })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.screenshot({ path: path.join(output, 'mobile.jpg'), type: 'jpeg', quality: 88, fullPage: true })
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'mobile must not overflow horizontally')
  await page.setViewportSize({ width: 320, height: 800 })
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), '320px viewport must not overflow')
  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.locator('.archive-select select').selectOption('2026-07-01')
  await page.locator('.archive-month').getByRole('heading', { name: 'Juli 2026' }).waitFor()
  tooYoung = true
  await page.reload(); await page.getByText('Dein Discord-Konto muss mindestens 30 Tage alt sein.', { exact: true }).first().waitFor()
  assert.equal(await page.locator('.top-card .vote-button').last().isDisabled(), true)
  tooYoung = false; admin = true
  await page.reload(); await page.locator('.top-card .moderate-button').first().waitFor()
  await page.locator('.top-card .moderate-button').first().click()
  await page.getByRole('heading', { name: 'Ausgeblendete Clips' }).waitFor()
  await page.getByRole('button', { name: 'Wieder anzeigen' }).click()
  await page.getByRole('status').filter({ hasText: 'Clip wieder sichtbar.' }).waitFor()
  mode = 'submission'; admin = false
  await page.reload(); await page.locator('#clip-url').waitFor()
  await page.locator('#clip-url').fill('https://clips.twitch.tv/SyntheticNewClip')
  await page.getByRole('button', { name: 'Einreichen', exact: true }).click()
  await page.getByRole('status').filter({ hasText: 'Clip eingereicht.' }).waitFor()
  await page.getByText(/1 von 3 Clips diesen Monat/).waitFor()
  await page.screenshot({ path: path.join(output, 'submission.jpg'), type: 'jpeg', quality: 88, fullPage: true })
  await page.getByRole('button', { name: 'Abmelden', exact: true }).click()
  await page.getByRole('link', { name: 'Mit Twitch anmelden' }).waitFor()
  assert.equal(await page.getByRole('link', { name: 'Mit Twitch anmelden' }).getAttribute('href'), '/twitch/auth/login?next=%2Fclips')
  mode = 'unavailable'
  await page.reload(); await page.getByRole('button', { name: 'Erneut versuchen' }).waitFor()
  assert.match(await page.locator('.loading-card').innerText(), /nicht erreichbar/)
  mode = 'submission'
  await page.getByRole('button', { name: 'Erneut versuchen' }).click()
  await page.getByRole('link', { name: 'Mit Discord anmelden' }).waitFor()
  assert.deepEqual(errors, [], 'no uncaught browser errors')
  assert.ok(existsSync(path.join(output, 'desktop.jpg')))
  console.log('PASS: top-three voting, POST persistence, no browser storage, player parent/size, 390px and 320px layout, archive month, eligibility, hide/restore, submit, logout, error/retry.')
  console.log('Screenshots use synthetic contest data and existing gameplay posters; they are not live contest results.')
} finally {
  await browser?.close()
  await new Promise(resolve => server.close(resolve))
}
