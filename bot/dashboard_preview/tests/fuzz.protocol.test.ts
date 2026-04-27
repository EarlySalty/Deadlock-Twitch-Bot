import assert from 'node:assert/strict';
import test from 'node:test';

import fc from 'fast-check';

import { formatPercent, normalizeHourlyActivity } from '../src/utils/engagementKpi';

const requestedRuns = Number.parseInt(process.env.FUZZ_NUM_RUNS ?? '500', 10);
const numRuns = Number.isFinite(requestedRuns) && requestedRuns > 0 ? requestedRuns : 500;

test('normalizeHourlyActivity keeps a safe 24-slot histogram for arbitrary numeric inputs', () => {
  const numeric = fc.oneof(
    fc.integer({ min: -10000, max: 10000 }),
    fc.double({ noNaN: true, noDefaultInfinity: true })
  );

  const rowArb = fc.record({
    hour: numeric,
    count: numeric,
  });

  fc.assert(
    fc.property(fc.array(rowArb, { maxLength: 500 }), (rows) => {
      const normalized = normalizeHourlyActivity(rows);

      assert.equal(normalized.length, 24);
      for (let hour = 0; hour < normalized.length; hour += 1) {
        const entry = normalized[hour];
        assert.equal(entry.hour, hour);
        assert.equal(Number.isInteger(entry.count), true);
        assert.equal(entry.count >= 0, true);
      }
    }),
    { numRuns }
  );
});

test('formatPercent never throws and always returns a string for arbitrary finite numbers', () => {
  const valueArb = fc.oneof(
    fc.constant(null),
    fc.double({ noNaN: true, noDefaultInfinity: true })
  );

  fc.assert(
    fc.property(valueArb, fc.integer({ min: 0, max: 8 }), (value, digits) => {
      const formatted = formatPercent(value, digits);
      assert.equal(typeof formatted, 'string');
      if (value === null) {
        assert.equal(formatted, '-');
      }
    }),
    { numRuns }
  );
});
