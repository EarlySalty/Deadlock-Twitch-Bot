import assert from 'node:assert/strict';
import React from 'react';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { CategoryCollectorData } from '../src/api/categoryCollector';

// Wie in community.test.tsx: Fenster-Stub vor dem ersten App-Import, deshalb
// stehen alle App-Module unten als dynamische Importe.
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: { location: new URL('https://example.test/twitch/dashboard-v2/kategorien-weltweit'), __TWITCH_DASHBOARD_RUNTIME__: {} },
});
(globalThis as typeof globalThis & { React: typeof React }).React = React;

const { ApiHttpError } = await import('../src/api/httpError');
const { buildApiUrl } = await import('../src/api/core');
const { CATEGORY_COLLECTOR_ENDPOINT, parseCollectorDays } = await import('../src/api/categoryCollector');
const {
  buildHeatmap,
  buildTrendreihe,
  formatBytes,
  formatMessdauer,
  formatZeitstempel,
  heatmapFarbe,
  klassifiziereSammlerFehler,
  sammlerStatusAnsicht,
  sortiereTopKanaele,
  sprachLabel,
  zahlOderStrich,
} = await import('../src/pages/kategorieWeltweitViewModel');
const {
  KategorieWeltweitPage,
  NachrichtSprachenTabelle,
  SammlerFehlerKarte,
  SammlerSektionen,
  StreamSprachenTabelle,
} = await import('../src/pages/KategorieWeltweit');

const FIXTURE: CategoryCollectorData = {
  meta: {
    days: 7,
    period_start: '2026-09-11T00:00:00Z',
    period_end: '2026-09-18T00:00:00Z',
    timezone: 'UTC',
    first_seen_at: '2026-09-11T06:00:00Z',
    last_completed_poll_at: '2026-09-18T05:30:00Z',
    last_message_at: '2026-09-18T05:12:00Z',
    complete_polls: 42,
    incomplete_polls: 3,
    dropped_messages: 17,
    storage_bytes: 5_242_880,
    retention_days: 90,
    collector_status: 'running',
    measurement_seconds: 604_800,
  },
  stream_languages: [
    { language: 'de', unique_streams: 12, unique_channels: 8, broadcast_hours: 120.5, viewer_hours: 3_400.25, avg_viewers: 28.2 },
    { language: 'und', unique_streams: 4, unique_channels: 4, broadcast_hours: 9.0, viewer_hours: 40.0, avg_viewers: 4.4 },
  ],
  message_languages: [
    { language: 'de', messages: 30 },
    { language: 'en', messages: 7 },
  ],
  trend: [
    { bucket_at: '2026-09-16T00:00:00Z', avg_streams: 2.0, avg_viewers: 20.0, poll_samples: 24 },
    { bucket_at: '2026-09-18T00:00:00Z', avg_streams: 3.0, avg_viewers: 35.0, poll_samples: 18 },
  ],
  top_channels: [
    { language: 'de', user_id: '123', login: 'example', display_name: 'Example', broadcast_hours: 40.0, viewer_hours: 1_200.0, avg_viewers: 30.0 },
    { language: 'en', user_id: '456', login: 'second', display_name: 'Second', broadcast_hours: 10.0, viewer_hours: 500.0, avg_viewers: 50.0 },
  ],
  chat_heatmap: [
    { language: 'de', hour_utc: 12, messages: 30 },
    { language: 'de', hour_utc: 20, messages: 90 },
  ],
};

test('Periodenwahl bleibt auf 7/30/90 begrenzt', () => {
  assert.equal(parseCollectorDays(7), 7);
  assert.equal(parseCollectorDays('30'), 30);
  assert.equal(parseCollectorDays(90), 90);
  assert.equal(parseCollectorDays(14), 7);
  assert.equal(parseCollectorDays('abc'), 7);
  assert.equal(parseCollectorDays('30foo'), 7);
  assert.equal(parseCollectorDays('90.0'), 7);
  assert.equal(parseCollectorDays(null), 7);
});

