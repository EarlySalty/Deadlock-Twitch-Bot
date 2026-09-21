import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Sparkles, ShieldAlert, ShieldCheck, Wifi, Shield } from 'lucide-react';
import { SocialMedia } from '@/pages/SocialMedia';
import { PlanProvider } from '@/context/PlanContext';
import { useT } from '@/context/LanguageContext';
import { TrialExpiryModal } from '@/components/modals/TrialExpiryModal';
import { TrialBanner } from '@/components/banners/TrialBanner';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import {
  fetchMyAccess,
  fetchPartnerAccessList,
  setPartnerAccess,
} from '@/api/socialMedia';
import { dashboardRuntimeConfig, resolveEffectiveDemoMode } from '@/runtimeConfig';
import { ZUGRIFF_LABELS } from '@/components/socialmedia/labels';

/**
 * Eigenständiges Social-Media-Admin-Dashboard.
 *
 * Bewusst nicht im Analyse-Dashboard: Social Media ist ein eigener Bereich
 * (Clip-Pipeline, Layout-Editor, Auto-Aufbereitung, Discord-Approval) und hat
 * mit den Streamer-Analytics keine Überschneidung. Wird unter `/social-media-admin`
 * gemountet, ist Admin-only und liefert dieselbe React-Bundle-Auslieferung.
 */
export function SocialMediaAdminDashboard() {
  const t = useT();
  const [streamer, setStreamer] = useState<string>('');
  const hasAutoSetStreamer = useRef(false);

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus, isLoading: loadingAuth, isError: authError } = useAuthStatus();

  // Was diese Session darf: Admin sieht alles, Partner nur nach Freigabe.
  const { data: access, isLoading: loadingAccess } = useQuery({
    queryKey: ['social-media-access'],
    queryFn: fetchMyAccess,
    staleTime: 60 * 1000,
    retry: false,
  });

  const isAdminView = Boolean(authStatus?.isAdmin || authStatus?.isLocalhost);

  const queryClient = useQueryClient();
  const { data: accessList = [] } = useQuery({
    queryKey: ['social-media-access-list'],
    queryFn: fetchPartnerAccessList,
    enabled: isAdminView,
    staleTime: 60 * 1000,
    retry: false,
  });

  const accessMutation = useMutation({
    mutationFn: ({ login, granted }: { login: string; granted: boolean }) =>
      setPartnerAccess(login, granted),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media-access-list'] });
    },
  });

  const selectedGranted = accessList.some(
    (entry) => entry.streamer_login.toLowerCase() === streamer && entry.granted,
  );

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
      'inline-flex items-center gap-1.5 rounded-full border border-white/[0.08] bg-white/[0.04] px-2.5 py-1 text-xs font-medium backdrop-blur-sm';
    if (loadingAuth) return null;
    if (isDemoMode) {
      return (
        <div className={`${base} text-ui-warning`}>
          <Sparkles className="w-4 h-4" />
          <span>{t('Demo-Daten')}</span>
        </div>
      );
    }
    if (authError || !authStatus?.authenticated) {
      return (
        <div className={`${base} text-ui-danger-soft`}>
          <ShieldAlert className="w-4 h-4" />
          <span>{t('Nicht authentifiziert')}</span>
        </div>
      );
    }
    if (authStatus.isLocalhost) {
      return (
        <div className={`${base} text-ui-success-soft`}>
          <Wifi className="w-4 h-4" />
          <span>{t('Localhost (Admin)')}</span>
        </div>
      );
    }
    if (authStatus.isAdmin) {
      return (
        <div className={`${base} text-ui-accent-ink`}>
          <ShieldCheck className="w-4 h-4" />
          <span>{t('Admin')}</span>
        </div>
      );
    }
    return (
      <div className={`${base} text-ui-text-soft`}>
        <Shield className="w-4 h-4" />
        <span>{t('Partner')}</span>
      </div>
    );
  };

  return (
    <>
      <div className="border-b border-border py-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <p className="text-xs font-medium text-ui-faint">{t('Arbeitsbereich')}</p>
            <p className="mt-0.5 truncate text-sm font-medium text-ui-text-soft">
              {isAdminView ? t('Kanal auswählen und verwalten') : t('Dein Kanal')}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {isAdminView && streamer && (
              <button
                type="button"
                onClick={() =>
                  accessMutation.mutate({ login: streamer, granted: !selectedGranted })
                }
                disabled={accessMutation.isPending}
                title={
                  selectedGranted
                    ? t('Freigabe für diesen Streamer entziehen')
                    : t('Diesen Streamer für das eigene Social-Media-Dashboard freischalten')
                }
                className={`rounded-lg border px-3 py-2 text-sm font-medium transition-colors ${
                  selectedGranted
                    ? 'border-ui-success/20 bg-ui-success/10 text-ui-success-soft hover:bg-ui-success/15'
                    : 'border-white/[0.08] bg-white/[0.04] text-ui-text-soft hover:bg-white/[0.08] hover:text-white'
                }`}
              >
                {selectedGranted ? t(ZUGRIFF_LABELS.granted) : t(ZUGRIFF_LABELS.grant)}
              </button>
            )}
            {isAdminView && (
              <select
                aria-label={t('Streamer wählen')}
                value={streamer}
                onChange={(event) => {
                  if (document.querySelector('[data-unsaved="true"]') && !window.confirm(t('Ungespeicherte Änderungen verwerfen?'))) return;
                  hasAutoSetStreamer.current = true;
                  setStreamer(event.target.value);
                }}
                disabled={loadingStreamers}
                className="min-w-44 rounded-lg border border-white/[0.08] bg-ui-elevated px-3 py-2 text-sm font-medium text-ui-text outline-none transition-colors focus:border-ui-accent-strong/40"
              >
                <option value="">{t('Streamer wählen')}</option>
                {streamers.map((channel) => (
                  <option key={channel.login} value={channel.login.toLowerCase()}>
                    {channel.login}
                  </option>
                ))}
              </select>
            )}
            <AuthBadge />
          </div>
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

          {isAdminView ? (
            <SocialMedia key={streamer} streamer={streamer} isAdmin />
          ) : loadingAccess ? (
            <div className="panel-card rounded-2xl p-8 text-center text-text-secondary">
              {t('Zugriff wird geprüft…')}
            </div>
          ) : access?.allowed ? (
            <SocialMedia key={access.streamer ?? streamer} streamer={access.streamer ?? streamer} isAdmin={false} />
          ) : (
            <div className="panel-card rounded-2xl p-8 text-center">
              <ShieldAlert className="w-12 h-12 text-warning mx-auto mb-4" />
              <h2 className="text-xl font-bold text-white mb-2">{t('Noch nicht freigeschaltet')}</h2>
              <p className="text-text-secondary">
                {t(
                  'Social Media wird für deinen Kanal erst nach Freigabe aktiv. Melde dich bei EarlySalty, wenn du deine Clips hier aufbereiten möchtest.',
                )}
              </p>
            </div>
          )}
        </PlanProvider>
        {loadingStreamers && (
          <div className="mt-4 text-xs text-text-secondary">{t('Lade Streamer-Liste…')}</div>
        )}
        {!loadingStreamers && streamers.length === 0 && authStatus?.isAdmin && (
          <div className="mt-4 text-xs text-text-secondary">{t('Keine Streamer gefunden.')}</div>
        )}
    </>
  );
}
