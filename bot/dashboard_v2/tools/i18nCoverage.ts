import ts from 'typescript';
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { dictionaryFor } from '../src/i18n/dictionary';
import {
  EXCLUDED_PATHS,
  IGNORED_LITERALS,
  NEUTRAL_TEXTS,
  NON_VISIBLE_ATTRIBUTES,
  PENDING_FILES,
  VISIBLE_PROPERTY_NAMES,
} from './i18nCoverageData';

export interface Violation {
  file: string;
  line: number;
  rule: string;
  text: string;
}

export const ROOT = fileURLToPath(new URL('..', import.meta.url));

const LETTER = /\p{L}/u;
const UMLAUT = /[äöüÄÖÜß]/;
const GERMAN_WORD =
  /(?<![\p{L}\d_])(und|oder|nicht|der|die|das|den|dem|des|ein|eine|einen|einem|einer|ist|sind|wird|werden|wurde|wurden|für|mit|von|bei|noch|kein|keine|keinen|auf|aus|zum|zur|über|du|dein|deine|deinen|deinem|deiner|dich|ich|wir|auch|nur|schon|jetzt|mehr|wenn|dann|als|wie|hier|heute|gestern|bitte|fehler|laden|konnte|gibt|haben|kann|können|soll|seit|letzte|letzten|tage|stunden|minuten|zuschauer|neu|alle|aktiv|gespeichert|speichern|abbrechen|zurück|weiter|daten)(?![\p{L}\d_])/iu;
const TRANSLATE_CALLEES = new Set(['t', 'tr']);
const COMPARISON_OPERATORS = new Set([
  ts.SyntaxKind.EqualsEqualsEqualsToken,
  ts.SyntaxKind.ExclamationEqualsEqualsToken,
  ts.SyntaxKind.EqualsEqualsToken,
  ts.SyntaxKind.ExclamationEqualsToken,
]);
const LOCALE_METHODS = new Set(['toLocaleString', 'toLocaleDateString', 'toLocaleTimeString']);
const GERMAN_LOCALES = new Set(['de', 'de-DE', 'de-AT', 'de-CH']);

function normalize(text: string): string {
  return text
    .replace(/&[a-zA-Z]+;|&#\d+;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function isNeutral(text: string): boolean {
  return !LETTER.test(text) || NEUTRAL_TEXTS.has(text);
}

function looksLikeText(text: string): boolean {
  if (!LETTER.test(text)) return false;
  if (/^(\/|https?:|mailto:|#)/.test(text) || text.includes('://')) return false;
  return /\s/.test(text) || /^\p{Lu}/u.test(text) || UMLAUT.test(text);
}

export function looksGerman(text: string): boolean {
  return UMLAUT.test(text) || GERMAN_WORD.test(text);
}

function hasTranslation(text: string, dictionary: Record<string, string>): boolean {
  return Object.prototype.hasOwnProperty.call(dictionary, text);
}

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry) && !entry.endsWith('.d.ts')) {
      out.push(full);
    }
  }
  return out;
}

export function scopedFiles(): string[] {
  return listSourceFiles(join(ROOT, 'src'))
    .map((file) => relative(ROOT, file).split('\\').join('/'))
    .filter((file) => !EXCLUDED_PATHS.some((pattern) => pattern.test(file)))
    .sort();
}

function literalLeaves(node: ts.Expression): ts.Node[] {
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return [node];
  if (ts.isTemplateExpression(node)) return [node];
  if (ts.isParenthesizedExpression(node)) return literalLeaves(node.expression);
  if (ts.isConditionalExpression(node)) {
    return [...literalLeaves(node.whenTrue), ...literalLeaves(node.whenFalse)];
  }
  if (ts.isBinaryExpression(node)) {
    const op = node.operatorToken.kind;
    if (op === ts.SyntaxKind.PlusToken) return [...literalLeaves(node.left), ...literalLeaves(node.right)];
    if (
      op === ts.SyntaxKind.BarBarToken ||
      op === ts.SyntaxKind.QuestionQuestionToken ||
      op === ts.SyntaxKind.AmpersandAmpersandToken
    ) {
      return literalLeaves(node.right);
    }
  }
  return [];
}

function textOfLiteral(node: ts.Node): string {
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if (ts.isTemplateExpression(node)) {
    return [node.head.text, ...node.templateSpans.map((span) => span.literal.text)].join('{…}');
  }
  if (ts.isJsxText(node)) return node.getText();
  return '';
}

