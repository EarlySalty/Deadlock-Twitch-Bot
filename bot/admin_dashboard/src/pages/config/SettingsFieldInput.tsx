import { Plus, X } from 'lucide-react';
import { emptyDraft, type SettingsDraft, type SettingsField } from '@/api/settingsCatalog';

interface ControlProps {
  field: SettingsField;
  id: string;
  describedBy: string;
  value: SettingsDraft;
  disabled: boolean;
  onChange: (value: SettingsDraft) => void;
}

function SwitchControl({ id, describedBy, value, disabled, onChange }: ControlProps) {
  const checked = value === true;
  return <button id={id} type="button" role="switch" aria-checked={checked} aria-describedby={describedBy} disabled={disabled}
    className={['admin-button', checked ? 'admin-button-primary' : 'admin-button-secondary'].join(' ')}
    onClick={() => onChange(!checked)}>
    {checked ? 'An' : 'Aus'}
  </button>;
}

function ListControl({ field, id, describedBy, value, disabled, onChange }: ControlProps) {
  const items = Array.isArray(value) ? value : [];
  const numeric = field.kind === 'integer_list';
  const update = (index: number, text: string) => onChange(items.map((item, position) => position === index ? text : item));
  return <div id={id} className="space-y-2" aria-describedby={describedBy}>
    {items.length === 0 && <p className="text-sm text-text-secondary">Leere Liste</p>}
    {items.map((item, index) => <div key={index} className="flex min-w-0 items-center gap-2">
      <input className="admin-input min-w-0 flex-1" value={item} disabled={disabled} inputMode={numeric ? 'numeric' : undefined}
        aria-label={`${field.label}, Eintrag ${index + 1}`} onChange={event => update(index, event.target.value)} />
      <button type="button" className="admin-button admin-button-secondary shrink-0 px-3" disabled={disabled}
        aria-label={`Eintrag ${index + 1} entfernen`} onClick={() => onChange(items.filter((_, position) => position !== index))}>
        <X size={16} aria-hidden="true" />
      </button>
    </div>)}
    <button type="button" className="admin-button admin-button-secondary" disabled={disabled} onClick={() => onChange([...items, ''])}>
      <Plus size={16} aria-hidden="true" /> Eintrag hinzufügen
    </button>
  </div>;
}

function ValueControl(props: ControlProps) {
  const { field, id, describedBy, value, disabled, onChange } = props;
  if (field.kind === 'boolean') return <SwitchControl {...props} />;
  if (field.kind === 'string_list' || field.kind === 'integer_list') return <ListControl {...props} />;
  if (field.kind === 'choice') {
    return <select id={id} className="admin-input w-full" aria-describedby={describedBy} disabled={disabled}
      value={typeof value === 'string' ? value : ''} onChange={event => onChange(event.target.value)}>
      {field.choices.map(choice => <option key={choice} value={choice}>{choice}</option>)}
    </select>;
  }
  const numeric = field.kind === 'integer' || field.kind === 'number';
  return <input id={id} className="admin-input w-full" aria-describedby={describedBy} disabled={disabled}
    type="text" inputMode={field.kind === 'integer' ? 'numeric' : numeric ? 'decimal' : undefined}
    autoComplete="off" spellCheck={!numeric}
    value={typeof value === 'string' ? value : ''} onChange={event => onChange(event.target.value)} />;
}

interface SettingsFieldInputProps {
  field: SettingsField;
  value: SettingsDraft;
  fallback: SettingsDraft;
  changed: boolean;
  error: string | null;
  restartPending: boolean;
  disabled: boolean;
  onChange: (value: SettingsDraft) => void;
}

function bounds(field: SettingsField): string | null {
  if (field.kind !== 'integer' && field.kind !== 'number' && field.kind !== 'integer_list') return null;
  if (field.min !== null && field.max !== null) return `Bereich ${field.min} bis ${field.max}`;
  if (field.min !== null) return `Mindestens ${field.min}`;
  if (field.max !== null) return `Höchstens ${field.max}`;
  return null;
}

export function SettingsFieldInput({ field, value, fallback, changed, error, restartPending, disabled, onChange }: SettingsFieldInputProps) {
  const id = `setting-${field.path.replaceAll('.', '-')}`;
  const helpId = `${id}-help`;
  const usesDefault = value === null;
  const range = bounds(field);
  return <div className={['min-w-0 rounded-xl border p-3 sm:p-4', changed ? 'border-warning/70 bg-warning/5' : 'border-border'].join(' ')}>
    <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
      <label htmlFor={id} className="text-sm font-medium text-white">{field.label}</label>
      {changed && <span className="text-xs font-semibold text-warning">Geändert</span>}
    </div>
    <p className="mt-1 break-all font-mono text-[11px] text-text-secondary">{field.path}</p>
    {field.nullable && <label className="mt-3 flex items-center gap-2 text-sm text-text-secondary">
      <input type="checkbox" className="h-4 w-4 accent-primary" checked={usesDefault} disabled={disabled}
        onChange={event => onChange(event.target.checked ? null : fallback ?? emptyDraft(field))} />
      Dienststandard
    </label>}
    <div className="mt-3">
      {usesDefault
        ? <p id={id} className="text-sm text-text-secondary" aria-describedby={helpId}>Kein eigener Wert gespeichert. Der Dienst nutzt seinen Standard.</p>
        : <ValueControl field={field} id={id} describedBy={helpId} value={value} disabled={disabled} onChange={onChange} />}
    </div>
    <p id={helpId} className="mt-2 text-sm text-text-secondary">{field.help}{range && !usesDefault ? ` ${range}.` : ''}</p>
    {error && <p role="alert" className="mt-2 text-sm text-danger">{error}</p>}
    {restartPending && <p className="mt-2 text-sm text-warning">Wird nach dem nächsten Neustart des Dienstes aktiv.</p>}
  </div>;
}
