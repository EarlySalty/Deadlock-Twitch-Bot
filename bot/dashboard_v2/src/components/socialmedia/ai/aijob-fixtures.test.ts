import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

Object.assign(globalThis,{window:{location:{pathname:'/'}}});
const { decodeAiJob } = await import('./api');

const STATUSES = ['queued','preparing','transcribing','analyzing','planning','rendering','validating','review','failed','awaiting_confirmation','rejected'];
const directory = fileURLToPath(new URL('./fixtures/', import.meta.url));
const fixtures = readdirSync(directory).filter((name) => name.endsWith('.json')).sort();

test('backend fixtures cover every job status the dashboard knows', () => {
  assert.deepEqual(fixtures,STATUSES.map((status) => `${status}.json`).sort());
});

for (const name of fixtures) {
  test(`backend fixture ${name} decodes with decodeAiJob`, () => {
    const job = decodeAiJob(JSON.parse(readFileSync(directory + name,'utf8')));
    assert.equal(`${job.status}.json`,name);
    if (job.revision_proposal) assert.ok(job.revision_proposal.impact.trim());
  });
}
