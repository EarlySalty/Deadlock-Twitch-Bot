import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Clapperboard, Loader2 } from 'lucide-react';
import { SocialMedia } from '@/pages/SocialMedia';
import { AiEditorPanel } from '@/components/socialmedia/ai/AiEditorPanel';
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
import { socialWorkspaceDepartureAllowed } from '@/components/socialmedia/studioNavigation';
import { PREVIEW_SOCIAL_ROUTE } from '@/preview/routes';
import { AdminSocialMediaAccessRequests, SocialMediaAccessRequestCard } from '@/components/socialmedia/SocialMediaAccessRequests';
import { SocialMediaBetaNotice } from '@/components/socialmedia/BetaNotice';
import { SocialMediaRecording } from '@/components/socialmedia/recording/SocialMediaRecording';

const SocialMediaPage = import.meta.env.MODE === 'preview'
  ? lazy(() => import('@/preview/SocialWorkspaces'))
  : SocialMedia;

export function SocialMediaManager() {
  const t = useT();
  useEffect(() => {
    document.title = t('Social-Media-Manager');
  }, [t]);

  const [streamerUserId, setStreamerUserId] = useState('');
  const requestedChannel = useRef(new URLSearchParams(window.location.search));
  const hasAutoSetStreamer = useRef(false);

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus } = useAuthStatus();
  const selectedChannel = resolveSocialMediaChannel(streamers, streamerUserId);
  const streamer = selectedChannel?.login.toLowerCase() ?? '';

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
      queryClient.invalidateQueries({ queryKey: ['social-media-access-requests'] });
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
    window.history.replaceState({}, '', `${newUrl}${window.location.hash}`);
  }, [streamer, selectedChannel, isAdminView]);

  return (
    <div className="space-y-6">
      <header className="studio-channel-header">
        <div className="flex min-w-0 items-center gap-2">
          <Clapperboard aria-hidden="true" className="h-5 w-5 shrink-0 text-primary" />
          <h2 className="text-sm font-semibold">{t('Social-Media-Manager')}</h2>
          {loadingStreamers && <Loader2 aria-label={t('Lade Streamer-Liste…')} className="h-4 w-4 animate-spin text-primary" />}
        </div>
        {isAdminView && <div className="studio-channel-controls">
            {!streamer && <a
              href={`${PREVIEW_SOCIAL_ROUTE}?view=veroeffentlicht`}
              className="studio-button"
              onClick={(event) => {
                if (!socialWorkspaceDepartureAllowed(document, () => window.confirm(t('Ungespeicherte Änderungen verwerfen?')))) event.preventDefault();
              }}
            >{t('VOD-Archiv')}</a>}
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
                    : t('Diesen Streamer für den eigenen Social-Media-Manager freischalten')
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
                  if (!socialWorkspaceDepartureAllowed(document, () => window.confirm(t('Ungespeicherte Änderungen verwerfen?')))) return;
                  hasAutoSetStreamer.current = true;
                  const selected = resolveSocialMediaChannel(streamers, event.target.value);
                  if (selected?.twitchUserId !== selectedChannel?.twitchUserId) {
                    const params = new URLSearchParams(window.location.search);
                    params.delete('clip_db_id');
                    window.history.replaceState({}, '', `${window.location.pathname}?${params}${window.location.hash}`);
                  }
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
      </header>

      {(isAdminView || access?.allowed) && <SocialMediaBetaNotice />}

      {authStatus?.isAdmin && <AdminSocialMediaAccessRequests />}

      <PlanProvider
          plan={authStatus?.plan ?? null}
          isAdmin={authStatus?.isAdmin ?? false}
          isLocalhost={authStatus?.isLocalhost ?? false}
          isDemoMode={isDemoMode}
        >
          <TrialExpiryModal />
          <TrialBanner />

          <Suspense fallback={<p role="status" className="studio-workspace-note">{t('Wird geladen')}</p>}>
          {isAdminView ? (
            <SocialMediaPage key={selectedChannel?.twitchUserId ?? ''} streamer={streamer} twitchUserId={selectedChannel?.twitchUserId ?? undefined} isAdmin aiEditor={AiEditorPanel} />
          ) : loadingAccess ? (
            <div className="panel-card rounded-2xl p-8 text-center text-text-secondary">
              {t('Zugriff wird geprüft…')}
            </div>
          ) : access?.allowed ? (
            <SocialMediaPage key={authStatus?.twitchUserId ?? ''} twitchUserId={authStatus?.twitchUserId ?? undefined} streamer={access.streamer ?? authStatus?.twitchLogin ?? ''} isAdmin={false} aiEditor={AiEditorPanel} />
          ) : (
            <SocialMediaAccessRequestCard requested={Boolean(access?.requested)} />
          )}
          </Suspense>
        </PlanProvider>
        {loadingStreamers && (
          <div className="mt-4 text-xs text-text-secondary">{t('Lade Streamer-Liste…')}</div>
        )}
        {!loadingStreamers && streamers.length === 0 && authStatus?.isAdmin && (
          <div className="mt-4 text-xs text-text-secondary">{t('Keine Streamer gefunden.')}</div>
        )}
        <SocialMediaRecording active={!isAdminView && !isDemoMode && Boolean(access?.allowed)} />
    </div>
  );
}
