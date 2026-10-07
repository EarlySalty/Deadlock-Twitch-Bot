import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Clapperboard, ShieldAlert } from 'lucide-react';
import { DashboardHeader } from '@/components/layout/DashboardHeader';
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
import { resolveSocialMediaChannel } from '@/utils/socialMediaChannel';

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
  const [streamerUserId, setStreamerUserId] = useState('');
  const requestedChannel = useRef(new URLSearchParams(window.location.search));
  const hasAutoSetStreamer = useRef(false);

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus } = useAuthStatus();
  const selectedChannel = resolveSocialMediaChannel(streamers, streamerUserId);
  const streamer = selectedChannel?.login.toLowerCase() ?? '';

  // Was diese Session darf: Admin sieht alles, Partner nur nach Freigabe.
  const { data: access, isLoading: loadingAccess } = useQuery({
    queryKey: ['social-media-access', authStatus?.twitchUserId],
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
    mutationFn: ({ twitchUserId, granted }: { twitchUserId: string; granted: boolean }) =>
      setPartnerAccess(twitchUserId, granted),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media-access-list'] });
      queryClient.invalidateQueries({ queryKey: ['social-media-access'] });
    },
  });

  const selectedGranted = accessList.some(
    (entry) => entry.twitch_user_id === selectedChannel?.twitchUserId && entry.granted,
  );

  const isDemoShell = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });
  const isDemoMode = isDemoShell;

  useEffect(() => {
    if (!isAdminView || loadingStreamers || hasAutoSetStreamer.current) return;
    const params = requestedChannel.current;
    const hasRequestedChannel = params.has('streamer') || params.has('twitch_user_id');
    const channel = resolveSocialMediaChannel(
      streamers,
      hasRequestedChannel ? params.get('twitch_user_id') : authStatus?.twitchUserId,
      hasRequestedChannel ? params.get('streamer') : null,
    );
    const allowed = channel && (!isDemoShell || dashboardRuntimeConfig.allowedDemoProfiles.length === 0
      || dashboardRuntimeConfig.allowedDemoProfiles.includes(channel.login.toLowerCase()));
    setStreamerUserId(allowed ? channel.twitchUserId ?? '' : '');
    hasAutoSetStreamer.current = true;
  }, [streamers, loadingStreamers, isAdminView, authStatus?.twitchUserId, isDemoShell]);

  useEffect(() => {
    if (!isAdminView || !hasAutoSetStreamer.current) return;
    const params = new URLSearchParams(window.location.search);
    if (streamer) {
      params.set('streamer', streamer);
    } else {
      params.delete('streamer');
    }
    if (selectedChannel?.twitchUserId) {
      params.set('twitch_user_id', selectedChannel.twitchUserId);
    } else {
      params.delete('twitch_user_id');
    }
    const qs = params.toString();
    const newUrl = qs
      ? `${window.location.pathname}?${qs}`
      : window.location.pathname;
    window.history.replaceState({}, '', newUrl);
  }, [streamer, selectedChannel, isAdminView]);

  return (
    <div className="space-y-6">
      <DashboardHeader
        title={t('Social Media')}
        icon={<Clapperboard className="w-6 h-6 text-primary" />}
        isLoading={loadingStreamers}
        description={t('Fokus: {focus}', { focus: isAdminView ? streamer || t('Streamer wählen') : access?.streamer || authStatus?.twitchLogin || t('Dein Kanal') })}
      >
        {isAdminView && <div className="flex flex-wrap items-center gap-3">
            <a href="/social-media-admin?view=archiv" className="studio-button">{t('VOD-Archiv')}</a>
            {isAdminView && streamer && (
              <button
                type="button"
                onClick={() =>
                  selectedChannel?.twitchUserId && accessMutation.mutate({ twitchUserId: selectedChannel.twitchUserId, granted: !selectedGranted })
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
                value={selectedChannel?.twitchUserId ?? ''}
                onChange={(event) => {
                  if (document.querySelector('[data-unsaved="true"]') && !window.confirm(t('Ungespeicherte Änderungen verwerfen?'))) return;
                  hasAutoSetStreamer.current = true;
                  const selected = resolveSocialMediaChannel(streamers, event.target.value);
                  setStreamerUserId(selected?.twitchUserId ?? '');
                }}
                disabled={loadingStreamers}
                className="min-w-44 rounded-lg border border-white/[0.08] bg-ui-elevated px-3 py-2 text-sm font-medium text-ui-text outline-none transition-colors focus:border-ui-accent-strong/40"
              >
                <option value="">{t('Streamer wählen')}</option>
                {streamers.map((channel) => (
                  <option key={channel.twitchUserId ?? channel.login} value={channel.twitchUserId ?? ''} disabled={!channel.twitchUserId}>
                    {channel.login}
                  </option>
                ))}
              </select>
            )}
            {accessMutation.isError && <p role="alert" className="text-sm text-ui-danger-soft">{t('Die Freigabe konnte nicht geändert werden. Bitte prüfe die Kanalauswahl.')}</p>}
        </div>}
      </DashboardHeader>

      <PlanProvider
          plan={authStatus?.plan ?? null}
          isAdmin={authStatus?.isAdmin ?? false}
          isLocalhost={authStatus?.isLocalhost ?? false}
          isDemoMode={isDemoMode}
        >
          <TrialExpiryModal />
          <TrialBanner />

          {isAdminView ? (
            <SocialMedia key={selectedChannel?.twitchUserId ?? ''} streamer={streamer} twitchUserId={selectedChannel?.twitchUserId ?? undefined} isAdmin />
          ) : loadingAccess ? (
            <div className="panel-card rounded-2xl p-8 text-center text-text-secondary">
              {t('Zugriff wird geprüft…')}
            </div>
          ) : access?.allowed ? (
            <SocialMedia key={authStatus?.twitchUserId ?? ''} twitchUserId={authStatus?.twitchUserId ?? undefined} streamer={access.streamer ?? authStatus?.twitchLogin ?? ''} isAdmin={false} />
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
    </div>
  );
}
