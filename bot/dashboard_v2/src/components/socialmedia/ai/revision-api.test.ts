import { test } from 'node:test';
import assert from 'node:assert/strict';
import { aiPreviewJob } from './preview';

Object.assign(globalThis,{window:{location:{pathname:'/'}}});
const { decodeAiJob, reviseAiVersion, decideAiProposal, approveAiVersion, scheduleAiVersion } = await import('./api');

function mock(body: unknown, inspect: (url: string, init: RequestInit) => void = () => {}) {
  const previous = globalThis.fetch;
  globalThis.fetch = async (input,init = {}) => {inspect(String(input),init);return Response.json(body);};
  return () => {globalThis.fetch = previous;};
}

test('revision fields and confirmations decode without trusting incomplete versions', () => {
  const base = aiPreviewJob('revisions');
  assert.ok(base);
  assert.equal(decodeAiJob(base).versions[2].parent_version_id,2201);
  assert.throws(() => decodeAiJob({...base,root_job_id:0}));
  assert.throws(() => decodeAiJob({...base,revision_proposal:{reanalyze:true}}));
  assert.throws(() => decodeAiJob({...base,versions:[{...base.versions[0],approved_platforms:['youtube','youtube']}]}));
  assert.throws(() => decodeAiJob({...base,versions:[{...base.versions[0],original_url:'https://untrusted.example/video'}]}));
});

test('revision and proposal requests are explicit scoped writes with exact parent identity', async () => {
  const base = aiPreviewJob('revisions');
  assert.ok(base);
  const calls: Array<{url: string; init: RequestInit}> = [];
  const child = {...base,id:1104,parent_version_id:2203,status:'awaiting_confirmation',stage:'planning'};
  let restore = mock({job:child},(url,init) => calls.push({url,init}));
  try {
    assert.equal((await reviseAiVersion('123',1101,2203,'Kürzer')).id,1104);
    assert.equal(calls[0].url,'/twitch/social-media/api/ai/jobs/1101/versions/2203/revision');
    assert.equal(calls[0].init.method,'POST');
    assert.equal(calls[0].init.credentials,'same-origin');
    assert.deepEqual(JSON.parse(String(calls[0].init.body)),{feedback:'Kürzer',twitch_user_id:'123'});
  } finally {restore();}
  restore = mock({job:{...child,parent_version_id:2201}});
  try {await assert.rejects(reviseAiVersion('123',1101,2203,'Kürzer'));} finally {restore();}
  restore = mock({job:child},(_url,init) => assert.deepEqual(JSON.parse(String(init.body)),{twitch_user_id:'123',accept:true}));
  try {assert.equal((await decideAiProposal('123',1104,true)).id,1104);} finally {restore();}
});

test('platform approval and exact-file scheduling reject incomplete reservations', async () => {
  const base = aiPreviewJob('revisions');
  assert.ok(base);
  let restore = mock({job:base},(url,init) => {
    assert.equal(url,'/twitch/social-media/api/ai/jobs/1101/versions/2203/approve');
    assert.deepEqual(JSON.parse(String(init.body)),{platforms:['youtube'],twitch_user_id:'123'});
  });
  try {await approveAiVersion('123',1101,2203,['youtube']);} finally {restore();}
  const reservation = {produced_video_id:2203,scheduled_at:'2026-10-12T18:00:00Z',queue_ids:[['youtube',3301]]};
  restore = mock({job:base,reservation});
  try {assert.equal((await scheduleAiVersion('123',1101,2203,['youtube'])).scheduledAt,reservation.scheduled_at);} finally {restore();}
  for (const invalid of [
    {...reservation,produced_video_id:2201},
    {...reservation,queue_ids:[['tiktok',3301]]},
    {...reservation,queue_ids:[['youtube',3301],['youtube',3302]]},
  ]) {
    restore = mock({job:base,reservation:invalid});
    try {await assert.rejects(scheduleAiVersion('123',1101,2203,['youtube']));} finally {restore();}
  }
});
