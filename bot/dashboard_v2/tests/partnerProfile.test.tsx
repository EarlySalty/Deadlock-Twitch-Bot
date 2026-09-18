import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { PlanProvider } from '../src/context/PlanContext';
import { berlinInput, fromBerlinInput, monthDays } from '../src/utils/partnerProfile';
import { resolveTabParam } from '../src/tabAliases';
import { resolveVerwaltungTab } from '../src/pages/verwaltungTabs';
import { TAB_ENTITLEMENTS } from '../src/types/billing';
import type { PartnerProfileData } from '../src/api/partnerProfile';

Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: new URL('https://example.test/twitch/verwaltung#profil'), __TWITCH_DASHBOARD_RUNTIME__: {} } });
(globalThis as typeof globalThis & { React: typeof React }).React = React;
const { ProfileEditor, PartnerProfile } = await import('../src/pages/PartnerProfile');
const initial: PartnerProfileData = { login: 'alice', public_path: '/streamer/@alice', active: true, published: false, revision: 0,
  profile: { headline: '<script>bad()</script>', about: 'Grüße & Spaß', avatar_url: '', accent: 'gold', socials: [], featured: [], show_history: true, events: [] } };

test('profile is reachable in free partner management and analytics', () => {
  assert.deepEqual(resolveTabParam('profil'), { tab: 'profile' });
  assert.equal(TAB_ENTITLEMENTS.profile, undefined);
  assert.equal(resolveVerwaltungTab('#profil'), 'profil');
  const page = readFileSync(new URL('../src/pages/Verwaltung.tsx', import.meta.url), 'utf8');
  assert.match(page, /id: 'profil'/);
  assert.match(page, /<PartnerProfile/);
});
test('Berlin times are independent of the browser timezone and handle both offsets', () => {
  assert.equal(berlinInput('2026-09-18T18:30:00Z'), '2026-09-18T20:30');
  assert.equal(berlinInput('2026-01-18T18:30:00Z'), '2026-01-18T19:30');
  assert.equal(fromBerlinInput('2026-09-18T20:30'), '2026-09-18T18:30:00.000Z');
  assert.equal(fromBerlinInput('2026-01-18T19:30'), '2026-01-18T18:30:00.000Z');
});
test('DST gaps and duplicate hours are never silently shifted', () => {
  assert.throws(() => fromBerlinInput('2026-03-29T02:30'), /existiert.*nicht/);
  assert.throws(() => fromBerlinInput('2026-10-25T02:30'), /zweimal/);
  assert.equal(fromBerlinInput('2026-10-25T02:30', '2026-10-25T01:30:00Z'), '2026-10-25T01:30:00Z');
  assert.throws(() => fromBerlinInput('2026-02-30T18:00'));
  assert.throws(() => fromBerlinInput('not-a-date'));
});
test('month grid starts Monday and preserves leap days', () => {
  assert.equal(monthDays('2026-09').leading, 1);
  assert.equal(monthDays('2026-09').days.length, 30);
  assert.equal(monthDays('2028-02').days.length, 29);
  assert.deepEqual(monthDays('2026-13').days, []);
});
test('empty draft is private, escaped, and has an actionable calendar', () => {
  const html = renderToStaticMarkup(<ProfileEditor initial={initial} onReload={() => {}} />);
  assert.match(html, /Noch nicht veröffentlicht/);
  assert.doesNotMatch(html, /href="\/streamer\/@alice"/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;/);
  assert.match(html, /Grüße &amp; Spaß/);
  assert.match(html, /Termin am .* eintragen/);
  assert.match(html, /Keine automatische Löschung/);
});
test('paused profiles have disabled edits and no public opening link', () => {
  const html = renderToStaticMarkup(<ProfileEditor initial={{ ...initial, published: true, active: false }} onReload={() => {}} />);
  assert.match(html, /fieldset disabled=""/);
  assert.doesNotMatch(html, /href="\/streamer\/@alice"/);
  assert.match(html, /Bot-Verwaltung öffnen/);
});
test('published profile links to its own public page', () => {
  const html = renderToStaticMarkup(<ProfileEditor initial={{ ...initial, published: true }} onReload={() => {}} />);
  assert.match(html, /href="\/streamer\/@alice"/);
});
test('demo profile editor never starts a live request', () => {
  const client = new QueryClient();
  const html = renderToStaticMarkup(<QueryClientProvider client={client}><PlanProvider plan={null} isAdmin={false} isLocalhost={false} isDemoMode><PartnerProfile /></PlanProvider></QueryClientProvider>);
  assert.match(html, /Demo liest oder verändert keine persönlichen Partnerprofile/);
  assert.equal(client.getQueryCache().getAll()[0].state.fetchStatus, 'idle');
  client.clear();
});
