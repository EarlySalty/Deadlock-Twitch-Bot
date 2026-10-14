export type SettingsKind =
  | 'boolean'
  | 'integer'
  | 'number'
  | 'string'
  | 'choice'
  | 'string_list'
  | 'integer_list';

export interface SettingsGroup {
  id: string;
  label: string;
}

export interface SettingsField {
  path: string;
  group: string;
  label: string;
  help: string;
  kind: SettingsKind;
  nullable: boolean;
  restart_required: boolean;
  min: string | null;
  max: string | null;
  choices: string[];
  min_items: number | null;
  max_items: number | null;
}

export type SettingsWireValue = null | boolean | number | string | string[];

export interface SettingsCatalog {
  version: number;
  revision: string;
  groups: SettingsGroup[];
  fields: SettingsField[];
  values: Record<string, SettingsWireValue>;
}

export interface SettingsChange {
  path: string;
  value: SettingsWireValue;
}

export interface SettingsChangeRequest {
  revision: string;
  changes: SettingsChange[];
}

export type SettingsDraft = null | boolean | string | string[];

export type ParsedValue = { ok: true; value: SettingsWireValue } | { ok: false; error: string };

const INTEGER_PATTERN = /^(0|-?[1-9][0-9]*)$/;
const DECIMAL_PATTERN = /^-?[0-9]+([.,][0-9]+)?$/;
const MAX_TEXT_BYTES = 4096;

export function draftFromSaved(field: SettingsField, saved: SettingsWireValue | undefined): SettingsDraft {
  if (saved === undefined || saved === null) return null;
  if (Array.isArray(saved)) return [...saved];
  if (typeof saved === 'number') return String(saved);
  if (field.kind === 'boolean') return saved === true;
  return String(saved);
}

export function emptyDraft(field: SettingsField): SettingsDraft {
  if (field.kind === 'boolean') return false;
  if (field.kind === 'string_list' || field.kind === 'integer_list') return [];
  if (field.kind === 'choice') return field.choices[0] ?? '';
  return '';
}

function boundsText(field: SettingsField): string {
  if (field.min !== null && field.max !== null) return `zwischen ${field.min} und ${field.max}`;
  if (field.min !== null) return `ab ${field.min}`;
  if (field.max !== null) return `bis ${field.max}`;
  return '';
}

export function parseInteger(field: SettingsField, raw: string): ParsedValue {
  const text = raw.trim();
  if (!INTEGER_PATTERN.test(text)) {
    return { ok: false, error: 'Bitte eine ganze Zahl ohne Leerzeichen und führende Nullen eingeben.' };
  }
  const value = BigInt(text);
  const below = field.min !== null && value < BigInt(field.min);
  const above = field.max !== null && value > BigInt(field.max);
  if (below || above) return { ok: false, error: `Erlaubt sind Werte ${boundsText(field)}.` };
  return { ok: true, value: text };
}

function parseNumber(field: SettingsField, raw: string): ParsedValue {
  const text = raw.trim();
  if (!DECIMAL_PATTERN.test(text)) return { ok: false, error: 'Bitte eine Zahl eingeben, zum Beispiel 2,5.' };
  const value = Number(text.replace(',', '.'));
  const below = field.min !== null && value < Number(field.min);
  const above = field.max !== null && value > Number(field.max);
  if (!Number.isFinite(value) || below || above) {
    return { ok: false, error: `Erlaubt sind Werte ${boundsText(field)}.` };
  }
  return { ok: true, value };
}

function parseText(raw: string): ParsedValue {
  if (new TextEncoder().encode(raw).length > MAX_TEXT_BYTES) return { ok: false, error: 'Der Text ist zu lang.' };
  if ([...raw].some(char => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127)) {
    return { ok: false, error: 'Zeilenumbrüche und Steuerzeichen sind hier nicht erlaubt.' };
  }
  return { ok: true, value: raw };
}

function parseList(field: SettingsField, items: string[]): ParsedValue {
  if (field.min_items !== null && items.length < field.min_items) {
    return { ok: false, error: `Mindestens ${field.min_items} ${field.min_items === 1 ? 'Eintrag' : 'Einträge'} nötig.` };
  }
  if (field.max_items !== null && items.length > field.max_items) {
    return { ok: false, error: `Höchstens ${field.max_items} Einträge erlaubt.` };
  }
  const parsed: string[] = [];
  for (const [index, item] of items.entries()) {
    const result = field.kind === 'integer_list' ? parseInteger(field, item) : parseText(item);
    if (!result.ok) return { ok: false, error: `Eintrag ${index + 1}: ${result.error}` };
    parsed.push(String(result.value));
  }
  return { ok: true, value: parsed };
}

export function parseDraft(field: SettingsField, draft: SettingsDraft): ParsedValue {
  if (draft === null) {
    return field.nullable ? { ok: true, value: null } : { ok: false, error: 'Dieses Feld braucht einen Wert.' };
  }
  if (field.kind === 'boolean') return { ok: true, value: draft === true };
  if (Array.isArray(draft)) return parseList(field, draft);
  const text = String(draft);
  if (field.kind === 'integer') return parseInteger(field, text);
  if (field.kind === 'number') return parseNumber(field, text);
  if (field.kind === 'choice') {
    return field.choices.includes(text) ? { ok: true, value: text } : { ok: false, error: 'Bitte einen Eintrag aus der Liste wählen.' };
  }
  return parseText(text);
}

export function sameWireValue(left: SettingsWireValue | undefined, right: SettingsWireValue | undefined): boolean {
  const a = left ?? null;
  const b = right ?? null;
  if (Array.isArray(a) || Array.isArray(b)) {
    return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((item, index) => item === b[index]);
  }
  return a === b;
}

export function isDraftChanged(field: SettingsField, draft: SettingsDraft, saved: SettingsWireValue | undefined): boolean {
  const parsed = parseDraft(field, draft);
  return !parsed.ok || !sameWireValue(parsed.value, saved);
}

export function uniqueFields(fields: SettingsField[]): SettingsField[] {
  const seen = new Set<string>();
  return fields.filter(field => {
    if (seen.has(field.path)) return false;
    seen.add(field.path);
    return true;
  });
}

export function matchesSearch(field: SettingsField, query: string): boolean {
  if (!query) return true;
  const needle = query.toLocaleLowerCase('de');
  return [field.label, field.path, field.help].some(text => text.toLocaleLowerCase('de').includes(needle));
}

export function readValidationError(payload: unknown): { path: string | null; message: string | null } {
  if (!payload || typeof payload !== 'object') return { path: null, message: null };
  const record = payload as Record<string, unknown>;
  const path = [record.path, record.field].find(value => typeof value === 'string');
  const message = [record.message, record.error].find(value => typeof value === 'string');
  return { path: typeof path === 'string' ? path : null, message: typeof message === 'string' ? message : null };
}
