import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ChevronDown, RefreshCw, Save, Undo2 } from 'lucide-react';
import { ApiError, saveSettingsCatalog } from '@/api/client';
import {
  draftFromSaved,
  isDraftChanged,
  parseDraft,
  readValidationError,
  type SettingsCatalog,
  type SettingsChange,
  type SettingsDraft,
  type SettingsField,
  type SettingsGroup,
} from '@/api/settingsCatalog';
import { SettingsFieldInput } from './SettingsFieldInput';

function countLabel(count: number) {
  return count === 1 ? '1 Einstellung' : `${count} Einstellungen`;
}

type CardStatus = { kind: 'saved' | 'error' | 'conflict'; message: string } | null;

interface SettingsGroupCardProps {
  group: SettingsGroup;
  fields: SettingsField[];
  visible: SettingsField[];
  catalog: SettingsCatalog;
  drafts: Record<string, SettingsDraft>;
  open: boolean;
  restartPending: Set<string>;
  reloading: boolean;
  onToggle: () => void;
  onDraft: (path: string, value: SettingsDraft | undefined) => void;
  onSaved: (catalog: SettingsCatalog, saved: SettingsField[]) => void;
  onReload: () => void;
}

function collectChanges(fields: SettingsField[], catalog: SettingsCatalog, drafts: Record<string, SettingsDraft>) {
  const changes: SettingsChange[] = [];
  const errors: Record<string, string> = {};
  for (const field of fields) {
    if (!(field.path in drafts)) continue;
    const parsed = parseDraft(field, drafts[field.path]);
    if (!parsed.ok) errors[field.path] = parsed.error;
    else if (isDraftChanged(field, drafts[field.path], catalog.values[field.path])) changes.push({ path: field.path, value: parsed.value });
  }
  return { changes, errors };
}

function failureStatus(error: unknown, paths: Set<string>): { status: CardStatus; fieldError: [string, string] | null } {
  if (error instanceof ApiError && error.status === 409) {
    return { status: { kind: 'conflict', message: 'Die Einstellungen wurden inzwischen an anderer Stelle geändert. Lade den aktuellen Stand neu, deine Eingaben bleiben erhalten.' }, fieldError: null };
  }
  const message = error instanceof Error ? error.message : 'Speichern fehlgeschlagen.';
  if (error instanceof ApiError && error.status === 422) {
    const detail = readValidationError(error.payload);
    if (detail.path && paths.has(detail.path)) {
      return { status: { kind: 'error', message: 'Ein Wert wurde nicht übernommen. Bitte das markierte Feld prüfen.' }, fieldError: [detail.path, detail.message ?? message] };
    }
  }
  return { status: { kind: 'error', message: `${message} Deine Eingaben bleiben erhalten.` }, fieldError: null };
}

export function SettingsGroupCard(props: SettingsGroupCardProps) {
  const { group, fields, visible, catalog, drafts, open, restartPending, reloading, onToggle, onDraft, onSaved, onReload } = props;
  const [serverErrors, setServerErrors] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<CardStatus>(null);
  const { changes, errors } = collectChanges(fields, catalog, drafts);
  const changedCount = fields.filter(field => field.path in drafts && isDraftChanged(field, drafts[field.path], catalog.values[field.path])).length;
  const invalid = Object.keys(errors).length > 0;
  const save = useMutation({
    mutationFn: (body: SettingsChange[]) => saveSettingsCatalog({ revision: catalog.revision, changes: body }),
    onSuccess: (fresh, body) => {
      const paths = new Set(body.map(change => change.path));
      onSaved(fresh, fields.filter(field => paths.has(field.path)));
      setServerErrors({});
      setStatus({ kind: 'saved', message: body.length === 1 ? 'Eine Änderung gespeichert.' : `${body.length} Änderungen gespeichert.` });
    },
    onError: error => {
      const result = failureStatus(error, new Set(fields.map(field => field.path)));
      setStatus(result.status);
      setServerErrors(result.fieldError ? { [result.fieldError[0]]: result.fieldError[1] } : {});
    },
  });

  function change(path: string, value: SettingsDraft) {
    onDraft(path, value);
    setServerErrors(current => {
      if (!(path in current)) return current;
      const next = { ...current };
      delete next[path];
      return next;
    });
    if (status?.kind !== 'conflict') setStatus(null);
  }

  function discard() {
    for (const field of fields) onDraft(field.path, undefined);
    setServerErrors({});
    setStatus(null);
  }

  const panelId = `settings-group-${group.id}`;
  return <article className="panel-card min-w-0 rounded-2xl">
    <button type="button" className="flex w-full items-center justify-between gap-3 p-4 text-left sm:p-5" aria-expanded={open} aria-controls={panelId} onClick={onToggle}>
      <span className="min-w-0">
        <span className="block font-semibold text-white">{group.label}</span>
        <span className="mt-1 block text-sm text-text-secondary">
          {visible.length === fields.length ? countLabel(fields.length) : `${visible.length} von ${countLabel(fields.length)}`}
          {changedCount > 0 && <span className="text-warning"> · {changedCount} geändert</span>}
        </span>
      </span>
      <ChevronDown size={18} aria-hidden="true" className={['shrink-0 text-text-secondary transition-transform', open ? 'rotate-180' : ''].join(' ')} />
    </button>
    {open && <form id={panelId} noValidate className="space-y-4 border-t border-border p-3 sm:p-5" onSubmit={event => {
      event.preventDefault();
      if (invalid || changes.length === 0 || save.isPending) return;
      setStatus(null);
      save.mutate(changes);
    }}>
      <div className="grid gap-4 lg:grid-cols-2">
        {visible.map(field => {
          const saved = catalog.values[field.path];
          const draft = field.path in drafts ? drafts[field.path] : draftFromSaved(field, saved);
          return <SettingsFieldInput key={field.path} field={field} value={draft} fallback={draftFromSaved(field, saved)}
            changed={field.path in drafts && isDraftChanged(field, draft, saved)}
            error={errors[field.path] ?? serverErrors[field.path] ?? null}
            restartPending={restartPending.has(field.path) && !(field.path in drafts)}
            disabled={save.isPending} onChange={value => change(field.path, value)} />;
        })}
      </div>
      {status && <div role={status.kind === 'saved' ? 'status' : 'alert'}
        className={['flex flex-wrap items-center gap-3 rounded-xl border p-3 text-sm', status.kind === 'saved' ? 'border-border text-text-secondary' : 'border-danger/50 text-danger'].join(' ')}>
        <span className="min-w-0 flex-1">{status.message}</span>
        {status.kind === 'conflict' && <button type="button" className="admin-button admin-button-secondary" disabled={reloading} onClick={() => { setStatus(null); onReload(); }}>
          <RefreshCw size={16} aria-hidden="true" /> Neu laden
        </button>}
      </div>}
      <div className="flex flex-wrap items-center gap-3">
        <button type="submit" className="admin-button admin-button-primary" disabled={changes.length === 0 || invalid || save.isPending}>
          <Save size={16} aria-hidden="true" /> {save.isPending ? 'Wird gespeichert …' : 'Bereich speichern'}
        </button>
        {changedCount > 0 && <button type="button" className="admin-button admin-button-secondary" disabled={save.isPending} onClick={discard}>
          <Undo2 size={16} aria-hidden="true" /> Änderungen verwerfen
        </button>}
        {invalid && <span className="text-sm text-danger">Bitte zuerst die markierten Eingaben korrigieren.</span>}
      </div>
    </form>}
  </article>;
}
