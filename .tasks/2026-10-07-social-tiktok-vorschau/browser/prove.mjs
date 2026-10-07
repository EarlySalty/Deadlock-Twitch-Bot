import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
const root = path.dirname(fileURLToPath(import.meta.url));
const base = 'http://127.0.0.1:19328';
async function request(route, body, method = body ? 'POST' : 'GET') {
  const res = await fetch(`${base}${route}`, { method, headers: { 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  const data = await res.json();
  if (!res.ok || data.value?.error) throw new Error(JSON.stringify(data));
  return data.value;
}
const session = await request('/session', { capabilities: { alwaysMatch: { browserName: 'moli' } } });
const id = session.sessionId;
const prefix = `/session/${id}`;
const run = script => request(`${prefix}/execute/sync`, { script, args: [] });
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(script, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (await run(`return !!(${script});`)) return;
    await wait(150);
  }
  throw new Error(`Readiness missing: ${script}; ${JSON.stringify(await run('return document.body.innerText;'))}`);
}
async function navigate(state) {
  await request(`${prefix}/url`, { url: `http://127.0.0.1:19327/?state=${state}` });
  await until('window.previewEvidence && document.querySelector("dialog[open]")');
  await until('window.previewEvidence.summary().statusGets > 0');
}
async function screenshot(name) {
  const image = await request(`${prefix}/screenshot`);
  await fs.writeFile(path.join(root, name), Buffer.from(image, 'base64'));
}
const componentPath = path.resolve(root, '../../../bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx');
const hashComponent = async () => createHash('sha256').update(await fs.readFile(componentPath)).digest('hex');
const results = {
  asOf: new Date().toISOString(),
  browser: 'moli 1.1.14 --layout --image --font',
  synthetic: true,
  command: `node ${fileURLToPath(import.meta.url)}`,
  componentPath,
  componentSha256Start: await hashComponent(),
  limitations: ['Nur synthetische API-Antworten, kein echter Renderlauf oder TikTok-Aufruf.', 'Keine Zustimmung und keine Veröffentlichung ausgeführt.', 'Keine Videodatei geliefert, Medienwiedergabe nicht nachgewiesen.', 'Moli-Layout ist kein Chrome-, Firefox- oder Safari-Kompatibilitätsnachweis.', 'Beobachteter Polling-Lauf mit echten Zeitabständen, keine Suite oder Baseline-Bewertung.'],
  cases: [],
};
function check(name, success, details) {
  results.cases.push({ name, success: !!success, details });
  console.log(`${success ? 'PASS' : 'FAIL'} ${name}`);
}
try {
  await request(`${prefix}/window/rect`, { width: 1280, height: 1000 });
  await navigate('missing');
  await until('window.previewEvidence.summary().renderPosts === 1');
  let summary = await run('return window.previewEvidence.summary();');
  check('missing requests preview exactly once, creator blocked', summary.renderPosts === 1 && summary.creatorGets === 0, summary);
  await screenshot('missing-pending.png');
  await run('window.previewEvidence.setState("rendering"); return true;');
  const before = summary.statusGets;
  await until(`window.previewEvidence.summary().statusGets >= ${before + 2}`, 13000);
  summary = await run('return window.previewEvidence.summary();');
  check('rendering polls without another preview request', summary.renderPosts === 1 && summary.creatorGets === 0 && summary.statusGets >= before + 2, summary);
  await screenshot('rendering.png');
  await run('window.previewEvidence.setState("ready"); return true;');
  await until('window.previewEvidence.summary().creatorGets > 0 && document.querySelector("select")', 10000);
  summary = await run('return window.previewEvidence.summary();');
  check('ready exposes creator choices with no preselected privacy or consent', summary.options?.length === 3 && summary.selectedPrivacy === '' && summary.checkboxes?.length >= 5 && summary.checkboxes.every(c => !c.checked) && summary.submitDisabled && summary.confirmations === 0, summary);
  await screenshot('ready.png');
  await navigate('error');
  await until('document.querySelector("dialog [role=alert]") || document.querySelector("dialog button[data-retry]")');
  await screenshot('error.png');
  const retry = await run('const buttons = Array.from(document.querySelectorAll("dialog button")); const candidate = buttons.find(b => b.type === "button" && b.getAttribute("aria-label") !== "Schließen" && b.textContent !== "Abbrechen"); if (!candidate) return null; const result = { text: candidate.textContent, disabled: candidate.disabled }; candidate.click(); return result;');
  if (retry) await until('window.previewEvidence.summary().renderPosts === 1');
  summary = await run('return window.previewEvidence.summary();');
  check('error retry performs new render request', retry && !retry.disabled && summary.renderPosts === 1 && summary.creatorGets === 0, { retry, summary });
  await screenshot('error-retry-pending.png');
  await run('window.previewEvidence.setState("rendering"); return true;');
  await until('window.previewEvidence.summary().statusGets >= 3', 10000);
  await run('document.querySelector("dialog button[aria-label=Schließen]").click(); return true;');
  await until('!document.querySelector("dialog[open]")');
  const closed = await run('return window.previewEvidence.summary();');
  await wait(6500);
  summary = await run('return window.previewEvidence.summary();');
  check('closing stops polling across two intervals', !summary.dialogOpen && summary.statusGets === closed.statusGets && summary.renderPosts === closed.renderPosts && summary.confirmations === 0, { closed, later: summary });
} catch (error) {
  results.failure = String(error);
  console.error(error);
} finally {
  results.componentSha256End = await hashComponent();
  results.componentUnchangedDuringRun = results.componentSha256Start === results.componentSha256End;
  results.exitCode = results.failure || results.cases.some(c => !c.success) || results.cases.length !== 5 || !results.componentUnchangedDuringRun ? 1 : 0;
  await fs.writeFile(path.join(root, 'results.json'), JSON.stringify(results, null, 2));
  const compact = {
    asOf: results.asOf, command: results.command, browser: results.browser,
    componentPath, componentSha256: results.componentSha256End,
    componentUnchangedDuringRun: results.componentUnchangedDuringRun,
    exitCode: results.exitCode, failure: results.failure,
    passed: results.cases.filter(c => c.success).length,
    failed: results.cases.filter(c => !c.success).length,
    ignored: 0, baseline: 'nicht gemessen', limitations: results.limitations,
    cases: results.cases.map(c => {
      const data = c.details.summary ?? c.details.later ?? c.details;
      return { name: c.name, success: c.success, state: data.state,
        statusGets: data.statusGets, renderPosts: data.renderPosts,
        creatorGets: data.creatorGets, confirmations: data.confirmations,
        selectedPrivacy: data.selectedPrivacy, submitDisabled: data.submitDisabled,
        checkboxes: data.checkboxes,
        closedStatusGets: c.details.closed?.statusGets,
        retry: c.details.retry };
    }),
    evidencePaths: ['missing-pending.png', 'rendering.png', 'ready.png', 'error.png', 'error-retry-pending.png'].map(name => path.join(root, name)),
  };
  await fs.writeFile(path.join(root, 'results-compact.json'), JSON.stringify(compact, null, 2));
  await request(prefix, undefined, 'DELETE');
}
const passed = results.cases.filter(c => c.success).length;
console.log(JSON.stringify({ passed, failed: results.cases.length - passed, failure: results.failure }));
process.exitCode = results.exitCode;
