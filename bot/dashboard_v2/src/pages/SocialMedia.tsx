import { useEffect, useMemo, useRef, useState, type ComponentType, type DragEvent, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, Calendar, CalendarClock, CheckCircle2, ExternalLink, Film, Layers3, Loader2, Pencil, ShieldAlert, SlidersHorizontal } from 'lucide-react';
import { useLanguage, useT } from '@/context/LanguageContext';
import { PlatformCheck } from '@/components/socialmedia/PlatformCheck';
import { StatusChip } from '@/components/socialmedia/StatusChip';
import { HintChip } from '@/components/socialmedia/HintChip';
import { istGesperrt, istStandUnbekannt } from '@/components/socialmedia/kartenZustand';
import { StudioActivity } from '@/components/socialmedia/StudioActivity';
import { StudioAutopilotSwitches, StudioHero } from '@/components/socialmedia/StudioHero';
import { AiPlanOptions } from '@/components/socialmedia/ai/EditorOptionsPanel';
import { PLAN_PLATFORMS } from '@/components/socialmedia/pipelinePlan';
import { WorkspaceDialog } from '@/components/socialmedia/WorkspaceDialog';
import { PublishReviewPanel } from '@/components/socialmedia/autopilot/publish-review/PublishReviewPanel';
import { publishReviewClient, type PublishReviewClient } from '@/components/socialmedia/autopilot/publish-review/api';
import { ImportPoolPanel } from '@/components/socialmedia/autopilot/import-pool';
import { ScheduleQueuePanel, scheduleQueueApi, type ScheduleQueueApi } from '@/components/socialmedia/autopilot/schedule-queue';
import { PipelineOverview, type StudioOverviewData } from '@/components/socialmedia/StudioOverview';
import { StudioStatusBar } from '@/components/socialmedia/StudioStatusBar';
import { socialStageFromSearch, socialViewFromSearch, socialViewSearch, socialWorkspaceDepartureAllowed, type AiEditorMountProps } from '@/components/socialmedia/studioNavigation';
import { authorizationFact } from '@/components/socialmedia/platformPresentation';
import { loadQueueSnapshot, type QueueStage } from '@/components/socialmedia/queuePresentation';
import '@/components/socialmedia/studio.css';
import { LayoutSchematic } from '@/components/socialmedia/LayoutSchematic';
import { StudioSectionHeading } from '@/components/socialmedia/StudioSectionHeading';
import { VodArchiveList } from '@/components/socialmedia/VodArchiveList';
import { LayoutEditor, type VorschauClip } from '@/components/socialmedia/LayoutEditor';
import { LadeFehlerHinweis } from '@/components/socialmedia/LadeFehlerHinweis';
import { SocialMediaForbiddenError, fetchPostingPlan, fetchClips, fetchVodArchiveSettings, fetchPlatformStatus, disconnectPlatform, oauthStartUrl, type PlatformStatus, fetchStreamerLayout, saveStreamerLayout, saveVodArchiveSettings, uploadClip } from '@/api/socialMedia';
import { fehlerText, PLATFORM_LABELS, SOCIAL_MEDIA_TABS, type SocialMediaView, type SocialMediaViewTarget } from '@/components/socialmedia/labels';
import { type PostingPlan, DEFAULT_LAYOUT, type LayoutPayload, type SocialPlatform, type StreamerLayoutResponse, type VodArchivePrivacy, type VodArchiveSettings } from '@/types/socialMedia';

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
  };
}

const PLATTFORMEN: SocialPlatform[] = ['youtube', 'tiktok', 'instagram'];

const RELOAD_QUERY_ROOTS = new Set(['social-media', 'social-ai', 'social-ai-publishing-context', 'vod-archive']);

const TAB_ICONS: Record<SocialMediaView, ComponentType<{ className?: string }>> = {
  heute: Calendar,
  clips: Layers3,
  planen: CalendarClock,
  vod: Archive,
  einstellungen: SlidersHorizontal,
};