function calleeName(call: ts.CallExpression): string {
  const callee = call.expression;
  if (ts.isIdentifier(callee)) return callee.text;
  if (ts.isPropertyAccessExpression(callee)) return callee.name.text;
  return '';
}

function translatedArgument(call: ts.CallExpression): ts.Expression | undefined {
  const name = calleeName(call);
  if (TRANSLATE_CALLEES.has(name)) return call.arguments[0];
  if (name === 'translate') return call.arguments[1];
  return undefined;
}

function isTranslatedArgument(node: ts.Node): boolean {
  const parent = node.parent;
  return Boolean(parent && ts.isCallExpression(parent) && translatedArgument(parent) === node);
}

function isSkippedContext(node: ts.Node): boolean {
  const parent = node.parent;
  if (!parent) return true;
  if (ts.isImportDeclaration(parent) || ts.isExportDeclaration(parent)) return true;
  if (ts.isExternalModuleReference(parent) || ts.isImportTypeNode(parent)) return true;
  if (ts.isLiteralTypeNode(parent)) return true;
  if (ts.isPropertyAssignment(parent) && parent.name === node) return true;
  if (ts.isPropertySignature(parent) || ts.isEnumMember(parent)) return true;
  if (ts.isElementAccessExpression(parent) && parent.argumentExpression === node) return true;
  if (ts.isCaseClause(parent)) return true;
  if (ts.isBinaryExpression(parent) && COMPARISON_OPERATORS.has(parent.operatorToken.kind)) return true;
  if (ts.isJsxAttribute(parent)) return true;
  if (ts.isJsxExpression(parent)) return true;
  return isTranslatedArgument(node);
}

function insideJsxExpressionChain(node: ts.Node): boolean {
  let current: ts.Node | undefined = node.parent;
  while (
    current &&
    (ts.isParenthesizedExpression(current) ||
      ts.isConditionalExpression(current) ||
      ts.isBinaryExpression(current))
  ) {
    current = current.parent;
  }
  return Boolean(current && (ts.isJsxExpression(current) || ts.isJsxAttribute(current)));
}

function attributeName(attribute: ts.JsxAttribute): string {
  return attribute.name.getText();
}

