import { test } from 'node:test';
import assert from 'node:assert/strict';

import { sollTrialEndeZeigen } from '../src/components/modals/trialEnde';

const abgelaufen = '2026-08-01T00:00:00Z';

test('ohne Ablaufdatum bleibt das Fenster zu', () => {
  assert.equal(
    sollTrialEndeZeigen({ trialEndedAt: null, hasFullAccess: false, tier: 'free', gesehen: false }),
    false,
  );
});

test('nach Ablauf geht es genau einmal auf', () => {
  assert.equal(
    sollTrialEndeZeigen({ trialEndedAt: abgelaufen, hasFullAccess: false, tier: 'free', gesehen: false }),
    true,
  );
  assert.equal(
    sollTrialEndeZeigen({ trialEndedAt: abgelaufen, hasFullAccess: false, tier: 'free', gesehen: true }),
    false,
  );
});

test('wer wieder Zugriff hat, sieht es nicht', () => {
  assert.equal(
    sollTrialEndeZeigen({ trialEndedAt: abgelaufen, hasFullAccess: true, tier: 'free', gesehen: false }),
    false,
  );
  assert.equal(
    sollTrialEndeZeigen({ trialEndedAt: abgelaufen, hasFullAccess: false, tier: 'extended', gesehen: false }),
    false,
  );
});
