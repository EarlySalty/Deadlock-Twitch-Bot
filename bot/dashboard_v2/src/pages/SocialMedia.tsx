import { globalTimezoneUrl } from '@/utils/socialMediaChannel';
import { AlertTriangle } from 'lucide-react';
import { clipDisplayTitles } from '@/components/socialmedia/clipTitles';
import { PlatformCheck } from '@/components/socialmedia/PlatformCheck';
import { StudioActivity } from '@/components/socialmedia/StudioActivity';
import { ClipCard } from '@/components/socialmedia/ClipCard';
import { StudioHero } from '@/components/socialmedia/StudioHero';
import { useEffect, useMemo, useRef, useState, type ComponentType, type DragEvent, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AlertCircle, BarChart3, ChevronDown, CheckCircle2, Archive, Film, Loader2, ShieldAlert, Layers3, Calendar, Gamepad2, ExternalLink, Pencil, SlidersHorizontal, DownloadCloud, Crop, List, LayoutGrid, X } from 'lucide-react';
import { useLanguage, useT } from '@/context/LanguageContext';

import { TikTokPostDialog } from '@/components/socialmedia/TikTokPostDialog';

import { WorkspaceDialog } from '@/components/socialmedia/WorkspaceDialog';
import { PostingPlanDraft } from '@/components/socialmedia/PostingPlanDraft';
import { PublishReviewPanel } from '@/components/socialmedia/autopilot/publish-review/PublishReviewPanel';
import { publishReviewClient, type PublishReviewClient } from '@/components/socialmedia/autopilot/publish-review/api';
import { PlatformStatusDialog } from '@/components/socialmedia/PlatformStatusDialog';
import { ImportPoolPanel } from '@/components/socialmedia/autopilot/import-pool';
import { ScheduleQueuePanel, scheduleQueueApi, type ScheduleQueueApi } from '@/components/socialmedia/autopilot/schedule-queue';
import { PipelineOverview, type StudioOverviewData } from '@/components/socialmedia/StudioOverview';
import { StudioStatusBar } from '@/components/socialmedia/StudioStatusBar';
import { socialAiClipFromSearch, socialViewFromSearch, socialViewSearch, socialWorkspaceDepartureAllowed, socialWorkspaceView, type AiEditorMountProps } from '@/components/socialmedia/studioNavigation';
import { authorizationFact } from '@/components/socialmedia/platformPresentation';
import { filterQueue, formatTerminInZone, queueStats, queueStage, loadQueueSnapshot, QUEUE_FILTERS, type QueueStage } from '@/components/socialmedia/queuePresentation';
import '@/components/socialmedia/studio.css';
import { LayoutSchematic } from '@/components/socialmedia/LayoutSchematic';
import { StudioSectionHeading } from '@/components/socialmedia/StudioSectionHeading';
import { AnalyticsTab } from '@/components/socialmedia/AnalyticsTab';
import { VodArchiveList } from '@/components/socialmedia/VodArchiveList';
import { LayoutEditor, type VorschauClip } from '@/components/socialmedia/LayoutEditor';
import { EnrichmentPanel } from '@/components/socialmedia/EnrichmentPanel';
import { LadeFehlerHinweis } from '@/components/socialmedia/LadeFehlerHinweis';
import { clipFehler, istGesperrt, istStandUnbekannt, zeitplanFeldSchluessel, zeitplanFeldVerlassen, zeitplanFormularAbgleichen, type ZeitplanFormular } from '@/components/socialmedia/kartenZustand';
import { cancelScheduledPost, decideClipApproval, type TikTokPostOptions, SocialMediaForbiddenError, discardClip, fetchPostingPlan, fetchClips, fetchTwitchClips, fetchVodArchiveSettings, fetchPlatformStatus, disconnectPlatform, oauthStartUrl, type PlatformStatus, type SocialClipMitPosting, fetchStreamerLayout, saveStreamerLayout, saveVodArchiveSettings, setClipLayoutOverride, uploadClip } from '@/api/socialMedia';
import { APPROVAL_MODE_TEXTE, FELD_FEHLER, fehlerText, kategorieLabel, PLATFORM_LABELS, SOCIAL_MEDIA_TABS, type SocialMediaView, type SocialMediaViewTarget } from '@/components/socialmedia/labels';
import { type ApprovalMode, type ClipPoolForecast, type PlatformScheduleEntry, type PostingPlan, DEFAULT_LAYOUT, type LayoutPayload, type SocialPlatform, type StreamerLayoutResponse, type VodArchivePrivacy, type VodArchiveSettings } from '@/types/socialMedia';

export interface SocialMediaProps {
  streamer: string;
  twitchUserId?: string;
  isAdmin?: boolean;
  aiEditor?: ComponentType<AiEditorMountProps>;
  scheduleApi?: ScheduleQueueApi;
  publishClient?: PublishReviewClient;
  workspaceSlots?: {
    automation?: ReactNode;
    libraryImport?: ReactNode;
    publishReview?: ReactNode;
  };
}

const PLATTFORMEN: SocialPlatform[] = ['youtube', 'tiktok', 'instagram'];

const RELOAD_QUERY_ROOTS = new Set(['social-media', 'social-ai', 'social-ai-publishing-context', 'vod-archive']);

type EditMode = 'layout' | 'enrichment';

const TAB_ICONS: Record<SocialMediaView, React.ComponentType<{ className?: string }>> = {
  heute: Calendar,
  freigabe: Layers3,
  planen: Calendar,
  veroeffentlicht: Archive,
  einstellungen: SlidersHorizontal,
};

