import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { X } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import { usePremiumTeaser } from '../../hooks/useAnalytics';
import { WachstumsKurve } from '../pricing-v2/WachstumsKurve';
import { PREVIEW_PRICING_ROUTE } from '../../preview/routes';
import { sollTrialEndeZeigen, TRIAL_ENDE_GESEHEN_PRAEFIX } from './trialEnde';

/**
 * Der eine Moment am Ende des Trials.
 *
 * Spec-Regel: „Trial-Ende erzeugt genau einen Moment mit seinen Zahlen. Danach
 * nur noch die stillen Sperren." Vorher war das ein Countdown-Modal, das ab
 * sieben Tagen Restlaufzeit bei jedem Laden wiederkam — und das zusaetzlich nie
 * erschien, weil es an einem Feld haing (`trial_end_at`), das die API nicht
 * liefert. Jetzt: einmal nach Ablauf, gemerkt am Ablaufdatum, danach nie wieder.
 */

export function TrialExpiryModal() {
  const { trialEndedAt, hasFullAccess, tier } = usePlan();
  const { data: teaser } = usePremiumTeaser();
  const [sichtbar, setSichtbar] = useState(false);

  const schluessel = trialEndedAt ? `${TRIAL_ENDE_GESEHEN_PRAEFIX}${trialEndedAt}` : null;

  useEffect(() => {
    setSichtbar(
      sollTrialEndeZeigen({
        trialEndedAt,
        hasFullAccess,
        tier,
        gesehen: schluessel !== null && localStorage.getItem(schluessel) !== null,
      }),
    );
  }, [schluessel, trialEndedAt, hasFullAccess, tier]);

  const schliessen = () => {
    if (schluessel) localStorage.setItem(schluessel, new Date().toISOString());
    setSichtbar(false);
  };

  useEffect(() => {
    if (!sichtbar) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') schliessen();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // schliessen haengt nur an `schluessel`, das in den Deps steht.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sichtbar, schluessel]);

  return (
    <AnimatePresence>
      {sichtbar && (
        <motion.div key="trial-ende" className="fixed inset-0 z-50 flex items-center justify-center">
          <motion.div
            initial={{ opacity: 0, backdropFilter: 'blur(0px)' }}
            animate={{ opacity: 1, backdropFilter: 'blur(6px)' }}
            exit={{ opacity: 0, backdropFilter: 'blur(0px)' }}
            transition={{ duration: 0.28, ease: [0.23, 1, 0.32, 1] }}
            className="absolute inset-0 bg-black/70"
            onClick={schliessen}
          />
          <motion.div
            initial={{ opacity: 0, scale: 0.96 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.98 }}
            transition={{ duration: 0.28, ease: [0.23, 1, 0.32, 1] }}
            className="relative z-10 mx-4 w-full max-w-md rounded-2xl border border-border bg-card p-6 shadow-2xl"
            role="dialog"
            aria-modal="true"
            aria-label="Testzugang beendet"
          >
            <button
              type="button"
              onClick={schliessen}
              className="absolute right-4 top-4 rounded-lg p-1 text-text-secondary hover:bg-white/5 hover:text-white"
              aria-label="Schließen"
            >
              <X className="h-5 w-5" />
            </button>

            <h2 className="text-lg font-semibold text-white">Dein Testzugang ist vorbei</h2>

            {teaser && teaser.punkte.length >= 2 && (
              <div className="mt-4">
                <WachstumsKurve
                  punkte={teaser.punkte}
                  unscharf
                  beschreibung={`Verlauf deiner Zuschauerzahlen ueber ${teaser.tage} Tage`}
                />
                <p className="mt-2 text-sm text-white/50">
                  {teaser.tage} Tage, {teaser.sitzungen} Streams ausgewertet
                </p>
              </div>
            )}

            <p className="mt-4 text-white/70">
              Deine Daten laufen weiter mit. Sichtbar sind sie wieder mit Premium.
            </p>

            <div className="mt-6 flex flex-col gap-2 sm:flex-row">
              <button
                type="button"
                onClick={schliessen}
                data-press
                className="flex-1 rounded-xl border border-border bg-white/5 px-4 py-2.5 text-sm text-white"
              >
                Später
              </button>
              <a
                href={PREVIEW_PRICING_ROUTE}
                data-press
                className="flex-1 rounded-xl bg-primary px-4 py-2.5 text-center text-sm font-semibold text-[#0D0806]"
                onClick={() => schluessel && localStorage.setItem(schluessel, new Date().toISOString())}
              >
                Premium ansehen
              </a>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
