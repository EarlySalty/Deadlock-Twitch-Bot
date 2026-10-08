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
  limitations: ['Nur synthetische API-Antworten, kein echter Renderlauf oder TikTok-Aufruf.', 'Zustimmung nur im isolierten Formular gesetzt und zurückgesetzt. Kein Absenden, Speichern oder Veröffentlichen.', 'Keine Videodatei geliefert, Medienwiedergabe nicht nachgewiesen.', 'Moli-Layout ist kein Chrome-, Firefox- oder Safari-Kompatibilitätsnachweis.', 'Beobachteter Polling-Lauf mit echten Zeitabständen, keine Suite oder Baseline-Bewertung.'],
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
  await run('window.previewEvidence.setState(null); return true;');
  await until('window.previewEvidence.summary().retry.some(b => !b.disabled) && window.previewEvidence.summary().state === null', 10000);
  const invalidated = await run('return window.previewEvidence.summary();');
  await wait(6500);
  summary = await run('return window.previewEvidence.summary();');
  check('invalidated generation offers enabled retry and stops polling without auto loop', summary.renderPosts === 1 && summary.creatorGets === 0 && summary.statusGets === invalidated.statusGets && summary.retry.some(b => !b.disabled), { invalidated, summary });
  await screenshot('invalidated-retry.png');
  await run('window.previewEvidence.holdNextPost(); const b = Array.from(document.querySelectorAll("dialog button")).find(b => b.textContent === "Vorschau erneut erstellen"); b.click(); b.click(); return true;');
  await until('window.previewEvidence.summary().renderPosts === 2');
  summary = await run('return window.previewEvidence.summary();');
  check('concurrent retry clicks request one replacement generation', summary.renderPosts === 2 && summary.creatorGets === 0, summary);
  await run('window.previewEvidence.releasePost(); return true;');
  await until('window.previewEvidence.summary().text.includes("gleich erstellt")');
  await run('window.previewEvidence.setState("ready"); return true;');
  await until('window.previewEvidence.summary().creatorGets > 0 && document.querySelector("select")', 10000);
  summary = await run('return window.previewEvidence.summary();');
  check('ready exposes creator choices with no preselected privacy or consent', summary.options?.length === 3 && summary.selectedPrivacy === '' && summary.checkboxes?.length >= 5 && summary.checkboxes.every(c => !c.checked) && summary.submitDisabled && summary.confirmations === 0, summary);
  await screenshot('ready.png');
  await run('const s = document.querySelector("select"); s.value = "SELF_ONLY"; s.dispatchEvent(new Event("change", { bubbles: true })); window.previewEvidence.setState(null); window.previewEvidence.setPostState(null); window.previewEvidence.refresh(); return true;');
  await until('window.previewEvidence.summary().retry.some(b => !b.disabled)');
  await run('Array.from(document.querySelectorAll("dialog button")).find(b => b.textContent === "Vorschau erneut erstellen").click(); return true;');
  await until('window.previewEvidence.summary().renderPosts === 3 && window.previewEvidence.summary().retry.some(b => !b.disabled)');
  const stillMissing = await run('return window.previewEvidence.summary();');
  await wait(6500);
  summary = await run('return window.previewEvidence.summary();');
  check('replacement returning null remains explicitly retryable without repeated POST or GET', summary.renderPosts === 3 && summary.statusGets === stillMissing.statusGets && summary.retry.some(b => !b.disabled), { stillMissing, summary });
  await run('window.previewEvidence.setDigest("1".repeat(64)); window.previewEvidence.setState("ready"); window.previewEvidence.refresh(); return true;');
  await until('window.previewEvidence.summary().videoSrc?.includes("1".repeat(64)) && document.querySelector("select")');
  summary = await run('return window.previewEvidence.summary();');
  check('changed video retains valid visibility but requires unchecked consent', summary.selectedPrivacy === 'SELF_ONLY' && summary.checkboxes.every(c => !c.checked) && summary.submitDisabled && summary.confirmations === 0, summary);
  await screenshot('changed-video-ready.png');
  await navigate('missing');
  await until('window.previewEvidence.summary().renderPosts === 1');
  await run('window.previewEvidence.setState("ready"); window.previewEvidence.refresh(); return true;');
  await until('document.querySelector("select") && window.previewEvidence.summary().creatorGets > 0');
  await run('const s = document.querySelector("select"); s.value = "SELF_ONLY"; s.dispatchEvent(new Event("change", { bubbles: true })); return true;');
  await until('window.previewEvidence.summary().selectedPrivacy === "SELF_ONLY"');
  await run('document.querySelectorAll("dialog input[type=checkbox]")[0].click(); return true;');
  await until('window.previewEvidence.summary().checkboxes[0].checked');
  await run('Array.from(document.querySelectorAll("dialog input[type=checkbox]")).at(-1).click(); return true;');
  await until('window.previewEvidence.summary().checkboxes.at(-1).checked && window.previewEvidence.summary().submitDisabled === false');
  const checked = await run('return window.previewEvidence.summary();');
  check('synthetic consent explicitly checked makes valid form eligible without submitting', checked.checkboxes.at(-1).checked && !checked.submitDisabled && checked.confirmations === 0 && checked.selectedPrivacy === 'SELF_ONLY' && checked.checkboxes[0].checked, checked);
  await run('window.previewEvidence.setDigest("2".repeat(64)); window.previewEvidence.setState(null); window.previewEvidence.setPostState("ready"); window.previewEvidence.refresh(); return true;');
  await until('window.previewEvidence.summary().retry.some(b => !b.disabled)');
  await run('Array.from(document.querySelectorAll("dialog button")).find(b => b.textContent === "Vorschau erneut erstellen").click(); return true;');
  await until('window.previewEvidence.summary().renderPosts === 2 && window.previewEvidence.summary().videoSrc?.includes("2".repeat(64)) && document.querySelector("select")');
  await until('window.previewEvidence.summary().checkboxes.at(-1)?.checked === false && window.previewEvidence.summary().submitDisabled === true');
  const reset = await run('return window.previewEvidence.summary();');
  check('retry with replacement video clears checked consent and preserves valid form choices', checked.checkboxes.at(-1).checked && !reset.checkboxes.at(-1).checked && reset.submitDisabled && reset.confirmations === 0 && reset.caption === checked.caption && reset.selectedPrivacy === checked.selectedPrivacy && reset.checkboxes[0].checked && reset.creatorIdentity.approvedVideoSha256 !== checked.creatorIdentity.approvedVideoSha256, { before: checked, summary: reset });
  await screenshot('consent-reset-ready.png');
  await run('Array.from(document.querySelectorAll("dialog input[type=checkbox]")).at(-1).click(); return true;');
  await until('window.previewEvidence.summary().checkboxes.at(-1).checked && window.previewEvidence.summary().submitDisabled === false');
  const digestChecked = await run('return window.previewEvidence.summary();');
  await run('window.previewEvidence.setDigest("3".repeat(64)); window.previewEvidence.refreshCreator(); return true;');
  await until('window.previewEvidence.summary().videoSrc?.includes("3".repeat(64)) && window.previewEvidence.summary().checkboxes.at(-1)?.checked === false && window.previewEvidence.summary().submitDisabled === true');
  const digestReset = await run('return window.previewEvidence.summary();');
  check('creator refresh changing approved video alone clears checked consent without retry', digestChecked.checkboxes.at(-1).checked && !digestReset.checkboxes.at(-1).checked && digestReset.submitDisabled && digestReset.confirmations === 0 && digestReset.renderPosts === 2 && digestReset.selectedPrivacy === checked.selectedPrivacy && digestReset.caption === checked.caption && digestReset.checkboxes[0].checked, { before: digestChecked, summary: digestReset });
  await run('Array.from(document.querySelectorAll("dialog input[type=checkbox]")).at(-1).click(); return true;');
  await until('window.previewEvidence.summary().checkboxes.at(-1).checked && window.previewEvidence.summary().submitDisabled === false');
  const accountChecked = await run('return window.previewEvidence.summary();');
  await run('window.previewEvidence.setAccount(999002, "synthetic-replacement-only"); window.previewEvidence.refreshCreator(); return true;');
  await until('window.previewEvidence.summary().text.includes("Isoliertes Testkonto") && window.previewEvidence.summary().checkboxes.at(-1)?.checked === false && window.previewEvidence.summary().submitDisabled === true');
  const accountReset = await run('return window.previewEvidence.summary();');
  check('account and credential identity replacement clears checked consent with valid choices retained', accountChecked.checkboxes.at(-1).checked && !accountReset.checkboxes.at(-1).checked && accountReset.submitDisabled && accountReset.confirmations === 0 && accountReset.selectedPrivacy === checked.selectedPrivacy && accountReset.caption === checked.caption && accountReset.checkboxes[0].checked && accountReset.creatorIdentity.credentialId !== accountChecked.creatorIdentity.credentialId && accountReset.creatorIdentity.platformUserId !== accountChecked.creatorIdentity.platformUserId && accountReset.creatorIdentity.approvedVideoSha256 === accountChecked.creatorIdentity.approvedVideoSha256, { before: accountChecked, summary: accountReset });
  await screenshot('account-consent-reset-ready.png');
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
  await navigate('missing&hold=1');
  await until('window.previewEvidence.summary().renderPosts === 1');
  await run('document.querySelector("dialog button[aria-label=Schließen]").click(); window.previewEvidence.setState("ready"); return true;');
  await until('!document.querySelector("dialog[open]")');
  await run('Array.from(document.querySelectorAll("main button")).find(b => b.textContent === "Dialog öffnen").click(); return true;');
  await until('document.querySelector("select") && window.previewEvidence.summary().creatorGets > 0');
  const reopened = await run('return window.previewEvidence.summary();');
  await run('window.previewEvidence.releasePost(); return true;');
  await wait(6500);
  summary = await run('return window.previewEvidence.summary();');
  check('late prepare callback cannot overwrite ready data or restart polling after reopening', summary.statusGets === reopened.statusGets && summary.renderPosts === 1 && summary.creatorGets === reopened.creatorGets && summary.selectedPrivacy === '' && summary.checkboxes.every(c => !c.checked) && summary.submitDisabled, { reopened, summary });
  await screenshot('reopened-ready.png');
  const socialMediaPath = path.resolve(root, '../../../bot/dashboard_v2/src/pages/SocialMedia.tsx');
  const socialMediaSource = await fs.readFile(socialMediaPath, 'utf8');
  check('approved review clips retain existing explicit stop wiring, initial review retains approve and skip', socialMediaSource.includes("const canDecide = clip.status !== 'approved' && queueStage(clip) === 'review';") && socialMediaSource.includes(') : stoppbar ? (') && socialMediaSource.includes('onClick={onCancelScheduled}') && socialMediaSource.includes("onApprovalDecision('skip', selectedPlatforms)"), { sourcePath: socialMediaPath, sourceSha256: createHash('sha256').update(socialMediaSource).digest('hex'), kind: 'Source wiring assertion, not browser DOM proof' });
} catch (error) {
  results.failure = String(error);
  console.error(error);
} finally {
  results.componentSha256End = await hashComponent();
  results.componentUnchangedDuringRun = results.componentSha256Start === results.componentSha256End;
  results.exitCode = results.failure || results.cases.some(c => !c.success) || results.cases.length !== 15 || !results.componentUnchangedDuringRun ? 1 : 0;
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
        checkboxes: data.checkboxes, caption: data.caption, creatorIdentity: data.creatorIdentity,
        before: c.details.before ? { checkboxes: c.details.before.checkboxes, submitDisabled: c.details.before.submitDisabled, selectedPrivacy: c.details.before.selectedPrivacy, caption: c.details.before.caption, creatorIdentity: c.details.before.creatorIdentity } : undefined,
        closedStatusGets: c.details.closed?.statusGets,
        retry: c.details.retry ?? data.retry, kind: c.details.kind,
        sourcePath: c.details.sourcePath, sourceSha256: c.details.sourceSha256 };
    }),
    evidencePaths: ['missing-pending.png', 'rendering.png', 'ready.png', 'invalidated-retry.png', 'changed-video-ready.png', 'consent-reset-ready.png', 'account-consent-reset-ready.png', 'reopened-ready.png', 'error.png', 'error-retry-pending.png'].map(name => path.join(root, name)),
  };
  await fs.writeFile(path.join(root, 'results-compact.json'), JSON.stringify(compact, null, 2));
  await request(prefix, undefined, 'DELETE');
}
const passed = results.cases.filter(c => c.success).length;
console.log(JSON.stringify({ passed, failed: results.cases.length - passed, failure: results.failure }));
process.exitCode = results.exitCode;
