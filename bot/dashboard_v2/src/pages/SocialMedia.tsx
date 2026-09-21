import { useEffect, useMemo, useRef, useState, type DragEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  BarChart3,
  CheckCircle2,
  Archive,
  Film,
  Loader2,
  ShieldAlert,
  Layers3,
  Calendar,
  Gamepad2,
  ExternalLink,
  Wand2,
  SlidersHorizontal,
  Languages,
  DownloadCloud,
  XCircle,
  Clapperboard,
  Crop,
  MoreHorizontal,
  Plus,
  List,
  LayoutGrid,
} from 'lucide-react';
import { useLanguage, useT } from '@/context/LanguageContext';
import { LANGUAGES, LANGUAGE_LABELS, type Language } from '@/i18n/dictionary';
import { WorkspaceDialog } from '@/components/socialmedia/WorkspaceDialog';
import { PostingPlanDraft } from '@/components/socialmedia/PostingPlanDraft';
import {
  filterQueue,
  queueStats,
  queueStage,
  loadQueueSnapshot,
  QUEUE_FILTERS,
  type QueueStage,
} from '@/components/socialmedia/queuePresentation';
import '@/components/socialmedia/studio.css';
import { AnalyticsTab } from '@/components/socialmedia/AnalyticsTab';
import { LayoutEditor, type VorschauClip } from '@/components/socialmedia/LayoutEditor';
import { EnrichmentPanel } from '@/components/socialmedia/EnrichmentPanel';
import { LadeFehlerHinweis } from '@/components/socialmedia/LadeFehlerHinweis';
import {
  clipFehler,
  istGesperrt,
  istStandUnbekannt,
  zeitplanFeldSchluessel,
  zeitplanFeldVerlassen,
  zeitplanFormularAbgleichen,
  type ZeitplanFormular,
} from '@/components/socialmedia/kartenZustand';
import {
  cancelScheduledPost,
  decideClipApproval,
  SocialMediaForbiddenError,
  discardClip,
  fetchPostingPlan,
  fetchClips,
  fetchTwitchClips,
  fetchVodArchiveSettings,
  fetchPlatformStatus,
  disconnectPlatform,
  oauthStartUrl,
  type PlatformStatus,
  type SocialClipMitPosting,
  fetchStreamerLayout,
  saveStreamerLayout,
  saveVodArchiveSettings,
  setClipLayoutOverride,
  uploadClip,
  requestPreview,
  getPreviewStatus,
  previewFileUrl,
} from '@/api/socialMedia';
import {
  APPROVAL_MODE_TEXTE,
  FELD_FEHLER,
  fehlerText,
  kategorieLabel,
  PLATFORM_LABELS,
  SOCIAL_MEDIA_TABS,
  STATUS_LABELS,
  type SocialMediaView,
} from '@/components/socialmedia/labels';
import {
  type ApprovalMode,
  type ClipPoolForecast,
  type PlatformScheduleEntry,
  type PostingPlan,
  DEFAULT_LAYOUT,
  type LayoutPayload,
  type SocialPlatform,
  type StreamerLayoutResponse,
  type VodArchivePrivacy,
  type VodArchiveSettings,
} from '@/types/socialMedia';

interface SocialMediaProps {
  streamer: string;
  /** Reports und Cross-Auswertungen sind der Verwaltung vorbehalten. */
  isAdmin?: boolean;
}

/** Deutscher Text als Schluessel: ohne Uebersetzung bleibt er einfach stehen. */
type Translate = (text: string, params?: Record<string, string | number>) => string;

/** Die drei Plattformen in der Reihenfolge, in der sie auf der Karte stehen. */
const PLATTFORMEN: SocialPlatform[] = ['youtube', 'tiktok', 'instagram'];

/**
 * Termin in der Zeitzone des Kanals. Eine kaputte Zeitzone aus der Datenbank
 * darf die Karte nicht kippen, deshalb faellt die Formatierung still auf die
 * Browserzeit zurueck.
 */
function formatTerminInZone(iso: string, locale: string, timezone: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const optionen: Intl.DateTimeFormatOptions = {
    weekday: 'short',
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  };
  try {
    return date.toLocaleString(locale, { ...optionen, timeZone: timezone });
  } catch {
    return date.toLocaleString(locale, optionen);
  }
}

// Clip-Laenge als m:ss auf der Kachel. Reine Sekunden ("30s") lasen sich in der
// Metazeile schlecht, sobald ein Clip ueber eine Minute geht.
function formatClipDauer(sekunden: number | null | undefined): string {
  const gesamt = Math.max(0, Math.round(sekunden ?? 0));
  const minuten = Math.floor(gesamt / 60);
  const rest = gesamt % 60;
  return `${minuten}:${String(rest).padStart(2, '0')}`;
}

function formatRetention(retentionUntil: string | null, t: Translate): string {
  if (!retentionUntil) return '—';
  const target = new Date(retentionUntil);
  const now = new Date();
  const ms = target.getTime() - now.getTime();
  const days = Math.floor(ms / (1000 * 60 * 60 * 24));
  if (days < 0) return t('überfällig');
  if (days === 0) return t('heute');
  if (days === 1) return t('morgen');
  return t('{days} Tage', { days });
}

type EditMode = 'layout' | 'enrichment';

/** Zu jedem Bereich der Seite das Symbol; die Beschriftung kommt aus labels.ts. */
const TAB_ICONS: Record<SocialMediaView, React.ComponentType<{ className?: string }>> = {
  pool: Layers3,
  plan: Calendar,
  layout: Crop,
  konten: SlidersHorizontal,
};

