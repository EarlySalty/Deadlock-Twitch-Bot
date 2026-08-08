import { useState, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { X, Sparkles, Clock } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import { PREVIEW_PRICING_ROUTE } from '../../preview/routes';

const MODAL_SHOWN_KEY = 'trial-expiry-modal-shown';

export function TrialExpiryModal() {
  const { trialInfo, tier } = usePlan();
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    // Show modal only when trial is expiring soon and not yet shown in this period
    if (!trialInfo?.onTrialExpiringSoon || tier === 'extended') {
      setVisible(false);
      return;
    }

    // If user dismissed before, don't show again until trial period changes
    const shownKey = localStorage.getItem(MODAL_SHOWN_KEY);
    if (shownKey) {
      const shownDate = new Date(shownKey);
      const trialEnd = trialInfo.trialEndsAt ? new Date(trialInfo.trialEndsAt) : null;
      if (trialEnd && shownDate > trialEnd) {
        // Trial period changed, allow showing again
        localStorage.removeItem(MODAL_SHOWN_KEY);
        setVisible(true);
      }
    } else {
      setVisible(true);
    }
  }, [trialInfo, tier]);

  const handleUpgrade = () => {
    localStorage.setItem(MODAL_SHOWN_KEY, new Date().toISOString());
    window.location.href = PREVIEW_PRICING_ROUTE;
  };

  const handleDismiss = () => {
    localStorage.setItem(MODAL_SHOWN_KEY, new Date().toISOString());
    setVisible(false);
  };

  return (
    <AnimatePresence>
      {visible && (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Verdunkeln und zurueckdruecken: die Aufgabe dahinter ruht, solange der
          Dialog offen ist. Der Weichzeichner faehrt mit der Deckkraft hoch,
          damit die Flaeche wie ein ankommendes Material wirkt statt wie ein Bild,
          das eingeblendet wird. */}
      <motion.div
        initial={{ opacity: 0, backdropFilter: 'blur(0px)' }}
        animate={{ opacity: 1, backdropFilter: 'blur(6px)' }}
        exit={{ opacity: 0, backdropFilter: 'blur(0px)' }}
        transition={{ duration: 0.28, ease: [0.23, 1, 0.32, 1] }}
        className="absolute inset-0 bg-black/70"
        onClick={handleDismiss}
      />

      {/* Der Dialog haengt an keinem Ausloeser, also skaliert er aus der Mitte.
          Austritt schneller als Eintritt: beim Schliessen hat der Nutzer schon
          entschieden und wartet nur noch. */}
      <motion.div
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.98 }}
        transition={{ duration: 0.28, ease: [0.23, 1, 0.32, 1] }}
        className="relative z-10 mx-4 w-full max-w-lg rounded-2xl border border-border bg-gradient-to-b from-[#1F1815] to-[#1A1210] p-6 shadow-2xl"
        role="dialog"
        aria-modal="true"
      >
        <button
          type="button"
          onClick={handleDismiss}
          className="absolute right-4 top-4 rounded-lg p-1 text-text-secondary hover:bg-white/5 hover:text-white transition-colors"
          aria-label="Schließen"
        >
          <X className="h-5 w-5" />
        </button>

        <div className="text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-warning/20">
            <Clock className="h-7 w-7 text-warning" />
          </div>

          <h2 className="mb-2 text-xl font-bold text-white">
            Deine Testphase läuft ab
          </h2>
          <p className="mb-6 text-sm text-text-secondary">
            In weniger als 7 Tagen endet deine kostenlose Testphase.
            Danach werden einige Analytics-Funktionen eingeschränkt.
          </p>

          <div className="mb-6 rounded-xl border border-warning/30 bg-warning/10 p-4">
            <div className="flex items-center justify-center gap-2 text-warning">
              <Sparkles className="h-5 w-5" />
              <span className="font-semibold">
                {trialInfo?.trialDaysRemaining ?? 0} Tage verbleibend
              </span>
            </div>
          </div>

          <div className="mb-6 space-y-2 text-left text-sm text-text-secondary">
            <p className="font-medium text-white">Was passiert nach der Testphase?</p>
            <ul className="list-inside list-disc space-y-1 text-text-secondary">
              <li>Erweiterte Analytics werden deaktiviert</li>
              <li>Manche Tabs sind nicht mehr zugänglich</li>
              <li>Keine Daten gehen verloren, alles bleibt gespeichert</li>
            </ul>
          </div>

          <div className="flex flex-col gap-3 sm:flex-row">
            <button
              type="button"
              onClick={handleDismiss}
              className="flex-1 rounded-lg border border-border bg-white/5 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-white/10"
            >
              Vielleicht später
            </button>
            <button
              type="button"
              onClick={handleUpgrade}
              className="flex-1 flex items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2.5 text-sm font-semibold text-[#0D0806] transition hover:bg-primary/80"
            >
              <Sparkles className="h-4 w-4" />
              Jetzt upgraden
            </button>
          </div>
        </div>
      </motion.div>
    </div>
      )}
    </AnimatePresence>
  );
}
