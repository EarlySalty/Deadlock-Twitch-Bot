import { useEffect, useId, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

export function WorkspaceDialog({
  eyebrow,
  title,
  onClose,
  busy = false,
  children,
}: {
  eyebrow: string;
  title: string;
  onClose: () => void;
  busy?: boolean;
  children: ReactNode;
}) {
  const t = useT();
  const dialog = useRef<HTMLDialogElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  useEffect(() => {
    const element = dialog.current;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const overflow = document.body.style.overflow;
    element?.showModal();
    document.body.style.overflow = 'hidden';
    close.current?.focus();
    return () => {
      element?.close();
      document.body.style.overflow = overflow;
      if (opener?.isConnected) opener.focus();
    };
  }, []);
  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      className="social-studio studio-dialog"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
      onKeyDown={(event) => {
        if (event.key !== 'Tab') return;
        const focusable = Array.from(
          event.currentTarget.querySelectorAll<HTMLElement>(
            'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex="0"]',
          ),
        ).filter((node) => node.getClientRects().length > 0);
        const first = focusable[0];
        const last = focusable.at(-1);
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }}
    >
      <header className="flex items-center justify-between gap-4 border-b border-border px-5 py-4">
        <div className="min-w-0">
          <p className="text-xs text-primary">{eyebrow}</p>
          <h2 id={titleId} className="mt-1 break-words text-lg font-semibold">
            {title}
          </h2>
        </div>
        <button
          ref={close}
          type="button"
          disabled={busy}
          className="studio-icon-button shrink-0"
          aria-label={t('Schließen')}
          onClick={onClose}
        >
          <X className="h-4 w-4" />
        </button>
      </header>
      <div className="studio-dialog-content p-4 md:p-6">{children}</div>
    </dialog>
  );
}
