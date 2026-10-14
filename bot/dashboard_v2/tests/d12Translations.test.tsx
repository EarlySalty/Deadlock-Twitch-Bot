import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';
import { LanguageProvider } from '../src/context/LanguageContext';
import { dictionaryFor, translate } from '../src/i18n/dictionary';
import { UplinkHochkantEditor } from '../src/components/uplink/UplinkHochkantEditor';
import { UplinkYouTubeLive } from '../src/components/uplink/UplinkYouTubeLive';

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
    Object.assign(globalThis, { window: previousWindow });
  }
});
