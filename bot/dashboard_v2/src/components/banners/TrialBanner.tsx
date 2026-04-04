import { useState, useEffect } from 'react';
import { X, Sparkles } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';

const DISMISS_KEY = 'trial-banner-dismissed';

export function TrialBanner() {
  const { trialInfo, tier } = usePlan();
  const [dismissed, setDismissed] = useState(false);

  // Re-check dismiss state on mount (in case user upgraded in another session)
  useEffect(() => {
    // If no longer in trial or upgraded to extended, don't show
    if (!trialInfo?.isInTrial || tier === 'extended') {
      setDismissed(true);
      return;
    }

    const wasDismissed = localStorage.getItem(DISMISS_KEY);
    if (wasDismissed) {
      // Check if the dismissed date was before trial ended
      const dismissedDate = new Date(wasDismissed);
      const trialEnd = trialInfo.trialEndsAt ? new Date(trialInfo.trialEndsAt) : null;
      if (!trialEnd || dismissedDate > trialEnd) {
        // Trial ended after dismiss, reshow
        setDismissed(false);
      }
    }
  }, [trialInfo, tier]);

  if (!trialInfo?.isInTrial || tier === 'extended' || dismissed) {
    return null;
  }

  const handleDismiss = () => {
    localStorage.setItem(DISMISS_KEY, new Date().toISOString());
    setDismissed(true);
  };

  const handleUpgrade = () => {
    window.location.href = '/twitch/pricing';
  };

  return (
    <div className="mb-6 rounded-xl border border-primary/30 bg-gradient-to-r from-primary/20 via-accent/20 to-primary/20 p-4 shadow-lg backdrop-blur-sm">
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="flex items-start gap-3">
          <div className="rounded-lg bg-primary/30 p-2 text-white">
            <Sparkles className="h-5 w-5" />
          </div>
          <div className="space-y-1">
            <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-white/70">
              <span>Kostenlose Testphase</span>
            </div>
            <h3 className="text-lg font-semibold text-white">
              Noch {trialInfo.trialDaysRemaining} Tage kostenlos testen
            </h3>
            <p className="text-sm text-white/70">
              Alle Analytics-Funktionen ohne Einschränkungen nutzen.
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 rounded-lg border border-white/20 bg-white/10 px-4 py-2">
            <span className="text-2xl font-bold text-white">{trialInfo.trialDaysRemaining}</span>
            <span className="text-xs leading-tight text-white/70">
              <div>Tage</div>
              <div>verbleibend</div>
            </span>
          </div>

          <button
            type="button"
            onClick={handleUpgrade}
            className="flex items-center gap-2 rounded-lg bg-white px-4 py-2 text-sm font-semibold text-primary transition hover:bg-white/90"
          >
            Jetzt upgraden
          </button>

          <button
            type="button"
            onClick={handleDismiss}
            className="rounded-lg p-2 text-white/60 hover:bg-white/10 hover:text-white transition-colors"
            aria-label="Banner schliessen"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>
    </div>
  );
}
