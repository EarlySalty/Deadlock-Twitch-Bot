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

test('Karten bleiben neutral und verzichten auf Nieten und Streifen', () => {
  assert.match(INDEX, /--color-card:\s*#161616/);
  assert.doesNotMatch(INDEX, /\.panel-card::after|repeating-linear-gradient/);
  assert.doesNotMatch(INDEX, /#221a15|#2a221c|#1a1310/);
});

test('die animierte Gold-Aura der Shell ist entfernt', () => {
  assert.doesNotMatch(INDEX, /\.internal-home-vibe::before/);
  assert.doesNotMatch(INDEX, /\.internal-home-vibe::after/);
});

test('die Shell rendert keine BackgroundBlobs mehr', () => {
  assert.doesNotMatch(SHELL, /BackgroundBlobs/);
});

test('Raster und neutrale Grundtöne bleiben dezent', () => {
  assert.match(INDEX, /rgba\(255, 255, 255, 0\.05\)/);
  assert.match(INDEX, /--gradient-bg:\s*linear-gradient\(180deg, #101010 0%, #0d0d0d 55%, #121212 100%\)/);
});

test('das Raster wird nicht zu den Raendern hin ausgeblendet', () => {
  assert.doesNotMatch(block(INDEX, 'body::before'), /mask-image/);
});

test('Gold bleibt an der Kartenkante, Tiefe entsteht durch neutralen Schatten', () => {
  assert.match(block(INDEX, '.panel-card {'), /border: 1px solid var\(--color-border\)/);
  assert.match(block(INDEX, '.panel-card {'), /box-shadow: var\(--shadow-card-soft\)/);
  assert.doesNotMatch(block(INDEX, '.panel-card {'), /rgba\(241, 210, 153/);
  assert.doesNotMatch(INDEX, /hero-aura::before|hero-aura-spin/);
});

test('Avatar und Icon-Kacheln tragen keinen Gold-Glow mehr, nur den feinen Ring', () => {
  assert.doesNotMatch(block(INDEX, '.sidebar-avatar-glow {'), /color-mix/);
  assert.doesNotMatch(block(INDEX, '.sidebar-avatar-glow {'), /0 0 24px/);
  assert.match(block(INDEX, '.sidebar-avatar-glow {'), /0 0 0 2px rgba\(255, 255, 255, 0\.06\)/);
});
