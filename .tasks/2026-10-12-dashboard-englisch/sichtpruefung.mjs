import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';

const [, , output, mode = 'stored', base = 'http://127.0.0.1:4176', cdp = 'http://127.0.0.1:9351', nodeModules = '/home/nathanael/.worktrees/ttb-dashboard-englisch/bot/dashboard_v2/node_modules'] = process.argv;
const { chromium } = createRequire(path.join(nodeModules, 'x.js'))('playwright-core');

const ROUTES = (process.env.ROUTES ?? [
  '/',
  '/?tab=streams',
  '/?tab=audience',
  '/?tab=chat',
  '/?tab=viewers',
  '/?tab=growth',
  '/?tab=compare',
  '/?tab=category',
  '/?tab=experimental',
  '/?tab=schedule',
  '/?tab=planning',
  '/?tab=community',
  '/?tab=profile',
  '/?tab=coaching',
  '/?tab=ai',
  '/?tab=reports',
  '/?tab=monetization',
  '/?view=category',
  '/dashboard',
  '/verwaltung',
  '/overlay',
  '/titel',
  '/uplink',
  '/pricing',
  '/twitch/hilfe',
  '/twitch/feedback',
  '/twitch/challenges',
].join(',')).split(',');

const WIDTHS = (process.env.WIDTHS ?? '320,1280').split(',').map(Number);
const LANGS = (process.env.LANGS ?? 'en,de').split(',');
const GERMAN = /(?<![\p{L}\d_])(und|oder|nicht|keine?n?|noch|mit|für|über|wird|werden|deine?[nmrs]?|du|dir|dich|ist|sind|bei|auf|aus|vom|zum|zur|der|die|das|den|dem|des|ein|eine[nmrs]?|wenn|nach|jetzt|alle|bitte|letzte[nmrs]?|seit|Tage?n?|Woche|Stunden?|Minuten?|Zuschauer|Einstellungen|Übersicht|Zeitraum|Fehler|Speichern|Abbrechen|Laden|lädt|geladen)(?![\p{L}\d_])|[äöüÄÖÜß]/u;

function slug(route) {
  return route.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '') || 'start';
}

function initScript(lang, storageMode) {
  const languages = lang === 'en' ? ['en-US', 'en'] : ['de-DE', 'de'];
  const stored = storageMode === 'stored' ? lang : null;
  return `(() => {
    const languages = ${JSON.stringify(languages)};
    Object.defineProperty(Navigator.prototype, 'languages', { get: () => languages, configurable: true });
    Object.defineProperty(Navigator.prototype, 'language', { get: () => languages[0], configurable: true });
    try {
      const stored = ${JSON.stringify(stored)};
      if (stored) window.localStorage.setItem('dashboard.language', stored);
      else window.localStorage.removeItem('dashboard.language');
    } catch {}
  })();`;
}

async function measure(page, lang) {
  return page.evaluate(({ germanSource, lang }) => {
    const german = new RegExp(germanSource, 'u');
    const viewport = window.innerWidth;
    const visible = (el) => {
      const style = getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      return style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0;
    };
    const ownText = (el) => [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join(' ').trim();
    const clipped = [];
    const outside = [];
    for (const el of document.body.querySelectorAll('*')) {
      if (!visible(el)) continue;
      const text = ownText(el);
      if (!text) continue;
      const style = getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      const hides = ['hidden', 'clip'].includes(style.overflowX) || style.textOverflow === 'ellipsis';
      if (hides && el.scrollWidth > el.clientWidth + 1) clipped.push(text.slice(0, 80));
      let scroller = el.parentElement;
      let insideScroller = false;
      while (scroller && scroller !== document.body) {
        const s = getComputedStyle(scroller);
        if (['auto', 'scroll'].includes(s.overflowX)) { insideScroller = true; break; }
        scroller = scroller.parentElement;
      }
      if (!insideScroller && (rect.right > viewport + 1 || rect.left < -1)) outside.push(text.slice(0, 80));
    }
    const lines = document.body.innerText.split('\n').map((l) => l.trim()).filter(Boolean);
    const germanLines = lang === 'en' ? [...new Set(lines.filter((l) => german.test(l)))] : [];
    return {
      viewport,
      documentWidth: document.documentElement.scrollWidth,
      htmlLang: document.documentElement.lang,
      storedLanguage: window.localStorage.getItem('dashboard.language'),
      navigatorLanguage: navigator.language,
      clipped: [...new Set(clipped)].slice(0, 15),
      outside: [...new Set(outside)].slice(0, 15),
      germanLines: germanLines.slice(0, 40),
      germanLineCount: germanLines.length,
      textLength: document.body.innerText.length,
    };
  }, { germanSource: GERMAN.source, lang });
}

fs.mkdirSync(output, { recursive: true });
const browser = await chromium.connectOverCDP(cdp);
const context = browser.contexts()[0] ?? await browser.newContext();
const results = [];
try {
  for (const lang of LANGS) {
    for (const width of WIDTHS) {
      for (const route of ROUTES) {
        const name = `${lang}-${width}-${slug(route)}`;
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', (error) => errors.push(error.message));
        await page.setViewportSize({ width, height: 900 });
        await page.addInitScript(initScript(lang, mode));
        try {
          await page.goto(`${base}${route}`, { waitUntil: 'load', timeout: 30000 });
          await page.waitForLoadState('networkidle', { timeout: 15000 }).catch(() => {});
          await page.waitForFunction(() => !/(Lade |Wird geladen|Loading|wird geladen)/.test(document.body.innerText), null, { timeout: 25000 }).catch(() => {});
          await page.waitForTimeout(800);
          await page.evaluate(() => document.fonts.ready);
          const contentHeight = await page.evaluate(() => Math.max(
            document.documentElement.scrollHeight,
            ...[...document.querySelectorAll('*')]
              .filter((el) => ['auto', 'scroll'].includes(getComputedStyle(el).overflowY))
              .map((el) => el.scrollHeight),
          ));
          await page.setViewportSize({ width, height: Math.min(Math.max(contentHeight, 900), 9000) });
          await page.waitForTimeout(400);
          await page.screenshot({ path: `${output}/${name}.png` });
          results.push({ name, route, lang, width, ...(await measure(page, lang)), errors });
        } catch (error) {
          results.push({ name, route, lang, width, failed: String(error), errors });
        }
        await page.close();
      }
    }
  }
} finally {
  await browser.close();
}
fs.writeFileSync(`${output}/sichtpruefung.json`, JSON.stringify({ mode, base, results }, null, 2));
for (const r of results) {
  const flags = [
    r.failed ? `FAILED ${r.failed.slice(0, 120)}` : '',
    r.documentWidth > r.viewport ? `overflow ${r.documentWidth}>${r.viewport}` : '',
    r.clipped?.length ? `clipped ${r.clipped.length}` : '',
    r.outside?.length ? `outside ${r.outside.length}` : '',
    r.germanLineCount ? `german ${r.germanLineCount}` : '',
    r.errors.length ? `errors ${r.errors.length}` : '',
  ].filter(Boolean).join(' | ');
  console.log(`${r.name}: ${flags || 'ok'}`);
}
