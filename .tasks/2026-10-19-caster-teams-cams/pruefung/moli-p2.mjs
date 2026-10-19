import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '/home/nathanael/Documents/claude-config/tools/sichtpruefung/node_modules/playwright-core/index.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const screens = path.resolve(here, '../screens');
const mockPort = 4198;
const cdpPort = 9338;
const origin = `http://127.0.0.1:${mockPort}`;
const children = [];

function run(cmd, args, extra = {}) {
  const child = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'], ...extra });
  children.push(child);
  child.stderr.on('data', (chunk) => process.stderr.write(chunk));
  return child;
}

function waitExit(child) {
  return new Promise((resolve) => child.on('exit', resolve));
}

async function waitHttp(url, tries = 40) {
  for (let i = 0; i < tries; i += 1) {
    try {
      const res = await fetch(url, { redirect: 'manual' });
      if (res.ok || res.status === 404) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(`timeout ${url}`);
}

function stopAll() {
  for (const child of children) {
    try { child.kill('SIGTERM'); } catch {}
  }
}

async function shot(page, name) {
  const file = path.join(screens, name);
  await page.screenshot({ path: file, fullPage: true });
  return file;
}

async function tap(locator) {
  await locator.evaluate((el) => el.click());
}

async function typeInto(locator, value) {
  await locator.evaluate((el, next) => {
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set;
    el._valueTracker?.setValue('');
    setter?.call(el, next);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }, value);
}

async function overlayShot(page, route, name) {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.goto(`${origin}${route}`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => {
    const coach = [...document.querySelectorAll('.coach')].map((el) => el.textContent || '');
    return coach.some((text) => text.includes('Coach: Max Mustermann'));
  }, { timeout: 8000 });
  await page.evaluate(() => document.fonts?.ready);
  await page.screenshot({ path: path.join(screens, name) });
}

try {
  fs.mkdirSync(screens, { recursive: true });
  const mock = run(process.execPath, [path.join(here, 'mock-server.mjs')], { env: { ...process.env, P2_MOCK_PORT: String(mockPort) } });
  mock.stdout.on('data', (chunk) => process.stdout.write(chunk));
  const moli = run('/home/nathanael/.local/bin/moli', [
    'serve', '--host', '127.0.0.1', '-p', String(cdpPort), '--layout', '--image', '--font',
  ]);
  await waitHttp(`${origin}/twitch/api/v2/auth-status`);
  await waitHttp(`http://127.0.0.1:${cdpPort}/json/version`);
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
  const context = browser.contexts()[0] || await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));

  await page.setViewportSize({ width: 1440, height: 1600 });
  await page.goto(`${origin}/twitch/admin/content/caster-overlay`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Teams & Player' }).waitFor({ timeout: 15000 });
  await typeInto(page.getByLabel('Teamname').first(), 'Team Amber');
  await typeInto(page.getByLabel('Kürzel').first(), 'amb');
  await typeInto(page.getByPlaceholder('Discord-Name suchen oder frei eintragen').first(), 'Max Mustermann');
  await tap(page.getByRole('button', { name: 'Spieler hinzufügen' }).first());
  await typeInto(page.getByRole('textbox', { name: 'Spieler 1' }), 'Nani');
  const logo = path.join(here, 'logo.png');
  await page.locator('label').filter({ hasText: 'Logo hochladen' }).nth(0).locator('input[type=file]').setInputFiles(logo);
  const saveLibrary = page.getByRole('button', { name: 'In Teambibliothek speichern' }).first();
  const disabled = await saveLibrary.getAttribute('disabled');
  if (disabled !== null) throw new Error(`library save disabled, name=${await page.getByLabel('Teamname').first().inputValue()}`);
  await tap(saveLibrary);
  await new Promise((resolve) => setTimeout(resolve, 500));
  const afterSave = await page.evaluate(() => document.body.innerText);
  if (!afterSave.includes('Bibliothek aktualisieren') && !afterSave.includes('Teambibliothek gespeichert')) {
    throw new Error(`library save missing: ${afterSave.slice(-800)}`);
  }
  await tap(page.getByRole('button', { name: 'Live übernehmen' }));
  await page.getByText('Live übernommen.').waitFor({ timeout: 8000 });
  await tap(page.getByRole('button', { name: 'Seiten tauschen' }));
  await shot(page, 'p2-1440-editor.png');

  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Teams & Player' }).waitFor({ timeout: 15000 });
  await page.getByRole('combobox', { name: 'Aus Teambibliothek' }).first().waitFor();
  const library = await page.getByRole('combobox', { name: 'Aus Teambibliothek' }).first().inputValue();
  if (!library) throw new Error('library empty after reload');
  const coach = await page.getByPlaceholder('Discord-Name suchen oder frei eintragen').first().inputValue();
  if (coach !== 'Max Mustermann') throw new Error(`coach lost: ${coach}`);
  await shot(page, 'p2-1440-reload.png');

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await shot(page, 'p2-390-editor.png');
  await page.getByRole('heading', { name: 'Teams & Player' }).scrollIntoViewIfNeeded();
  await shot(page, 'p2-390-teams.png');

  await overlayShot(page, '/twitch/caster-overlay', 'p2-1920-caster-coach.png');
  await overlayShot(page, '/twitch/caster-overlay/scoreboard', 'p2-1920-scoreboard-coach.png');
  await overlayShot(page, '/twitch/caster-overlay/versus', 'p2-1920-versus-coach.png');

  const proof = {
    errors,
    libraryAfterReload: library,
    coachAfterReload: coach,
    files: fs.readdirSync(screens).filter((name) => name.startsWith('p2-')),
  };
  fs.writeFileSync(path.join(here, 'moli-p2.json'), JSON.stringify(proof, null, 2));
  console.log(JSON.stringify(proof));
  await browser.close();
  stopAll();
  await Promise.all(children.map(waitExit));
  if (errors.length) process.exit(2);
} catch (error) {
  console.error(error);
  try {
    const pages = [];
    console.error(String(error?.message || error));
  } catch {}
  stopAll();
  process.exit(1);
}
