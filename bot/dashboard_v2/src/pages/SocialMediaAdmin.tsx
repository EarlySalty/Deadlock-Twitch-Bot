import { useEffect, useRef, useState } from 'react';
import { Sparkles, ShieldAlert, ShieldCheck, Wifi, Shield, Film } from 'lucide-react';
import { SocialMedia } from '@/pages/SocialMedia';
import { PlanProvider } from '@/context/PlanContext';
import { TrialExpiryModal } from '@/components/modals/TrialExpiryModal';
import { TrialBanner } from '@/components/banners/TrialBanner';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import { dashboardRuntimeConfig, resolveEffectiveDemoMode } from '@/runtimeConfig';

/**
 * Eigenständiges Social-Media-Admin-Dashboard.
 *
 * Bewusst nicht im Analyse-Dashboard: Social Media ist ein eigener Bereich
 * (Clip-Pipeline, Layout-Editor, Auto-Aufbereitung, Discord-Approval) und hat
 * mit den Streamer-Analytics keine Überschneidung. Wird unter `/social-media-admin`
 * gemountet, ist Admin-only und liefert dieselbe React-Bundle-Auslieferung.
 */
export function SocialMediaAdminDashboard() {
  const [streamer, setStreamer] = useState<string>('');
  const hasAutoSetStreamer = useRef(false);

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus, isLoading: loadingAuth, isError: authError } = useAuthStatus();

  const isDemoShell = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });
  const isDemoMode = isDemoShell;

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const urlStreamer = params.get('streamer');
    if (urlStreamer) {
      const normalized = urlStreamer.trim().toLowerCase();
      if (
        !isDemoShell ||
        dashboardRuntimeConfig.allowedDemoProfiles.length === 0 ||
        dashboardRuntimeConfig.allowedDemoProfiles.includes(normalized)
      ) {
        setStreamer(normalized);
        hasAutoSetStreamer.current = true;
      }
    }
  }, [isDemoShell]);

  useEffect(() => {
    const fallback =
      authStatus?.twitchLogin ??
      (isDemoShell ? dashboardRuntimeConfig.defaultDemoProfile : null);
    if (!hasAutoSetStreamer.current && fallback) {
      setStreamer(fallback);
      hasAutoSetStreamer.current = true;
    }
  }, [authStatus, isDemoShell]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (streamer) {
      params.set('streamer', streamer);
    } else {
      params.delete('streamer');
    }
    const qs = params.toString();
    const newUrl = qs
      ? `${window.location.pathname}?${qs}`
      : window.location.pathname;
    window.history.replaceState({}, '', newUrl);
  }, [streamer]);

  const AuthBadge = () => {
    const base =
      'flex items-center gap-2 px-3 py-1.5 rounded-full border text-xs font-semibold tracking-wide backdrop-blur-md';
    if (loadingAuth) return null;
    if (isDemoMode) {
      return (
        <div className={`${base} bg-warning/10 border-warning/30 text-warning`}>
          <Sparkles className="w-4 h-4" />
          <span>Demo-Daten</span>
        </div>
      );
    }
    if (authError || !authStatus?.authenticated) {
      return (
        <div className={`${base} bg-error/10 border-error/30 text-error`}>
          <ShieldAlert className="w-4 h-4" />
          <span>Nicht authentifiziert</span>
        </div>
      );
    }
    if (authStatus.isLocalhost) {
      return (
        <div className={`${base} bg-success/10 border-success/30 text-success`}>
          <Wifi className="w-4 h-4" />
          <span>Localhost (Admin)</span>
        </div>
      );
    }
    if (authStatus.isAdmin) {
      return (
        <div className={`${base} bg-primary/10 border-primary/30 text-primary`}>
          <ShieldCheck className="w-4 h-4" />
          <span>Admin</span>
        </div>
      );
    }
    return (
      <div className={`${base} bg-accent/10 border-accent/30 text-accent`}>
        <Shield className="w-4 h-4" />
        <span>Partner</span>
      </div>
    );
  };

  return (
    <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-28 right-[-7rem] h-[25rem] w-[25rem] rounded-full bg-primary/12 blur-3xl" />
        <div className="absolute top-[28%] -left-24 h-[20rem] w-[20rem] rounded-full bg-accent/14 blur-3xl" />
      </div>
      <div className="relative max-w-[1700px] mx-auto">
        <div className="flex justify-between items-center mb-6">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-primary/15 border border-primary/30 grid place-items-center">
              <Film className="w-5 h-5 text-primary" />
            </div>
            <div>
              <div className="text-[11px] uppercase tracking-[0.18em] font-bold text-primary/90">
                Admin-Tooling
              </div>
              <h1 className="display-font font-extrabold text-white text-xl md:text-2xl tracking-tight leading-tight">
                Social-Media-Pipeline
              </h1>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <a
              href="/analyse"
              className="text-xs text-text-secondary hover:text-text-primary transition-colors"
            >
              ← Analyse-Dashboard
            </a>
            <AuthBadge />
          </div>
        </div>

        <PlanProvider
          plan={authStatus?.plan ?? null}
          isAdmin={authStatus?.isAdmin ?? false}
          isLocalhost={authStatus?.isLocalhost ?? false}
          isDemoMode={isDemoMode}
        >
          <TrialExpiryModal />
          <TrialBanner />

          {!authStatus?.isAdmin && !authStatus?.isLocalhost ? (
            <div className="panel-card rounded-2xl p-8 text-center">
              <ShieldAlert className="w-12 h-12 text-warning mx-auto mb-4" />
              <h2 className="text-xl font-bold text-white mb-2">Admin-Zugriff erforderlich</h2>
              <p className="text-text-secondary">
                Das Social-Media-Dashboard ist ein internes Admin-Tool und für Partner-Streamer
                aktuell nicht freigegeben.
              </p>
            </div>
          ) : (
            <SocialMedia streamer={streamer} />
          )}
        </PlanProvider>
        {/* Streamer-Liste & Loading-Indikator absichtlich ohne Header — die SocialMedia-
            Page hat eigene Streamer-Auswahl + KPI-Hero. */}
        {loadingStreamers && (
          <div className="mt-4 text-xs text-text-secondary">Lade Streamer-Liste…</div>
        )}
        {!loadingStreamers && streamers.length === 0 && authStatus?.isAdmin && (
          <div className="mt-4 text-xs text-text-secondary">Keine Streamer gefunden.</div>
        )}
      </div>
    </div>
  );
}