export function checkSource(file: string, source: string, dictionary: Record<string, string>): Violation[] {
  const kind = file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, kind);
  const ignored = new Set(IGNORED_LITERALS[file] ?? []);
  const violations: Violation[] = [];
  const report = (node: ts.Node, rule: string, text: string) => {
    if (ignored.has(text)) return;
    const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
    violations.push({ file, line: line + 1, rule, text });
  };

  const checkVisibleLeaves = (expression: ts.Expression, rule: string, requireTextShape: boolean) => {
    for (const leaf of literalLeaves(expression)) {
      const text = normalize(textOfLiteral(leaf));
      if (isNeutral(text)) continue;
      if (requireTextShape && !looksLikeText(text)) continue;
      report(leaf, rule, text);
    }
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxText(node)) {
      const text = normalize(node.getText(sf));
      if (!isNeutral(text)) report(node, 'jsx-text', text);
    } else if (ts.isJsxAttribute(node) && node.initializer) {
      const name = attributeName(node);
      if (!NON_VISIBLE_ATTRIBUTES.has(name) && !name.startsWith('data-')) {
        const init = node.initializer;
        if (ts.isStringLiteral(init)) {
          checkVisibleLeaves(init, 'jsx-attribute', true);
        } else if (ts.isJsxExpression(init) && init.expression) {
          checkVisibleLeaves(init.expression, 'jsx-attribute', true);
        }
      }
    } else if (ts.isJsxExpression(node) && node.expression && !ts.isJsxAttribute(node.parent)) {
      checkVisibleLeaves(node.expression, 'jsx-expression', false);
    } else if (ts.isCallExpression(node)) {
      const arg = translatedArgument(node);
      if (arg && (ts.isStringLiteral(arg) || ts.isNoSubstitutionTemplateLiteral(arg))) {
        if (!isNeutral(arg.text) && !hasTranslation(arg.text, dictionary)) {
          report(arg, 'missing-translation', arg.text);
        }
      } else if (arg && ts.isTemplateExpression(arg)) {
        report(arg, 'template-in-translate', normalize(textOfLiteral(arg)));
      }
      const method = calleeName(node);
      if (LOCALE_METHODS.has(method) && node.arguments.length === 0) {
        report(node, 'browser-locale', node.getText(sf));
      }
    } else if (ts.isTemplateExpression(node)) {
      if (!isSkippedContext(node) && !insideJsxExpressionChain(node)) {
        const text = normalize(textOfLiteral(node));
        if (looksGerman(text)) report(node, 'german-template', text);
      }
    } else if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      if (GERMAN_LOCALES.has(node.text) && !isSkippedContext(node)) {
        report(node, 'fixed-locale', node.text);
      } else if (!isSkippedContext(node) && !insideJsxExpressionChain(node)) {
        const text = normalize(node.text);
        const parent = node.parent;
        const visibleProperty =
          parent &&
          ts.isPropertyAssignment(parent) &&
          parent.initializer === node &&
          VISIBLE_PROPERTY_NAMES.has(parent.name.getText(sf).replace(/['"]/g, ''));
        const suspicious = visibleProperty ? looksLikeText(text) : looksGerman(text);
        if (suspicious && !isNeutral(text)) {
          report(node, visibleProperty ? 'visible-property' : 'german-literal', text);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return violations;
}

export function checkFile(file: string, dictionary = dictionaryFor('en')): Violation[] {
  return checkSource(file, readFileSync(join(ROOT, file), 'utf8'), dictionary);
}

export function coverageReport(files = scopedFiles()): Map<string, Violation[]> {
  const dictionary = dictionaryFor('en');
  const report = new Map<string, Violation[]>();
  for (const file of files) report.set(file, checkFile(file, dictionary));
  return report;
}

export function pendingFiles(): Set<string> {
  return new Set(PENDING_FILES);
}

function collectTexts(file: string, source: string): string[] {
  const kind = file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, kind);
  const texts: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxText(node)) {
      texts.push(normalize(node.getText(sf)));
    } else if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      texts.push(normalize(node.text));
    } else if (ts.isTemplateExpression(node)) {
      texts.push(normalize(node.head.text), ...node.templateSpans.map((span) => normalize(span.literal.text)));
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return texts.filter((text) => LETTER.test(text));
}

function gitShow(ref: string, file: string): string | null {
  try {
    return execFileSync('git', ['show', `${ref}:bot/dashboard_v2/${file}`], {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
  } catch {
    return null;
  }
}

function mergeBase(ref: string): string {
  return execFileSync('git', ['merge-base', ref, 'HEAD'], { cwd: ROOT, encoding: 'utf8' }).trim();
}

function changedFiles(base: string): string[] {
  const out = execFileSync('git', ['diff', '--name-only', base, '--', 'src'], {
    cwd: ROOT,
    encoding: 'utf8',
  });
  return out
    .split('\n')
    .filter(Boolean)
    .map((path) => path.replace(/^bot\/dashboard_v2\//, ''))
    .filter((path) => /\.(ts|tsx)$/.test(path));
}

export function lostGermanTexts(ref: string): Violation[] {
  const base = mergeBase(ref);
  const files = changedFiles(base);
  const headTexts: string[] = [];
  for (const file of files) {
    try {
      headTexts.push(...collectTexts(file, readFileSync(join(ROOT, file), 'utf8')));
    } catch {
      continue;
    }
  }
  const pool = headTexts.join('\u0000');
  const lost: Violation[] = [];
  for (const file of files) {
    const before = gitShow(base, file);
    if (before === null) continue;
    for (const text of new Set(collectTexts(file, before))) {
      if (!looksLikeText(text) && !looksGerman(text)) continue;
      if (!pool.includes(text)) lost.push({ file, line: 0, rule: 'german-text-changed', text });
    }
  }
  return lost;
}

function printViolations(violations: Violation[]): void {
  for (const v of violations) console.log(`${v.file}:${v.line} [${v.rule}] ${v.text}`);
}

function main(args: string[]): number {
  if (args[0] === '--preserve') {
    const lost = lostGermanTexts(args[1] ?? 'origin/main');
    printViolations(lost);
    console.log(`${lost.length} German texts missing after change`);
    return lost.length ? 1 : 0;
  }
  if (args[0] === '--summary') {
    const report = coverageReport();
    for (const [file, violations] of report) {
      if (violations.length) console.log(`${String(violations.length).padStart(5)} ${file}`);
    }
    return 0;
  }
  const files = args.length ? args : scopedFiles().filter((file) => !pendingFiles().has(file));
  const violations = [...coverageReport(files).values()].flat();
  printViolations(violations);
  console.log(`${violations.length} violations in ${files.length} files`);
  return violations.length ? 1 : 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  process.exitCode = main(process.argv.slice(2));
}
