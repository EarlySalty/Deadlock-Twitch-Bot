import { useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { X } from 'lucide-react';
import { useBillingCatalog } from '../../hooks/useAnalytics';
import { euroAusCents } from './preis';
import { getPlanCheckoutHref } from '../../preview/routes';

/**
 * Sheet hinter einer gesperrten Karte.
 *
 * Regel aus der Spec: gesperrte Karten bleiben sichtbar und anklickbar, ein
 * Tipp oeffnet den Preis an Ort und Stelle. Kein Sprung auf die Preisseite —
 * wer gerade eine Auswertung ansieht, soll dort bleiben.
 */

interface PremiumSheetProps {
  offen: boolean;
  onSchliessen: () => void;
  /** Name der Karte, hinter der das Sheet aufgeht. */
  titel: string;
}

export function PremiumSheet({ offen, onSchliessen, titel }: PremiumSheetProps) {
  const { data: katalog } = useBillingCatalog(1);
  const premium = katalog?.plans.find((plan) => plan.id === 'premium');

  useEffect(() => {
    if (!offen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onSchliessen();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [offen, onSchliessen]);

  return (
    <AnimatePresence>
      {offen && (
        <motion.div key="premium-sheet" className="fixed inset-0 z-50 flex items-end justify-center">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.24, ease: [0.23, 1, 0.32, 1] }}
            className="absolute inset-0 bg-black/60"
            onClick={onSchliessen}
          />
          <motion.div
            initial={{ y: 24, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: 16, opacity: 0 }}
            transition={{ duration: 0.24, ease: [0.23, 1, 0.32, 1] }}
            className="relative z-10 mb-4 w-full max-w-md rounded-2xl border border-border bg-card p-6 shadow-2xl"
            role="dialog"
            aria-modal="true"
            aria-label={`${titel} freischalten`}
          >
            <button
              type="button"
              onClick={onSchliessen}
              className="absolute right-4 top-4 rounded-lg p-1 text-text-secondary hover:bg-white/5 hover:text-white"
              aria-label="Schließen"
            >
              <X className="h-5 w-5" />
            </button>

            <p className="text-sm text-white/50">{titel}</p>
            <p className="mt-1 text-white">Diese Auswertung gehört zu Premium.</p>

            {premium && (
              <p className="mt-4 text-white/70">
                {euroAusCents(premium.yearly_gross_cents ?? 0)} im Jahr oder{' '}
                {euroAusCents(premium.monthly_gross_cents ?? 0)} im Monat, jederzeit kündbar.
              </p>
            )}

            <a
              href={getPlanCheckoutHref('premium', false, 12)}
              data-press
              className="mt-5 flex w-full items-center justify-center rounded-xl bg-primary px-4 py-3 font-semibold text-[#0D0806]"
            >
              Freischalten
            </a>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