const retryUnlessForbidden = (failureCount: number, err: unknown) => !(err instanceof SocialMediaForbiddenError) && failureCount < 2;

export function SocialMedia({ streamer, twitchUserId, isAdmin = false, aiEditor: AiEditor, scheduleApi = scheduleQueueApi, publishClient = publishReviewClient, workspaceSlots }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const t = useT();
  const [showUpload, setShowUpload] = useState(false);
  const [layoutChoice, setLayoutChoice] = useState<LayoutPayload | null>(null);
  const [activeView, setActiveView] = useState<SocialMediaView>(() => socialViewFromSearch(window.location.search));
  const [clipStage, setClipStage] = useState<QueueStage | undefined>(() => socialStageFromSearch(window.location.search));
  const [reloadKey, setReloadKey] = useState(0);
  const [reloading, setReloading] = useState(false);
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const stage = socialStageFromSearch(window.location.search);
    params.set('view', socialViewFromSearch(window.location.search));
    params.delete('clip_db_id');
    if (stage && stage !== 'all') params.set('stage', stage);
    else params.delete('stage');
    window.history.replaceState({}, '', `${window.location.pathname}?${params}${window.location.hash}`);
  }, []);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  const layoutQuery = useQuery<StreamerLayoutResponse, Error>({
    queryKey: ['social-media', 'streamer-layout', twitchUserId],
    queryFn: () => fetchStreamerLayout(twitchUserId),
    enabled: !!twitchUserId,
    retry: retryUnlessForbidden,
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
    retry: retryUnlessForbidden,
  });

  const vorschauClipsQuery = useQuery({
    queryKey: ['social-media', 'vorschau-clips', streamer, twitchUserId],
    queryFn: () => fetchClips({ status: 'all', twitch_user_id: twitchUserId, page: 1, page_size: 12 }),
    enabled: !!twitchUserId && activeView === 'planen',
    staleTime: 5 * 60 * 1000,
    retry: retryUnlessForbidden,
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
    retry: retryUnlessForbidden,
  });

  const vodArchiveQuery = useQuery<VodArchiveSettings, Error>({
    queryKey: ['social-media', 'vod-archive-settings', twitchUserId],
    queryFn: () => fetchVodArchiveSettings(twitchUserId),
    enabled: !!twitchUserId,
    retry: retryUnlessForbidden,
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

  const platformStatusQuery = useQuery({
    queryKey: ['social-media', 'platform-status', twitchUserId],
    queryFn: () => fetchPlatformStatus(twitchUserId),
    enabled: !!twitchUserId,
    retry: retryUnlessForbidden,
  });

  const isForbidden =
    layoutQuery.error instanceof SocialMediaForbiddenError ||
    clipsQuery.error instanceof SocialMediaForbiddenError ||
    postingPlanQuery.error instanceof SocialMediaForbiddenError;

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
      setReloadKey((key) => key + 1);
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

  const vodArchiveMutation = useMutation({
    mutationFn: (payload: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) =>
      saveVodArchiveSettings(twitchUserId, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'vod-archive-settings', twitchUserId] });
    },
  });

  const targetPlatformMutation = useMutation({
    mutationFn: async ({ platform, active }: { platform: SocialPlatform; active: boolean }) => {
      const current = await scheduleApi.load(twitchUserId);
      const selected = new Set(current.settings.target_platforms);
      if (active) selected.add(platform);
      else selected.delete(platform);
      return scheduleApi.save({ target_platforms: PLAN_PLATFORMS.map((item) => item.id).filter((id) => selected.has(id)) }, twitchUserId);
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-schedule'] });
      setReloadKey((key) => key + 1);
    },
  });

  const changeView = (target: SocialMediaViewTarget, stage?: QueueStage) => {
    const next = socialViewFromSearch(`?view=${target}`);
    const nextStage = next === 'clips' ? stage ?? socialStageFromSearch(`?view=${target}`) : undefined;
    if (next === activeView && nextStage === clipStage) return true;
    if (!socialWorkspaceDepartureAllowed(document, () => window.confirm(t('Ungespeicherte Änderungen verwerfen?')))) return false;
    setActiveView(next);
    setClipStage(nextStage);
    const search = socialViewSearch(window.location.search, next, nextStage);
    window.history.replaceState({}, '', `${window.location.pathname}?${search}${window.location.hash}`);
    return true;
  };

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
    changeView(view, stage);
  };
  const refreshSchedule = () => {
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-schedule', twitchUserId] });
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-publish', twitchUserId] });
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
  };
  const refreshAll = () => {
    if (reloading) return;
    setReloading(true);
    setReloadKey((key) => key + 1);
    void queryClient.invalidateQueries({ predicate: (query) => RELOAD_QUERY_ROOTS.has(String(query.queryKey[0])) }).finally(() => setReloading(false));
  };

  const layoutCards = (
    <div className="studio-templates">
      <div className="studio-templates-bar">
        <p className="studio-templates-hint">{t('Lege den Standard-Ausschnitt für neue Clips fest. Einzelne Clips kannst du weiterhin separat anpassen.')}</p>
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
      </div>
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
    </div>
  );

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

  if (!streamer) {
    if (isAdmin && (activeView === 'vod' || clipStage === 'posted')) {
      return (
        <div className="social-studio studio-archive space-y-6" data-studio-version="industrial-gold-live-v1">
          <VodArchiveList twitchUserId={twitchUserId} isAdmin={isAdmin} />
        </div>
      );
    }
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

  return (
    <div className="social-studio space-y-6" data-studio-version="industrial-gold-live-v1">
      <StudioHero streamer={streamer} onUpload={() => setShowUpload(true)} onRefresh={refreshAll} refreshing={reloading}
        autopilot={twitchUserId ? <StudioAutopilotSwitches owner={twitchUserId} settings={scheduleQuery.data?.settings} failed={scheduleQuery.isError} api={scheduleApi} onSaved={() => setReloadKey((key) => key + 1)} /> : undefined} />
      <StudioStatusBar poolFailed={postingPlanQuery.isError} data={overviewData} pool={postingPlanQuery.isError ? undefined : postingPlanQuery.data?.pool} onNavigate={openView} />

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
              onClick={() => changeView(id, id === 'clips' ? 'all' : undefined)}
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
                const nextId = SOCIAL_MEDIA_TABS[nextIndex].id;
                if (changeView(nextId, nextId === 'clips' ? 'all' : undefined)) {
                  document.getElementById(`studio-tab-${nextId}`)?.focus();
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
        className={activeView === 'clips' ? 'studio-pipeline min-w-0 space-y-4' : activeView === 'vod' ? 'studio-archive min-w-0' : 'studio-automation-workspace min-w-0'}
      >
        {activeView === 'heute' ? (
          <PipelineOverview data={overviewData} now={now} onNavigate={openView} twitchUserId={twitchUserId} streamer={streamer} isAdmin={isAdmin} statisticsPlatforms={(platformStatusQuery.data?.platforms ?? []).filter((p) => p.capabilities?.statistics).map((p) => p.platform)} publishClient={publishClient} testRunApi={scheduleApi} onTestRunChanged={refreshSchedule} activity={<StudioActivity twitchUserId={twitchUserId} data={overviewData} now={now} onNavigate={openView} />} />
        ) : activeView === 'clips' ? (
          <div className="studio-library-import">
            {workspaceSlots?.libraryImport ?? <ImportPoolPanel twitchUserId={twitchUserId} reloadKey={reloadKey} onChanged={refreshSchedule} initialStage={clipStage ?? 'all'} />}
          </div>
        ) : activeView === 'planen' ? (
          <div className="studio-plan-workspace">
            {workspaceSlots?.automation ? (
              <div className="studio-card">{workspaceSlots.automation}</div>
            ) : (
              <ScheduleQueuePanel twitchUserId={twitchUserId} api={scheduleApi} onChanged={refreshSchedule} reloadKey={reloadKey}
                layoutSlot={layoutCards}
                aiOptionsSlot={AiEditor && twitchUserId ? <AiPlanOptions channel={twitchUserId} onOpenSettings={() => openView('einstellungen')} /> : undefined}
                onOpenHeute={() => openView('heute')} />
            )}
          </div>
        ) : activeView === 'vod' ? (
          <div className="studio-vod-workspace">
            <VodArchiveCard
              streamer={streamer}
              settings={vodArchiveQuery.data ?? null}
              isLoading={vodArchiveQuery.isLoading}
              ladeFehler={vodArchiveQuery.error}
              isSaving={vodArchiveMutation.isPending}
              error={vodArchiveMutation.error}
              onChange={(next) => vodArchiveMutation.mutate(next)}
            />
            <VodArchiveList twitchUserId={twitchUserId} isAdmin={isAdmin} />
          </div>
        ) : (
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
                targetPlatforms={scheduleQuery.data?.settings.target_platforms ?? null}
                targetSaving={targetPlatformMutation.isPending}
                targetError={targetPlatformMutation.isError}
                onToggleTarget={(platform, active) => targetPlatformMutation.mutate({ platform, active })}
              />
              {AiEditor && twitchUserId && <section className="studio-card studio-production-card" aria-labelledby="studio-production-title">
                <AiEditor twitchUserId={twitchUserId} streamer={streamer} settingsOnly />
              </section>}
            </div>
            {twitchUserId && <section className="studio-card"><PublishReviewPanel twitchUserId={twitchUserId} client={publishClient} section="notifications" onChanged={refreshSchedule} reloadKey={reloadKey} /></section>}
          </div>
        )}
      </section>
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
    </div>
  );
}

interface UploadCardProps {
  onUpload: (file: File) => void;
  isUploading: boolean;
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
  targetPlatforms,
  targetSaving,
  targetError,
  onToggleTarget,
}: {
  now: number;
  streamer: string;
  twitchUserId?: string;
  platforms: PlatformStatus[];
  isLoading: boolean;
  ladeFehler: unknown;
  onDisconnect: (platform: string) => void;
  isDisconnecting: boolean;
  error: unknown;
  disconnectResult: { success: boolean; revocation_pending: boolean } | null;
  targetPlatforms: string[] | null;
  targetSaving: boolean;
  targetError: boolean;
  onToggleTarget: (platform: SocialPlatform, active: boolean) => void;
}) {
  const { t, locale } = useLanguage();
  const byName = new Map(platforms.map((p) => [p.platform, p]));
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
          let ton: 'neutral' | 'success' | 'warning' | 'danger' = 'neutral';
          let detail: string | null = null;
          if (standUnbekannt) {
            zeile = isLoading ? t('Wird geladen') : t('Zustand unbekannt');
            ton = ladeFehler ? 'danger' : 'neutral';
          } else if (!status) {
            zeile = t('Zustand unbekannt');
          } else if (!uploadAvailable) {
            zeile = t('Noch nicht verfügbar');
            detail = status.capabilities.reason ? t(status.capabilities.reason) : null;
          } else if (!connected) {
            zeile = t('nicht verbunden');
          } else if (abgelaufen) {
            zeile = t('Abgelaufen');
            detail = t('Zugang abgelaufen, bitte neu verbinden');
            ton = 'warning';
          } else if (sammelverbindung) {
            zeile = t('Gemeinsames Konto');
            detail = t('Dieser Kanal nutzt ein gemeinsames Konto. Verbinde ein eigenes Konto, um es hier verwalten zu können.');
            ton = 'warning';
          } else if (ablauf) {
            zeile = t('Neu verbinden bis {datum}', { datum: ablauf });
            ton = 'warning';
          } else {
            zeile = t('Verbunden');
            ton = 'success';
          }
          const autoPostingBlocker: AutoPostingBlocker = standUnbekannt ? 'unknown' : !uploadAvailable ? 'no_upload' : connected && !abgelaufen ? null : 'disconnected';
          const autoPostingAktiv = targetPlatforms?.includes(platform) ?? false;
          const autoPostingHinweis = autoPostingHint(autoPostingBlocker, autoPostingAktiv);
          const hinweise = [
            connected && status?.username ? status.username : null,
            detail,
            t(authorizationFact(status, isLoading, Boolean(ladeFehler)).label),
            !standUnbekannt && connected && !abgelaufen && !ablauf && status?.automatically_renewed ? t('Verlängert sich automatisch.') : null,
            !standUnbekannt && platform === 'tiktok' ? t('TikTok-Statistiken sind in dieser Beta nicht verfügbar.') : null,
          ].filter((hinweis): hinweis is string => Boolean(hinweis));

          return (
            <PlatformCheck key={platform} platform={platform}>
              <div className="studio-connection-name">
                <span className="text-sm font-medium text-ui-text">{PLATFORM_LABELS[platform] ?? platform}</span>
                <StatusChip tone={ton}>{zeile}</StatusChip>
                {hinweise.length > 0 && <HintChip hint={hinweise.join(' · ')} />}
              </div>
              <div className="studio-connection-actions">
                <AutoPostingSwitch
                  active={autoPostingAktiv}
                  blocker={autoPostingBlocker}
                  describedBy={autoPostingHinweis ? `studio-blocker-${platform}` : undefined}
                  disabled={gesperrt || targetPlatforms === null || targetSaving}
                  onChange={(active) => onToggleTarget(platform, active)}
                />
              {standUnbekannt ? null : connected && !status?.uses_global_fallback ? (
                <>
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
                </>
              ) : !uploadAvailable ? null : (
                <a
                  href={oauthStartUrl(platform, twitchUserId)}
                  className="shrink-0 rounded-lg border border-ui-accent-strong/25 bg-ui-accent-strong/12 px-3 py-1.5 text-sm font-medium text-ui-accent-ink transition-colors hover:bg-ui-accent-strong/18"
                >
                  {status?.uses_global_fallback ? t('Eigenes Konto verbinden') : abgelaufen ? t('Neu verbinden') : t('Verbinden')}
                </a>
              )}
              </div>
              {autoPostingHinweis && <p id={`studio-blocker-${platform}`} className="studio-connection-blocker text-xs text-text-secondary">{t(autoPostingHinweis)}</p>}
            </PlatformCheck>
          );
        })}
      </div>

      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
      {targetError ? <p role="alert" className="text-xs text-danger">{t('Auto-Posting konnte nicht gespeichert werden. Bitte versuche es erneut.')}</p> : null}
    </section>
  );
}