export function SocialMedia({ streamer, isAdmin = false }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const t = useT();
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
    /oauth/.test(window.location.search) ? 'konten' : 'pool',
  );
  const [showAnalytics, setShowAnalytics] = useState(false);

  const layoutQuery = useQuery<StreamerLayoutResponse, Error>({
    queryKey: ['social-media', 'streamer-layout', streamer],
    queryFn: () => fetchStreamerLayout(streamer),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const clipsQuery = useQuery({
    queryKey: ['social-media', 'clips', streamer],
    queryFn: ({ signal }) =>
      loadQueueSnapshot(
        (page) => fetchClips({ status: 'all', streamer, page, page_size: 100 }, signal),
        signal,
      ),
    enabled: !!streamer,
    staleTime: 30 * 1000,
    refetchOnWindowFocus: false,
    retry: (failureCount, err) => !(err instanceof SocialMediaForbiddenError) && failureCount < 2,
  });

  // Vorschauclips fuer den Layout-Editor. Bewusst eine eigene Abfrage ohne den
  // Statusfilter der Liste: steht der Filter auf "Veroeffentlicht" und ist dort
  // nichts drin, haette der Editor sonst kein Bild.
  const vorschauClipsQuery = useQuery({
    queryKey: ['social-media', 'vorschau-clips', streamer],
    queryFn: () =>
      fetchClips({ status: 'all', streamer: streamer || undefined, page: 1, page_size: 12 }),
    // Vorschauclips werden nur im fokussierten Layout-Bereich gebraucht.
    enabled: !!streamer && activeView === 'layout',
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
    queryKey: ['social-media', 'posting-plan', streamer],
    queryFn: () => fetchPostingPlan(streamer),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const vodArchiveQuery = useQuery<VodArchiveSettings, Error>({
    queryKey: ['social-media', 'vod-archive-settings', streamer],
    queryFn: () => fetchVodArchiveSettings(streamer),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
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
    mutationFn: (layout: LayoutPayload) => saveStreamerLayout({ streamer_login: streamer, layout }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'streamer-layout', streamer] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) => uploadClip({ file, streamer_login: streamer }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  const discardMutation = useMutation({
    mutationFn: (clipDbId: number) => discardClip(clipDbId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  const overrideMutation = useMutation({
    mutationFn: ({ clipDbId, layout }: { clipDbId: number; layout: LayoutPayload | null }) =>
      setClipLayoutOverride(clipDbId, layout),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  // Alle drei Zeitplan-Aufrufe liefern den kompletten Plan zurueck. Wir setzen
  // ihn direkt in den Cache, damit der neu berechnete Termin und die
  // Vorratsrechnung ohne zweite Abfrage sofort stehen.
  const planKey = ['social-media', 'posting-plan', streamer];
  const uebernehmePlan = (plan: PostingPlan) => {
    queryClient.setQueryData(planKey, plan);
  };

  // Ohne Kanal zeigt und kappt das Backend die globale Sammelverbindung. Beides
  // waere hier falsch: die Karte gehoert zum gewaehlten Kanal.
  const platformStatusQuery = useQuery({
    queryKey: ['social-media', 'platform-status', streamer],
    queryFn: () => fetchPlatformStatus(streamer),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: (platform: string) => disconnectPlatform(platform, streamer),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'platform-status', streamer] });
    },
  });

  /** Nachschub aus Twitch holen, der einzige Ausweg aus der Vorratswarnung. */
  const clipsHolenMutation = useMutation({
    mutationFn: () => fetchTwitchClips(streamer),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  /** Veto: einen bereits eingeplanten Post vor dem Termin stoppen. */
  const abbrechenMutation = useMutation({
    mutationFn: (clipDbId: number) => cancelScheduledPost(clipDbId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
  });

  const vodArchiveMutation = useMutation({
    mutationFn: (payload: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) =>
      saveVodArchiveSettings(streamer, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ['social-media', 'vod-archive-settings', streamer],
      });
    },
  });

  const approvalMutation = useMutation({
    mutationFn: ({
      clipDbId,
      decision,
      platforms,
    }: {
      clipDbId: number;
      decision: 'approve' | 'skip' | 'edit';
      platforms: SocialPlatform[];
    }) => decideClipApproval({ clipDbId, decision, platforms }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'posting-plan', streamer] });
    },
    // Kein window.alert mit roher Backend-Meldung. Der Fehler geht ueber
    // `clipFehler` als Zeile in den Fuss der betroffenen Clip-Karte, genau wie
    // bei Override, Abbrechen und Verwerfen.
  });

  const stats = useMemo(() => queueStats(clipsQuery.data?.items ?? []), [clipsQuery.data]);
  const filteredClips = useMemo(
    () => filterQueue(clipsQuery.data?.items ?? [], statusFilter, search, platformFilter),
    [clipsQuery.data, statusFilter, search, platformFilter],
  );
  const pageCount = Math.max(1, Math.ceil(filteredClips.length / 24));
  const currentPage = Math.min(page, pageCount);
  const visibleClips = filteredClips.slice((currentPage - 1) * 24, currentPage * 24);
  const editorClip = editingClip?.clip ?? null;
  const actionsBusy =
    approvalMutation.isPending || discardMutation.isPending || abbrechenMutation.isPending;
  const changeView = (next: SocialMediaView) => {
    if (next === activeView) return;
    if (
      document.querySelector('[data-unsaved="true"]') &&
      !window.confirm(t('Ungespeicherte Änderungen verwerfen?'))
    )
      return;
    setActiveView(next);
  };

  const defaultApprovalPlatforms = useMemo(
    () =>
      (postingPlanQuery.data?.platforms ?? [])
        .filter((entry) => entry.auto_post && entry.posts_per_week > 0)
        .map((entry) => entry.platform),
    [postingPlanQuery.data],
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
      <SocialHero
        streamer={streamer}
        activeView={activeView}
        onUpload={() => setShowUpload(true)}
        onOpenAnalytics={() => setShowAnalytics(true)}
      />

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
              onClick={() => changeView(id)}
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
                changeView(SOCIAL_MEDIA_TABS[nextIndex].id);
                document.getElementById(`studio-tab-${SOCIAL_MEDIA_TABS[nextIndex].id}`)?.focus();
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
        className="min-w-0"
      >
        {activeView === 'plan' ? (
          <PostingPlanDraft
            key={streamer}
            streamer={streamer}
            plan={postingPlanQuery.data ?? null}
            loading={postingPlanQuery.isLoading}
            loadError={postingPlanQuery.error}
            onSaved={uebernehmePlan}
          >
            {({ draft, patch, busy }) => (
              <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_260px]">
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
                    isZeitzoneSaving={busy}
                    zeitzoneError={null}
                    onTimezoneChange={(timezone) => patch((plan) => ({ ...plan, timezone }))}
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
        ) : activeView === 'layout' ? (
          <div className="space-y-5">
            <p className="text-sm text-text-secondary">
              {t(
                'Lege den Standard-Ausschnitt für neue Clips fest. Einzelne Clips kannst du weiterhin separat anpassen.',
              )}
            </p>
            {layoutQuery.isError ? <LadeFehlerHinweis fehler={layoutQuery.error} /> : null}
            <div className="grid gap-4 md:grid-cols-3">
              {(
                [
                  { mode: 'blur_pad', title: 'Gameplay mit Hintergrund', cam: false },
                  { mode: 'pip', title: 'Facecam & Gameplay', cam: true },
                  { mode: 'stacked', title: 'Split Screen', cam: true },
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
                    });
                  }}
                >
                  <div aria-hidden="true" className="studio-template-preview">
                    <span>9:16</span>
                    {preset.cam && <i className={`studio-template-cam ${preset.mode}`} />}
                  </div>
                  <h3 className="text-sm font-semibold">{t(preset.title)}</h3>
                  <p className="mt-2 text-sm text-text-secondary">{t('Layout anpassen')}</p>
                </button>
              ))}
            </div>
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
        ) : activeView === 'konten' ? (
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
            <PlatformConnectionsCard
              streamer={streamer}
              platforms={platformStatusQuery.data?.platforms ?? []}
              isLoading={platformStatusQuery.isLoading}
              ladeFehler={platformStatusQuery.error}
              onDisconnect={(platform) => disconnectMutation.mutate(platform)}
              isDisconnecting={disconnectMutation.isPending}
              error={disconnectMutation.error}
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
            <LanguageCard />
          </div>
        ) : (
          <>
            <div className="studio-metrics">
              <SocialMetric
                label={t('Wartet auf Freigabe')}
                value={clipsQuery.isError ? '—' : !clipsQuery.data ? '…' : stats.awaitingApproval}
                detail={t('{count} Clips im Pool', { count: stats.total })}
                tone="gold"
              />
              <SocialMetric
                label={t('Puffer')}
                value={
                  !postingPlanQuery.data
                    ? '…'
                    : postingPlanQuery.data.pool.reicht_fuer_tage == null
                      ? t('Manuell')
                      : t('{days} Tage', { days: postingPlanQuery.data.pool.reicht_fuer_tage })
                }
                detail={t('{count} Posts pro Woche', {
                  count: postingPlanQuery.data?.pool.posts_pro_woche ?? 0,
                })}
                tone={postingPlanQuery.data?.pool.warnung ? 'warning' : 'success'}
              />
              <SocialMetric
                label={t('Geplante Posts')}
                value={clipsQuery.isError ? '—' : !clipsQuery.data ? '…' : stats.scheduled}
                detail={t('Ein Post je Zielplattform')}
                tone="neutral"
              />
              <SocialMetric
                label={t('Fehler')}
                value={clipsQuery.isError ? '—' : !clipsQuery.data ? '…' : stats.failed}
                detail={stats.failed === 0 ? t('Keine offenen Fehler') : t('Bitte prüfen')}
                tone={stats.failed === 0 ? 'neutral' : 'danger'}
              />
            </div>

            <VorratsHinweis
              pool={postingPlanQuery.data?.pool ?? null}
              onClipsHolen={() => clipsHolenMutation.mutate()}
              isHolend={clipsHolenMutation.isPending}
            />

            <div className="space-y-3">
              <div className="studio-toolbar mt-6">
                <StatusFilter
                  value={statusFilter}
                  onChange={(next) => {
                    setStatusFilter(next);
                    setPage(1);
                  }}
                />
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
                  <button
                    type="button"
                    className="studio-button"
                    disabled={clipsHolenMutation.isPending}
                    onClick={() => clipsHolenMutation.mutate()}
                  >
                    <DownloadCloud className="h-4 w-4" />
                    {t('Clips holen')}
                  </button>
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
                        timezone={postingPlanQuery.data?.timezone ?? 'Europe/Berlin'}
                        defaultPlatforms={defaultApprovalPlatforms}
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
          eyebrow={t('Template & Layout')}
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

      {showAnalytics && (
        <WorkspaceDialog
          eyebrow={t('Performance')}
          title={t('Auswertung')}
          onClose={() => setShowAnalytics(false)}
        >
          <AnalyticsTab streamer={streamer} isAdmin={isAdmin} />
        </WorkspaceDialog>
      )}
    </div>
  );
}

function SocialHero({
  streamer,
  activeView,
  onUpload,
  onOpenAnalytics,
}: {
  streamer: string;
  activeView: SocialMediaView;
  onUpload: () => void;
  onOpenAnalytics: () => void;
}) {
  const t = useT();
  const titles = {
    pool: 'Deine Clip-Pipeline',
    plan: 'Auto-Pilot & Zeitplan',
    layout: 'Templates & Layouts',
    konten: 'Konten & Einstellungen',
  };
  return (
    <header className="flex flex-wrap items-center justify-between gap-4 py-2">
      <div className="min-w-0">
        <h1 className="studio-heading">{t(titles[activeView])}</h1>
        <p className="mt-2 text-sm text-text-secondary">
          {streamer} · {t('Aus guten Momenten werden deine nächsten Posts.')}
        </p>
      </div>
      <div className="flex gap-2">
        <button type="button" onClick={onOpenAnalytics} className="studio-button">
          <BarChart3 className="h-4 w-4" />
          {t('Auswertung')}
        </button>
        <button type="button" onClick={onUpload} className="studio-primary">
          <Plus className="h-4 w-4" />
          {t('Clip hinzufügen')}
        </button>
      </div>
    </header>
  );
}

function SocialMetric({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string | number;
  detail: string;
  tone: 'gold' | 'success' | 'warning' | 'danger' | 'neutral';
}) {
  const toneClass = {
    gold: 'text-ui-accent',
    success: 'text-ui-success',
    warning: 'text-ui-warning',
    danger: 'text-ui-danger',
    neutral: 'text-ui-text',
  }[tone];

  return (
    <div className="studio-metric">
      <p className="text-xs font-medium text-ui-faint">{label}</p>
      <p className={`studio-metric-value ${toneClass}`}>{value}</p>
      <p className="mt-1 text-xs text-ui-faint">{detail}</p>
    </div>
  );
}

function ClipEditorDialog({
  error,
  clip,
  mode,
  isSaving,
  onClose,
  onSaveLayout,
  onResetLayout,
}: {
  error: unknown;
  clip: SocialClipMitPosting;
  mode: EditMode;
  isSaving: boolean;
  onClose: () => void;
  onSaveLayout: (layout: LayoutPayload) => void;
  onResetLayout: () => void;
}) {
  const t = useT();
  return (
    <WorkspaceDialog
      eyebrow={mode === 'layout' ? t('Clip-Layout') : t('Metadaten')}
      title={clip.title}
      busy={isSaving}
      onClose={onClose}
    >
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
        <EnrichmentPanel clipDbId={clip.clip_db_id} onClose={onClose} />
      )}
    </WorkspaceDialog>
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
  onSubtitlesChange,
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

      <div className="rounded-xl border border-border bg-white/[0.02] px-3 py-2.5">
        <label className="flex items-center justify-between gap-3">
          <span>
            <span className="block text-sm font-medium text-ui-text">
              {t('Untertitel einbrennen')}
            </span>
            <span className="mt-0.5 block text-xs leading-5 text-ui-faint">
              {t('Brennt gesprochene Wörter als Untertitel ins Hochformat-Video.')}
            </span>
          </span>
          <input
            type="checkbox"
            disabled={gesperrt}
            checked={plan?.subtitles_enabled ?? true}
            onChange={(event) => onSubtitlesChange(event.target.checked)}
            className="h-4 w-4 accent-primary disabled:opacity-60"
          />
        </label>
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

/** Zeitzonenliste des Browsers, der eigene Standard steht vorn. */
function zeitzonenListe(aktuelle: string): string[] {
  let alle: string[];
  try {
    alle = Intl.supportedValuesOf('timeZone');
  } catch {
    // Aeltere Browser kennen supportedValuesOf nicht; dann bleibt die Liste
    // auf dem eigenen Standard plus der gespeicherten Zone.
    alle = [];
  }
  const vorn = ['Europe/Berlin', aktuelle].filter(
    (zone, index, liste) => zone && liste.indexOf(zone) === index,
  );
  return [...vorn, ...alle.filter((zone) => !vorn.includes(zone))];
}

function PostingScheduleCard({
  plan,
  isLoading,
  ladeFehler,
  isSaving,
  error,
  onChange,
  isZeitzoneSaving,
  zeitzoneError,
  onTimezoneChange,
}: {
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
  isZeitzoneSaving: boolean;
  zeitzoneError: unknown;
  onTimezoneChange: (timezone: string) => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const locale = language === 'en' ? 'en-GB' : 'de-DE';
  const zeitzone = plan?.timezone ?? 'Europe/Berlin';
  const zonen = useMemo(() => zeitzonenListe(zeitzone), [zeitzone]);
  // Ohne Plan zeigt die Karte Europe/Berlin und eine leere Plattformliste.
  // Beides ist geraten, deshalb bleibt hier bis zum naechsten erfolgreichen
  // Abruf alles gesperrt.
  const gesperrt = istGesperrt({ isLoading, isSaving, ladeFehler });
  const zeitzoneGesperrt = istGesperrt({
    isLoading,
    isSaving: isZeitzoneSaving,
    ladeFehler,
  });

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
        {(isSaving || isZeitzoneSaving) && (
          <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />
        )}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <label className="block">
        <span className="text-xs font-medium text-ui-faint">{t('Zeitzone des Kanals')}</span>
        <select
          value={zeitzone}
          disabled={zeitzoneGesperrt}
          onChange={(event) => onTimezoneChange(event.target.value)}
          className="mt-1 w-full rounded-lg border border-border bg-ui-elevated px-3 py-2 text-sm text-ui-text outline-none transition-colors focus:border-ui-accent-strong/35 disabled:opacity-60"
        >
          {zonen.map((zone) => (
            <option key={zone} value={zone}>
              {zone}
            </option>
          ))}
        </select>
      </label>
      <p className="text-sm text-ui-muted">{t('Zeiten gelten in {tz}.', { tz: zeitzone })}</p>
      {zeitzoneError ? (
        <div className="text-xs text-danger">{fehlerText(zeitzoneError, t)}</div>
      ) : null}

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
            className="flex items-center justify-between gap-3 rounded-xl border border-border bg-white/[0.02] px-3 py-2.5"
          >
            <span>
              <span className="block text-sm font-medium text-ui-text">
                {t(kategorieLabel(kategorie.category_key, kategorie.display_name))}
              </span>
              <span className="mt-0.5 block text-xs leading-5 text-ui-faint">
                {kategorie.enrichment_enabled
                  ? t('Mit Titel- und Hashtag-Vorschlägen.')
                  : t('Ohne Vorschläge, Clip geht so raus.')}
              </span>
            </span>
            <input
              type="checkbox"
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
      <AlertCircle className={`w-4 h-4 shrink-0 ${knapp ? 'text-warning' : 'text-primary'}`} />
      <div>
        <div className={`text-sm font-semibold ${knapp ? 'text-warning' : 'text-white'}`}>
          {t('Vorrat reicht noch für {posts} Posts.', { posts: pool.reicht_fuer_posts })}
        </div>
        <div className="text-xs text-text-secondary mt-0.5">
          {pool.reicht_fuer_tage === null
            ? t('{clips} Clips im Pool.', { clips: pool.verfuegbare_clips })
            : t(
                '{clips} Clips im Pool, das sind rund {tage} Tage bei {proWoche} Posts pro Woche.',
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
  streamer,
  platforms,
  isLoading,
  ladeFehler,
  onDisconnect,
  isDisconnecting,
  error,
}: {
  streamer: string;
  platforms: PlatformStatus[];
  isLoading: boolean;
  /** Fehler des Status-Abrufs: dann ist unbekannt, was verbunden ist. */
  ladeFehler: unknown;
  onDisconnect: (platform: string) => void;
  isDisconnecting: boolean;
  error: unknown;
}) {
  const { t, locale } = useLanguage();
  const byName = new Map(platforms.map((p) => [p.platform, p]));
  // Ohne Statusabruf ist jede Zeile geraten. Ein verbundener Kanal saehe dann
  // wie "nicht verbunden" aus und der Streamer startet einen ueberfluessigen
  // OAuth-Flow, deshalb verschwinden hier alle Knoepfe.
  const standUnbekannt = istStandUnbekannt(ladeFehler);
  const gesperrt = istGesperrt({ isLoading, isSaving: isDisconnecting, ladeFehler });
  const rueckmeldung = leseOauthRueckmeldung();

  return (
    <div className="space-y-4 rounded-2xl border border-border bg-ui-panel p-4">
      <div className="flex items-center gap-2">
        <ExternalLink className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">
          {t('Verbindungen')} · {streamer}
        </h3>
        {isLoading && <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />}
      </div>

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

      <LadeFehlerHinweis fehler={ladeFehler} />

      <div className="space-y-2">
        {PLATTFORMEN.map((platform) => {
          const status = byName.get(platform);
          const connected = status?.connected ?? false;
          // Ein abgelaufener Zugang, dessen Erneuerung dauerhaft scheitert,
          // sieht sonst aus wie eine gesunde Verbindung, waehrend jeder Upload
          // ins Leere laeuft.
          const abgelaufen = connected && (status?.expired ?? false);
          const sammelverbindung =
            connected && !abgelaufen && (status?.uses_global_fallback ?? false);
          const ablauf = status?.expires_at
            ? new Date(status.expires_at).toLocaleDateString(locale, {
                day: '2-digit',
                month: '2-digit',
                year: 'numeric',
              })
            : null;

          let zeile: string;
          let tonKlasse = 'text-text-secondary';
          if (standUnbekannt) {
            zeile = t('Zustand unbekannt');
            tonKlasse = 'text-danger';
          } else if (!connected) {
            zeile = t('nicht verbunden');
          } else if (abgelaufen) {
            zeile = t('Zugang abgelaufen, bitte neu verbinden');
            tonKlasse = 'text-warning';
          } else if (sammelverbindung) {
            zeile = t('nutzt die Sammelverbindung');
            tonKlasse = 'text-warning';
          } else {
            zeile = status?.username ?? t('verbunden');
          }

          return (
            <div
              key={platform}
              className="flex items-center justify-between gap-3 rounded-xl border border-border bg-white/[0.02] px-3 py-2.5"
            >
              <div className="min-w-0">
                <div className="text-sm font-medium text-ui-text">
                  {PLATFORM_LABELS[platform] ?? platform}
                </div>
                <div className={`text-xs truncate ${tonKlasse}`}>{zeile}</div>
                {!standUnbekannt && connected && !abgelaufen && ablauf && (
                  <div className="text-[11px] text-text-secondary">
                    {t('Zugang läuft am {datum} ab.', { datum: ablauf })}
                  </div>
                )}
              </div>
              {standUnbekannt ? null : connected && !abgelaufen ? (
                <button
                  type="button"
                  disabled={gesperrt}
                  onClick={() => {
                    // Der Kanalname gehoert in die Frage: es gibt eine
                    // Sammelverbindung, und niemand soll aus Versehen alle
                    // Kanaele kappen.
                    const frage = sammelverbindung
                      ? t(
                          '{platform} für {streamer} trennen? Der Kanal nutzt die Sammelverbindung.',
                          {
                            platform: PLATFORM_LABELS[platform] ?? platform,
                            streamer,
                          },
                        )
                      : t('{platform} für {streamer} trennen?', {
                          platform: PLATFORM_LABELS[platform] ?? platform,
                          streamer,
                        });
                    if (window.confirm(frage)) onDisconnect(platform);
                  }}
                  className="rounded-lg border border-border bg-white/[0.03] px-3 py-1.5 text-sm font-medium text-ui-muted transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-40"
                >
                  {t('Trennen')}
                </button>
              ) : (
                <a
                  href={oauthStartUrl(platform, streamer)}
                  className="shrink-0 rounded-lg border border-ui-accent-strong/25 bg-ui-accent-strong/12 px-3 py-1.5 text-sm font-medium text-ui-accent-ink transition-colors hover:bg-ui-accent-strong/18"
                >
                  {abgelaufen ? t('Neu verbinden') : t('Verbinden')}
                </a>
              )}
            </div>
          );
        })}
      </div>

      {error ? <div className="text-xs text-danger">{fehlerText(error, t)}</div> : null}
    </div>
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
    <div className="space-y-4 rounded-2xl border border-border bg-ui-panel p-4">
      <div className="flex items-center gap-2">
        <Archive className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">
          {t('VOD-Archiv')} · {settings?.streamer_login ?? streamer}
        </h3>
        {isSaving && <Loader2 className="ml-auto h-4 w-4 animate-spin text-ui-accent" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <label className="flex items-center justify-between gap-3 rounded-xl border border-border bg-white/[0.02] px-3 py-2.5">
        <span className="text-sm font-medium text-ui-text">{t('Automatisch sichern')}</span>
        <input
          type="checkbox"
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
    </div>
  );
}

/**
 * Sprache der Oberflaeche. Steht bewusst neben den anderen Schaltern in den
 * Einstellungen und nicht im Kopf: es ist eine Einstellung, die man einmal
 * setzt. Die Wahl liegt im Browser (localStorage) und gilt fuer alle Routen
 * dieses Dashboards, ein Datenbankfeld braucht es dafuer nicht.
 */
function LanguageCard() {
  const { language, setLanguage, t } = useLanguage();

  return (
    <div className="space-y-4 rounded-2xl border border-border bg-ui-panel p-4">
      <div className="flex items-center gap-2">
        <Languages className="h-4 w-4 text-ui-accent" />
        <h3 className="text-sm font-semibold text-white">{t('Sprache')}</h3>
      </div>
      <p className="text-sm leading-6 text-ui-muted">
        {t(
          'Gilt für dieses Dashboard in diesem Browser. Nicht übersetzte Stellen bleiben auf Deutsch.',
        )}
      </p>
      <div className="grid grid-cols-2 gap-2">
        {LANGUAGES.map((option: Language) => (
          <button
            key={option}
            type="button"
            lang={option}
            aria-pressed={language === option}
            onClick={() => setLanguage(option)}
            className={`rounded-lg border px-3 py-2 text-sm font-medium transition-colors ${
              language === option
                ? 'border-ui-accent-strong/30 bg-ui-accent-strong/12 text-ui-accent-ink'
                : 'border-border bg-white/[0.02] text-ui-muted hover:bg-white/[0.06] hover:text-white'
            }`}
          >
            {LANGUAGE_LABELS[option]}
          </button>
        ))}
      </div>
    </div>
  );
}

interface ClipCardProps {
  clip: SocialClipMitPosting;
  /** Zeitzone des Kanals, damit geplante Termine nicht in UTC dastehen. */
  timezone: string;
  /** Vorauswahl aus dem Auto-Pilot, falls am Clip noch keine Zielplattformen stehen. */
  defaultPlatforms: SocialPlatform[];
  onOpenEditor: (mode: EditMode) => void;
  onDiscard: () => void;
  onApprovalDecision: (decision: 'approve' | 'skip' | 'edit', platforms: SocialPlatform[]) => void;
  approvalPending: boolean;
  onCancelScheduled: () => void;
  cancelPending: boolean;
  cancelResult: { cancelled: number; already_running: number } | null;
  nichtEingeplant: SocialPlatform[];
  /** Fehler der letzten Aktion an genau diesem Clip. */
  fehler: unknown;
}

function ClipCard({
  clip,
  timezone,
  defaultPlatforms,
  onOpenEditor,
  onDiscard,
  onApprovalDecision,
  approvalPending,
  onCancelScheduled,
  cancelPending,
  cancelResult,
  nichtEingeplant,
  fehler,
}: ClipCardProps) {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const status = STATUS_LABELS[clip.status] ?? STATUS_LABELS.pending;
  const canDecide = queueStage(clip) === 'review';
  const termine = PLATTFORMEN.flatMap((platform) =>
    clip.scheduled_at?.[platform] && !clip.platform_status[platform]
      ? [{ platform, zeit: clip.scheduled_at[platform] as string }]
      : [],
  );
  const stoppbar = clip.status === 'approved' && termine.length > 0;
  const terminal =
    clip.status === 'discarded' || clip.status === 'skipped' || Boolean(clip.discarded_at);
  const uploadFehler = PLATTFORMEN.flatMap((platform) =>
    clip.upload_errors?.[platform] ? [{ platform, text: clip.upload_errors[platform] }] : [],
  );
  const initialPlatforms = clip.approval?.approved_platforms?.length
    ? clip.approval.approved_platforms
    : defaultPlatforms;
  const [selectedPlatforms, setSelectedPlatforms] = useState<SocialPlatform[]>(initialPlatforms);
  const touchedPlatforms = useRef(false);
  const platformKey = JSON.stringify(initialPlatforms);
  useEffect(() => {
    if (!touchedPlatforms.current) setSelectedPlatforms(JSON.parse(platformKey));
  }, [platformKey]);
  const [vorschauFehlt, setVorschauFehlt] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node)) menu.current?.removeAttribute('open');
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        menu.current?.removeAttribute('open');
        menu.current?.querySelector('summary')?.focus();
      }
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', escape);
    };
  }, [menuOpen]);
  const [previewActive, setPreviewActive] = useState(false);
  const previewKey = ['social-media', 'preview', clip.clip_db_id];
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: () => getPreviewStatus(clip.clip_db_id),
    enabled: previewActive,
    refetchOnWindowFocus: false,
    retry: false,
    refetchInterval: (query) =>
      ['pending', 'rendering'].includes(query.state.data?.status ?? '') ? 3000 : false,
  });
  const render = useMutation({
    mutationFn: () => requestPreview(clip.clip_db_id),
    onSuccess: (response) => {
      queryClient.setQueryData(previewKey, {
        clip_db_id: clip.clip_db_id,
        status: response.status ?? 'pending',
        ready: response.status === 'ready',
      });
      setPreviewActive(true);
      void queryClient.invalidateQueries({ queryKey: previewKey });
    },
  });
  const rendering =
    render.isPending || preview.data?.status === 'pending' || preview.data?.status === 'rendering';
  const ready = preview.data?.status === 'ready';
  const fehlerZeile = fehlerText(fehler, t);
  const tone = {
    orange: 'text-primary',
    messing: 'text-accent',
    success: 'text-success',
    warning: 'text-warning',
    danger: 'text-danger',
    muted: 'text-text-secondary',
  }[status.tone];
  const runMenu = (action: () => void) => {
    const trigger = menu.current?.querySelector('summary');
    menu.current?.removeAttribute('open');
    trigger?.focus();
    action();
  };
  return (
    <article className="studio-clip" aria-label={clip.title}>
      <div className="studio-clip-row">
        <div className="studio-clip-media">
          {ready ? (
            <video
              controls
              preload="metadata"
              src={previewFileUrl(clip.clip_db_id)}
              aria-label={clip.title}
            />
          ) : clip.thumbnail_url && vorschauFehlt !== clip.thumbnail_url ? (
            <img
              src={clip.thumbnail_url}
              alt=""
              loading="lazy"
              decoding="async"
              onError={() => setVorschauFehlt(clip.thumbnail_url)}
            />
          ) : (
            <div className="grid h-full min-h-28 place-items-center text-text-secondary">
              <Film className="h-8 w-8" />
            </div>
          )}
          {!ready && (
            <span className="absolute bottom-2 right-2 rounded bg-black/80 px-1.5 py-0.5 text-xs text-white">
              {formatClipDauer(clip.duration_seconds)}
            </span>
          )}
          {rendering && (
            <div className="absolute inset-0 grid place-items-center bg-black/75 p-3 text-center text-sm text-white">
              <span>
                <Loader2 className="mx-auto mb-2 h-5 w-5 animate-spin text-primary" />
                {t('Vorschau wird gerendert…')}
              </span>
            </div>
          )}
        </div>
        <div className="min-w-0 space-y-2.5">
          <span className={`studio-status ${tone}`}>
            {stoppbar ? t('Geplant') : t(status.label)}
          </span>
          <h3 className="break-words text-sm font-semibold leading-6">
            {clip.title || t('Clip ohne Titel')}
          </h3>
          <p className="text-xs text-text-secondary">
            {clip.source_kind === 'manual_upload' ? t('Upload') : 'Twitch'} / {clip.streamer_login}{' '}
            · {t('{views} Views', { views: clip.view_count.toLocaleString(locale) })}
          </p>
          {canDecide ? (
            <fieldset className="studio-platforms" disabled={approvalPending}>
              <legend className="sr-only">{t('Zielplattformen')}</legend>
              {PLATTFORMEN.map((platform) => (
                <label key={platform} className="studio-platform-choice">
                  <input
                    type="checkbox"
                    checked={selectedPlatforms.includes(platform)}
                    onChange={(event) => {
                      touchedPlatforms.current = true;
                      setSelectedPlatforms((current) =>
                        event.target.checked
                          ? [...current.filter((p) => p !== platform), platform]
                          : current.filter((p) => p !== platform),
                      );
                    }}
                  />
                  {PLATFORM_LABELS[platform]}
                </label>
              ))}
              {selectedPlatforms.length === 0 && (
                <span className="w-full text-warning">
                  {t('Wähle zuerst mindestens eine Zielplattform.')}
                </span>
              )}
            </fieldset>
          ) : (
            <p className="text-xs text-text-secondary">
              {PLATTFORMEN.filter(
                (p) => clip.platform_status[p] || clip.approval?.approved_platforms.includes(p),
              )
                .map((p) => PLATFORM_LABELS[p])
                .join(' · ')}
            </p>
          )}
          {termine.length > 0 && (stoppbar || clip.status === 'publishing') && (
            <div className="space-y-1 text-xs text-text-secondary">
              {termine.map(({ platform, zeit }) => (
                <p key={platform}>
                  {PLATFORM_LABELS[platform]}: {formatTerminInZone(zeit, locale, timezone)}
                </p>
              ))}
            </div>
          )}
          {clip.layout_override && <p className="text-xs text-accent">{t('Eigenes Layout')}</p>}
          {uploadFehler.length > 0 && queueStage(clip) === 'failed' && (
            <div className="break-words text-sm text-danger">
              {uploadFehler.map(({ platform, text }) => (
                <p key={platform}>
                  {PLATFORM_LABELS[platform]}: {text}
                </p>
              ))}
            </div>
          )}
          {(render.isError || preview.isError || preview.data?.status === 'error') && (
            <p role="alert" className="text-sm text-danger">
              {t('Vorschau konnte nicht gerendert werden.')}
            </p>
          )}
          {cancelResult && (
            <p role="status" className="text-sm text-text-secondary">
              {cancelResult.already_running
                ? t('Gestoppt, aber {count} Plattform war schon durch.', {
                    count: cancelResult.already_running,
                  })
                : t('{count} geplante Posts gestoppt.', { count: cancelResult.cancelled })}
            </p>
          )}
          {nichtEingeplant.length > 0 && (
            <p role="status" className="text-sm text-warning">
              {t('Auf {platforms} passiert nichts, dort steht die Kadenz auf null.', {
                platforms: nichtEingeplant.map((p) => PLATFORM_LABELS[p]).join(', '),
              })}
            </p>
          )}
          {fehlerZeile && (
            <p role="alert" className="text-sm text-danger">
              {fehlerZeile}
            </p>
          )}
        </div>
        <div className="studio-clip-actions">
          {canDecide ? (
            <>
              <button
                type="button"
                className="studio-primary"
                disabled={approvalPending || selectedPlatforms.length === 0}
                onClick={() => onApprovalDecision('approve', selectedPlatforms)}
              >
                <CheckCircle2 className="h-4 w-4" />
                {t('Clip freigeben')}
              </button>
              <button
                type="button"
                className="studio-icon-button"
                title={t('Ablehnen')}
                aria-label={t('Ablehnen')}
                disabled={approvalPending}
                onClick={() => onApprovalDecision('skip', selectedPlatforms)}
              >
                <XCircle className="h-4 w-4" />
              </button>
            </>
          ) : stoppbar ? (
            <button
              type="button"
              className="studio-button"
              disabled={approvalPending || cancelPending}
              onClick={onCancelScheduled}
            >
              <XCircle className="h-4 w-4" />
              {t('Post stoppen')}
            </button>
          ) : (
            !terminal &&
            clip.status !== 'publishing' && (
              <button
                type="button"
                className="studio-button"
                disabled={approvalPending}
                onClick={onDiscard}
              >
                <Archive className="h-4 w-4" />
                {t('Archivieren')}
              </button>
            )
          )}
          <details
            ref={menu}
            className="relative"
            onToggle={(event) => setMenuOpen(event.currentTarget.open)}
          >
            <summary className="studio-icon-button" aria-label={t('Weitere Aktionen')}>
              <MoreHorizontal className="h-4 w-4" />
            </summary>
            <div className="studio-menu">
              <button
                type="button"
                disabled={approvalPending || terminal || clip.status === 'publishing'}
                onClick={() => runMenu(() => onOpenEditor('layout'))}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
              >
                <Crop className="h-4 w-4" />
                {t('Layout anpassen')}
              </button>
              <button
                type="button"
                disabled={approvalPending || terminal || clip.status === 'publishing'}
                onClick={() => runMenu(() => onOpenEditor('enrichment'))}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
              >
                <Wand2 className="h-4 w-4" />
                {t('Transkript & Metadaten')}
              </button>
              <button
                type="button"
                disabled={rendering || terminal}
                onClick={() => runMenu(() => render.mutate())}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
              >
                <Clapperboard className="h-4 w-4" />
                {ready ? t('Vorschau neu rendern') : t('Vorschau rendern')}
              </button>
              {clip.clip_url && (
                <a
                  href={clip.clip_url}
                  target="_blank"
                  rel="noreferrer"
                  className="flex items-center gap-2 rounded-lg px-3 py-2 text-sm hover:bg-white/5"
                >
                  <ExternalLink className="h-4 w-4" />
                  {t('Original ansehen')}
                </a>
              )}
              <p className="mt-2 border-t border-border px-3 pt-2 text-xs text-text-secondary">
                {t('Aufbewahrung')}: {formatRetention(clip.retention_until, t)}
              </p>
            </div>
          </details>
        </div>
      </div>
    </article>
  );
}
