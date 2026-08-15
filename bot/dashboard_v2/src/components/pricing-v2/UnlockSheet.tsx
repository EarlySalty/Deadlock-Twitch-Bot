import { useState } from 'react';
import { getPlanCheckoutHref } from '../../preview/routes';

const TAX_NOTE = 'Kein Ausweis von Umsatzsteuer gemäß § 19 UStG.';

export function UnlockSheet({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [cycle, setCycle] = useState<1 | 12>(12);
  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center bg-black/50 p-4 sm:items-center"
      onClick={onClose}
    >
      <div
        className="rise-in w-full max-w-md rounded-2xl border border-white/10 bg-[#1a1210] p-6"
        data-press
        onClick={(event) => event.stopPropagation()}
      >
        <p className="text-sm text-white/50">Premium macht deine Zahlen sichtbar.</p>
        <div className="mt-4 space-y-2">
          <label className="flex cursor-pointer items-center gap-3 rounded-xl border border-white/10 px-3 py-2">
            <input
              type="radio"
              name="premium-cycle"
              checked={cycle === 12}
              onChange={() => setCycle(12)}
            />
            <span>29,90 € / Jahr, 2 Monate gratis</span>
          </label>
          <label className="flex cursor-pointer items-center gap-3 rounded-xl border border-white/10 px-3 py-2">
            <input
              type="radio"
              name="premium-cycle"
              checked={cycle === 1}
              onChange={() => setCycle(1)}
            />
            <span>2,99 € / Monat</span>
          </label>
        </div>
        <p className="mt-3 text-xs text-white/40">{TAX_NOTE}</p>
        <a
          href={getPlanCheckoutHref('premium', false, cycle)}
          className="mt-4 inline-flex w-full items-center justify-center rounded-xl bg-white/10 px-4 py-3 text-sm font-medium"
          data-press
        >
          Freischalten
        </a>
        <p className="mt-2 text-center text-xs text-white/40">jederzeit kündbar</p>
        <button type="button" className="mt-3 w-full text-sm text-white/50" onClick={onClose}>
          Schließen
        </button>
      </div>
    </div>
  );
}
