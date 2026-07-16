import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { parseCommissionRate } from './commissionRate.ts';

describe('parseCommissionRate', () => {
  it('accepts integer rates from 0 through 100', () => {
    assert.equal(parseCommissionRate('0'), 0);
    assert.equal(parseCommissionRate('42'), 42);
    assert.equal(parseCommissionRate('100'), 100);
  });

  it('rejects empty, non-integer, and out-of-range rates', () => {
    for (const value of ['', ' ', '1.5', '-1', '101', 'not a number']) {
      assert.equal(parseCommissionRate(value), null);
    }
  });
});
