import type { ReactNode } from 'react';

interface StickyActionBarProps {
  lastSavedAt?: string | null;
  dirty: boolean;
  onSave: () => void;
  onDiscard: () => void;
  saving?: boolean;
  children?: ReactNode;
}

export function StickyActionBar({
  lastSavedAt,
  dirty,
  onSave,
  onDiscard,
  saving = false,
  children,
}: StickyActionBarProps) {
  return (
    <div className="sticky bottom-0 z-20 pt-4">
      <div className="glass rounded-[1.6rem] border border-white/10 px-4 py-4">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div className="text-sm text-text-secondary">
            {dirty ? 'Ungespeicherte Änderungen vorhanden.' : lastSavedAt ? `Zuletzt gespeichert: ${lastSavedAt}` : 'Keine Änderungen.'}
          </div>

          <div className="flex flex-wrap items-center gap-3">
            {children}
            <button className="admin-button admin-button-secondary disabled:cursor-not-allowed disabled:opacity-50" disabled={!dirty || saving} onClick={onDiscard}>
              Verwerfen
            </button>
            <button className="admin-button admin-button-primary disabled:cursor-not-allowed disabled:opacity-50" disabled={!dirty || saving} onClick={onSave}>
              {saving ? 'Speichert …' : 'Speichern'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
