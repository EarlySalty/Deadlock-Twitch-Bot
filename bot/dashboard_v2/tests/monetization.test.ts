import { test } from 'node:test';
import assert from 'node:assert/strict';

import { fmtDrop } from '../src/utils/monetization';

test('fmtDrop stellt positive Verluste und negative Zuwächse korrekt dar', () => {
  assert.equal(fmtDrop(12.34), '−12.3%');
  assert.equal(fmtDrop(-4.56), '+4.6%');
  assert.equal(fmtDrop(0), '0.0%');
  assert.equal(fmtDrop(null), '-');
});
