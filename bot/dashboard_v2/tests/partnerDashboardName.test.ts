/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const ROOT = join(import.meta.dirname, '..');
const SRC = join(ROOT, 'src');
const read = (rel: string) => readFileSync(join(ROOT, rel), 'utf8');

const srcFiles = readdirSync(SRC, { recursive: true })
  .map((entry) => String(entry).replace(/\\/g, '/'))
  .filter((rel) => rel.endsWith('.ts') || rel.endsWith('.tsx'));

test('der Tab-Titel heißt Partner Dashboard', () => {
  const html = read('index.html');
  assert.match(html, /<title>Partner Dashboard · Deutsche Deadlock Community<\/title>/);
  assert.doesNotMatch(html, /Twitch Analytics/);
});

test('das Header-Badge trägt Partner Dashboard statt Twitch Analytics', () => {
  const header = read('src/components/layout/Header.tsx');
  assert.match(header, /t\('Partner Dashboard'\)/);
  assert.doesNotMatch(header, /Twitch Analytics/);
});

test('kein Quelltext nennt noch Twitch Analytics oder Analyse Dashboard', () => {
  for (const rel of srcFiles) {
    const src = readFileSync(join(SRC, rel), 'utf8');
    assert.doesNotMatch(src, /Twitch Analytics/, `${rel} nennt noch Twitch Analytics`);
    assert.doesNotMatch(src, /Analyse Dashboard/, `${rel} nennt noch Analyse Dashboard`);
    if (rel === 'pages/SocialMediaAdmin.tsx') {
      continue;
    }
    assert.doesNotMatch(src, /Analyse-Dashboard/, `${rel} nennt noch Analyse-Dashboard`);
  }
});

test('SocialMediaAdmin nennt Analyse-Dashboard nur im Kommentar', () => {
  const src = read('src/pages/SocialMediaAdmin.tsx');
  const zeilen = src.split('\n').filter((zeile) => zeile.includes('Analyse-Dashboard'));
  assert.equal(zeilen.length, 1);
  assert.match(zeilen[0], /^\s*\*/);
});

test('Home und Hilfe verwenden dieselbe Einrichtung statt unabhängiger Dialoge', () => {
  const home = read('src/pages/InternalHomeLanding.tsx');
  const app = read('src/App.tsx');
  assert.match(home, /<EinrichtungCard/);
  assert.match(app, /<OnboardingProvider>/);
  assert.match(app, /<EinrichtungCard help/);
  assert.doesNotMatch(home + app, /WelcomeTour|PricingTour|AnalyticsTour|pricing-tour-pending/);
});

test('der Backlink im Woerterbuch heißt Partner Dashboard', () => {
  const dict = read('src/i18n/dictionary.ts');
  assert.match(dict, /'← Partner Dashboard': '← Partner dashboard'/);
  assert.doesNotMatch(dict, /Analyse-Dashboard/);
});
