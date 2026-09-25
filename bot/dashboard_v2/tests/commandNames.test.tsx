import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import {
  CommandNameApiError,
  fetchCommandNames,
  saveCommandName,
  type CommandNameSetting,
} from '../src/api/commandNames';
import { CommandNameRow } from '../src/components/verwaltung/CommandNameSection';

Object.assign(globalThis, { React });
const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

const raid: CommandNameSetting = {
  command: 'raid',
  default_name: '!raid',
  default_aliases: ['!traid'],
  custom_name: '!dachraid',
  effective_name: '!dachraid',
  effective_aliases: [],
  group: 'mod',
  group_label: 'Moderation',
  summary: 'Startet einen Raid.',
};

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
    return { ok: true, json: async () => ({ commands: [raid] }) };
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

test('Zeile zeigt Standard, aktiven eigenen Namen und Reset getrennt', () => {
  const html = renderToStaticMarkup(
    <CommandNameRow
      row={raid}
      draft="!dachraid"
      pending={false}
      message="Gespeichert."
      onDraft={() => {}}
      onSave={() => {}}
      onReset={() => {}}
    />,
  );
  assert.ok(html.includes('!raid'));
  assert.ok(html.includes('!traid'));
  assert.ok(html.includes('aktiv als !dachraid'));
  assert.ok(html.includes('aria-label="!raid eigener Name"'));
  assert.ok(html.includes('Zurücksetzen'));
  assert.ok(html.includes('role="status"'));
});

test('unveränderte Standardzeile hat keinen Reset', () => {
  const standard = { ...raid, custom_name: null, effective_name: '!raid', effective_aliases: ['!traid'] };
  const html = renderToStaticMarkup(
    <CommandNameRow
      row={standard}
      draft=""
      pending={false}
      message=""
      onDraft={() => {}}
      onSave={() => {}}
      onReset={() => {}}
    />,
  );
  assert.ok(!html.includes('Zurücksetzen'));
  assert.ok(html.includes('disabled=""'));
});
