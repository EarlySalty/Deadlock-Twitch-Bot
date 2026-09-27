import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import {
  CommandNameApiError,
  fetchCommandNames,
  saveCommandName,
  validCommandNameInput,
  enqueueCommandNameSave,
  type CommandNameSaveQueue,
} from '../src/api/commandNames';
import { CommandNamesProvider, EditableCommandName } from '../src/components/verwaltung/CommandNameSection';

Object.assign(globalThis, { React });
const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

test('API lädt und speichert kanalbezogene Command-Namen mit Sessioncookie', async () => {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    if (init.method === 'POST') {
      const body = JSON.parse(String(init.body));
      return {
        ok: true,
        json: async () => ({
          ok: true,
          command: body.command,
          custom_name: '!dachraid',
          effective_name: '!dachraid',
          effective_aliases: [],
        }),
      };
    }
    return { ok: true, json: async () => ({ commands: [{ command: 'raid', effective_name: '!dachraid' }] }) };
  }) as typeof fetch;

  const loaded = await fetchCommandNames();
  assert.equal(loaded.commands[0].effective_name, '!dachraid');

  const saved = await saveCommandName('raid', 'DACHRAID');
  assert.equal(saved.command, 'raid');

  for (const call of calls) {
    assert.equal(call.url, '/twitch/api/v2/streamer/command-names');
    assert.equal(call.init.credentials, 'same-origin');
  }
  assert.deepEqual(JSON.parse(String(calls.at(-1)?.init.body)), {
    command: 'raid',
    name: 'DACHRAID',
  });
  assert.equal(calls.at(-1)?.init.keepalive, true);
});

test('API reicht verständlichen Konfliktfehler durch', async () => {
  globalThis.fetch = (async () => ({
    ok: false,
    status: 409,
    json: async () => ({
      error: 'name_conflict',
      message: '!ping wird in deinem Kanal bereits für !ping verwendet.',
    }),
  })) as typeof fetch;

  await assert.rejects(
    saveCommandName('raid', '!ping'),
    (error: unknown) => error instanceof CommandNameApiError
      && error.status === 409
      && error.code === 'name_conflict'
      && error.message.includes('!ping'),
  );
});

test('Eingabe akzeptiert nur Buchstaben und Zahlen ohne zweites Präfix', () => {
  assert.equal(validCommandNameInput('DACHraid7'), true);
  for (const invalid of ['!!raid', '#raid', 'raid_test', 'raid-test', 'räid']) {
    assert.equal(validCommandNameInput(invalid), false);
  }
});

test('Inline-Editor zeigt während des Ladens keinen zweiten Einstellungsdialog', () => {
  const html = renderToStaticMarkup(
    <CommandNamesProvider><EditableCommandName command="connect" /></CommandNamesProvider>,
  );
  assert.ok(html.includes('Befehl wird geladen'));
  assert.ok(!html.includes('Speichern</button>'));
});

test('laufender Save und Rückkehr zum alten Namen werden in SQL-Reihenfolge geschrieben', async () => {
  let releaseFirst: (() => void) | undefined;
  const firstFinished = new Promise<void>(resolve => { releaseFirst = resolve; });
  let markStarted: (() => void) | undefined;
  const firstStarted = new Promise<void>(resolve => { markStarted = resolve; });
  const writes: string[] = [];
  const queue: CommandNameSaveQueue = {
    savedName: 'rank', lastAttempt: null, queued: 0, tail: Promise.resolve(),
  };
  const save = async (name: string) => {
    writes.push(name);
    if (name === 'abc') {
      markStarted?.();
      await firstFinished;
    }
  };
  const first = enqueueCommandNameSave(queue, 'abc', save);
  assert.ok(first);
  await firstStarted;
  assert.deepEqual(writes, ['abc']);
  queue.lastAttempt = null; // neue Eingabe während der erste Request noch läuft
  const second = enqueueCommandNameSave(queue, 'rank', save);
  assert.ok(second);
  assert.deepEqual(writes, ['abc']);
  releaseFirst?.();
  await second;
  assert.deepEqual(writes, ['abc', 'rank']);
  assert.equal(queue.savedName, 'rank');
  assert.equal(queue.queued, 0);
});
