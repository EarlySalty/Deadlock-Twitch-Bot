import { useEffect } from 'react';
import { CheckCircle2, CircleAlert, X } from 'lucide-react';

interface ToastProps {
  open: boolean;
  message: string;
  tone?: 'success' | 'error';
  onClose: () => void;
}

export function Toast({ open, message, tone = 'success', onClose }: ToastProps) {
  const isError = tone === 'error';

  useEffect(() => {
    if (!open || !message || isError) {
      return undefined;
    }
    const timeoutId = window.setTimeout(() => {
      onClose();
    }, 4000);
    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [isError, message, onClose, open]);

  if (!open || !message) {
    return null;
  }

  return (
    <div className="fixed bottom-5 right-5 z-50 max-w-sm">
      <div
        className={[
          'glass flex items-start gap-3 rounded-2xl border px-4 py-3 shadow-2xl',
          isError ? 'border-danger/25' : 'border-success/25',
        ].join(' ')}
      >
        {isError ? (
          <CircleAlert className="mt-0.5 h-5 w-5 text-danger" />
        ) : (
          <CheckCircle2 className="mt-0.5 h-5 w-5 text-success" />
        )}
        <p className="flex-1 text-sm leading-6 text-white">{message}</p>
        <button onClick={onClose} className="rounded-full p-1 text-white/60 transition hover:text-white">
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
