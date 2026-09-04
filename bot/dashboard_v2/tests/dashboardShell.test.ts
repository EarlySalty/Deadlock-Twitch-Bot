/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');

const APP = read('App.tsx');
const SHELL = read('components/layout/DashboardShell.tsx');

const PAGES = [
  'pages/InternalHomeLanding.tsx',
  'pages/Uplink.tsx',
  'pages/Verwaltung.tsx',
  'pages/OverlayBuilder.tsx',
  'pages/SocialMediaAdmin.tsx',
  'pages/Pricing.tsx',
] as const;

const ROUTES = ['home', 'analyse', 'social', 'uplink', 'verwaltung', 'overlay', 'pricing'] as const;

test('App.tsx importiert die Shell und wickelt jede der sieben Routen darin ein', () => {
  assert.match(APP, /import \{ DashboardShell \} from '@\/components\/layout\/DashboardShell'/);
  for (const route of ROUTES) {
    assert.match(
      APP,
      new RegExp(`activeRoute="${route}"`),
      `App.tsx muss die Route ${route} in DashboardShell wickeln`,
    );
  }
});

test('keine Seite setzt einen eigenen Gesamtrahmen mehr', () => {
  for (const page of PAGES) {
    const src = read(page);
    assert.doesNotMatch(src, /internal-home-vibe/, `${page} darf den Shell-Hintergrund nicht selbst setzen`);
    assert.doesNotMatch(src, /max-w-\[/, `${page} darf keine eigene Gesamt-Maximalbreite setzen`);
  }
});

test('die Shell traegt Hintergrund, Gesamtbreite, Sidebar-Spalte und den Main-Slot', () => {
  assert.match(SHELL, /internal-home-vibe/);
  assert.match(SHELL, /max-w-\[2200px\]/);
  assert.match(SHELL, /lg:grid-cols-\[220px_minmax\(0,1fr\)\]/);
  assert.match(SHELL, /<DashboardSidebar activeRoute=\{activeRoute\} \/>/);
  assert.match(SHELL, /<main[^>]*>\{children\}<\/main>/);
});
