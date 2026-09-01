import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Sparkles, ShieldAlert, ShieldCheck, Wifi, Shield, Film } from 'lucide-react';
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
import { fehlerText, ZUGRIFF_LABELS } from '@/components/socialmedia/labels';

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

  const {
    data: streamers = [],
    isLoading: loadingStreamers,
    isError: streamerListError,
    error: streamerListFehler,
  } = useStreamerList();
  const { data: authStatus, isLoading: loadingAuth, isError: authError } = useAuthStatus();

  // Was diese Session darf: Admin sieht alles, Partner nur nach Freigabe.
  const {
    data: access,
    isLoading: loadingAccess,
    isFetching: fetchingAccess,
    isError: accessError,
    error: accessFehler,
  } = useQuery({
    queryKey: ['social-media-access'],
    queryFn: fetchMyAccess,
    staleTime: 60 * 1000,
    retry: false,
  });

  const isAdminView = Boolean(authStatus?.isAdmin || authStatus?.isLocalhost);

  const queryClient = useQueryClient();
  const {
    data: accessList,
    isLoading: loadingAccessList,
    isFetching: fetchingAccessList,
    isError: accessListError,
    error: accessListFehler,
  } = useQuery({
    queryKey: ['social-media-access-list'],
    queryFn: fetchPartnerAccessList,
    enabled: isAdminView,
    staleTime: 60 * 1000,
    retry: false,
  });

  const accessMutation = useMutation({
    mutationFn: ({ login, granted }: { login: string; granted: boolean }) =>
      setPartnerAccess(login, granted),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['social-media-access-list'] });
    },
  });

  const accessListKnown =
    isAdminView &&
    !loadingAccessList &&
    !fetchingAccessList &&
    !accessListError &&
    accessList !== undefined;
  const accessKnown =
    !loadingAccess && !fetchingAccess && !accessError && access !== undefined;
  const mutationStandKnown = accessListKnown && accessKnown && !streamerListError;
  const selectedGranted = mutationStandKnown && (accessList ?? []).some(
    (entry) => entry.streamer_login.toLowerCase() === streamer && entry.granted,
  );
  const canChangeAccess =
    !!streamer &&
    mutationStandKnown &&
    !accessMutation.isPending;
  const accessMutationFehler = fehlerText(accessMutation.error, t);

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
      'flex min-h-11 max-w-full items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold tracking-wide backdrop-blur-md';
    if (loadingAuth) {
      return (
        <div role="status" aria-live="polite" className={`${base} border-border bg-background/80 text-text-secondary`}>
          <Shield aria-hidden="true" className="w-4 h-4 shrink-0" />
          <span>{t('Anmeldung wird geprüft…')}</span>
        </div>
      );
    }
    if (isDemoMode) {
      return (
        <div className={`${base} bg-warning/10 border-warning/30 text-warning`}>
          <Sparkles aria-hidden="true" className="w-4 h-4 shrink-0" />
          <span>{t('Demo-Daten')}</span>
        </div>
      );
    }
    if (authError || !authStatus?.authenticated) {
      return (
        <div role="alert" className={`${base} bg-error/10 border-error/30 text-error`}>
          <ShieldAlert aria-hidden="true" className="w-4 h-4 shrink-0" />
          <span>{t('Nicht authentifiziert')}</span>
        </div>
      );
    }
    if (authStatus.isLocalhost) {
      return (
        <div className={`${base} bg-success/10 border-success/30 text-success`}>
          <Wifi aria-hidden="true" className="w-4 h-4 shrink-0" />
          <span>{t('Localhost (Admin)')}</span>
        </div>
      );
    }
    if (authStatus.isAdmin) {
      return (
        <div className={`${base} bg-primary/10 border-primary/30 text-primary`}>
          <ShieldCheck aria-hidden="true" className="w-4 h-4 shrink-0" />
          <span>{t('Admin')}</span>
        </div>
      );
    }
    return (
      <div className={`${base} bg-accent/10 border-accent/30 text-accent`}>
        <Shield aria-hidden="true" className="w-4 h-4 shrink-0" />
        <span>{t('Partner')}</span>
      </div>
    );
  };

  return (
    <div className="min-h-screen overflow-x-hidden relative px-3 py-4 md:px-7 md:py-8">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-28 right-[-7rem] h-[25rem] w-[25rem] rounded-full bg-primary/12 blur-3xl" />
        <div className="absolute top-[28%] -left-24 h-[20rem] w-[20rem] rounded-full bg-accent/14 blur-3xl" />
      </div>
      <div className="relative max-w-[1700px] mx-auto">
        <header className="mb-6 flex min-w-0 flex-col gap-4 sm:flex-row sm:flex-wrap sm:items-center sm:justify-between">
          <div className="flex min-w-0 items-center gap-3">
            <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl border border-primary/30 bg-primary/15">
              <Film aria-hidden="true" className="w-5 h-5 text-primary" />
            </div>
            <div className="min-w-0">
              <div className="text-[11px] uppercase tracking-[0.18em] font-bold text-primary/90">
                {isAdminView ? t('Alle Kanäle') : t('Dein Kanal')}
              </div>
              <h1 className="display-font break-words font-extrabold text-white text-xl md:text-2xl tracking-tight leading-tight">
                {t('Social Media')}
              </h1>
            </div>
          </div>
          <div className="flex w-full min-w-0 flex-wrap items-stretch gap-3 sm:w-auto sm:items-center sm:justify-end">
            {isAdminView && streamer && (
              <button
                type="button"
                onClick={() => {
                  if (canChangeAccess) {
                    accessMutation.mutate({ login: streamer, granted: !selectedGranted });
                  }
                }}
                disabled={!canChangeAccess}
                title={
                  !mutationStandKnown
                    ? t('Der Freigabestand wird geprüft. Änderungen sind gesperrt.')
                    : selectedGranted
                      ? t('Freigabe für diesen Streamer entziehen')
                      : t('Diesen Streamer für das eigene Social-Media-Dashboard freischalten')
                }
                className={`min-h-11 w-full rounded-xl border px-3 py-2 text-sm font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60 sm:w-auto ${
                  selectedGranted
                    ? 'border-success/40 bg-success/10 text-success hover:bg-success/20'
                    : 'border-border bg-background/80 text-text-secondary hover:text-white'
                }`}
              >
                {!mutationStandKnown
                  ? t('Freigabestand wird geprüft…')
                  : selectedGranted
                    ? t(ZUGRIFF_LABELS.granted)
                    : t(ZUGRIFF_LABELS.grant)}
              </button>
            )}
            {isAdminView && (
              <div className="flex w-full min-w-0 flex-col gap-1.5 sm:w-auto">
                <label htmlFor="social-media-admin-streamer" className="text-xs font-semibold text-text-secondary">
                  {t('Streamer auswählen')}
                </label>
                <select
                  id="social-media-admin-streamer"
                  value={streamer}
                  onChange={(event) => {
                    accessMutation.reset();
                    hasAutoSetStreamer.current = true;
                    setStreamer(event.target.value);
                  }}
                  disabled={loadingStreamers || streamerListError}
                  aria-invalid={streamerListError || undefined}
                  className="min-h-11 w-full min-w-0 max-w-full rounded-xl border border-primary/70 bg-background/80 px-3 py-2 text-sm font-medium text-white transition-colors focus-visible:border-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-60 sm:w-auto sm:min-w-[220px]"
                >
                  <option value="">{t('— Streamer wählen —')}</option>
                  {streamers.map((channel) => (
                    <option key={channel.login} value={channel.login.toLowerCase()}>
                      {channel.login}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <a
              href="/analyse"
              className="inline-flex min-h-11 items-center rounded-lg px-1 text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            >
              {t('← Analyse-Dashboard')}
            </a>
            <AuthBadge />
          </div>
        </header>

        <div className="mb-6 space-y-3">
          {loadingStreamers && (
            <div role="status" aria-live="polite" className="rounded-xl border border-border bg-background/70 p-3 text-sm text-text-secondary">
              {t('Lade Streamer-Liste…')}
            </div>
          )}
          {streamerListError && (
            <div role="alert" className="rounded-xl border border-error/35 bg-error/10 p-3 text-sm text-error">
              {t('Die Streamer-Liste konnte nicht geladen werden.')} {fehlerText(streamerListFehler, t)}
            </div>
          )}
          {isAdminView && (loadingAccessList || fetchingAccessList) && !accessListError && (
            <div role="status" aria-live="polite" className="rounded-xl border border-border bg-background/70 p-3 text-sm text-text-secondary">
              {t('Freigabestand wird geprüft…')}
            </div>
          )}
          {isAdminView && accessListError && (
            <div role="alert" className="rounded-xl border border-error/35 bg-error/10 p-3 text-sm text-error">
              {t('Der Freigabestand konnte nicht geladen werden. Änderungen bleiben gesperrt.')} {fehlerText(accessListFehler, t)}
            </div>
          )}
          {isAdminView && accessError && (
            <div role="alert" className="rounded-xl border border-error/35 bg-error/10 p-3 text-sm text-error">
              {t('Der eigene Social-Media-Zugriff konnte nicht geprüft werden.')} {fehlerText(accessFehler, t)}
            </div>
          )}
          {accessMutation.isPending && (
            <div role="status" aria-live="polite" className="rounded-xl border border-border bg-background/70 p-3 text-sm text-text-secondary">
              {t('Freigabe wird gespeichert…')}
            </div>
          )}
          {accessMutationFehler && (
            <div role="alert" className="rounded-xl border border-error/35 bg-error/10 p-3 text-sm text-error">
              {t('Die Freigabe konnte nicht gespeichert werden.')} {accessMutationFehler}
            </div>
          )}
          {accessMutation.isSuccess && !accessMutation.isPending && !accessMutationFehler && (
            <div role="status" aria-live="polite" className="rounded-xl border border-success/35 bg-success/10 p-3 text-sm text-success">
              {t('Die Freigabe wurde gespeichert.')}
            </div>
          )}
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
            <SocialMedia streamer={streamer} isAdmin />
          ) : !accessKnown ? (
            accessError ? (
              <div role="alert" className="panel-card rounded-2xl border border-error/35 p-8 text-center text-error">
                <ShieldAlert aria-hidden="true" className="w-12 h-12 text-error mx-auto mb-4" />
                <h2 className="text-xl font-bold text-white mb-2">{t('Zugriff konnte nicht geprüft werden')}</h2>
                <p>{t('Der Social-Media-Bereich bleibt sicherheitshalber gesperrt.')} {fehlerText(accessFehler, t)}</p>
              </div>
            ) : (
              <div role="status" aria-live="polite" className="panel-card rounded-2xl p-8 text-center text-text-secondary">
                {t('Zugriff wird geprüft…')}
              </div>
            )
          ) : access?.allowed ? (
            <SocialMedia streamer={access.streamer ?? streamer} isAdmin={false} />
          ) : (
            <div className="panel-card rounded-2xl p-8 text-center">
              <ShieldAlert aria-hidden="true" className="w-12 h-12 text-warning mx-auto mb-4" />
              <h2 className="text-xl font-bold text-white mb-2">{t('Noch nicht freigeschaltet')}</h2>
              <p className="text-text-secondary">
                {t(
                  'Social Media wird für deinen Kanal erst nach Freigabe aktiv. Melde dich bei EarlySalty, wenn du deine Clips hier aufbereiten möchtest.',
                )}
              </p>
            </div>
          )}
        </PlanProvider>
        {!loadingStreamers && !streamerListError && streamers.length === 0 && isAdminView && (
          <div className="mt-4 text-xs text-text-secondary">{t('Keine Streamer gefunden.')}</div>
        )}
      </div>
    </div>
  );
}
