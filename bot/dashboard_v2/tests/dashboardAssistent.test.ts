import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { vorschlaegeFuer } from '../src/components/assistent/vorschlaege';

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const APP = readFileSync(join(DASHBOARD_ROOT, 'src', 'App.tsx'), 'utf8');
const API = readFileSync(join(DASHBOARD_ROOT, 'src', 'api', 'assistent.ts'), 'utf8');
const WIDGET = readFileSync(
  join(DASHBOARD_ROOT, 'src', 'components', 'assistent', 'DashboardAssistent.tsx'),
  'utf8',
);

const SEITEN = ['home', 'verwaltung', 'uplink', 'social-media', 'analyse', 'analyse/audience', 'standard'];

test('vorschlaegeFuer liefert je Seite und Sprache genau drei Fragen', () => {
  for (const seite of SEITEN) {
    for (const sprache of ['de', 'en'] as const) {
      const fragen = vorschlaegeFuer(seite, sprache);
      assert.equal(fragen.length, 3, `${seite}/${sprache}: nicht drei Fragen`);
      for (const frage of fragen) {
        assert.ok(frage.trim().length > 0, `${seite}/${sprache}: leere Frage`);
      }
    }
  }
});

test('unbekannte Seiten fallen auf die Standardvorschläge zurück', () => {
  assert.deepEqual(vorschlaegeFuer('pricing', 'de'), vorschlaegeFuer('standard', 'de'));
  assert.deepEqual(vorschlaegeFuer('overlay', 'en'), vorschlaegeFuer('standard', 'en'));
});

test('englische Vorschläge unterscheiden sich von den deutschen', () => {
  assert.notDeepEqual(vorschlaegeFuer('home', 'de'), vorschlaegeFuer('home', 'en'));
});

test('App.tsx bindet den Assistenten innerhalb des LanguageProvider ein', () => {
  const auf = APP.indexOf('<LanguageProvider');
  const zu = APP.indexOf('</LanguageProvider>');
  const widget = APP.indexOf('<DashboardAssistent');
  assert.ok(auf >= 0, 'LanguageProvider fehlt');
  assert.ok(zu > auf, 'schließendes LanguageProvider-Tag fehlt');
  assert.ok(widget > auf && widget < zu, 'DashboardAssistent liegt nicht im LanguageProvider');
});

test('App.tsx rendert den Assistenten nur außerhalb von Demo und Preview', () => {
  assert.match(APP, /!isPreviewModeEnabled\(\)/);
  assert.match(APP, /!hasDemoRuntimeConfig\(\)/);
  assert.match(APP, /!resolveEffectiveDemoMode\(/);
  assert.match(APP, /zeigeAssistent && <DashboardAssistent \/>/);
});

test('assistent.ts sendet page und language und setzt X-CSRF-Token bedingt', () => {
  assert.match(API, /JSON\.stringify\(\{ question, history, page, language \}\)/);
  assert.match(API, /csrfToken \?/);
  assert.match(API, /'X-CSRF-Token'/);
});

test('das Widget trägt den Knopftext "Hilfe bekommen"', () => {
  assert.match(WIDGET, /'Hilfe bekommen'/);
});
