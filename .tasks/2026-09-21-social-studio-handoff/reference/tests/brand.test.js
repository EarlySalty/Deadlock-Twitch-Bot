import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';
import { COMMUNITY_LOGO } from '../src/brand.js';

const root = new URL('../', import.meta.url);
const read = file => fs.readFileSync(new URL(file, root), 'utf8');
const sources = ['index.html', ...fs.readdirSync(new URL('src/', root), {recursive:true})
  .filter(file => /\.(css|js|jsx)$/.test(file))
  .map(file => 'src/' + file)];

test('presentation uses semantic tokens, not a separate Tailwind palette', () => {
  const foreign = /\b(?:text|bg|border|ring|accent|from|via|to|divide|placeholder)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|green|emerald|teal|cyan|blue|indigo|violet|purple|rose)-\d+/g;
  for (const file of sources) assert.deepEqual(read(file).match(foreign) ?? [], [], file);
});
test('colors match the Twitch dashboard brand source', () => {
  const css = read('src/brand-theme.css');
  for (const [name,value] of Object.entries({
    primary:'#C5A059', accent:'#D6B676', bg:'#0d0d0d', card:'#161616',
    'text-primary':'#f2eee6', 'text-secondary':'#9d968a', 'on-gold':'#241A12',
    success:'#43B581', warning:'#E8A33D', danger:'#FF5A3C'
  })) assert.ok(css.includes('--color-'+name+': '+value+';'), name);
});
test('existing community logo matches the verified display-size copy', () => {
  const bytes = fs.readFileSync(new URL('assets/deadlock-d-logo.webp', root));
  assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'),
    'd6ee8c4862e96df8789aec9cc7dd292a847a94fb7dfdf831958cbbfb7e550170');
  assert.deepEqual(Buffer.from(COMMUNITY_LOGO.split(',')[1],'base64'),bytes);
  assert.match(read('index.html'), /class="brand-logo/);
  assert.match(read('src/react/DashboardShell.jsx'), /<CommunityBrand/);
});
test('Manrope and Sora reuse the community font host', () => {
  const css = read('src/styles.css');
  assert.match(css, /font-family: "Manrope"/);
  assert.match(css, /font-family: "Sora"/);
  assert.match(css, /deutsche-deadlock-community\.de\/brand\/fonts\/manrope-latin\.woff2/);
  assert.doesNotMatch(css,/font-family:\s*(?:Inter|Geist)/);
});
test('approved sidebar, queue and editor dimensions remain intact', () => {
  assert.ok(read('index.html').includes('lg:grid-cols-[216px_minmax(0,1fr)]'));
  assert.ok(read('src/app.js').includes('md:w-[196px]'));
  assert.ok(read('src/app.js').includes('md:grid-cols-[1fr_280px]'));
  assert.ok(read('src/styles.css').includes('max-width:1080px'));
});
test('dark label on brand-gold primary action exceeds 4.5 contrast', () => {
  const lum = hex => {
    const s = [1,3,5].map(i => parseInt(hex.slice(i,i+2),16)/255)
      .map(v => v<=.04045 ? v/12.92 : ((v+.055)/1.055)**2.4);
    return s[0]*.2126+s[1]*.7152+s[2]*.0722;
  };
  for (const surface of ['#C5A059','#D6B676','#E6C78F'])
    assert.ok((lum(surface)+.05)/(lum('#241A12')+.05) >= 4.5);
});
