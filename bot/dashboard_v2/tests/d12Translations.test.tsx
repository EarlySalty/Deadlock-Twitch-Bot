import { test, vi } from 'vitest';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MetadataPanel } from '../src/components/socialmedia/ai/MetadataPanel';
import type { AiVersionMetadata } from '../src/components/socialmedia/ai/api';
import { LanguageProvider } from '../src/context/LanguageContext';
import { dictionaryFor, translate } from '../src/i18n/dictionary';
import { UplinkHochkantEditor } from '../src/components/uplink/UplinkHochkantEditor';
import { UplinkYouTubeLive } from '../src/components/uplink/UplinkYouTubeLive';

vi.mock('../src/components/socialmedia/ai/api', () => ({
  AI_COPY_PLATFORMS: ['youtube', 'tiktok', 'instagram'],
  fetchAiVersionMetadata: vi.fn(),
  saveAiVersionMetadata: vi.fn(),
}));

const mutationFailure = vi.hoisted(() => ({ message: '' }));
vi.mock('@tanstack/react-query', async (importOriginal) => {
  const original = await importOriginal<typeof import('@tanstack/react-query')>();
  return {
    ...original,
    useMutation: (...args: Parameters<typeof original.useMutation>) => {
      const mutation = original.useMutation(...args);
      return mutationFailure.message ? { ...mutation, isError: true, error: new Error(mutationFailure.message) } : mutation;
    },
  };
});

Object.assign(globalThis, { React });

const files = [
  'uplink/UplinkHochkantEditor.tsx',
  'uplink/UplinkYouTubeLive.tsx',
  'socialmedia/ai/MetadataPanel.tsx',
  'socialmedia/autopilot/publish-review/AiRevisionPanel.tsx',
];

test('D12 fixed translation keys have English entries and preserve German text', () => {
  for (const file of files) {
    const source = readFileSync(new URL(`../src/components/${file}`, import.meta.url), 'utf8');
    const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const visit = (node: ts.Node) => {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 't' && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) {
        const key = node.arguments[0].text;
        assert.ok(Object.hasOwn(dictionaryFor('en'), key), `${file}: ${key}`);
        assert.equal(translate('de', key), key);
      }
      if (ts.isJsxText(node)) assert.ok(!/[a-zäöüß]/i.test(node.text), `${file}: untranslated JSX text`);
      ts.forEachChild(node, visit);
    };
    visit(tree);
  }
});

test('D12 Uplink renders English labels and interpolated status, with German unchanged', () => {
  const previousWindow = globalThis.window;
  try {
    for (const language of ['en', 'de'] as const) {
      Object.assign(globalThis, { window: { location: { search: `?lang=${language}` } } });
      const html = renderToStaticMarkup(<LanguageProvider>
        <UplinkHochkantEditor quelle={null} ziel={{ breite: 1080, hoehe: 1920 }} standbildUrl={null} stand={null} beschaeftigt={false} onSpeichern={() => {}} onZuruecksetzen={() => {}} />
        <UplinkYouTubeLive verbunden kanalName="Example" einstellungen={null} status={{ zustand: 'live', hinweis: null, broadcastId: 'example', seit: '2026-10-14T12:00:00Z', wiederaufnehmbar: false }} beschaeftigt={false} onSpeichern={() => {}} onBeenden={() => {}} />
      </LanguageProvider>);
      if (language === 'en') {
        for (const text of ['Source with crop', 'Show camera', 'Save as new version', 'Live on YouTube since', 'Broadcast ID:']) assert.ok(html.includes(text), text);
        for (const text of ['Quelle mit Ausschnitt', 'Kamera anzeigen', 'Speichern', 'Ecke', 'Sichtbarkeit']) assert.ok(!html.includes(text), text);
        assert.ok(!/\{\w+\}/.test(html));
      } else {
        for (const text of ['Quelle mit Ausschnitt', 'Kamera anzeigen', 'Als neuen Stand speichern', 'Auf YouTube live seit', 'Broadcast-Kennung:']) assert.ok(html.includes(text), text);
      }
    }
  } finally {
    mutationFailure.message = '';
    Object.assign(globalThis, { window: previousWindow });
  }
});


test('D12 metadata validation and loading states follow the dashboard language', () => {
  const previousWindow = globalThis.window;
  try {
    for (const language of ['en', 'de'] as const) {
      Object.assign(globalThis, { window: { location: { search: `?lang=${language}` } } });
      const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
      const metadata: AiVersionMetadata = {
        video_id: 12,
        editable: true,
        targets: ['youtube', 'tiktok', 'instagram'],
        platforms: {
          youtube: { title: '<invalid>', description: 'a'.repeat(5001), hashtags: ['a'.repeat(101)] },
          tiktok: { title: '', description: 'a'.repeat(2201), hashtags: Array.from({ length: 31 }, () => 'tag') },
          instagram: { title: '', description: '', hashtags: [] },
        },
      };
      client.setQueryData(['social-ai', 'metadata', 'synthetic', 12], metadata);
      const render = (videoId: number) => renderToStaticMarkup(<QueryClientProvider client={client}><LanguageProvider><MetadataPanel channel="synthetic" videoId={videoId} /></LanguageProvider></QueryClientProvider>);
      const html = render(12);
      const expected = language === 'en'
        ? ['Title, description and hashtags', 'Save text', 'Please enter a title.', 'Please use no more than 30 hashtags.', 'Please enter a description or hashtags.', 'YouTube does not allow angle brackets in titles.', 'hashtags together are too long for YouTube', '2,200 characters long on TikTok', '100 characters long.', 'Title 9/100 characters']
        : ['Titel, Beschreibung und Hashtags', 'Texte speichern', 'Bitte gib einen Titel ein.', 'Bitte verwende höchstens 30 Hashtags.', 'Bitte gib eine Beschreibung oder Hashtags ein.', 'YouTube erlaubt im Titel keine spitzen Klammern.', 'Hashtags sind für YouTube zusammen zu lang', '2200 Zeichen lang sein.', '100 Zeichen lang sein.', 'Titel 9/100 Zeichen'];
      for (const text of expected) assert.ok(html.includes(text), text);
      if (language === 'en') assert.ok(!/Bitte|Zeichen|Beschreibung|dürfen|höchstens/.test(html));
      assert.ok(render(13).includes(language === 'en' ? 'Loading titles and descriptions…' : 'Titel und Beschreibungen werden geladen…'));
      assert.equal(translate(language, 'Bitte wähle zuerst einen Kanal.'), language === 'en' ? 'Please select a channel first.' : 'Bitte wähle zuerst einen Kanal.');
      mutationFailure.message = 'Unbekannter deutscher Backendfehler';
      const failed = render(12);
      assert.ok(failed.includes(language === 'en' ? 'The change could not be confirmed.' : mutationFailure.message));
      if (language === 'en') assert.ok(!failed.includes(mutationFailure.message));
      mutationFailure.message = '';
      client.clear();
    }
  } finally {
    mutationFailure.message = '';
    Object.assign(globalThis, { window: previousWindow });
  }
});
