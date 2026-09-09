import assert from 'node:assert/strict';
import React from 'react';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { SubReminderSection } from '../src/components/verwaltung/SubReminderSection';
import { subReminderSettings } from '../src/api/subReminder';
(globalThis as typeof globalThis & { React: typeof React }).React = React;

test('Subkarte nennt öffentliche eigene Zustimmung und jederzeitiges Abbestellen', () => {
  const html=renderToStaticMarkup(<SubReminderSection />);
  assert.match(html,/!sub erinnerung an/);
  assert.match(html,/!sub erinnerung aus/);
  assert.match(html,/öffentlich im Chat/);
  assert.match(html,/zunächst aus/);
  assert.doesNotMatch(html,/Prime|30 Tage|läuft morgen/);
});

test('GET aktiviert nichts, POST sendet ausschließlich expliziten Schalter',async()=>{
  const old=globalThis.fetch;
  const calls:RequestInit[]=[];
  globalThis.fetch=async(_input,init)=>{calls.push(init??{});return new Response(JSON.stringify({enabled:false,available:false}));};
  try {
    assert.deepEqual(await subReminderSettings(),{enabled:false,available:false});
    assert.equal(calls[0].method,undefined);
    await subReminderSettings(true);
    assert.equal(calls[1].body,JSON.stringify({enabled:true}));
    assert.equal(calls[1].credentials,'same-origin');
  } finally {globalThis.fetch=old;}
});
