import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { StatCommandRows } from '../src/components/verwaltung/StatCommandSection';
import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { STAT_COMMANDS, fetchStatCommandSettings, toggleStatCommand } from '../src/api/statCommands';

// Die Node-Testkonfiguration nutzt den klassischen JSX-Transform.
Object.assign(globalThis, { React });
const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

test('acht getrennte Statistikschalter, keine geschützten Befehle', () => {
  assert.deepEqual(STAT_COMMANDS.map(entry => entry.command), ['rank', 'wins', 'winrate', 'mmr', 'live', 'lastmatch', 'streak', 'mostplayed']);
  for (const alias of ['!climb', '!last', '!main']) assert.ok(STAT_COMMANDS.some(entry => entry.description.includes(alias)));
});

test('lädt acht Schalter mit Sessioncookie und schreibt nur den geänderten Schlüssel', async () => {
  const calls: Array<{url: string; init: RequestInit}> = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({url, init});
    return { ok: true, json: async () => init.method === 'POST' ? { ok: true, ...JSON.parse(String(init.body)) } : { commands: Object.fromEntries(STAT_COMMANDS.map(({command}) => [command, true])) } };
  }) as typeof fetch;
  const data = await fetchStatCommandSettings();
  assert.equal(Object.keys(data.commands).length, 8);
  for (const {command} of STAT_COMMANDS) {
    const saved = await toggleStatCommand(command, false);
    assert.deepEqual(saved, {ok: true, command, enabled: false});
    assert.deepEqual(JSON.parse(String(calls.at(-1)?.init.body)), {command, enabled: false});
  }
  for (const {url, init} of calls) {
    assert.equal(url, '/twitch/api/v2/streamer/stat-command-settings');
    assert.equal(init.credentials, 'same-origin');
  }
});

test('Fehler beim Laden oder Speichern werden nicht als Erfolg behandelt', async () => {
  globalThis.fetch = (async () => ({ok: false, status: 500})) as typeof fetch;
  await assert.rejects(fetchStatCommandSettings(), /500/);
  await assert.rejects(toggleStatCommand('rank', false), /500/);
});


test('zeigt acht unabhängige Schalter und sperrt nur die gerade gespeicherte Reihe', () => {
  const commands = Object.fromEntries(STAT_COMMANDS.map(({command}) => [command, command !== 'rank'])) as import('../src/api/statCommands').StatCommandSettings;
  const html = renderToStaticMarkup(<StatCommandRows settings={commands} pending={{rank:true}} messages={{rank:'Gespeichert.'}} onToggle={() => {}} />);
  assert.equal((html.match(/<button /g) ?? []).length, 8);
  assert.equal((html.match(/disabled=""/g) ?? []).length, 1);
  assert.ok(html.includes('aria-label="!rank aktivieren"'));
  assert.ok(html.includes('aria-label="!wins deaktivieren"'));
  assert.ok(html.includes('role="status"'));
  for (const alias of ['!climb', '!last', '!main']) assert.ok(html.includes(alias));
});