type AutoPostingBlocker = 'unknown' | 'no_upload' | 'disconnected' | null;

function autoPostingHint(blocker: AutoPostingBlocker, active: boolean): string | null {
  if (blocker === 'no_upload') return 'Automatisches Posten ist für diese Plattform noch nicht verfügbar.';
  if (blocker === 'disconnected') return active ? 'Verbinde das Konto erneut, sonst wird hier nichts gepostet.' : 'Erst nach dem Verbinden verfügbar.';
  return null;
}

function AutoPostingSwitch({ active, blocker, describedBy, disabled, onChange }: {
  active: boolean;
  blocker: AutoPostingBlocker;
  describedBy?: string;
  disabled: boolean;
  onChange: (active: boolean) => void;
}) {
  const t = useT();
  const hint = autoPostingHint(blocker, active);
  return (
    <label className="studio-switch studio-target-switch" title={hint ? t(hint) : undefined}>
      <input type="checkbox" role="switch" checked={active} aria-describedby={describedBy} disabled={disabled || (blocker !== null && !active)} onChange={(event) => onChange(event.target.checked)} />
      <span>{t('Auto-Posting')}</span>
    </label>
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
  ladeFehler: unknown;
  isSaving: boolean;
  error: unknown;
  onChange: (next: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) => void;
}) {
  const t = useT();
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
              className={`flex h-auto min-h-10 items-center justify-center rounded-lg border px-2 py-2 text-center text-sm font-medium leading-tight transition-colors ${
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

