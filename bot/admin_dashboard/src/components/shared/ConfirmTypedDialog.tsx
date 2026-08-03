import { useEffect, useState } from 'react';
import { AlertTriangle } from 'lucide-react';

interface ConfirmTypedDialogProps {
  open: boolean;
  title: string;
  description: string;
  /** Genau dieser Text muss eingetippt werden (Vergleich ohne Groß-/Kleinschreibung). */
  expected: string;
  /** Zeilen, die auflisten, was die Aktion konkret tut. */
  steps?: string[];
  inputLabel?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Zweite Bestätigungsstufe für Aktionen, die von außen sichtbar sind: Der Admin
 * muss den Ziel-Login abtippen. Der Server prüft dieselbe Eingabe noch einmal —
 * dieser Dialog ist Bequemlichkeit, nicht der eigentliche Schutz.
 */
export function ConfirmTypedDialog({
  open,
  title,
  description,
  expected,
  steps = [],
  inputLabel = 'Zur Bestätigung eintippen',
  confirmLabel = 'Bestätigen',
  cancelLabel = 'Abbrechen',
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmTypedDialogProps) {
  const [typed, setTyped] = useState('');

  useEffect(() => {
    if (!open) {
      setTyped('');
    }
  }, [open]);

  if (!open) {
    return null;
  }

  const matches = typed.trim().toLowerCase() === expected.trim().toLowerCase();

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/65 px-4 backdrop-blur-sm">
      <div className="panel-card w-full max-w-lg rounded-3xl p-6">
        <div className="flex items-start gap-4">
          <div className="rounded-2xl border border-danger/30 bg-danger/10 p-3 text-danger">
            <AlertTriangle className="h-5 w-5" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-semibold text-white">{title}</h3>
            <p className="text-sm leading-6 text-text-secondary">{description}</p>
          </div>
        </div>

        {steps.length ? (
          <ul className="mt-5 space-y-2 rounded-2xl border border-white/10 bg-bg/55 p-4">
            {steps.map((step) => (
              <li key={step} className="flex gap-2 text-sm leading-6 text-text-secondary">
                <span className="text-danger">•</span>
                <span>{step}</span>
              </li>
            ))}
          </ul>
        ) : null}

        <label className="mt-5 block text-sm text-text-secondary">
          {inputLabel}: <span className="font-mono text-white">{expected}</span>
          <input
            className="admin-input mt-2"
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            autoFocus
            spellCheck={false}
            autoComplete="off"
            placeholder={expected}
          />
        </label>

        <div className="mt-6 flex justify-end gap-3">
          <button onClick={onCancel} className="admin-button admin-button-secondary" disabled={busy}>
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            className="admin-button admin-button-danger"
            disabled={busy || !matches}
          >
            {busy ? 'Läuft …' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
