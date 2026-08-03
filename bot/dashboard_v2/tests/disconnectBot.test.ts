import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import {
  disconnectBot,
  roleLabel,
  roleNeedsAttention,
  unmodNeedsAttention,
} from '../src/api/disconnectBot';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

function mockFetch(status: number, body: unknown) {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
    };
  }) as unknown as typeof fetch;
  return calls;
}

test('schickt nur die Bestätigung, den Login bestimmt die Session', async () => {
  const calls = mockFetch(200, {
    ok: true,
    login: 'earlysalty',
    unmod: 'removed',
    departnered: true,
    opt_out: true,
    message: 'ok',
  });
  await disconnectBot('earlysalty');
  assert.equal(calls[0].url, '/twitch/api/v2/streamer/disconnect-bot');
  assert.equal(calls[0].init.method, 'POST');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), { confirm_login: 'earlysalty' });
  // Ohne Cookie käme die Session nicht mit und der Server wüsste nicht, wer trennt.
  assert.equal((calls[0].init as { credentials?: string }).credentials, 'same-origin');
});

test('Fehlerantwort wird zur Exception mit der Server-Meldung', async () => {
  mockFetch(400, {
    error: 'confirmation_mismatch',
    message: 'Bestätigung stimmt nicht mit deinem Kanalnamen überein — nichts geändert.',
  });
  await assert.rejects(
    () => disconnectBot('falsch'),
    /Bestätigung stimmt nicht/,
    'Eine abgelehnte Trennung darf nicht als Erfolg durchgehen',
  );
});

// Der Teilschritt-Report ist der Grund, warum die Aktion überhaupt einzeln
// meldet: ein fehlgeschlagener Unmod ist KEIN sauberer Lauf, auch wenn die
// Partnerschaft beendet wurde.
test('nur removed und not_moderator gelten als erledigt', () => {
  assert.equal(unmodNeedsAttention('removed'), false);
  assert.equal(unmodNeedsAttention('not_moderator'), false);
  for (const outcome of ['no_token', 'unknown_channel', 'unavailable', 'failed', '']) {
    assert.equal(unmodNeedsAttention(outcome), true, `${outcome} muss auffallen`);
  }
});

// Der Bug vom 2026-08-03: der Server meldete "revoked", obwohl der Entzug
// mangels Guild-Kandidat übersprungen wurde. Ein übersprungener Entzug ist
// nur dann harmlos, wenn es gar keine Discord-Verknüpfung gibt.
test('übersprungener Rollen-Entzug gilt nicht als erledigt', () => {
  assert.equal(roleNeedsAttention('revoked'), false);
  assert.equal(roleNeedsAttention('skipped:no_discord_link'), false);
  for (const role of [
    'skipped:no_guild',
    'skipped:no_relay',
    'skipped:invalid_user_id',
    'skipped:not_implemented',
    'failed:502 Bad Gateway',
    'irgendwas',
  ]) {
    assert.equal(roleNeedsAttention(role), true, `${role} muss auffallen`);
  }
});

// Ein fehlendes Feld ist ein ungeklärter Ausgang, kein Erfolg — sonst sieht
// eine alte Server-Version wie ein sauberer Lauf aus.
test('fehlender Rollen-Ausgang gilt als offen', () => {
  assert.equal(roleNeedsAttention(undefined), true);
  assert.match(roleLabel(undefined), /ACHTUNG/);
});

test('Rollen-Klartext nennt den Grund statt ihn zu schlucken', () => {
  assert.match(roleLabel('skipped:no_guild'), /ACHTUNG.*no_guild/);
  assert.match(roleLabel('failed:502'), /ACHTUNG.*502/);
  assert.equal(roleLabel('revoked'), 'entzogen');
  assert.match(roleLabel('skipped:no_discord_link'), /kein Discord-Account/);
});
