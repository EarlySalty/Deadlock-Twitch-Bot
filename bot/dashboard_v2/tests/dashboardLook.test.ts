/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');
const INDEX = read('index.css');
const SHELL = read('components/layout/DashboardShell.tsx');

const block = (css: string, selector: string): string => {
  const start = css.indexOf(selector);
  assert.ok(start >= 0, `Selektor ${selector} fehlt in index.css`);
  const open = css.indexOf('{', start);
  const close = css.indexOf('}', open);
  return css.slice(open, close);
};

test('Kacheln tragen keine Nieten und keine Gusseisen-Streifen mehr', () => {
  assert.doesNotMatch(INDEX, /\.panel-card::after/);
  assert.doesNotMatch(block(INDEX, '.glass {'), /repeating-linear-gradient/);
  assert.doesNotMatch(block(INDEX, '.panel-card {'), /repeating-linear-gradient/);
});

test('die animierte Gold-Aura der Shell ist entfernt', () => {
  assert.doesNotMatch(INDEX, /\.internal-home-vibe::before/);
  assert.doesNotMatch(INDEX, /\.internal-home-vibe::after/);
});

test('die Shell rendert keine BackgroundBlobs mehr', () => {
  assert.doesNotMatch(SHELL, /BackgroundBlobs/);
});

test('Raster und Grund-Verlauf stammen aus der Vorlage', () => {
  assert.match(INDEX, /rgba\(255, 255, 255, 0\.045\)/);
  assert.match(INDEX, /--gradient-bg:\s*linear-gradient\(180deg, #0f0f0e 0%, #0b0b0b 55%, #101010 100%\)/);
});
