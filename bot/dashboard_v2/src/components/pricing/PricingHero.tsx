import { useState } from 'react';
import { motion } from 'framer-motion';
import { Zap, TrendingUp, Users, Loader2, Sparkles, CreditCard, ArrowRight } from 'lucide-react';
import { PREVIEW_BILLING_ROUTE, isPreviewModeEnabled } from '../../preview/routes';

type TrialState = 'idle' | 'loading' | 'granted' | 'already_used' | 'has_paid_plan' | 'error';

async function startTrial(): Promise<TrialState> {
  try {
    // P1.45: Same-Origin-CSRF statt erzwungenem X-CSRF-Token-Header (auth-status
    // liefert csrfToken:null → harter Header-Zwang erzeugte Prod-403, Vorfall #241c11).
    // Same-Origin-Fetch (Browser sendet Origin/Referer) + same-site Session-Cookie
    // decken den CSRF-Vektor ab; X-Requested-With signalisiert einen XHR.
    const res = await fetch('/twitch/api/billing/trial/start', {
      method: 'POST',
      credentials: 'same-origin',
      mode: 'same-origin',
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
    });
    if (res.status === 401) {
      const data = await res.json().catch(() => ({}));
      window.location.href = data.login_url ?? '/twitch/auth/login?next=%2Ftwitch%2Fpricing';
      return 'loading';
    }
    const data = await res.json().catch(() => ({}));
    return (data.status as TrialState) ?? 'error';
  } catch {
    return 'error';
  }
}

const MSG: Record<string, string> = {
  granted: 'Deine 30-Tage-Testphase läuft! Du wirst weitergeleitet…',
  already_used: 'Du hast die kostenlose Testphase bereits genutzt.',
  has_paid_plan: 'Du hast bereits ein aktives Abo.',
  error: 'Etwas ist schiefgelaufen. Bitte versuche es erneut.',
};

export default function PricingHero() {
  const [trialState, setTrialState] = useState<TrialState>('idle');

  const handleTrial = async () => {
    if (isPreviewModeEnabled()) {
      window.location.href = PREVIEW_BILLING_ROUTE;
      return;
    }
    setTrialState('loading');
    const result = await startTrial();
    setTrialState(result);
    if (result === 'granted') {
      setTimeout(() => { window.location.href = '/twitch/dashboard'; }, 1500);
    }
  };

  const isBlocked = trialState === 'already_used' || trialState === 'has_paid_plan';

  return (
    <section className="relative text-center mb-12 overflow-hidden">
      <div className="absolute inset-0 -z-10">
        <div className="absolute inset-0 bg-gradient-to-b from-[#c8a86b]/10 via-transparent to-transparent" />
        <div
          className="absolute inset-0 opacity-30"
          style={{ background: 'radial-gradient(ellipse 80% 50% at 50% 0%, rgba(201, 168, 106, 0.15), transparent 70%)' }}
        />
      </div>

      <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.5 }}>
        <div className="inline-flex items-center gap-2 px-4 py-1.5 rounded-full bg-[#c8a86b]/10 border border-[#c8a86b]/20 mb-6">
          <Zap className="w-4 h-4 text-[#c8a86b]" />
          <span className="text-sm font-medium text-[#c8a86b]">Dein Growth Coach für Twitch</span>
        </div>

        <h1 className="text-4xl md:text-5xl font-bold text-white mb-4 tracking-tight">
          Mehr Wachstum, mehr{' '}
          <span className="text-transparent bg-clip-text bg-gradient-to-r from-[#c8a86b] to-[#55978f]">
            Insights
          </span>
        </h1>

        <p className="text-lg md:text-xl text-white/60 max-w-2xl mx-auto mb-8">
          Verstehe deine Zuschauer, optimiere deinen Content und wachse schneller –
          mit KI-gestützten Analysen, die dir zeigen, was wirklich funktioniert.
        </p>

        <div className="flex flex-wrap justify-center gap-6 mb-8">
          <div className="flex items-center gap-2 text-white/50">
            <TrendingUp className="w-5 h-5 text-[#55978f]" />
            <span>Tracke deinen Fortschritt</span>
          </div>
          <div className="flex items-center gap-2 text-white/50">
            <Users className="w-5 h-5 text-[#55978f]" />
            <span>Verstehe deine Community</span>
          </div>
        </div>

        <button
          onClick={handleTrial}
          disabled={trialState === 'loading' || trialState === 'granted' || isBlocked}
          className={`inline-flex items-center gap-2 px-8 py-4 rounded-xl font-semibold text-lg transition-all duration-200 ${
            trialState === 'granted'
              ? 'bg-[#55978f] text-white shadow-lg shadow-[#55978f]/25'
              : isBlocked
              ? 'bg-white/10 text-white/40 cursor-default'
              : trialState === 'error'
              ? 'bg-danger/80 text-white'
              : 'bg-gradient-to-r from-[#c8a86b] to-[#efd49d] text-white shadow-lg shadow-[#c8a86b]/25 hover:shadow-[#c8a86b]/40 hover:scale-105'
          }`}
        >
          {trialState === 'loading' && <Loader2 className="w-5 h-5 animate-spin" />}
          {trialState === 'idle' && '30 Tage kostenlos starten'}
          {trialState === 'loading' && 'Wird gestartet…'}
          {trialState === 'granted' && '✓ Testphase gestartet!'}
          {(isBlocked || trialState === 'error') && (MSG[trialState] ?? '30 Tage kostenlos starten')}
        </button>

        {isBlocked && (
          <p className="mt-3 text-sm text-white/40">{MSG[trialState]}</p>
        )}

        {/* Callout — visuell verbunden, kein separates Card */}
        <div className="mt-8 pt-6 border-t border-white/8 flex flex-col sm:flex-row items-center justify-between gap-4 max-w-xl mx-auto">
          <div className="flex items-center gap-3 text-left">
            <div className="flex-shrink-0 w-9 h-9 rounded-xl bg-gradient-to-br from-[#c8a86b]/20 to-[#55978f]/20 flex items-center justify-center border border-[#c8a86b]/25">
              <Sparkles className="w-4 h-4 text-[#c8a86b]" />
            </div>
            <div>
              <p className="text-white/80 text-sm font-medium flex items-center gap-1.5">
                <CreditCard className="w-3.5 h-3.5 text-white/35 flex-shrink-0" />
                Keine Kreditkarte erforderlich – risikofrei starten
              </p>
              <p className="text-white/35 text-xs mt-0.5">Einmalig pro Account · Jederzeit kündbar</p>
            </div>
          </div>
          <a
            href="#plans"
            className="flex-shrink-0 flex items-center gap-1.5 px-4 py-2 rounded-xl bg-white/6 hover:bg-white/10 border border-white/12 text-white/70 hover:text-white text-sm font-medium transition-all duration-200"
          >
            Mehr erfahren
            <ArrowRight className="w-3.5 h-3.5" />
          </a>
        </div>
      </motion.div>
    </section>
  );
}