export function SocialMedia({ streamer, twitchUserId, isAdmin = false, aiEditor: AiEditor, scheduleApi = scheduleQueueApi, publishClient = publishReviewClient, workspaceSlots }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const t = useT();
  const { locale } = useLanguage();
  const [statusFilter, setStatusFilter] = useState<QueueStage>('all');
  const [search, setSearch] = useState('');
  const [platformFilter, setPlatformFilter] = useState<SocialPlatform | 'all'>('all');
  const [page, setPage] = useState(1);
  const [gridView, setGridView] = useState(false);
  const [showUpload, setShowUpload] = useState(false);
  const [layoutChoice, setLayoutChoice] = useState<LayoutPayload | null>(null);
  const [editingClip, setEditingClip] = useState<{
    clip: SocialClipMitPosting;
    mode: EditMode;
  } | null>(null);
  const [activeView, setActiveView] = useState<SocialMediaView>(() =>
    socialViewFromSearch(window.location.search),
  );
  const [statusClipId, setStatusClipId] = useState<number | null>(null);
  const [selectedAiClipId, setSelectedAiClipId] = useState(() => socialAiClipFromSearch(window.location.search));
  const workspaceView = socialWorkspaceView(activeView);
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    params.set('view', socialViewFromSearch(window.location.search));
    window.history.replaceState({}, '', `${window.location.pathname}?${params}${window.location.hash}`);
  }, []);
  const [showAiEditor, setShowAiEditor] = useState(() => ['ai', 'review'].includes(new URLSearchParams(window.location.search).get('view') ?? ''));
  const [showAnalytics, setShowAnalytics] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const [publishedView, setPublishedView] = useState<'clips' | 'vod'>(() => new URLSearchParams(window.location.search).get('view') === 'vod' ? 'vod' : 'clips');
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  const [tiktokPost, setTikTokPost] = useState<{ clipDbId: number; platforms: SocialPlatform[] } | null>(null);

  const layoutQuery = useQuery<StreamerLayoutResponse, Error>({
    queryKey: ['social-media', 'streamer-layout', twitchUserId],
    queryFn: () => fetchStreamerLayout(twitchUserId),
    enabled: !!twitchUserId,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const clipsQuery = useQuery({
    queryKey: ['social-media', 'clips', streamer, twitchUserId],
    queryFn: ({ signal }) =>
      loadQueueSnapshot(
        (page) => fetchClips({ status: 'all', twitch_user_id: twitchUserId, page, page_size: 100 }, signal),
        signal,
      ),
    enabled: !!twitchUserId,
    staleTime: 30 * 1000,
    refetchOnWindowFocus: false,
    retry: (failureCount, err) => !(err instanceof SocialMediaForbiddenError) && failureCount < 2,
  });

  // Vorschauclips fuer den Layout-Editor. Bewusst eine eigene Abfrage ohne den
  // Statusfilter der Liste: steht der Filter auf "Veroeffentlicht" und ist dort
  // nichts drin, haette der Editor sonst kein Bild.
  const vorschauClipsQuery = useQuery({
    queryKey: ['social-media', 'vorschau-clips', streamer, twitchUserId],
    queryFn: () =>
      fetchClips({ status: 'all', twitch_user_id: twitchUserId, page: 1, page_size: 12 }),
    // Vorschauclips werden nur im fokussierten Layout-Bereich gebraucht.
    enabled: !!twitchUserId && workspaceView === 'konten',
    staleTime: 5 * 60 * 1000,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const vorschauClips: VorschauClip[] = useMemo(
    () =>
      (vorschauClipsQuery.data?.items ?? [])
        .filter((clip) => !!clip.thumbnail_url)
        .slice(0, 12)
        .map((clip) => ({
          id: String(clip.clip_db_id),
          titel: clip.title || clip.clip_id,
          bildUrl: clip.thumbnail_url as string,
        })),
    [vorschauClipsQuery.data],
  );

  const postingPlanQuery = useQuery<PostingPlan, Error>({
    queryKey: ['social-media', 'posting-plan', twitchUserId],
    queryFn: () => fetchPostingPlan(twitchUserId),
    enabled: !!twitchUserId,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const vodArchiveQuery = useQuery<VodArchiveSettings, Error>({
    queryKey: ['social-media', 'vod-archive-settings', twitchUserId],
    queryFn: () => fetchVodArchiveSettings(twitchUserId),
    enabled: !!twitchUserId,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const scheduleQuery = useQuery({
    queryKey: ['social-media', 'autopilot-schedule', twitchUserId],
    queryFn: ({ signal }) => scheduleApi.load(twitchUserId, signal),
    enabled: !!twitchUserId,
    staleTime: 15 * 1000,
    refetchInterval: 30 * 1000,
    retry: (failureCount, err) => !(err instanceof SocialMediaForbiddenError) && failureCount < 1,
  });

  const publishQuery = useQuery({
    queryKey: ['social-media', 'autopilot-publish', twitchUserId],
    queryFn: ({ signal }) => publishClient.load(twitchUserId!, signal),
    enabled: !!twitchUserId,
    staleTime: 15 * 1000,
    refetchInterval: 30 * 1000,
    retry: (failureCount, err) => !(err instanceof SocialMediaForbiddenError) && failureCount < 1,
  });

  const isForbidden =
    layoutQuery.error instanceof SocialMediaForbiddenError ||
    clipsQuery.error instanceof SocialMediaForbiddenError ||
    postingPlanQuery.error instanceof SocialMediaForbiddenError;

  // Construct a normalized LayoutPayload (with cam_enabled + mode) from the API response.
  // Backend sometimes returns layout without those fields nested — copy from response level.
  const layoutForEditor: LayoutPayload = useMemo(() => {
    const data = layoutQuery.data;
    if (!data) return DEFAULT_LAYOUT;
    const layout = data.layout ?? DEFAULT_LAYOUT;
    return {
      ...layout,
      cam_enabled: data.cam_enabled ?? layout.cam_enabled ?? true,
      mode: data.mode ?? layout.mode ?? 'pip',
    };
  }, [layoutQuery.data]);

  const saveLayoutMutation = useMutation({
    mutationFn: (layout: LayoutPayload) => saveStreamerLayout({ twitch_user_id: twitchUserId, layout }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'streamer-layout', twitchUserId] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) => uploadClip({ file, twitch_user_id: twitchUserId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  const discardMutation = useMutation({
    mutationFn: (clipDbId: number) => discardClip(clipDbId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  const overrideMutation = useMutation({
    mutationFn: ({ clipDbId, layout }: { clipDbId: number; layout: LayoutPayload | null }) =>
      setClipLayoutOverride(clipDbId, layout),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  // Alle drei Zeitplan-Aufrufe liefern den kompletten Plan zurueck. Wir setzen
  // ihn direkt in den Cache, damit der neu berechnete Termin und die
  // Vorratsrechnung ohne zweite Abfrage sofort stehen.
  const planKey = ['social-media', 'posting-plan', twitchUserId];
  const uebernehmePlan = (plan: PostingPlan) => {
    queryClient.setQueryData(planKey, plan);
  };

  // Ohne Kanal zeigt und kappt das Backend die globale Sammelverbindung. Beides
  // waere hier falsch: die Karte gehoert zum gewaehlten Kanal.
  const platformStatusQuery = useQuery({
    queryKey: ['social-media', 'platform-status', twitchUserId],
    queryFn: () => fetchPlatformStatus(twitchUserId),
    enabled: !!twitchUserId,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: async (platform: string) => ({
      ...await disconnectPlatform(platform, twitchUserId),
      twitchUserId,
    }),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'platform-status', result.twitchUserId] });
    },
  });

  /** Nachschub aus Twitch holen, der einzige Ausweg aus der Vorratswarnung. */
  const clipsHolenMutation = useMutation({
    mutationFn: () => fetchTwitchClips(twitchUserId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  /** Veto: einen bereits eingeplanten Post vor dem Termin stoppen. */
  const abbrechenMutation = useMutation({
    mutationFn: (clipDbId: number) => cancelScheduledPost(clipDbId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
  });

  const vodArchiveMutation = useMutation({
    mutationFn: (payload: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) =>
      saveVodArchiveSettings(twitchUserId, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ['social-media', 'vod-archive-settings', twitchUserId],
      });
    },
  });

  const approvalMutation = useMutation({
    mutationFn: ({
      clipDbId,
      decision,
      platforms,
      tiktokOptions,
    }: {
      clipDbId: number;
      decision: 'approve' | 'skip' | 'edit';
      platforms: SocialPlatform[];
      tiktokOptions?: TikTokPostOptions;
    }) => decideClipApproval({ clipDbId, decision, platforms, tiktokOptions }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', twitchUserId] });
    },
    // Kein window.alert mit roher Backend-Meldung. Der Fehler geht ueber
    // `clipFehler` als Zeile in den Fuss der betroffenen Clip-Karte, genau wie
    // bei Override, Abbrechen und Verwerfen.
  });

  const stats = useMemo(() => queueStats(clipsQuery.data?.items ?? []), [clipsQuery.data]);
  const displayTitles = useMemo(() => clipDisplayTitles(clipsQuery.data?.items ?? [], locale, postingPlanQuery.data?.timezone ?? 'Europe/Berlin'), [clipsQuery.data, locale, postingPlanQuery.data?.timezone]);
  const filteredClips = useMemo(
    () => filterQueue((clipsQuery.data?.items ?? []).filter(clip => activeView !== 'veroeffentlicht' || ['posted', 'failed'].includes(queueStage(clip))), statusFilter, search, platformFilter),
    [clipsQuery.data, activeView, statusFilter, search, platformFilter],
  );
  const pageCount = Math.max(1, Math.ceil(filteredClips.length / 24));
  const currentPage = Math.min(page, pageCount);
  const visibleClips = filteredClips.slice((currentPage - 1) * 24, currentPage * 24);
  const editorClip = editingClip?.clip ?? null;
  const actionsBusy =
    approvalMutation.isPending || discardMutation.isPending || abbrechenMutation.isPending;
  const changeView = (target: SocialMediaViewTarget, clipDbId = selectedAiClipId) => {
    const next = socialViewFromSearch(`?view=${target}`);
    if (next === activeView && clipDbId === selectedAiClipId) return true;
    if (!socialWorkspaceDepartureAllowed(document, () => window.confirm(t('Ungespeicherte Änderungen verwerfen?')))) return false;
    setActiveView(next);
    setSelectedAiClipId(target === 'ai' ? clipDbId : undefined);
    const search = socialViewSearch(window.location.search, next, clipDbId);
    window.history.replaceState({}, '', `${window.location.pathname}?${search}${window.location.hash}`);
    return true;
  };
  const statusClip = clipsQuery.data?.items.find((clip) => clip.clip_db_id === statusClipId);
  const selectedAiClip = clipsQuery.data?.items.find((clip) => clip.clip_db_id === selectedAiClipId);

  const defaultApprovalPlatforms = useMemo(
    () =>
      (postingPlanQuery.data?.platforms ?? [])
        .filter((entry) => entry.auto_post && entry.posts_per_week > 0)
        .map((entry) => entry.platform),
    [postingPlanQuery.data],
  );

  const overviewData: StudioOverviewData = {
    schedule: scheduleQuery.data,
    scheduleFailed: scheduleQuery.isError,
    publish: publishQuery.data,
    publishFailed: publishQuery.isError,
    clips: clipsQuery.data?.items,
    clipsFailed: clipsQuery.isError,
    platforms: platformStatusQuery.data?.platforms,
    platformsFailed: platformStatusQuery.isError,
    timezone: postingPlanQuery.data?.timezone ?? 'Europe/Berlin',
  };
  const openView = (view: SocialMediaViewTarget, stage?: QueueStage) => {
    if (!changeView(view)) return;
    if (!stage) return;
    setPublishedView('clips');
    setStatusFilter(stage);
    setSearch('');
    setPlatformFilter('all');
    setPage(1);
    window.requestAnimationFrame(() => document.getElementById('studio-clip-list')?.scrollIntoView({ block: 'start' }));
  };
  const refreshSchedule = () => {
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-schedule', twitchUserId] });
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-publish', twitchUserId] });
  };
  const refreshAll = () => {
    void queryClient.invalidateQueries({ predicate: (query) => RELOAD_QUERY_ROOTS.has(String(query.queryKey[0])) });
    setReloadKey((key) => key + 1);
  };
  const legacyPlan = postingPlanQuery.data;
  const legacyWeekly = legacyPlan?.platforms.filter((entry) => entry.auto_post).reduce((sum, entry) => sum + entry.posts_per_week, 0);

  if (isForbidden) {
    return (
      <div className="panel-card rounded-2xl p-12 text-center max-w-2xl mx-auto mt-12">
        <ShieldAlert className="w-12 h-12 text-danger mx-auto mb-4" />
        <h2 className="text-2xl font-bold text-white mb-2">{t('Noch nicht freigeschaltet')}</h2>
        <p className="text-text-secondary">
          {t(
            'Social Media wird für deinen Kanal erst nach Freigabe aktiv. Melde dich bei EarlySalty, wenn du deine Clips hier aufbereiten möchtest.',
          )}
        </p>
      </div>
    );
  }

  if (!streamer && !(isAdmin && workspaceView === 'archiv')) {
    return (
      <div className="panel-card rounded-2xl p-12 text-center max-w-2xl mx-auto mt-12">
        <Film className="w-12 h-12 text-text-secondary mx-auto mb-4" />
        <h2 className="text-2xl font-bold text-white mb-2">{t('Streamer auswählen')}</h2>
        <p className="text-text-secondary">
          {t('Wähle oben einen Streamer aus, um Layouts, Clips und Uploads zu verwalten.')}
        </p>
      </div>
    );
  }

  const modeSlot = (
    <div className="studio-mode-slot">
      {workspaceSlots?.publishReview ?? (twitchUserId && <PublishReviewPanel twitchUserId={twitchUserId} client={publishClient} section="mode" onChanged={refreshSchedule} reloadKey={reloadKey} />)}
          <details className="studio-legacy-pipeline">
            <summary>
              <span className="studio-legacy-summary-text">
                <strong>{t('Neue Twitch-Clips')}</strong>
                <span>
                  {legacyPlan
                    ? t('{mode} · {count} Posts pro Woche · steuert weiterhin die Clip-Freigaben in der Übersicht', {
                        mode: t(APPROVAL_MODE_TEXTE[legacyPlan.approval_mode].label),
                        count: legacyWeekly ?? 0,
                      })
                    : t('Freigabemodus, Plattformzeiten und Kategorien der bisherigen Clip-Pipeline')}
                </span>
              </span>
              <ChevronDown className="studio-legacy-chevron h-4 w-4" aria-hidden="true" />
            </summary>
          <div className="studio-legacy-plan">
            <p className="studio-workspace-note">{t('Diese Einstellungen gelten weiter für Clips, die du in der Pipeline freigibst. Der Auto-Pilot oben nutzt seinen eigenen Zeitplan.')}</p>
          <PostingPlanDraft
            twitchUserId={twitchUserId}
            key={twitchUserId}
            streamer={streamer}
            plan={postingPlanQuery.data ?? null}
            loading={postingPlanQuery.isLoading}
            loadError={postingPlanQuery.error}
            onSaved={uebernehmePlan}
          >
            {({ draft, patch, busy }) => (
              <div className="studio-plan-grid" data-saving={busy ? 'true' : undefined}>
                <div className="space-y-5">
                  <ApprovalModeCard
                    plan={draft}
                    isLoading={postingPlanQuery.isLoading}
                    ladeFehler={postingPlanQuery.error}
                    isSaving={busy}
                    error={null}
                    onChange={(mode) => patch((plan) => ({ ...plan, approval_mode: mode }))}
                    onSubtitlesChange={(enabled) =>
                      patch((plan) => ({ ...plan, subtitles_enabled: enabled }))
                    }
                  />
                  <PostingScheduleCard
                    twitchUserId={twitchUserId}
                    availablePlatforms={platformStatusQuery.data?.platforms.filter((status) => status.capabilities?.upload).map((status) => status.platform) ?? []}
                    plan={draft}
                    isLoading={postingPlanQuery.isLoading}
                    ladeFehler={postingPlanQuery.error}
                    isSaving={busy}
                    error={null}
                    onChange={(platform, payload) =>
                      patch((plan) => ({
                        ...plan,
                        platforms: plan.platforms.map((p) =>
                          p.platform === platform ? { ...p, ...payload } : p,
                        ),
                      }))
                    }
                  />
                  <CategoryCard
                    plan={draft}
                    isLoading={postingPlanQuery.isLoading}
                    ladeFehler={postingPlanQuery.error}
                    isSaving={busy}
                    error={null}
                    onChange={(categoryKey, autoPost) =>
                      patch((plan) => ({
                        ...plan,
                        categories: plan.categories.map((c) =>
                          c.category_key === categoryKey ? { ...c, auto_post: autoPost } : c,
                        ),
                      }))
                    }
                  />
                </div>
                <aside className="space-y-5">
                  <div className="rounded-xl border border-border bg-card p-5">
                    <h3 className="text-sm text-text-secondary">{t('Dein Posting-Rhythmus')}</h3>
                    <p className="mt-4 text-2xl font-semibold">
                      {draft
                        ? draft.platforms
                            .filter((p) => p.auto_post)
                            .reduce((sum, p) => sum + p.posts_per_week, 0)
                        : '…'}{' '}
                      <span className="text-sm font-normal text-text-secondary">
                        {t('Posts pro Woche')}
                      </span>
                    </p>
                    <p className="mt-3 text-sm leading-6 text-text-secondary">
                      {t('Zeiten gelten in {tz}.', { tz: draft?.timezone ?? '…' })}
                    </p>
                  </div>
                  <VorratsHinweis
                    pool={postingPlanQuery.data?.pool ?? null}
                    onClipsHolen={() => clipsHolenMutation.mutate()}
                    isHolend={clipsHolenMutation.isPending}
                  />
                  {clipsHolenMutation.isError && (
                    <p role="alert" className="text-sm text-danger">
                      {fehlerText(clipsHolenMutation.error, t)}
                    </p>
                  )}
                  {clipsHolenMutation.isSuccess && (
                    <p role="status" className="text-sm text-success">
                      {t('{count} Clips von Twitch geholt.', {
                        count: clipsHolenMutation.data.clips_found,
                      })}
                    </p>
                  )}
                  <p className="text-sm leading-6 text-text-secondary">
                    {t(
                      'Freigaben und geplante Posts bleiben in der Pipeline sichtbar. Noch nicht gestartete Posts lassen sich stoppen.',
                    )}
                  </p>
                </aside>
              </div>
            )}
          </PostingPlanDraft>
          </div>
          </details>
    </div>
  );

  return (
    <div className="social-studio space-y-6" data-studio-version="industrial-gold-live-v1">
      <StudioHero streamer={streamer} onUpload={() => setShowUpload(true)} onRefresh={refreshAll} />
      <StudioStatusBar poolFailed={postingPlanQuery.isError} data={overviewData} pool={postingPlanQuery.isError ? undefined : postingPlanQuery.data?.pool} now={now} onNavigate={openView} />

      <nav className="studio-tabs" role="tablist" aria-label={t('Social Studio')}>
        {SOCIAL_MEDIA_TABS.map(({ id, label }, index) => {
          const Icon = TAB_ICONS[id];
          return (
            <button
              key={id}
              type="button"
              role="tab"
              id={`studio-tab-${id}`}
              aria-controls="studio-panel"
              aria-selected={activeView === id}
              tabIndex={activeView === id ? 0 : -1}
              className="studio-tab"
              onClick={() => { if (changeView(id)) { setStatusFilter('all'); setPage(1); } }}
              onKeyDown={(event) => {
                const nextIndex =
                  event.key === 'Home'
                    ? 0
                    : event.key === 'End'
                      ? SOCIAL_MEDIA_TABS.length - 1
                      : event.key === 'ArrowRight'
                        ? (index + 1) % SOCIAL_MEDIA_TABS.length
                        : event.key === 'ArrowLeft'
                          ? (index + SOCIAL_MEDIA_TABS.length - 1) % SOCIAL_MEDIA_TABS.length
                          : null;
                if (nextIndex === null) return;
                event.preventDefault();
                if (changeView(SOCIAL_MEDIA_TABS[nextIndex].id)) {
                  document.getElementById(`studio-tab-${SOCIAL_MEDIA_TABS[nextIndex].id}`)?.focus();
                }
              }}
            >
              <Icon className="h-4 w-4" />
              {t(label)}
            </button>
          );
        })}
      </nav>
      <section
        id="studio-panel"
        role="tabpanel"
        aria-labelledby={`studio-tab-${activeView}`}
        className={workspaceView === 'pool' || false ? 'studio-pipeline min-w-0 space-y-4' : workspaceView === 'archiv' ? 'studio-archive min-w-0' : selectedAiClipId !== undefined ? 'studio-ai-workspace min-w-0' : 'studio-automation-workspace min-w-0'}
      >
        {activeView === 'heute' ? (
          <PipelineOverview data={overviewData} now={now} onNavigate={openView} activity={<StudioActivity twitchUserId={twitchUserId} data={overviewData} now={now} onNavigate={openView} onAi={() => setShowAiEditor(true)} />} />
        ) : workspaceView === 'plan' ? (
          <div className="studio-plan-workspace">
            {workspaceSlots?.automation ? (
              <div className="studio-plan-top">
                <div className="studio-card">{workspaceSlots.automation}</div>
                <div className="studio-card">{modeSlot}</div>
              </div>
            ) : (
              <ScheduleQueuePanel twitchUserId={twitchUserId} api={scheduleApi} onChanged={refreshSchedule} reloadKey={reloadKey} modeSlot={modeSlot} />
            )}
            <div className="studio-library-import">{workspaceSlots?.libraryImport ?? <ImportPoolPanel twitchUserId={twitchUserId} reloadKey={reloadKey} />}</div>
          </div>
        ) : workspaceView === 'konten' ? (
          <div className="studio-settings">
            <div className="studio-settings-row">
              <PlatformConnectionsCard
                now={now}
                twitchUserId={twitchUserId}
                streamer={streamer}
                platforms={platformStatusQuery.data?.platforms ?? []}
                isLoading={platformStatusQuery.isLoading}
                ladeFehler={platformStatusQuery.error}
                onDisconnect={(platform) => disconnectMutation.mutate(platform)}
                isDisconnecting={disconnectMutation.isPending}
                error={disconnectMutation.error}
                disconnectResult={disconnectMutation.data?.twitchUserId === twitchUserId ? disconnectMutation.data ?? null : null}
              />
              <VodArchiveCard
                streamer={streamer}
                settings={vodArchiveQuery.data ?? null}
                isLoading={vodArchiveQuery.isLoading}
                ladeFehler={vodArchiveQuery.error}
                isSaving={vodArchiveMutation.isPending}
                error={vodArchiveMutation.error}
                onChange={(next) => vodArchiveMutation.mutate(next)}
              />
            </div>
            <section className="studio-card studio-templates-card">
            <StudioSectionHeading
              title={t('Vorlagen & Layouts')}
              icon={Crop}
              description={t('Lege den Standard-Ausschnitt für neue Clips fest. Einzelne Clips kannst du weiterhin separat anpassen.')}
              actions={
                <button
                  type="button"
                  className="studio-button"
                  disabled={layoutQuery.isLoading || layoutQuery.isError || !layoutQuery.data}
                  onClick={() => {
                    saveLayoutMutation.reset();
                    setLayoutChoice(layoutForEditor);
                  }}
                >
                  {t('Gespeichertes Layout öffnen')}
                </button>
              }
            />
            {layoutQuery.isError ? <LadeFehlerHinweis fehler={layoutQuery.error} /> : null}
            <div className="grid gap-4 md:grid-cols-3">
              {(
                [
                  { mode: 'blur_pad', title: 'Gameplay mit Hintergrund', cam: false, description: 'Das ganze Spielbild bleibt sichtbar, der Hintergrund füllt das Hochformat.' },
                  { mode: 'pip', title: 'Facecam & Gameplay', cam: true, description: 'Deine Kamera sitzt über dem vollständigen Spielbild.' },
                  { mode: 'stacked', title: 'Kamera oben mit DDC-Branding', cam: true, description: 'Kamera und Spielbild untereinander, mit DDC-Trenner.' },
                ] as const
              ).map((preset) => (
                <button
                  key={preset.mode}
                  type="button"
                  className="studio-template"
                  disabled={layoutQuery.isLoading || layoutQuery.isError || !layoutQuery.data}
                  onClick={() => {
                    saveLayoutMutation.reset();
                    setLayoutChoice({
                      ...layoutForEditor,
                      mode: preset.mode,
                      cam_enabled: preset.cam,
                      cam_position: preset.mode === 'stacked'
                        ? { ...layoutForEditor.cam_position, h: DEFAULT_LAYOUT.cam_position.h }
                        : layoutForEditor.cam_position,
                    });
                  }}
                >
                  <LayoutSchematic mode={preset.mode} />
                  <div className="studio-template-copy">
                    <h3>{t(preset.title)}</h3>
                    <p>{t(preset.description)}</p>
                    <span className="studio-template-action">{t('Layout anpassen')} <Pencil className="h-3.5 w-3.5" aria-hidden="true" /></span>
                  </div>
                </button>
              ))}
            </div>
            </section>
            {AiEditor && twitchUserId && <section className="studio-card studio-production-card" aria-labelledby="studio-production-title">
              <AiEditor twitchUserId={twitchUserId} streamer={streamer} settingsOnly scheduleApi={scheduleApi} />
            </section>}
            {twitchUserId && <section className="studio-card"><PublishReviewPanel twitchUserId={twitchUserId} client={publishClient} section="notifications" onChanged={refreshSchedule} reloadKey={reloadKey} /></section>}
          </div>
        ) : activeView === 'veroeffentlicht' && (publishedView === 'vod' || !streamer) ? (
          <>
            {streamer && <PublishedSwitch value={publishedView} onChange={setPublishedView} />}
            <VodArchiveList twitchUserId={twitchUserId} isAdmin={isAdmin} reloadKey={reloadKey} />
          </>
        ) : (
          <>
            {activeView === 'veroeffentlicht' && <PublishedSwitch value={publishedView} onChange={setPublishedView} />}
            <div id="studio-clip-list" className="studio-clip-list space-y-3">
              <div className="studio-toolbar">
                {activeView === 'veroeffentlicht' ? (
                  statusFilter !== 'all' && (
                    <button type="button" className="studio-filter" aria-pressed="true" onClick={() => { setStatusFilter('all'); setPage(1); }}>
                      {t(QUEUE_FILTERS.find(({ id }) => id === statusFilter)?.label ?? 'Alle')}
                      <X className="h-3.5 w-3.5" aria-label={t('Filter entfernen')} />
                    </button>
                  )
                ) : (
                  <StatusFilter
                    value={statusFilter}
                    onChange={(next) => {
                      setStatusFilter(next);
                      setPage(1);
                    }}
                  />
                )}
                <div className="flex max-w-full flex-wrap items-center gap-2 xl:ml-auto">
                  <input
                    type="search"
                    aria-label={t('Clips durchsuchen')}
                    placeholder={t('Clips durchsuchen…')}
                    value={search}
                    onChange={(event) => {
                      setSearch(event.target.value);
                      setPage(1);
                    }}
                    className="studio-search"
                  />
                  <select
                    aria-label={t('Plattform filtern')}
                    value={platformFilter}
                    onChange={(event) => {
                      setPlatformFilter(event.target.value as SocialPlatform | 'all');
                      setPage(1);
                    }}
                    className="studio-select"
                  >
                    <option value="all">{t('Plattform')}</option>
                    {PLATTFORMEN.map((p) => (
                      <option key={p} value={p}>
                        {PLATFORM_LABELS[p]}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="studio-icon-button"
                    aria-label={t('Listenansicht')}
                    aria-pressed={!gridView}
                    onClick={() => setGridView(false)}
                  >
                    <List className="h-4 w-4" />
                  </button>
                  <button
                    type="button"
                    className="studio-icon-button"
                    aria-label={t('Kartenansicht')}
                    aria-pressed={gridView}
                    onClick={() => setGridView(true)}
                  >
                    <LayoutGrid className="h-4 w-4" />
                  </button>
                  {activeView === 'veroeffentlicht' ? (
                    <button type="button" className="studio-button" onClick={() => setShowAnalytics(true)}>
                      <BarChart3 className="h-4 w-4" aria-hidden />
                      {t('Auswertung')}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="studio-button"
                      disabled={clipsHolenMutation.isPending}
                      onClick={() => clipsHolenMutation.mutate()}
                    >
                      <DownloadCloud className="h-4 w-4" />
                      {t('Clips holen')}
                    </button>
                  )}
                </div>
              </div>

              {clipsHolenMutation.isError && (
                <div className="text-xs text-danger">{fehlerText(clipsHolenMutation.error, t)}</div>
              )}
              {clipsHolenMutation.isSuccess && !clipsHolenMutation.isPending && (
                <div className="text-xs text-success">
                  {t('{count} Clips von Twitch geholt.', {
                    count: clipsHolenMutation.data?.clips_found ?? 0,
                  })}
                </div>
              )}

              {clipsQuery.isLoading ? (
                <div className="panel-card rounded-2xl p-12 flex items-center justify-center">
                  <Loader2 className="w-6 h-6 text-orange animate-spin" />
                </div>
              ) : clipsQuery.isError ? (
                <div role="alert" className="rounded-xl border border-danger/30 p-6">
                  <p className="text-sm text-danger">
                    {t('Die Pipeline konnte nicht vollständig geladen werden.')}
                  </p>
                  <button
                    type="button"
                    className="studio-button mt-3"
                    onClick={() => void clipsQuery.refetch()}
                  >
                    {t('Erneut versuchen')}
                  </button>
                </div>
              ) : filteredClips.length === 0 ? (
                <div className="panel-card rounded-2xl p-12 text-center">
                  <AlertCircle className="w-10 h-10 text-text-secondary mx-auto mb-3" />
                  <p className="text-white font-bold mb-1">{t('Keine Clips für diesen Filter')}</p>
                  <p className="text-sm text-text-secondary">
                    {t(
                      'Sobald neue Twitch-Clips eingehen oder du eine MP4 hochlädst, erscheinen sie hier.',
                    )}
                  </p>
                </div>
              ) : (
                <div className={gridView ? 'studio-queue-grid' : 'space-y-3'}>
                  {visibleClips.map((clip) => {
                    return (
                      <ClipCard
                        key={clip.clip_db_id}
                        clip={clip}
                        displayTitle={displayTitles.get(clip.clip_db_id)}
                        now={now}
                        onOpenPlatformStatus={() => setStatusClipId(clip.clip_db_id)}
                        onOpenAiEditor={() => changeView('ai', clip.clip_db_id)}
                        timezone={postingPlanQuery.data?.timezone ?? 'Europe/Berlin'}
                        defaultPlatforms={defaultApprovalPlatforms}
                        availablePlatforms={(platformStatusQuery.data?.platforms ?? []).filter((p) => p.capabilities?.upload).map((p) => p.platform as SocialPlatform)}
                        onOpenConnections={() => changeView('konten')}
                        onOpenEditor={(mode) => (
                          overrideMutation.reset(),
                          setEditingClip({ clip, mode })
                        )}
                        onDiscard={() => {
                          if (
                            window.confirm(t('Clip "{title}" verwerfen?', { title: clip.title }))
                          ) {
                            discardMutation.mutate(clip.clip_db_id);
                          }
                        }}
                        onApprovalDecision={(decision, platforms) => {
                          if (decision === 'approve' && platforms.includes('tiktok')) {
                            approvalMutation.reset();
                            setTikTokPost({ clipDbId: clip.clip_db_id, platforms });
                            return;
                          }
                          approvalMutation.mutate({
                            clipDbId: clip.clip_db_id,
                            decision,
                            platforms,
                          });
                          if (decision === 'edit') {
                            setEditingClip({ clip, mode: 'enrichment' });
                          }
                        }}
                        approvalPending={actionsBusy}
                        nichtEingeplant={
                          approvalMutation.isSuccess &&
                          approvalMutation.variables?.clipDbId === clip.clip_db_id
                            ? (approvalMutation.data?.approval?.not_scheduled ?? [])
                            : []
                        }
                        onCancelScheduled={() => abbrechenMutation.mutate(clip.clip_db_id)}
                        cancelPending={
                          abbrechenMutation.isPending &&
                          abbrechenMutation.variables === clip.clip_db_id
                        }
                        cancelResult={
                          abbrechenMutation.isSuccess &&
                          abbrechenMutation.variables === clip.clip_db_id
                            ? abbrechenMutation.data
                            : null
                        }
                        fehler={clipFehler(clip.clip_db_id, [
                          {
                            clipDbId: approvalMutation.variables?.clipDbId,
                            error: approvalMutation.error,
                          },
                          {
                            clipDbId: overrideMutation.variables?.clipDbId,
                            error: overrideMutation.error,
                          },
                          { clipDbId: abbrechenMutation.variables, error: abbrechenMutation.error },
                          { clipDbId: discardMutation.variables, error: discardMutation.error },
                        ])}
                      />
                    );
                  })}
                </div>
              )}
              <div className="flex flex-wrap items-center justify-between gap-3 py-3 text-sm text-text-secondary">
                <span>
                  {clipsQuery.isFetching
                    ? t('Aktualisiere…')
                    : t('{visible} von {total} Clips', {
                        visible: filteredClips.length,
                        total: stats.total,
                      })}
                </span>
                {pageCount > 1 && (
                  <div className="flex items-center gap-3">
                    <button
                      type="button"
                      className="studio-button"
                      disabled={currentPage <= 1}
                      onClick={() => setPage(currentPage - 1)}
                    >
                      {t('Zurück')}
                    </button>
                    <span>
                      {currentPage} / {pageCount}
                    </span>
                    <button
                      type="button"
                      className="studio-button"
                      disabled={currentPage >= pageCount}
                      onClick={() => setPage(currentPage + 1)}
                    >
                      {t('Weiter')}
                    </button>
                  </div>
                )}
              </div>
              <button type="button" className="studio-upload" onClick={() => setShowUpload(true)}>
                <span>{t('Noch einen eigenen Clip auf Lager?')}</span>
                <span>{t('MP4 hinzufügen')} →</span>
              </button>
            </div>
          </>
        )}
      </section>
      {activeView === 'freigabe' && twitchUserId && <>
        <button type="button" className="studio-button" onClick={() => setShowAiEditor(true)}>{t('KI-Aufträge ansehen')}</button>
        {workspaceSlots?.publishReview ?? <PublishReviewPanel twitchUserId={twitchUserId} client={publishClient} section="videos" onChanged={refreshSchedule} reloadKey={reloadKey} />}
      </>}
      {(showAiEditor || selectedAiClipId !== undefined) && <WorkspaceDialog eyebrow={t('Mit KI bearbeiten')} title={selectedAiClip?.title ?? t('KI-Bearbeitung & Prüfung')} onClose={() => { setShowAiEditor(false); setSelectedAiClipId(undefined); const params = new URLSearchParams(window.location.search); params.delete('clip_db_id'); window.history.replaceState({}, '', `${window.location.pathname}?${params}${window.location.hash}`); }}>
        {AiEditor && twitchUserId ? <AiEditor twitchUserId={twitchUserId} streamer={streamer} clipDbId={selectedAiClipId} /> : <p>{t('KI-Bearbeitung noch nicht verfügbar')}</p>}
      </WorkspaceDialog>}
      {showUpload && (
        <WorkspaceDialog
          eyebrow={t('MP4 hochladen')}
          title={t('Clip hinzufügen')}
          busy={uploadMutation.isPending}
          onClose={() => setShowUpload(false)}
        >
          <UploadCard
            onUpload={(file) => uploadMutation.mutate(file)}
            isUploading={uploadMutation.isPending}
            uploadError={uploadMutation.error}
            uploadSuccess={uploadMutation.isSuccess}
          />
        </WorkspaceDialog>
      )}
      {layoutChoice && (
        <WorkspaceDialog
          eyebrow={t('Vorlage & Layout')}
          title={t('9:16 Layout-Editor')}
          busy={saveLayoutMutation.isPending}
          onClose={() => setLayoutChoice(null)}
        >
          <LayoutEditor
            initialLayout={layoutChoice}
            baselineLayout={layoutForEditor}
            isSaving={saveLayoutMutation.isPending}
            onSave={(layout) =>
              saveLayoutMutation.mutate(layout, { onSuccess: () => setLayoutChoice(null) })
            }
            saveLabel={t('Layout für {streamer} speichern', { streamer })}
            vorschauClips={vorschauClips}
            geltungHinweis={t('Dieser Ausschnitt wird für neue Clips dieses Kanals verwendet.')}
          />
          {saveLayoutMutation.isError && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {fehlerText(saveLayoutMutation.error, t)}
            </p>
          )}
          {saveLayoutMutation.isSuccess && (
            <p role="status" className="mt-3 text-sm text-success">
              {t('Layout gespeichert.')}
            </p>
          )}
        </WorkspaceDialog>
      )}
      {editingClip && editorClip && (
        <ClipEditorDialog
          clip={editorClip}
          mode={editingClip.mode}
          isSaving={overrideMutation.isPending}
          error={overrideMutation.error}
          onClose={() => setEditingClip(null)}
          onOpenAiEditor={() => {
            if (changeView('ai', editorClip.clip_db_id)) setEditingClip(null);
          }}
          onSaveLayout={(layout) =>
            overrideMutation.mutate(
              { clipDbId: editorClip.clip_db_id, layout },
              { onSuccess: () => setEditingClip(null) },
            )
          }
          onResetLayout={() =>
            overrideMutation.mutate(
              { clipDbId: editorClip.clip_db_id, layout: null },
              { onSuccess: () => setEditingClip(null) },
            )
          }
        />
      )}

      {statusClip && (
        <PlatformStatusDialog
          clip={statusClip}
          platforms={platformStatusQuery.data?.platforms ?? []}
          loading={platformStatusQuery.isPending}
          error={platformStatusQuery.error}
          onClose={() => setStatusClipId(null)}
        />
      )}
      {tiktokPost && (
        <TikTokPostDialog
          clipDbId={tiktokPost.clipDbId}
          pending={approvalMutation.isPending}
          error={approvalMutation.error}
          onClose={() => setTikTokPost(null)}
          onConfirm={(tiktokOptions) => approvalMutation.mutate(
            { ...tiktokPost, decision: 'approve', tiktokOptions },
            { onSuccess: () => setTikTokPost(null) },
          )}
        />
      )}
      {showAnalytics && (
        <WorkspaceDialog
          eyebrow={t('Performance')}
          title={t('Auswertung')}
          onClose={() => setShowAnalytics(false)}
        >
          <AnalyticsTab streamer={streamer} twitchUserId={twitchUserId} isAdmin={isAdmin} statisticsPlatforms={(platformStatusQuery.data?.platforms ?? []).filter((p) => p.capabilities?.statistics).map((p) => p.platform)} />
        </WorkspaceDialog>
      )}
    </div>
  );
}

function ClipEditorDialog({
  error,
  clip,
  mode,
  isSaving,
  onClose,
  onOpenAiEditor,
  onSaveLayout,
  onResetLayout,
}: {
  error: unknown;
  clip: SocialClipMitPosting;
  mode: EditMode;
  isSaving: boolean;
  onClose: () => void;
  onOpenAiEditor: () => void;
  onSaveLayout: (layout: LayoutPayload) => void;
  onResetLayout: () => void;
}) {
  const t = useT();
  const [enrichmentSaving, setEnrichmentSaving] = useState(false);
  const busy = isSaving || enrichmentSaving;
  return (
    <WorkspaceDialog
      eyebrow={mode === 'layout' ? t('Clip-Layout') : t('Metadaten')}
      title={clip.title}
      busy={busy}
      onClose={onClose}
    >
      {clip.source_kind !== 'manual_upload' && <button type="button" disabled={busy} className="studio-button mb-4" onClick={onOpenAiEditor}>{t('KI-Bearbeitung & Prüfung')}</button>}
      {mode === 'layout' ? (
        <div className="space-y-4">
          <LayoutEditor
            initialLayout={clip.effective_layout}
            isSaving={isSaving}
            saveLabel={t('Override speichern')}
            resetLabel={t('Schließen')}
            geltungHinweis={t('Diese Anpassung gilt für diesen Clip.')}
            vorschauClips={
              clip.thumbnail_url
                ? [{ id: String(clip.clip_db_id), titel: clip.title, bildUrl: clip.thumbnail_url }]
                : []
            }
            onSave={onSaveLayout}
            onReset={onClose}
          />
          {error ? (
            <p role="alert" className="text-sm text-danger">
              {fehlerText(error, t)}
            </p>
          ) : null}
          {clip.layout_override && (
            <div className="flex justify-end">
              <button
                type="button"
                onClick={() => {
                  if (window.confirm(t('Override entfernen und Streamer-Default verwenden?'))) {
                    onResetLayout();
                  }
                }}
                className="rounded-lg px-3 py-2 text-sm font-medium text-ui-muted transition-colors hover:bg-white/[0.05] hover:text-white"
              >
                {t('Override entfernen')}
              </button>
            </div>
          )}
        </div>
      ) : (
        <EnrichmentPanel clipDbId={clip.clip_db_id} onClose={onClose} onSavingChange={setEnrichmentSaving} />
      )}
    </WorkspaceDialog>
  );
}

function PublishedSwitch({ value, onChange }: { value: 'clips' | 'vod'; onChange: (next: 'clips' | 'vod') => void }) {
  const t = useT();
  return (
    <div className="studio-subtabs" role="group" aria-label={t('Ansicht wählen')}>
      <button type="button" className="studio-subtab" aria-pressed={value === 'clips'} onClick={() => onChange('clips')}>{t('Veröffentlichte Clips')}</button>
      <button type="button" className="studio-subtab" aria-pressed={value === 'vod'} onClick={() => onChange('vod')}>{t('VOD-Archiv')}</button>
    </div>
  );
}

function StatusFilter({
  value,
  onChange,
}: {
  value: QueueStage;
  onChange: (next: QueueStage) => void;
}) {
  const t = useT();
  return (
    <div className="studio-filters" aria-label={t('Clip-Status')}>
      {QUEUE_FILTERS.map(({ id, label }) => (
        <button
          key={id}
          type="button"
          className="studio-filter"
          aria-pressed={value === id}
          onClick={() => onChange(id)}
        >
          {t(label)}
        </button>
      ))}
    </div>
  );
}

interface UploadCardProps {
  onUpload: (file: File) => void;
  isUploading: boolean;
  /** Kommt als Fehlercode aus dem API-Modul und wird hier erst uebersetzt. */
  uploadError: unknown;
  uploadSuccess: boolean;
}

function UploadCard({ onUpload, isUploading, uploadError, uploadSuccess }: UploadCardProps) {
  const t = useT();
  const [dragActive, setDragActive] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const handleFiles = (files: FileList | null) => {
    if (isUploading || !files || files.length === 0) return;
    const file = files[0];
    if (file.type !== 'video/mp4' && !file.name.toLowerCase().endsWith('.mp4')) {
      alert(t('Bitte eine MP4-Datei wählen.'));
      return;
    }
    onUpload(file);
  };

  const handleDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setDragActive(false);
    handleFiles(e.dataTransfer?.files ?? null);
  };

  return (
    <div className="space-y-3">
      <div
        onDragEnter={(e) => {
          e.preventDefault();
          setDragActive(true);
        }}
        onDragOver={(e) => {
          e.preventDefault();
          setDragActive(true);
        }}
        onDragLeave={(e) => {
          e.preventDefault();
          setDragActive(false);
        }}
        onDrop={handleDrop}
        role="button"
        tabIndex={isUploading ? -1 : 0}
        aria-disabled={isUploading}
        aria-label={t('MP4 hochladen')}
        onKeyDown={(event) => {
          if (!isUploading && (event.key === 'Enter' || event.key === ' ')) {
            event.preventDefault();
            inputRef.current?.click();
          }
        }}
        onClick={() => {
          if (!isUploading) inputRef.current?.click();
        }}
        className={`relative cursor-pointer rounded-xl border border-dashed p-6 text-center transition-colors ${
          dragActive
            ? 'border-ui-accent-strong/50 bg-ui-accent-strong/10'
            : 'border-white/[0.12] bg-black/10 hover:border-white/[0.2] hover:bg-white/[0.03]'
        }`}
      >
        <input
          ref={inputRef}
          type="file"
          accept="video/mp4,.mp4"
          disabled={isUploading}
          className="hidden"
          onChange={(e) => {
            handleFiles(e.target.files);
            e.target.value = '';
          }}
        />
        <Film className="mx-auto mb-2 h-7 w-7 text-ui-faint" />
        <p className="text-sm font-medium text-ui-text">{t('MP4 hier ablegen')}</p>
        <p className="mt-1 text-xs text-ui-faint">{t('oder klicken zum Auswählen · max 200 MB')}</p>
        <p className="mt-3 text-xs leading-5 text-ui-faint">
          {t('Das aktuelle Standard-Layout wird automatisch angewendet.')}
        </p>
      </div>

      {isUploading && (
        <div className="flex items-center gap-2 text-xs text-accent">
          <Loader2 className="w-4 h-4 animate-spin" /> {t('Upload läuft…')}
        </div>
      )}
      {uploadError ? <div className="text-xs text-danger">{fehlerText(uploadError, t)}</div> : null}
      {uploadSuccess && !isUploading && (
        <div className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" />{' '}
          {t('Upload erfolgreich. Clip ist in der Pipeline.')}
        </div>
      )}

      <div className="border-t border-border pt-3 space-y-2 text-[11px] text-text-secondary">
        <div className="flex items-center gap-1.5">
          <Calendar className="w-3 h-3" /> {t('Retention: 14 Tage ab Erstellung')}
        </div>
        <div className="flex items-center gap-1.5">
          <Layers3 className="w-3 h-3" /> {t('Auto-Apply: Streamer-Default-Layout')}
        </div>
      </div>
    </div>
  );
}

/**
 * Freigabe-Modus des Kanals. Eine Entscheidung mit drei Stufen, deshalb
 * Segment-Knoepfe statt Dropdown: alle Optionen sind gleichzeitig sichtbar.
 */
function ApprovalModeCard({
  plan,
  isLoading,
  ladeFehler,
  isSaving,
  error,
  onChange,
}: {
  plan: PostingPlan | null;
  isLoading: boolean;
  /** Fehler des Zeitplan-Abrufs: dann ist der gespeicherte Modus unbekannt. */
  ladeFehler: unknown;
  isSaving: boolean;
  error: unknown;
  onChange: (mode: ApprovalMode) => void;
  onSubtitlesChange: (enabled: boolean) => void;
}) {
  const t = useT();
  // Ohne Plan zeigt die Karte 'manual' als aktiv an. Ein Kanal auf
  // 'full_auto' saehe damit den falschen Modus markiert, und ein Klick auf
  // einen der Knoepfe wuerde ihn festschreiben.
  const gesperrt = istGesperrt({ isLoading, isSaving, ladeFehler });
  const modi = plan?.approval_modes ?? (['manual', 'veto_window', 'full_auto'] as ApprovalMode[]);
  // Ist der Stand unbekannt, wird kein Modus als aktiv markiert: lieber gar
  // keine Angabe als eine falsche.
  const aktiv: ApprovalMode | null =
    plan?.approval_mode ?? (istStandUnbekannt(ladeFehler) ? null : 'manual');

  return (
    <div className="space-y-4 rounded-2xl border border-border bg-ui-panel p-4">
      <div className="flex items-center gap-2">
        <ShieldAlert className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">{t('Freigabe')}</h3>
        {isSaving && <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <div className="grid gap-3 md:grid-cols-3">
        {modi.map((mode) => {
          const texte = APPROVAL_MODE_TEXTE[mode];
          if (!texte) return null;
          const active = aktiv === mode;
          return (
            <button
              key={mode}
              type="button"
              disabled={gesperrt}
              onClick={() => onChange(mode)}
              className={`w-full rounded-xl border px-3 py-2.5 text-left transition-colors disabled:opacity-60 ${
                active
                  ? 'border-ui-accent-strong/30 bg-ui-accent-strong/10'
                  : 'border-border bg-white/[0.02] hover:bg-white/[0.05]'
              }`}
              style={{ transitionProperty: 'border-color, background-color' }}
            >
              <div
                className={`text-sm font-medium ${active ? 'text-ui-accent-ink' : 'text-ui-text'}`}
              >
                {t(texte.label)}
              </div>
              <div className="mt-0.5 text-xs leading-5 text-ui-faint">{t(texte.hinweis)}</div>
            </button>
          );
        })}
      </div>

      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </div>
  );
}

/** Formatiert einen Termin kurz und lesbar, ohne Sekunden. */
/**
 * Auto-Posting und Kadenz je Plattform. Die Defaults kommen aus der Recherche:
 * hoechstens ein Post pro Tag, rund vier pro Woche.
 */
const ZEIT_MUSTER = /^\d{2}:\d{2}$/;

/**
 * Prueft die Eingabe, bevor sie zum Backend geht. Das Backend antwortet auf
 * eine leere Liste mit 400, und die alte Kadenz laeuft unbemerkt weiter: der
 * Fehler muss deshalb am Feld stehen, nicht erst nach dem Speichern.
 */
function pruefeZeiten(eingabe: string): { zeiten: string[] } | { fehler: string } {
  const zeiten = eingabe
    .split(',')
    .map((wert) => wert.trim())
    .filter(Boolean);
  if (zeiten.length === 0) return { fehler: FELD_FEHLER.zeitLeer };
  if (zeiten.length > 12) return { fehler: FELD_FEHLER.zuVieleZeiten };
  for (const zeit of zeiten) {
    if (!ZEIT_MUSTER.test(zeit)) return { fehler: FELD_FEHLER.zeitFormat };
    const [stunde, minute] = zeit.split(':').map(Number);
    if (stunde > 23 || minute > 59) return { fehler: FELD_FEHLER.zeitUngueltig };
  }
  return { zeiten };
}

function PostingScheduleCard({
  twitchUserId,
  availablePlatforms,
  plan,
  isLoading,
  ladeFehler,
  isSaving,
  error,
  onChange,
}: {
  twitchUserId?: string;
  availablePlatforms: string[];
  plan: PostingPlan | null;
  isLoading: boolean;
  /** Fehler des Zeitplan-Abrufs: dann sind Zeitzone und Kadenz unbekannt. */
  ladeFehler: unknown;
  isSaving: boolean;
  error: unknown;
  onChange: (
    platform: SocialPlatform,
    payload: Partial<Omit<PlatformScheduleEntry, 'platform' | 'next_slot'>>,
  ) => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const locale = language === 'en' ? 'en-GB' : 'de-DE';
  const zeitzone = plan?.timezone ?? 'Europe/Berlin';
  // Ohne Plan zeigt die Karte Europe/Berlin und eine leere Plattformliste.
  // Beides ist geraten, deshalb bleibt hier bis zum naechsten erfolgreichen
  // Abruf alles gesperrt.
  const gesperrt = istGesperrt({ isLoading, isSaving, ladeFehler });
  const [formular, setFormular] = useState<Record<string, ZeitplanFormular>>({});
  const [feldFehler, setFeldFehler] = useState<Record<string, string>>({});

  // Kontrollierte Felder brauchen einen Abgleich mit der Serverantwort: das
  // Backend sortiert und entdoppelt die Zeiten, und die Karte wird bei einer
  // Planaenderung nicht neu gemountet. Nur der Inhalt ist die Abhaengigkeit,
  // nicht die Objektidentitaet, sonst wird waehrend des Tippens ueberschrieben.
  const serverStand = JSON.stringify(
    (plan?.platforms ?? []).map((eintrag) => [
      eintrag.platform,
      eintrag.posts_per_week,
      eintrag.max_posts_per_day,
      eintrag.post_times,
    ]),
  );

  // Felder, die der Nutzer geaendert und noch nicht abgeschickt hat. Ohne
  // diese Liste hat der Abgleich mit der Serverantwort das ganze Formular
  // ueberschrieben: wer zwei Felder schnell hintereinander aendert, verlor die
  // zweite Eingabe, sobald die Antwort auf die erste eintraf. Ein Ref statt
  // eines States, weil daran nichts haengt, was neu gezeichnet werden muss.
  const offeneFelderRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const vomServer: Record<string, ZeitplanFormular> = {};
    for (const eintrag of plan?.platforms ?? []) {
      vomServer[eintrag.platform] = {
        postsProWoche: String(eintrag.posts_per_week),
        maxProTag: String(eintrag.max_posts_per_day),
        zeiten: eintrag.post_times.join(', '),
      };
    }
    setFormular((aktuell) =>
      zeitplanFormularAbgleichen(aktuell, vomServer, offeneFelderRef.current),
    );
    setFeldFehler({});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverStand]);

  const setzeFeld = (platform: string, feld: keyof ZeitplanFormular, wert: string) => {
    offeneFelderRef.current.add(zeitplanFeldSchluessel(platform, feld));
    setFormular((aktuell) => ({
      ...aktuell,
      [platform]: { ...aktuell[platform], [feld]: wert },
    }));
  };

  // Das Feld ist verlassen: ab jetzt darf der Server es wieder ueberschreiben,
  // etwa mit sortierten Zeiten. Gilt auch fuer eine ungueltige Eingabe, die
  // gar nicht erst abgeschickt wurde, sonst haelt der Abgleich den ungueltigen
  // Text fest, waehrend der Effekt die Fehlermeldung wegraeumt (siehe
  // `zeitplanFeldVerlassen`).
  const feldAbgeschlossen = (platform: string, feld: keyof ZeitplanFormular) => {
    offeneFelderRef.current.delete(zeitplanFeldSchluessel(platform, feld));
  };

  const setzeFehler = (schluessel: string, text: string | null) => {
    setFeldFehler((aktuell) => {
      const naechstes = { ...aktuell };
      if (text) naechstes[schluessel] = text;
      else delete naechstes[schluessel];
      return naechstes;
    });
  };

  return (
    <div className="studio-schedule space-y-4 rounded-xl border border-border bg-ui-panel">
      <div className="flex items-center gap-2">
        <Calendar className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">{t('Zeitplan')}</h3>
        {isSaving && (
          <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />
        )}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <p className="text-sm text-ui-muted">{t('Zeiten gelten in {tz}.', { tz: zeitzone })} <a href={globalTimezoneUrl(twitchUserId)}>{t('Zeitzone in Verwaltung ändern')}</a></p>

      <div className="space-y-3">
        {(plan?.platforms ?? []).map((eintrag) => {
          const termin = eintrag.next_slot
            ? formatTerminInZone(eintrag.next_slot, locale, zeitzone)
            : null;
          const werte = formular[eintrag.platform] ?? {
            postsProWoche: String(eintrag.posts_per_week),
            maxProTag: String(eintrag.max_posts_per_day),
            zeiten: eintrag.post_times.join(', '),
          };
          const zeitenFehler = feldFehler[`${eintrag.platform}:zeiten`];
          if (!availablePlatforms.includes(eintrag.platform)) {
            return (
              <div key={eintrag.platform} className="rounded-xl border border-border px-4 py-3 text-sm text-ui-muted">
                {t('{platform}: Noch nicht verfügbar. Automatisches Posten ist hier gesperrt.', { platform: PLATFORM_LABELS[eintrag.platform] ?? eintrag.platform })}
              </div>
            );
          }
          // Die Kadenz bleibt sichtbar und aenderbar, auch wenn Auto-Posting
          // aus ist: sonst laesst sie sich nicht vorbereiten und wirkt beim
          // naechsten Einschalten wie aus dem Nichts.
          const gedaempft = eintrag.auto_post ? '' : 'opacity-60';
          return (
            <div
              key={eintrag.platform}
              className="space-y-3 rounded-xl border border-border bg-white/[0.02] px-4 py-3"
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium text-ui-text">
                  {PLATFORM_LABELS[eintrag.platform] ?? eintrag.platform}
                </span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={eintrag.auto_post}
                  aria-label={t('Automatisch posten')}
                  disabled={gesperrt}
                  onClick={() => onChange(eintrag.platform, { auto_post: !eintrag.auto_post })}
                  className={`relative inline-flex h-7 w-12 shrink-0 rounded-full border transition-colors disabled:opacity-60 ${
                    eintrag.auto_post
                      ? 'border-ui-accent-strong/30 bg-ui-accent-strong/35'
                      : 'border-border bg-white/[0.05]'
                  }`}
                  style={{ transitionProperty: 'border-color, background-color' }}
                >
                  <span
                    className={`absolute top-1 h-5 w-5 rounded-full bg-white ${
                      eintrag.auto_post ? 'translate-x-6' : 'translate-x-1'
                    }`}
                    style={{ transitionProperty: 'transform' }}
                  />
                </button>
              </div>

              <div className={`space-y-3 ${gedaempft}`}>
                {!eintrag.auto_post && (
                  <p className="text-xs text-ui-faint">{t('Gilt, sobald Auto-Posting an ist.')}</p>
                )}
                <div className="grid grid-cols-2 gap-3">
                  <label className="block">
                    <span className="text-xs text-ui-faint">{t('Posts pro Woche')}</span>
                    <input
                      type="number"
                      required
                      step={1}
                      min={0}
                      max={70}
                      value={werte.postsProWoche}
                      disabled={gesperrt}
                      onChange={(event) =>
                        setzeFeld(eintrag.platform, 'postsProWoche', event.target.value)
                      }
                      onBlur={() => {
                        const wert = Number(werte.postsProWoche);
                        const gueltig = werte.postsProWoche.trim() !== '' && Number.isFinite(wert);
                        const plan = zeitplanFeldVerlassen(
                          gueltig
                            ? { gueltig: true, unveraendert: wert === eintrag.posts_per_week }
                            : { gueltig: false, fehler: FELD_FEHLER.keineZahl },
                        );
                        setzeFehler(`${eintrag.platform}:woche`, plan.fehler);
                        feldAbgeschlossen(eintrag.platform, 'postsProWoche');
                        if (plan.absenden) {
                          onChange(eintrag.platform, { posts_per_week: wert });
                        }
                      }}
                      className="mt-1 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white"
                    />
                    {feldFehler[`${eintrag.platform}:woche`] && (
                      <span className="mt-1 block text-xs text-danger">
                        {t(feldFehler[`${eintrag.platform}:woche`])}
                      </span>
                    )}
                  </label>
                  <label className="block">
                    <span className="text-xs text-ui-faint">{t('Höchstens pro Tag')}</span>
                    <input
                      type="number"
                      required
                      step={1}
                      min={0}
                      max={10}
                      value={werte.maxProTag}
                      disabled={gesperrt}
                      onChange={(event) =>
                        setzeFeld(eintrag.platform, 'maxProTag', event.target.value)
                      }
                      onBlur={() => {
                        const wert = Number(werte.maxProTag);
                        const gueltig = werte.maxProTag.trim() !== '' && Number.isFinite(wert);
                        const plan = zeitplanFeldVerlassen(
                          gueltig
                            ? { gueltig: true, unveraendert: wert === eintrag.max_posts_per_day }
                            : { gueltig: false, fehler: FELD_FEHLER.keineZahl },
                        );
                        setzeFehler(`${eintrag.platform}:tag`, plan.fehler);
                        feldAbgeschlossen(eintrag.platform, 'maxProTag');
                        if (plan.absenden) {
                          onChange(eintrag.platform, { max_posts_per_day: wert });
                        }
                      }}
                      className="mt-1 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white"
                    />
                    {feldFehler[`${eintrag.platform}:tag`] && (
                      <span className="mt-1 block text-xs text-danger">
                        {t(feldFehler[`${eintrag.platform}:tag`])}
                      </span>
                    )}
                  </label>
                </div>
                <label className="block">
                  <span className="text-xs text-ui-faint">
                    {t('Uhrzeiten, mit Komma getrennt')}
                  </span>
                  <input
                    type="text"
                    required
                    pattern="([01][0-9]|2[0-3]):[0-5][0-9](,\s*([01][0-9]|2[0-3]):[0-5][0-9]){0,11}"
                    value={werte.zeiten}
                    placeholder="18:00, 21:00"
                    disabled={gesperrt}
                    onChange={(event) => setzeFeld(eintrag.platform, 'zeiten', event.target.value)}
                    onBlur={() => {
                      const ergebnis = pruefeZeiten(werte.zeiten);
                      const plan = zeitplanFeldVerlassen(
                        'fehler' in ergebnis
                          ? { gueltig: false, fehler: ergebnis.fehler }
                          : {
                              gueltig: true,
                              unveraendert:
                                ergebnis.zeiten.join(',') === eintrag.post_times.join(','),
                            },
                      );
                      setzeFehler(`${eintrag.platform}:zeiten`, plan.fehler);
                      feldAbgeschlossen(eintrag.platform, 'zeiten');
                      if (plan.absenden && !('fehler' in ergebnis)) {
                        onChange(eintrag.platform, { post_times: ergebnis.zeiten });
                      }
                    }}
                    className={`mt-1 w-full rounded-lg border bg-background/80 px-3 py-2 text-sm text-white ${
                      zeitenFehler ? 'border-danger' : 'border-border'
                    }`}
                  />
                  {zeitenFehler && (
                    <span className="mt-1 block text-xs text-danger">{t(zeitenFehler)}</span>
                  )}
                </label>
              </div>

              {termin && eintrag.auto_post && (
                <div className="text-xs text-ui-muted">
                  {t('Nächster Post: {termin}', { termin })}
                </div>
              )}
            </div>
          );
        })}
      </div>
      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </div>
  );
}

/**
 * Auto-Posting je Spielkategorie. Angereichert wird nur Deadlock; andere
 * Kategorien gehen ohne Titel- und Hashtag-Vorschlaege raus.
 */
function CategoryCard({
  plan,
  isLoading,
  ladeFehler,
  isSaving,
  error,
  onChange,
}: {
  plan: PostingPlan | null;
  isLoading: boolean;
  /** Fehler des Zeitplan-Abrufs: dann ist unbekannt, welche Kategorien an sind. */
  ladeFehler: unknown;
  isSaving: boolean;
  error: unknown;
  onChange: (categoryKey: string, autoPost: boolean) => void;
}) {
  const t = useT();
  const gesperrt = istGesperrt({ isLoading, isSaving, ladeFehler });

  return (
    <div className="space-y-4 rounded-2xl border border-border bg-ui-panel p-4">
      <div className="flex items-center gap-2">
        <Gamepad2 className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">{t('Kategorien')}</h3>
        {isSaving && <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <div className="space-y-2">
        {(plan?.categories ?? []).map((kategorie) => (
          <label
            key={kategorie.category_key}
            className="studio-switch flex items-center justify-between gap-3 rounded-xl border border-border bg-white/[0.02] px-3 py-2.5"
          >
            <span>
              <span className="block text-sm font-medium text-ui-text">
                {t(kategorieLabel(kategorie.category_key, kategorie.display_name))}
              </span>
              <span className="mt-0.5 block text-xs leading-5 text-ui-faint">
                {t('Titel, Beschreibung und Hashtags bearbeitest du im Clip-Editor.')}
              </span>
            </span>
            <input
              type="checkbox"
              role="switch"
              checked={kategorie.auto_post}
              disabled={gesperrt}
              onChange={(event) => onChange(kategorie.category_key, event.target.checked)}
              className="h-4 w-4 shrink-0 accent-primary"
            />
          </label>
        ))}
      </div>
      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </div>
  );
}

/**
 * Vorratswarnung. Steht bewusst als eigene betonte Zeile ueber dem Clip-Pool
 * und nicht kleingedruckt in einer Karte: wer keinen Nachschub liefert, hoert
 * irgendwann auf zu posten, ohne es zu merken.
 */
function VorratsHinweis({
  pool,
  onClipsHolen,
  isHolend,
}: {
  pool: ClipPoolForecast | null;
  onClipsHolen: () => void;
  isHolend: boolean;
}) {
  const t = useT();
  if (!pool || pool.aktive_plattformen === 0) return null;

  const knapp = pool.warnung;
  return (
    <div
      className={`rounded-xl border px-4 py-3 flex flex-wrap items-center gap-3 ${
        knapp ? 'border-warning/45 bg-warning/10' : 'border-border bg-bg/40'
      }`}
    >
      {knapp ? <AlertTriangle className="w-4 h-4 shrink-0 text-warning" /> : <CheckCircle2 className="w-4 h-4 shrink-0 text-success" />}
      <div>
        <div className={`text-sm font-semibold ${knapp ? 'text-warning' : 'text-white'}`}>
          {pool.reicht_fuer_tage === null ? t('{clips} Clips verfügbar', { clips: pool.verfuegbare_clips }) : t('{clips} Clips verfügbar, reicht für etwa {days} Tage', { clips: pool.verfuegbare_clips, days: pool.reicht_fuer_tage })}
        </div>
        <div className="text-xs text-text-secondary mt-0.5">
          {pool.reicht_fuer_tage === null
            ? t('Vorrat ohne Posting-Rhythmus')
            : t(
                'Posting-Rhythmus: {proWoche} Posts pro Woche.',
                {
                  clips: pool.verfuegbare_clips,
                  tage: pool.reicht_fuer_tage,
                  proWoche: pool.posts_pro_woche,
                },
              )}
        </div>
      </div>
      {/* Die Warnung ohne Ausweg war die eigentliche Luecke: hier steht der
          Knopf, der den Vorrat wieder auffuellt. */}
      {knapp && (
        <button
          type="button"
          onClick={onClipsHolen}
          disabled={isHolend}
          className="ml-auto inline-flex items-center gap-1.5 rounded-xl border border-warning/45 bg-warning/15 px-3 py-1.5 text-xs font-bold text-warning hover:bg-warning/25 disabled:opacity-50"
        >
          {isHolend ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <DownloadCloud className="w-3.5 h-3.5" />
          )}
          {t('Clips jetzt holen')}
        </button>
      )}
    </div>
  );
}

/**
 * VOD-Archiv: Twitch-Aufzeichnungen automatisch sichern und auf YouTube
 * spiegeln. Der Download laeuft auch ohne YouTube-Verbindung, deshalb steht
 * hier nur ein Schalter und die Sichtbarkeit der Uploads.
 *
 * Gilt immer fuer den gerade gewaehlten Kanal, deshalb steht er in der
 * Ueberschrift: die Seite zeigt je nach Auswahl unterschiedliche Schalter.
 */
/// Verbindungen zu den Plattformen. Der OAuth-Flow ist ein Redirect auf den
/// Anbieter, deshalb ist "Verbinden" ein Link und kein fetch.
/**
 * Rueckmeldung nach dem OAuth-Umweg. Der Server schickt den Browser mit
 * `?oauth_success=` oder `?oauth_error=` auf das Dashboard zurueck. Ohne diese
 * Auswertung endet jeder Verbindungsversuch wortlos auf derselben Seite, und
 * ein gescheiterter Versuch sieht aus wie ein abgebrochener.
 */
type OauthRueckmeldung = { art: 'ok'; platform: string } | { art: 'fehler'; code: string } | null;

function leseOauthRueckmeldung(): OauthRueckmeldung {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(window.location.search);
  const erfolg = params.get('oauth_success');
  if (erfolg) return { art: 'ok', platform: erfolg };
  const fehler = params.get('oauth_error');
  if (fehler) return { art: 'fehler', code: fehler };
  return null;
}

function PlatformConnectionsCard({
  now,
  streamer,
  twitchUserId,
  platforms,
  isLoading,
  ladeFehler,
  onDisconnect,
  isDisconnecting,
  error,
  disconnectResult,
}: {
  now: number;
  streamer: string;
  twitchUserId?: string;
  platforms: PlatformStatus[];
  isLoading: boolean;
  /** Fehler des Status-Abrufs: dann ist unbekannt, was verbunden ist. */
  ladeFehler: unknown;
  onDisconnect: (platform: string) => void;
  isDisconnecting: boolean;
  error: unknown;
  disconnectResult: { success: boolean; revocation_pending: boolean } | null;
}) {
  const { t, locale } = useLanguage();
  const byName = new Map(platforms.map((p) => [p.platform, p]));
  // Ohne Statusabruf ist jede Zeile geraten. Ein verbundener Kanal saehe dann
  // wie "nicht verbunden" aus und der Streamer startet einen ueberfluessigen
  // OAuth-Flow, deshalb verschwinden hier alle Knoepfe.
  const standUnbekannt = istStandUnbekannt(ladeFehler) || isLoading || platforms.length === 0;
  const gesperrt = istGesperrt({ isLoading, isSaving: isDisconnecting, ladeFehler });
  const rueckmeldung = leseOauthRueckmeldung();

  return (
    <section className="studio-card space-y-4">
      <StudioSectionHeading title={`${t('Verbindungen')} · ${streamer}`} icon={ExternalLink} level={3}
        actions={isLoading && <Loader2 className="h-4 w-4 animate-spin text-ui-accent" />} />

      {rueckmeldung?.art === 'ok' && (
        <div className="rounded-xl border border-success/40 bg-success/10 px-4 py-3 text-xs text-success">
          {PLATFORM_LABELS[rueckmeldung.platform]
            ? t('{platform} ist jetzt verbunden.', {
                platform: PLATFORM_LABELS[rueckmeldung.platform],
              })
            : t('Das Konto ist jetzt verbunden.')}
        </div>
      )}
      {rueckmeldung?.art === 'fehler' && (
        <div
          role="alert"
          className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-xs text-danger space-y-1"
        >
          <div className="font-semibold">{t('Verbinden hat nicht geklappt')}</div>
          <div>{fehlerText({ code: rueckmeldung.code }, t)}</div>
        </div>
      )}

      {disconnectResult && (
        <p role="status" className="text-sm text-text-secondary">
          {disconnectResult.revocation_pending
            ? t('Der gespeicherte Kontozugang wurde gelöscht. Entferne die App zusätzlich in den Einstellungen deines Plattformkontos; der Widerruf dort ist noch nicht bestätigt.')
            : t('Der gespeicherte Kontozugang wurde gelöscht und die Berechtigung beim Anbieter widerrufen.')}
        </p>
      )}
      <LadeFehlerHinweis fehler={ladeFehler} />
      <p className="text-xs leading-5 text-text-secondary">{t('Gespeicherter Kontostand, keine Live-Prüfung beim Anbieter.')}</p>

      <div className="space-y-2">
        {PLATTFORMEN.map((platform) => {
          const status = byName.get(platform);
          const connected = status?.connected ?? false;
          const uploadAvailable = status?.capabilities?.upload ?? false;
          // Ein abgelaufener Zugang, dessen Erneuerung dauerhaft scheitert,
          // sieht sonst aus wie eine gesunde Verbindung, waehrend jeder Upload
          // ins Leere laeuft.
          const abgelaufen = connected && ((status?.needs_reauth ?? false) || (status?.expired ?? false));
          const sammelverbindung =
            connected && !abgelaufen && (status?.uses_global_fallback ?? false);
          const verbindungsablauf = platform === 'instagram' || !status?.automatically_renewed
            ? status?.expires_at
            : status?.refresh_expires_at;
          const ablaufZeit = verbindungsablauf ? new Date(verbindungsablauf).getTime() : NaN;
          const baldAbgelaufen = Number.isFinite(ablaufZeit) && ablaufZeit < now + 30 * 24 * 60 * 60 * 1000;
          const ablauf = baldAbgelaufen
            ? new Date(ablaufZeit).toLocaleDateString(locale, {
                day: '2-digit',
                month: '2-digit',
                year: 'numeric',
              })
            : null;

          let zeile: string;
          let tonKlasse = 'text-text-secondary';
          if (standUnbekannt) {
            zeile = isLoading ? t('Wird geladen') : t('Zustand unbekannt');
            tonKlasse = ladeFehler ? 'text-danger' : 'text-text-secondary';
          } else if (!status) {
            zeile = t('Zustand unbekannt');
          } else if (!uploadAvailable) {
            zeile = t(status?.capabilities.reason ?? 'Noch nicht verfügbar');
          } else if (!connected) {
            zeile = t('nicht verbunden');
          } else if (abgelaufen) {
            zeile = t('Zugang abgelaufen, bitte neu verbinden');
            tonKlasse = 'text-warning';
          } else if (sammelverbindung) {
            zeile = t('Dieser Kanal nutzt ein gemeinsames Konto. Verbinde ein eigenes Konto, um es hier verwalten zu können.');
            tonKlasse = 'text-warning';
          } else {
            zeile = status?.username ?? t('verbunden');
          }

          return (
            <PlatformCheck key={platform} platform={platform}>
              <div className="min-w-0">
                <div className="text-sm font-medium text-ui-text">
                  {PLATFORM_LABELS[platform] ?? platform}
                </div>
                <div className={`text-xs ${tonKlasse}`}>{zeile}</div>
                <p className="mt-1 text-xs text-text-secondary">{t(authorizationFact(status, isLoading, Boolean(ladeFehler)).label)}</p>
                {!standUnbekannt && connected && !abgelaufen && (
                  <div className="text-[11px] text-text-secondary">
                    {ablauf
                      ? t('Bitte bis {datum} neu verbinden.', { datum: ablauf })
                      : status?.automatically_renewed
                        ? t('Verlängert sich automatisch.')
                        : null}
                  </div>
                )}
                {!standUnbekannt && platform === 'tiktok' && (
                  <p className="mt-1 text-xs text-text-secondary">{t('TikTok-Statistiken sind in dieser Beta nicht verfügbar.')}</p>
                )}
              </div>
              {standUnbekannt ? null : connected && !status?.uses_global_fallback ? (
                <div className="flex shrink-0 flex-wrap items-center gap-2">
                  {abgelaufen && uploadAvailable && (
                    <a href={oauthStartUrl(platform, twitchUserId)} className="rounded-lg border border-ui-accent-strong/25 px-3 py-1.5 text-sm text-ui-accent-ink">
                      {t('Neu verbinden')}
                    </a>
                  )}
                  <button
                    type="button"
                    disabled={gesperrt}
                    onClick={() => {
                      const frage = t('{platform} für {streamer} trennen und den gespeicherten Kontozugang löschen? Uploads brauchen danach eine verfügbare Verbindung.', {
                        platform: PLATFORM_LABELS[platform] ?? platform,
                        streamer,
                      });
                      if (window.confirm(frage)) onDisconnect(platform);
                    }}
                    className="rounded-lg border border-border bg-white/[0.03] px-3 py-1.5 text-sm font-medium text-ui-muted transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-40"
                  >
                    {t('Trennen')}
                  </button>
                </div>
              ) : !uploadAvailable ? null : (
                <a
                  href={oauthStartUrl(platform, twitchUserId)}
                  className="shrink-0 rounded-lg border border-ui-accent-strong/25 bg-ui-accent-strong/12 px-3 py-1.5 text-sm font-medium text-ui-accent-ink transition-colors hover:bg-ui-accent-strong/18"
                >
                  {status?.uses_global_fallback ? t('Eigenes Konto verbinden') : abgelaufen ? t('Neu verbinden') : t('Verbinden')}
                </a>
              )}
            </PlatformCheck>
          );
        })}
      </div>

      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </section>
  );
}

function VodArchiveCard({
  streamer,
  settings,
  isLoading,
  ladeFehler,
  isSaving,
  error,
  onChange,
}: {
  streamer: string;
  settings: VodArchiveSettings | null;
  isLoading: boolean;
  /** Fehler des Einstellungs-Abrufs: dann ist der gespeicherte Stand unbekannt. */
  ladeFehler: unknown;
  isSaving: boolean;
  error: unknown;
  onChange: (next: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) => void;
}) {
  const t = useT();
  // Ohne geladene Einstellungen zeigt die Karte "aus / Privat". Ein Klick auf
  // den Schalter schickt `privacy: 'private'` mit und stuft eine gespeicherte
  // Sichtbarkeit still herunter, deshalb bleibt hier alles gesperrt.
  const gesperrt = istGesperrt({ isLoading, isSaving, ladeFehler });
  const enabled = settings?.enabled ?? false;
  const privacy = settings?.privacy ?? 'private';
  const options = settings?.privacy_options ?? ['private', 'unlisted', 'public'];
  const labels: Record<VodArchivePrivacy, string> = {
    private: t('Privat'),
    unlisted: t('Nicht gelistet'),
    public: t('Öffentlich'),
  };

  return (
    <section className="studio-card space-y-4">
      <StudioSectionHeading title={`${t('VOD-Archiv')} · ${settings?.streamer_login ?? streamer}`} icon={Archive} level={3}
        actions={isSaving && <Loader2 className="h-4 w-4 animate-spin text-ui-accent" />} />

      <LadeFehlerHinweis fehler={ladeFehler} />

      <label className="studio-switch flex items-center justify-between gap-3 rounded-xl border border-border bg-white/[0.02] px-3 py-2.5">
        <span className="text-sm font-medium text-ui-text">{t('Automatisch sichern')}</span>
        <input
          type="checkbox"
          role="switch"
          checked={enabled}
          disabled={gesperrt}
          onChange={(event) => onChange({ enabled: event.target.checked, privacy })}
          className="h-4 w-4 accent-orange"
        />
      </label>

      <div className="space-y-2">
        <div className="text-xs uppercase tracking-[0.14em] text-text-secondary">
          {t('Sichtbarkeit auf YouTube')}
          {settings?.privacy_forced && (
            <span className="normal-case tracking-normal text-warning">
              {' '}
              {t('· YouTube erzwingt privat, bis das Google-Projekt auditiert ist')}
            </span>
          )}
        </div>
        <div className="grid grid-cols-3 gap-2">
          {options.map((option) => (
            <button
              key={option}
              type="button"
              disabled={gesperrt || settings?.privacy_forced}
              onClick={() => onChange({ enabled, privacy: option })}
              className={`rounded-lg border px-3 py-2 text-sm font-medium transition-colors ${
                privacy === option
                  ? 'border-ui-accent-strong/30 bg-ui-accent-strong/12 text-ui-accent-ink'
                  : 'border-border bg-white/[0.02] text-ui-muted hover:bg-white/[0.06] hover:text-white'
              } disabled:opacity-40`}
            >
              {labels[option]}
            </button>
          ))}
        </div>
      </div>

      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </section>
  );
}

