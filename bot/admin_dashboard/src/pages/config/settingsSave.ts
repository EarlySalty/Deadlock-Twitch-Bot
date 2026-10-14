import { ApiError } from '@/api/client';
import {
  isDraftChanged,
  parseDraft,
  readValidationError,
  type SettingsCatalog,
  type SettingsChange,
  type SettingsDraft,
  type SettingsField,
} from '@/api/settingsCatalog';

export type CardStatus = { kind: 'saved' | 'error' | 'conflict'; message: string };

export interface CardFeedback {
  status: CardStatus | null;
  serverErrors: Record<string, string>;
}

export interface GroupSave {
  groupId: string;
  revision: string;
  changes: SettingsChange[];
  submitted: Record<string, SettingsDraft>;
}

export function collectChanges(fields: SettingsField[], catalog: SettingsCatalog, drafts: Record<string, SettingsDraft>) {
  const changes: SettingsChange[] = [];
  const errors: Record<string, string> = {};
  const submitted: Record<string, SettingsDraft> = {};
  for (const field of fields) {
    if (!(field.path in drafts)) continue;
    const draft = drafts[field.path];
    const parsed = parseDraft(field, draft);
    if (!parsed.ok) errors[field.path] = parsed.error;
    else if (isDraftChanged(field, draft, catalog.values[field.path])) {
      changes.push({ path: field.path, value: parsed.value });
      submitted[field.path] = draft;
    }
  }
  return { changes, errors, submitted };
}

export function failureFeedback(error: unknown, paths: Set<string>): CardFeedback {
  if (error instanceof ApiError && error.status === 409) {
    return { status: { kind: 'conflict', message: 'Die Einstellungen wurden inzwischen an anderer Stelle geändert. Lade den aktuellen Stand neu, deine Eingaben bleiben erhalten.' }, serverErrors: {} };
  }
  const message = error instanceof Error ? error.message : 'Speichern fehlgeschlagen.';
  if (error instanceof ApiError && error.status === 422) {
    const detail = readValidationError(error.payload);
    if (detail.path && paths.has(detail.path)) {
      return { status: { kind: 'error', message: 'Ein Wert wurde nicht übernommen. Bitte das markierte Feld prüfen.' }, serverErrors: { [detail.path]: detail.message ?? message } };
    }
  }
  return { status: { kind: 'error', message: `${message} Deine Eingaben bleiben erhalten.` }, serverErrors: {} };
}

export function savedFeedback(count: number): CardFeedback {
  return { status: { kind: 'saved', message: count === 1 ? 'Eine Änderung gespeichert.' : `${count} Änderungen gespeichert.` }, serverErrors: {} };
}