test('Endpunkt und Days-Parameter passen zum API-Vertrag', () => {
  assert.equal(
    buildApiUrl(CATEGORY_COLLECTOR_ENDPOINT, { days: 30 }),
    'https://example.test/twitch/api/v2/admin/category-collector?days=30',
  );
  assert.equal(
    buildApiUrl(CATEGORY_COLLECTOR_ENDPOINT, { days: parseCollectorDays('999') }),
    'https://example.test/twitch/api/v2/admin/category-collector?days=7',
  );
});

test('Stream- und Nachrichtensprachen sind getrennt beschriftet und "und" heißt unsicher', () => {
  const streamHtml = renderToStaticMarkup(<StreamSprachenTabelle daten={FIXTURE.stream_languages} />);
  assert.match(streamHtml, /Stream-Sprachen/);
  assert.match(streamHtml, /nach Sendestunden gewichtet/);
  assert.match(streamHtml, /Deutsch \(de\)/);
  assert.match(streamHtml, /Nicht sicher erkannt/);
  assert.doesNotMatch(streamHtml, /Nachrichtensprache/);
  assert.doesNotMatch(streamHtml, /Flagge|Region|Land/);

  const nachrichtHtml = renderToStaticMarkup(<NachrichtSprachenTabelle daten={FIXTURE.message_languages} />);
  assert.match(nachrichtHtml, /Chat-Nachrichtensprachen/);
  assert.match(nachrichtHtml, /unabhängig von der Stream-Sprache/);
  assert.match(nachrichtHtml, /Deutsch \(de\)/);
  assert.doesNotMatch(nachrichtHtml, /Sendestunden/);
});

test('Null-Schnitte erscheinen als Strich und nie als 0', () => {
  const html = renderToStaticMarkup(
    <StreamSprachenTabelle
      daten={[
        {
          language: 'fr',
          unique_streams: null as unknown as number,
          unique_channels: null as unknown as number,
          broadcast_hours: null as unknown as number,
          viewer_hours: null as unknown as number,
          avg_viewers: null as unknown as number,
        },
      ]}
    />,
  );
  // Null = nicht beobachtet: Strich, kein 0-Ersatz, kein "null"-Text.
  assert.doesNotMatch(html, />0</);
  assert.doesNotMatch(html, />null</);
  assert.match(html, />-</);
  // Ein echter Zähler 0 bleibt dagegen eine 0 (counts sind >= 0).
  const echtNull = renderToStaticMarkup(
    <StreamSprachenTabelle
      daten={[
        {
          language: 'fr',
          unique_streams: 0,
          unique_channels: 0,
          broadcast_hours: 0,
          viewer_hours: 0,
          avg_viewers: 0,
        },
      ]}
    />,
  );
  assert.match(echtNull, />0</);
  assert.equal(zahlOderStrich(null), '-');
  assert.equal(zahlOderStrich(undefined), '-');
  assert.equal(zahlOderStrich(0), '0');
  assert.equal(zahlOderStrich(1234.5, 1), '1.234,5');
});

test('Kein gestarteter Sammler zeigt ehrlichen Leerzustand statt Nullen', () => {
  const leer: CategoryCollectorData = {
    ...FIXTURE,
    meta: { ...FIXTURE.meta, collector_status: 'not_started' },
    stream_languages: [],
    message_languages: [],
    trend: [],
    top_channels: [],
    chat_heatmap: [],
  };
  const html = renderToStaticMarkup(<SammlerSektionen data={leer} />);
  assert.match(html, /Noch keine Daten/);
  assert.match(html, /noch nichts beobachtet/);
  // Keine Schein-Statistik: keine Tabellen, keine Kennzahlen-Sektionen.
  assert.doesNotMatch(html, /Ø Zuschauer/);
  assert.doesNotMatch(html, /sammler-topkanaele/);
  assert.doesNotMatch(html, /Impact-Metrik/);
  assert.doesNotMatch(html, /<table/);
  // Auch laufender Sammler ohne beobachtete Schnitte bleibt ehrlich leer.
  const leerLaufend = {
    ...leer,
    meta: { ...leer.meta, collector_status: 'running' as const },
  };
  const htmlLaufend = renderToStaticMarkup(<SammlerSektionen data={leerLaufend} />);
  assert.match(htmlLaufend, /Noch keine Daten/);
  assert.doesNotMatch(htmlLaufend, /<table/);
});

