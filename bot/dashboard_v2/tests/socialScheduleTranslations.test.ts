import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';
import { dictionaryFor, translate } from '../src/i18n/dictionary';
import { PLAN_EN, planText } from '../src/components/socialmedia/pipelinePlan';

const source = readFileSync(new URL('../src/components/socialmedia/PipelinePlanControls.tsx', import.meta.url), 'utf8');
const tree = ts.createSourceFile('PipelinePlanControls.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const logic = ts.createSourceFile('pipelinePlan.ts', readFileSync(new URL('../src/components/socialmedia/pipelinePlan.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const panel = readFileSync(new URL('../src/components/socialmedia/autopilot/schedule-queue/ScheduleQueuePanel.tsx', import.meta.url), 'utf8');

function descendants(node: ts.Node): ts.Node[] {
  const nodes = [node];
  ts.forEachChild(node, child => { nodes.push(...descendants(child)); });
  return nodes;
}

const nodes = descendants(tree);
const placeholders = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
const logicNodes = descendants(logic);
const planDeclaration = logicNodes.find(node => ts.isVariableDeclaration(node) && node.name.getText(logic) === 'PLAN_EN');
const englishValues = new Set(planDeclaration ? descendants(planDeclaration).filter(ts.isPropertyAssignment).map(node => node.initializer) : []);
const untranslated = new Set(['YouTube', 'TikTok', 'Instagram', 'Hashtags', 'Server-Fehler', 'zwischenzeitlich geändert']);

test('plan editor text has an English entry with matching placeholders', () => {
  const keys = [...nodes, ...logicNodes].filter(ts.isStringLiteral).filter(node => !englishValues.has(node)).map(node => node.text)
    .filter(text => /[äöüß]|^[A-Z][a-z]|\{\w+\}/.test(text) && !text.includes('/') && !untranslated.has(text));
  assert.ok(planDeclaration);
  assert.ok(keys.length > 60);
  const english = planText('en');
  const german = planText('de');
  for (const key of keys) {
    assert.ok(Object.hasOwn(PLAN_EN, key), key);
    assert.equal(german(key), key);
    assert.notEqual(english(key), key);
    assert.deepEqual(placeholders(PLAN_EN[key]), placeholders(key), key);
  }
  for (const day of ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag']) assert.notEqual(english(day), day);
  assert.equal(english('{day} {time} ist doppelt.', { day: 'Montag', time: '08:00' }), 'Montag 08:00 is listed twice.');
});

test('schedule panel mounts only the plan editor without the old queue card', () => {
  assert.ok(panel.includes('<PipelinePlanControls'));
  for (const legacy of ['Produktionswarteschlange', 'Als Nächstes', 'Auftrag entfernen', 'QueueCard', 'modeSlot']) assert.ok(!panel.includes(legacy), legacy);
});

test('production queue failure messages are covered by the selected-language dictionary', () => {
  const dictionary = dictionaryFor('en');
  const messages = ['runner.rs', 'recovery.rs'].flatMap(file => {
    const producer = readFileSync(new URL(`../../../rust/crates/tb-social-media/src/autopilot_schedule/${file}`, import.meta.url), 'utf8');
    const rustMessages = [...producer.matchAll(/(?:#\[error\(|Some\()"([^"\n]+)"/g)].map(match => match[1]);
    const sqlMessages = [...producer.matchAll(/last_error\s*=\s*'((?:[^']|'')+)'/g)].map(match => match[1].replaceAll("''", "'"));
    assert.ok(rustMessages.length > 0, file);
    if (file === 'recovery.rs') assert.equal(sqlMessages.length, 3);
    return [...rustMessages, ...sqlMessages];
  });
  assert.equal(messages.length, 12);
  for (const message of messages) {
    assert.ok(Object.hasOwn(dictionary, message), message);
    assert.notEqual(translate('en', message), message);
    assert.equal(translate('de', message), message);
  }
});

test('plan timestamps use the selected locale rather than a fixed German locale', () => {
  const formatters = nodes.filter(ts.isNewExpression).filter(node => node.expression.getText(tree) === 'Intl.DateTimeFormat');
  assert.equal(formatters.length, 1);
  assert.equal(formatters[0].arguments?.[0].getText(tree), 'locale');
  const calls = nodes.filter(ts.isCallExpression).filter(node => node.expression.getText(tree) === 'slotLabel');
  assert.equal(calls.length, 3);
  assert.ok(calls.every(call => call.arguments[2].getText(tree) === 'locale'));
  const rawText = nodes.filter(ts.isJsxText).map(node => node.text.trim()).filter(Boolean);
  assert.deepEqual(rawText, []);
});
