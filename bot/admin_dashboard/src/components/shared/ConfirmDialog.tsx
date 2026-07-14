import { AlertTriangle } from 'lucide-react';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  tone?: 'danger' | 'default';
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel = 'Bestätigen',
  cancelLabel = 'Abbrechen',
  tone = 'default',
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  if (!open) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/65 px-4 backdrop-blur-sm">
      <div className="panel-card w-full max-w-md rounded-3xl p-6">
        <div className="flex items-start gap-4">
          <div
            className={[
              'rounded-2xl border p-3',
              tone === 'danger'
                ? 'border-danger/30 bg-danger/10 text-danger'
                : 'border-warning/30 bg-warning/10 text-warning',
            ].join(' ')}
          >
            <AlertTriangle className="h-5 w-5" />
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-semibold text-white">{title}</h3>
            <p className="text-sm leading-6 text-text-secondary">{description}</p>
          </div>
        </div>
        <div className="mt-6 flex justify-end gap-3">
          <button onClick={onCancel} className="admin-button admin-button-secondary" disabled={busy}>
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            className={`admin-button ${tone === 'danger' ? 'admin-button-danger' : 'admin-button-primary'}`}
            disabled={busy}
          >
            {busy ? 'Läuft …' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