test('API-Fehler werden als Fehler gezeigt, nicht als Leerdaten', () => {
  for (const status of [401, 403, 503, 500]) {
    const html = renderToStaticMarkup(<SammlerFehlerKarte error={new ApiHttpError('Server', status)} />);
    assert.match(html, /nicht als leere Statistik/, `HTTP ${status} muss als Fehler sichtbar sein`);
    assert.match(html, /role="alert"/);
  }
  assert.match(klassifiziereSammlerFehler(new ApiHttpError('x', 403)).titel, /Kein Zugriff/);
  assert.match(klassifiziereSammlerFehler(new ApiHttpError('x', 503)).titel, /nicht bereit/);
  assert.match(klassifiziereSammlerFehler(new ApiHttpError('x', 401)).titel, /Nicht angemeldet/);
  assert.match(klassifiziereSammlerFehler(new Error('Netzwerk weg')).text, /Netzwerk weg/);
});

test('Nicht-Admins sehen weder Datenabfrage noch Inhalt', () => {
  const client = new QueryClient();
  client.setQueryData(['auth-status'], {
    authenticated: true,
    level: 'partner',
    isAdmin: false,
    isLocalhost: false,
  });
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <KategorieWeltweitPage />
    </QueryClientProvider>,
  );
  assert.match(html, /Kein Zugriff/);
  assert.match(html, /nur für Admins/);
  assert.doesNotMatch(html, /Ø Zuschauer/);
  const sammlerQueries = client
    .getQueryCache()
    .getAll()
    .filter((query) => query.queryKey[0] === 'category-collector');
  assert.equal(sammlerQueries.length, 0);
  client.clear();
});

test('Ohne Session gibt es einen eigenen Hinweis, nicht den Admin-Hinweis', () => {
  const client = new QueryClient();
  client.setQueryData(['auth-status'], {
    authenticated: false,
    level: 'none',
    isAdmin: false,
    isLocalhost: false,
  });
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <KategorieWeltweitPage />
    </QueryClientProvider>,
  );
  assert.match(html, /Nicht angemeldet/);
  assert.doesNotMatch(html, /nur für Admins/);
  const sammlerQueries = client
    .getQueryCache()
    .getAll()
    .filter((query) => query.queryKey[0] === 'category-collector');
  assert.equal(sammlerQueries.length, 0);
  client.clear();
});

test('Fehlgeschlagene Zugriffsprüfung ist ein Fehler, kein Behaupten eines Login-Zustands', () => {
  const client = new QueryClient();
  // Fehlgeschlagene Auth-Abfrage: kein Datenobjekt im Cache, frisch genug,
  // damit der Render keinen erneuten Abruf startet.
  client.setQueryData(['auth-status'], null);
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <KategorieWeltweitPage />
    </QueryClientProvider>,
  );
  assert.match(html, /Zugriffsprüfung fehlgeschlagen/);
  assert.doesNotMatch(html, /Nicht angemeldet/);
  assert.doesNotMatch(html, /nur für Admins/);
  const sammlerQueries = client
    .getQueryCache()
    .getAll()
    .filter((query) => query.queryKey[0] === 'category-collector');
  assert.equal(sammlerQueries.length, 0);
  client.clear();
});

test('Trendreihe lässt Stunden ohne Messung als Lücke, nicht als Null', () => {
  const reihe = buildTrendreihe(FIXTURE.trend);
  assert.equal(reihe.length, 49);
  assert.equal(reihe[0].luecke, false);
  assert.equal(reihe[1].luecke, true);
  assert.equal(reihe[1].avg_streams, null);
  assert.equal(reihe[1].avg_viewers, null);
  assert.equal(reihe[48].avg_viewers, 35);
  assert.equal(reihe[48].poll_samples, 18);
  assert.deepEqual(buildTrendreihe([]), []);
});

