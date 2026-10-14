import { ChevronDown, RefreshCw, Save, Undo2 } from 'lucide-react';
import {
  draftFromSaved,
  isDraftChanged,
  type SettingsCatalog,
  type SettingsDraft,
  type SettingsField,
  type SettingsGroup,
} from '@/api/settingsCatalog';
import { SettingsFieldInput } from './SettingsFieldInput';
import { collectChanges, type CardFeedback } from './settingsSave';

function countLabel(count: number) {
  return count === 1 ? '1 Einstellung' : `${count} Einstellungen`;
}

interface SettingsGroupCardProps {
  group: SettingsGroup;
  fields: SettingsField[];
  visible: SettingsField[];
  catalog: SettingsCatalog;
  drafts: Record<string, SettingsDraft>;
  open: boolean;
  restartPending: Set<string>;
  feedback: CardFeedback | undefined;
  saving: boolean;
  busy: boolean;
  onToggle: () => void;
  onDraft: (path: string, value: SettingsDraft) => void;
  onDiscard: () => void;
  onSave: () => void;
  onReload: () => void;
}

export function SettingsGroupCard(props: SettingsGroupCardProps) {
  const { group, fields, visible, catalog, drafts, open, restartPending, feedback, saving, busy, onToggle, onDraft, onDiscard, onSave, onReload } = props;
  const { changes, errors } = collectChanges(fields, catalog, drafts);
  const changedCount = fields.filter(field => field.path in drafts && isDraftChanged(field, drafts[field.path], catalog.values[field.path])).length;
  const invalid = Object.keys(errors).length > 0;
  const status = feedback?.status ?? null;
  const serverErrors = feedback?.serverErrors ?? {};
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
      if (invalid || changes.length === 0 || busy) return;
      onSave();
    }}>
      <div className="grid gap-4 lg:grid-cols-2">
        {visible.map(field => {
          const saved = catalog.values[field.path];
          const draft = field.path in drafts ? drafts[field.path] : draftFromSaved(field, saved);
          return <SettingsFieldInput key={field.path} field={field} value={draft} fallback={draftFromSaved(field, saved)}
            changed={field.path in drafts && isDraftChanged(field, draft, saved)}
            error={errors[field.path] ?? serverErrors[field.path] ?? null}
            restartPending={restartPending.has(field.path) && !(field.path in drafts)}
            disabled={busy} onChange={value => onDraft(field.path, value)} />;
        })}
      </div>
      {status && <div role={status.kind === 'saved' ? 'status' : 'alert'}
        className={['flex flex-wrap items-center gap-3 rounded-xl border p-3 text-sm', status.kind === 'saved' ? 'border-border text-text-secondary' : 'border-danger/50 text-danger'].join(' ')}>
        <span className="min-w-0 flex-1">{status.message}</span>
        {status.kind === 'conflict' && <button type="button" className="admin-button admin-button-secondary" disabled={busy} onClick={onReload}>
          <RefreshCw size={16} aria-hidden="true" /> Neu laden
        </button>}
      </div>}
      <div className="flex flex-wrap items-center gap-3">
        <button type="submit" className="admin-button admin-button-primary" disabled={changes.length === 0 || invalid || busy}>
          <Save size={16} aria-hidden="true" /> {saving ? 'Wird gespeichert …' : 'Bereich speichern'}
        </button>
        {changedCount > 0 && <button type="button" className="admin-button admin-button-secondary" disabled={busy} onClick={onDiscard}>
          <Undo2 size={16} aria-hidden="true" /> Änderungen verwerfen
        </button>}
        {invalid && <span className="text-sm text-danger">Bitte zuerst die markierten Eingaben korrigieren.</span>}
      </div>
    </form>}
  </article>;
}
