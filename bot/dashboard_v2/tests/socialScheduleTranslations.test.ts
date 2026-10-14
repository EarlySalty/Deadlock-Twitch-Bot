import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';
import { dictionaryFor, translate } from '../src/i18n/dictionary';

const source = readFileSync(new URL('../src/components/socialmedia/autopilot/schedule-queue/ScheduleQueuePanel.tsx', import.meta.url), 'utf8');
const tree = ts.createSourceFile('ScheduleQueuePanel.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

function descendants(node: ts.Node): ts.Node[] {
  const nodes = [node];
  ts.forEachChild(node, child => { nodes.push(...descendants(child)); });
  return nodes;
}

const nodes = descendants(tree);

test('schedule presentation keys cover dynamic status, holds, failures and accessibility labels', () => {
  const dictionary = dictionaryFor('en');
  const keys = nodes.filter(ts.isStringLiteral).filter(node => /[äöüß]|^[A-Z][a-z]|\{(?:number|title)\}/.test(node.text) && !node.text.includes('/') && node.text !== 'Server-Fehler');
  assert.ok(keys.length > 60);
  for (const key of keys) {
    assert.ok(Object.hasOwn(dictionary, key.text), key.text);
    assert.equal(translate('de', key.text), key.text);
    const placeholders = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
    assert.deepEqual(placeholders(dictionary[key.text]), placeholders(key.text));
  }
  const weekdayDeclaration = nodes.find(node => ts.isVariableDeclaration(node) && node.name.getText(tree) === 'WEEKDAYS');
  assert.ok(weekdayDeclaration && ts.isVariableDeclaration(weekdayDeclaration) && weekdayDeclaration.initializer && ts.isArrayLiteralExpression(weekdayDeclaration.initializer));
  assert.equal(weekdayDeclaration.initializer.elements.length, 7);
  for (const day of weekdayDeclaration.initializer.elements) {
    assert.ok(ts.isStringLiteral(day));
    assert.notEqual(translate('en', day.text), day.text);
  }
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

test('schedule timestamps use the selected locale rather than a fixed German locale', () => {
  const formatters = nodes.filter(ts.isNewExpression).filter(node => node.expression.getText(tree) === 'Intl.DateTimeFormat');
  assert.equal(formatters.length, 1);
  assert.equal(formatters[0].arguments?.[0].getText(tree), 'locale');
  const calls = nodes.filter(ts.isCallExpression).filter(node => node.expression.getText(tree) === 'slotLabel');
  assert.equal(calls.length, 2);
  assert.ok(calls.every(call => call.arguments[2].getText(tree) === 'locale'));
  const rawText = nodes.filter(ts.isJsxText).map(node => node.text.trim()).filter(Boolean);
  assert.deepEqual(rawText, [':', '·']);
});