test('Heatmap ordnet Sprachen zeilenweise und Stunden in UTC, Lücken bleiben null', () => {
  const ansicht = buildHeatmap(FIXTURE.chat_heatmap);
  assert.equal(ansicht.reihen.length, 1);
  assert.equal(ansicht.reihen[0].language, 'de');
  assert.equal(ansicht.reihen[0].stunden.length, 24);
  assert.equal(ansicht.reihen[0].stunden[12], 30);
  assert.equal(ansicht.reihen[0].stunden[20], 90);
  assert.equal(ansicht.reihen[0].stunden[3], null);
  assert.equal(ansicht.max, 90);
  assert.equal(heatmapFarbe(null, 90), 'rgba(197, 160, 89, 0.06)');
  assert.notEqual(heatmapFarbe(90, 90), 'rgba(197, 160, 89, 0.06)');
});

test('Top-Kanäle sortieren nach Zuschauerstunden, Filterton bleibt Sprachcode', () => {
  const sortiert = sortiereTopKanaele([
    { language: 'de', viewer_hours: 100 },
    { language: 'en', viewer_hours: 900 },
    { language: 'und', viewer_hours: 500 },
  ]);
  assert.deepEqual(sortiert.map((k) => k.language), ['en', 'und', 'de']);
});

test('Meta-Formatierungen: Messdauer echt, Speicher menschlich, Zeitstempel UTC', () => {
  assert.equal(formatMessdauer(0), 'Noch keine Messung');
  assert.equal(formatMessdauer(604_800), '7 Tage');
  assert.equal(formatMessdauer(5_400), '1 Std. 30 Min.');
  assert.equal(formatBytes(0), '0 B');
  assert.equal(formatBytes(5_242_880), '5.0 MB');
  assert.equal(formatBytes(3 * 1024 * 1024 * 1024), '3.0 GB');
  assert.equal(formatZeitstempel(null), 'Bisher keine');
  assert.match(formatZeitstempel('2026-09-18T05:30:00Z'), /18\.09\.2026/);
  assert.match(formatZeitstempel('2026-09-18T05:30:00Z'), /05:30/);
});

test('Sprachetikett bleibt ohne Flagge und Region', () => {
  assert.equal(sprachLabel('und'), 'Nicht sicher erkannt');
  assert.equal(sprachLabel('de'), 'Deutsch (de)');
  assert.equal(sprachLabel('xyz'), 'xyz');
  assert.equal(sprachLabel(''), 'Unbekannte Sprache');
});

test('Statustexte erklären, sie bewerten nicht', () => {
  assert.equal(sammlerStatusAnsicht('not_started').label, 'Noch nicht gestartet');
  assert.ok(sammlerStatusAnsicht('not_started').hinweis);
  assert.equal(sammlerStatusAnsicht('running').label, 'Läuft');
  assert.equal(sammlerStatusAnsicht('stale').label, 'Stockend');
  assert.equal(sammlerStatusAnsicht('disabled').label, 'Deaktiviert');
  assert.equal(sammlerStatusAnsicht('error').label, 'Fehler');
});

test("Stundenwerte werden nicht rekursiv zu falschen Tagesmitteln verrechnet", () => {
  const values = [10, 90, 20];
  const points = values.map((value, index) => ({ bucket_at: '2026-09-18T0' + index + ':00:00Z', avg_streams: value, avg_viewers: value * 10, poll_samples: index + 1 }));
  assert.deepEqual(buildTrendreihe(points).map(point => point.avg_streams), values);
  assert.equal(buildTrendreihe([{ ...points[0], bucket_at: "invalid" }]).length, 0);
  assert.equal(buildTrendreihe([{ ...points[0], avg_streams: null, avg_viewers: null }])[0].luecke, true);
});

test("Unbekannte 503-Ursache und ungültige Heatmapwerte werden nicht erfunden", () => {
  assert.doesNotMatch(klassifiziereSammlerFehler(new ApiHttpError("x", 503)).text, /Tabellen.*nicht eingerichtet/);
  assert.equal(buildHeatmap([{ language: "de", hour_utc: 99, messages: 10 }]).reihen.length, 0);
  assert.equal(buildHeatmap([{ language: "de", hour_utc: 4, messages: Number.NaN }]).reihen.length, 0);
});
