import { useEffect, useMemo, useRef, useState, type DragEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import {
  AlertCircle,
  BarChart3,
  CheckCircle2,
  Clock,
  Archive,
  Film,
  HardDrive,
  Loader2,
  ShieldAlert,
  Sparkles,
  Trash2,
  Upload,
  Layers3,
  Calendar,
  Gamepad2,
  ExternalLink,
  Pencil,
  PlayCircle,
  Wand2,
  SlidersHorizontal,
  Languages,
  DownloadCloud,
  CalendarClock,
  LockKeyhole,
  XCircle,
} from 'lucide-react';
import { KpiCard } from '@/components/cards/KpiCard';
import { useLanguage, useT } from '@/context/LanguageContext';
import { LANGUAGES, LANGUAGE_LABELS, type Language } from '@/i18n/dictionary';
import { AnalyticsTab } from '@/components/socialmedia/AnalyticsTab';
import { LayoutEditor, type VorschauClip } from '@/components/socialmedia/LayoutEditor';
import { EnrichmentPanel } from '@/components/socialmedia/EnrichmentPanel';
import { ClipPreparationWorkbench } from '@/components/socialmedia/ClipPreparationWorkbench';
import { LadeFehlerHinweis } from '@/components/socialmedia/LadeFehlerHinweis';
import {
  freigabeAktion,
  pruefeManuellenUpload,
  verwerfenWarnung,
  type ManuellerUploadFehler,
} from '@/components/socialmedia/clipVorbereitung';
import {
  clipFehler,
  istGesperrt,
  istStandUnbekannt,
  pruefeGanzeZahlImBereich,
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
  markClipPublished,
  fetchPlatformStatus,
  disconnectPlatform,
  oauthStartUrl,
  type PlatformStatus,
  type SocialClipMitPosting,
  fetchStreamerLayout,
  saveStreamerLayout,
  saveCategoryAutoPost,
  savePlatformSchedule,
  savePostingPlanSettings,
  saveVodArchiveSettings,
  setClipLayoutOverride,
  uploadClip,
} from '@/api/socialMedia';
import {
  APPROVAL_MODE_TEXTE,
  APPROVAL_STATE_LABELS,
  FELD_FEHLER,
  fehlerText,
  kategorieLabel,
  PLATFORM_LABELS,
  SOCIAL_MEDIA_TABS,
  statusFilterLabel,
  STATUS_FILTER_IDS,
  STATUS_LABELS,
  STATUS_META,
  TONE_BADGE,
  type SocialMediaView,
} from '@/components/socialmedia/labels';
import {
  type ApprovalMode,
  type ClipPoolForecast,
  type ClipPreparation,
  type DiscardClipResult,
  type PlatformScheduleEntry,
  type PostingPlan,
  DEFAULT_LAYOUT,
  type ClipStatus,
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
 * TikTok Direct Post verlangt eine eigene Auswahl und ausdrückliche Zustimmung
 * pro Clip. Bis diese Oberfläche existiert, darf die allgemeine Freigabe den
 * Providerweg auch nicht scheinbar aktivieren.
 */
const DIREKTFREIGABE_GESPERRT = new Set<SocialPlatform>(['tiktok']);

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
  veroeffentlicht: BarChart3,
  konten: SlidersHorizontal,
};

export function SocialMedia({ streamer, isAdmin = false }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const t = useT();
  const [statusFilter, setStatusFilter] = useState<ClipStatus | 'all'>('pending');
  const [editingClip, setEditingClip] = useState<{ id: number; mode: EditMode } | null>(null);
  const [activeView, setActiveView] = useState<SocialMediaView>('pool');
  const [aktionsmeldung, setAktionsmeldung] = useState<{ id: number; text: string } | null>(null);
  const aktionsmeldungRef = useRef<HTMLParagraphElement | null>(null);
  const aktionsmeldungIdRef = useRef(0);

  const meldeAktion = (text: string) => {
    setAktionsmeldung({ id: ++aktionsmeldungIdRef.current, text });
  };

  // Der Status entsteht erst mit dem naechsten React-Render. Danach bekommt er
  // bewusst den Fokus, damit Tastaturnutzende nach entfernten Karten nicht ins
  // Dokument zurueckfallen und die abgeschlossene Aktion direkt hoeren.
  useEffect(() => {
    if (aktionsmeldung) aktionsmeldungRef.current?.focus();
  }, [aktionsmeldung]);

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
    queryKey: ['social-media', 'clips', streamer, statusFilter],
    queryFn: () =>
      fetchClips({
        status: statusFilter,
        streamer: streamer || undefined,
        page: 1,
        page_size: 24,
      }),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  // Vorschauclips fuer den Layout-Editor. Bewusst eine eigene Abfrage ohne den
  // Statusfilter der Liste: steht der Filter auf "Veroeffentlicht" und ist dort
  // nichts drin, haette der Editor sonst kein Bild.
  const vorschauClipsQuery = useQuery({
    queryKey: ['social-media', 'vorschau-clips', streamer],
    queryFn: () => fetchClips({ status: 'all', streamer: streamer || undefined, page: 1, page_size: 12 }),
    // Nur im Clip-Pool: auf den anderen Reitern gibt es keinen Layout-Editor,
    // der die Bilder braucht.
    enabled: !!streamer && activeView === 'pool',
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
    mutationFn: (layout: LayoutPayload) =>
      saveStreamerLayout({ streamer_login: streamer, layout }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'streamer-layout', streamer] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clip-preparation'] });
    },
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) => uploadClip({ file, streamer_login: streamer }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
    },
  });

  const discardMutation = useMutation({
    mutationFn: (clipDbId: number) => discardClip(clipDbId),
    onSuccess: (_data, clipDbId) => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({
        queryKey: ['social-media', 'clip-preparation', clipDbId],
      });
      meldeAktion(t('Der Clip wurde verworfen.'));
    },
  });

  const overrideMutation = useMutation({
    mutationFn: ({ clipDbId, layout }: { clipDbId: number; layout: LayoutPayload | null }) =>
      setClipLayoutOverride(clipDbId, layout),
    onSuccess: (_data, { clipDbId }) => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      queryClient.invalidateQueries({
        queryKey: ['social-media', 'clip-preparation', clipDbId],
      });
    },
  });

  // Alle drei Zeitplan-Aufrufe liefern den kompletten Plan zurueck. Wir setzen
  // ihn direkt in den Cache, damit der neu berechnete Termin und die
  // Vorratsrechnung ohne zweite Abfrage sofort stehen.
  const planKey = ['social-media', 'posting-plan', streamer];
  const uebernehmePlan = (plan: PostingPlan) => {
    queryClient.setQueryData(planKey, plan);
  };

  const approvalModeMutation = useMutation({
    mutationFn: (payload: { approval_mode?: ApprovalMode; timezone?: string }) =>
      savePostingPlanSettings(streamer, payload),
    onSuccess: uebernehmePlan,
  });

  const platformScheduleMutation = useMutation({
    mutationFn: ({
      platform,
      payload,
    }: {
      platform: SocialPlatform;
      payload: Partial<
        Omit<
          PlatformScheduleEntry,
          'platform' | 'next_slot' | 'provider_release_blocked' | 'release_block_reason'
        >
      >;
    }) => savePlatformSchedule(streamer, platform, payload),
    onSuccess: uebernehmePlan,
  });

  const categoryMutation = useMutation({
    mutationFn: ({ categoryKey, autoPost }: { categoryKey: string; autoPost: boolean }) =>
      saveCategoryAutoPost(streamer, categoryKey, autoPost),
    onSuccess: uebernehmePlan,
  });

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
    onSuccess: (_data, platform) => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'platform-status', streamer] });
      meldeAktion(t('{platform} wurde getrennt.', {
        platform: PLATFORM_LABELS[platform] ?? platform,
      }));
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
      meldeAktion(t('Die geplante Veröffentlichung wurde gestoppt.'));
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
    onSuccess: (_data, variablen) => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      if (variablen.decision === 'approve') {
        meldeAktion(t('Der Clip wurde freigegeben.'));
      } else if (variablen.decision === 'skip') {
        meldeAktion(t('Der Clip wurde übersprungen.'));
      }
    },
    // Kein window.alert mit roher Backend-Meldung. Der Fehler geht ueber
    // `clipFehler` als Zeile in den Fuss der betroffenen Clip-Karte, genau wie
    // bei Override, Abbrechen und Verwerfen.
  });

  const stats = useMemo(() => {
    const list = clipsQuery.data?.items ?? [];
    const total = clipsQuery.data?.total ?? list.length;
    // Die Listen-API liefert derzeit keinen belastbaren Veröffentlichungszeitpunkt.
    // `created_at` ist der Erfassungszeitpunkt des Clips und darf deshalb nicht
    // als „heute veröffentlicht“ ausgegeben werden.
    const publishedAll = list.filter((c) => c.status === 'published_all').length;
    const nextRetention = list
      .map((c) => (c.retention_until ? new Date(c.retention_until).getTime() : null))
      .filter((v): v is number => v !== null)
      .sort((a, b) => a - b)[0];
    const manualUploads = list.filter((c) => c.source_kind === 'manual_upload').length;
    return {
      total,
      publishedAll,
      nextRetention: nextRetention ? new Date(nextRetention).toISOString() : null,
      manualUploads,
    };
  }, [clipsQuery.data]);

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
    <div className="space-y-6">
      <SocialHero
        streamer={streamer}
        isDefaultLayout={layoutQuery.data?.is_default ?? false}
        releaseEnabled={postingPlanQuery.data?.release_enabled ?? null}
      />

      {postingPlanQuery.data?.release_enabled === false && <TestbetriebBanner />}

      {postingPlanQuery.isError && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 rounded-2xl border border-danger/35 bg-danger/10 px-4 py-3 text-sm text-danger"
        >
          <AlertCircle className="h-5 w-5 shrink-0" />
          <span className="font-semibold">
            {t('Der Freigabemodus konnte nicht geladen werden. Freigaben bleiben gesperrt.')}
          </span>
          <button
            type="button"
            onClick={() => postingPlanQuery.refetch()}
            disabled={postingPlanQuery.isFetching}
            className="ml-auto inline-flex items-center gap-1.5 rounded-lg border border-danger/35 px-3 py-1.5 text-xs font-bold hover:bg-danger/10 disabled:opacity-50"
          >
            {postingPlanQuery.isFetching && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t('Erneut versuchen')}
          </button>
        </div>
      )}

      <div
        role="group"
        aria-label={t('Social-Media-Bereich wählen')}
        className="inline-flex flex-wrap rounded-2xl border border-border bg-bg/60 p-1.5 gap-1.5"
      >
        {SOCIAL_MEDIA_TABS.map(({ id, label }) => {
          const Icon = TAB_ICONS[id];
          const active = activeView === id;
          return (
            <button
              key={id}
              type="button"
              aria-pressed={active}
              onClick={() => setActiveView(id)}
              className={`inline-flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-semibold transition ${
                active
                  ? 'bg-gradient-to-r from-primary to-accent text-on-gold shadow-[0_6px_24px_-10px_rgba(197,160,89,0.55)]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              <Icon className="w-4 h-4" />
              {t(label)}
            </button>
          );
        })}
      </div>

      {aktionsmeldung && (
        <p
          key={aktionsmeldung.id}
          ref={aktionsmeldungRef}
          role="status"
          aria-live="polite"
          tabIndex={-1}
          className="rounded-xl border border-success/30 bg-success/10 px-4 py-3 text-sm font-semibold text-success focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-success"
        >
          {aktionsmeldung.text}
        </p>
      )}

      {activeView === 'veroeffentlicht' ? (
        <AnalyticsTab streamer={streamer} isAdmin={isAdmin} />
      ) : activeView === 'plan' ? (
        <div className="space-y-6">
          <VorratsHinweis
            pool={postingPlanQuery.data?.pool ?? null}
            onClipsHolen={() => clipsHolenMutation.mutate()}
            isHolend={clipsHolenMutation.isPending}
          />
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
            <ApprovalModeCard
              plan={postingPlanQuery.data ?? null}
              isLoading={postingPlanQuery.isLoading}
              ladeFehler={postingPlanQuery.error}
              isSaving={approvalModeMutation.isPending}
              error={approvalModeMutation.error}
              onChange={(mode) => approvalModeMutation.mutate({ approval_mode: mode })}
            />
            <CategoryCard
              plan={postingPlanQuery.data ?? null}
              isLoading={postingPlanQuery.isLoading}
              ladeFehler={postingPlanQuery.error}
              isSaving={categoryMutation.isPending}
              error={categoryMutation.error}
              onChange={(categoryKey, autoPost) =>
                categoryMutation.mutate({ categoryKey, autoPost })
              }
            />
            <div className="xl:col-span-2">
              <PostingScheduleCard
                plan={postingPlanQuery.data ?? null}
                isLoading={postingPlanQuery.isLoading}
                ladeFehler={postingPlanQuery.error}
                isSaving={platformScheduleMutation.isPending}
                error={platformScheduleMutation.error}
                onChange={(platform, payload) =>
                  platformScheduleMutation.mutate({ platform, payload })
                }
                isZeitzoneSaving={approvalModeMutation.isPending}
                zeitzoneError={approvalModeMutation.error}
                onTimezoneChange={(timezone) => approvalModeMutation.mutate({ timezone })}
              />
            </div>
          </div>
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
          <VorratsHinweis
            pool={postingPlanQuery.data?.pool ?? null}
            onClipsHolen={() => clipsHolenMutation.mutate()}
            isHolend={clipsHolenMutation.isPending}
          />
          {!clipsQuery.isError && (
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            <KpiCard
              title={t('Clips im Pool')}
              value={stats.total}
              icon={Film}
              color="purple"
              subValue={
                statusFilter === 'all'
                  ? t('alle Stati')
                  : t(STATUS_LABELS[statusFilter]?.label ?? '')
              }
            />
            <KpiCard
              title={t('Vollständig veröffentlicht')}
              value={stats.publishedAll}
              icon={CheckCircle2}
              color="green"
              subValue={t('im geladenen Clip-Pool')}
            />
            <KpiCard
              title={t('Manuelle Uploads')}
              value={stats.manualUploads}
              icon={HardDrive}
              color="yellow"
              subValue={t('MP4-Drops aus dem Editor')}
            />
            <KpiCard
              title={t('Früheste Frist der geladenen Clips')}
              value={formatRetention(stats.nextRetention, t)}
              icon={Clock}
              color="blue"
              subValue={t('Offene Clips bleiben erhalten')}
            />
          </div>
          )}

          <div className="grid items-start grid-cols-1 xl:grid-cols-[minmax(0,1fr)_360px] gap-6">
            <div className="space-y-4">
              {layoutQuery.isLoading ? (
                <div role="status" aria-live="polite" className="panel-card rounded-2xl p-12 flex items-center justify-center gap-2 text-sm text-text-secondary">
                  <Loader2 aria-hidden="true" className="w-6 h-6 text-orange animate-spin" />
                  {t('Layout wird geladen…')}
                </div>
              ) : layoutQuery.isError ? (
                <div role="alert" className="panel-card rounded-2xl border-danger/35 bg-danger/10 p-6 text-sm text-danger">
                  <p className="font-bold">{t('Das gespeicherte Layout konnte nicht geladen werden.')}</p>
                  <p className="mt-1">{fehlerText(layoutQuery.error, t)}</p>
                  <button
                    type="button"
                    onClick={() => layoutQuery.refetch()}
                    disabled={layoutQuery.isFetching}
                    className="mt-3 inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-danger/35 px-3 py-1.5 text-xs font-bold hover:bg-danger/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger disabled:opacity-50"
                  >
                    {layoutQuery.isFetching && <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />}
                    {t('Erneut versuchen')}
                  </button>
                </div>
              ) : (
                <LayoutEditor
                  initialLayout={layoutForEditor}
                  isSaving={saveLayoutMutation.isPending}
                  onSave={(layout) => saveLayoutMutation.mutate(layout)}
                  saveLabel={t('Default für {streamer} speichern', { streamer })}
                  vorschauClips={vorschauClips}
                  geltungHinweis={t('Der Ausschnitt gilt danach für alle Clips dieses Kanals.')}
                />
              )}
              {saveLayoutMutation.isError && (
                <div role="alert" className="px-3 text-xs text-danger">
                  {t('Speichern fehlgeschlagen: {message}', {
                    message: fehlerText(saveLayoutMutation.error, t) ?? '',
                  })}
                </div>
              )}
              {saveLayoutMutation.isSuccess && (
                <div role="status" aria-live="polite" className="px-3 text-xs text-success">
                  {t('Layout gespeichert.')}
                </div>
              )}
            </div>

            <div className="self-start">
              <UploadCard
                streamer={streamer}
                onUpload={(file) => uploadMutation.mutate(file)}
                isUploading={uploadMutation.isPending}
                uploadError={uploadMutation.error as Error | null}
                uploadSuccess={uploadMutation.isSuccess}
              />
            </div>
          </div>

          <div className="space-y-4">
            <div className="flex flex-wrap items-center gap-3">
              <h3 className="text-lg font-bold text-white inline-flex items-center gap-2">
                <Layers3 className="w-5 h-5 text-orange" /> {t('Pipeline')}
              </h3>
              <StatusFilter value={statusFilter} onChange={setStatusFilter} />
              <button
                type="button"
                onClick={() => clipsHolenMutation.mutate()}
                disabled={clipsHolenMutation.isPending || !streamer}
                className="inline-flex items-center gap-1.5 rounded-xl border border-accent/35 bg-accent/12 px-3 py-1.5 text-xs font-bold text-accent hover:bg-accent/20 disabled:opacity-50"
              >
                {clipsHolenMutation.isPending ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <DownloadCloud className="w-3.5 h-3.5" />
                )}
                {t('Clips jetzt holen')}
              </button>
              <div role="status" aria-live="polite" className="ml-auto text-xs text-text-secondary">
                {clipsQuery.isFetching
                  ? t('Aktualisiere…')
                  : t('{count} Treffer', { count: clipsQuery.data?.items.length ?? 0 })}
              </div>
            </div>

            {clipsHolenMutation.isError && (
              <div role="alert" className="text-xs text-danger">
                {fehlerText(clipsHolenMutation.error, t)}
              </div>
            )}
            {clipsHolenMutation.isSuccess && !clipsHolenMutation.isPending && (
              <div role="status" aria-live="polite" className="text-xs text-success">
                {t('{count} Clips von Twitch geholt.', {
                  count: clipsHolenMutation.data?.clips_found ?? 0,
                })}
              </div>
            )}

            {clipsQuery.isLoading ? (
              <div role="status" aria-live="polite" className="panel-card rounded-2xl p-12 flex items-center justify-center gap-2 text-sm text-text-secondary">
                <Loader2 aria-hidden="true" className="w-6 h-6 text-orange animate-spin" />
                {t('Clips werden geladen…')}
              </div>
            ) : clipsQuery.isError ? (
              <div role="alert" className="panel-card rounded-2xl border-danger/35 bg-danger/10 p-8 text-sm text-danger text-center">
                <p className="font-bold">{t('Die Clip-Liste konnte nicht geladen werden.')}</p>
                <p className="mt-1">{fehlerText(clipsQuery.error, t)}</p>
                <button
                  type="button"
                  onClick={() => clipsQuery.refetch()}
                  disabled={clipsQuery.isFetching}
                  className="mt-3 inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-danger/35 px-3 py-1.5 text-xs font-bold hover:bg-danger/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger disabled:opacity-50"
                >
                  {clipsQuery.isFetching && <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />}
                  {t('Erneut versuchen')}
                </button>
              </div>
            ) : (clipsQuery.data?.items ?? []).length === 0 ? (
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
              <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
                {(clipsQuery.data?.items ?? []).map((clip) => {
                  const editingMode =
                    editingClip && editingClip.id === clip.clip_db_id ? editingClip.mode : null;
                  return (
                    <ClipCard
                      key={clip.clip_db_id}
                      clip={clip}
                      timezone={postingPlanQuery.data?.timezone ?? 'Europe/Berlin'}
                      releaseEnabled={postingPlanQuery.data?.release_enabled ?? null}
                      canReconcile={isAdmin === true}
                      onActionComplete={meldeAktion}
                      editingMode={editingMode}
                      onOpenEditor={(mode) => setEditingClip({ id: clip.clip_db_id, mode })}
                      onCloseEditor={() => setEditingClip(null)}
                      onDiscard={() => {
                        if (window.confirm(t('Clip "{title}" verwerfen?', { title: clip.title }))) {
                          discardMutation.mutate(clip.clip_db_id);
                        }
                      }}
                      onSaveOverride={(layout) =>
                        overrideMutation.mutateAsync({ clipDbId: clip.clip_db_id, layout }).then(() => undefined)
                      }
                      onResetOverride={() =>
                        overrideMutation.mutateAsync({ clipDbId: clip.clip_db_id, layout: null }).then(() => undefined)
                      }
                      overridePending={
                        overrideMutation.isPending &&
                        overrideMutation.variables?.clipDbId === clip.clip_db_id
                      }
                      onApprovalDecision={(decision, platforms) => {
                        approvalMutation.mutate({
                          clipDbId: clip.clip_db_id,
                          decision,
                          platforms,
                        });
                        if (decision === 'edit') {
                          setEditingClip({ id: clip.clip_db_id, mode: 'enrichment' });
                        }
                      }}
                      approvalPending={
                        approvalMutation.isPending &&
                        approvalMutation.variables?.clipDbId === clip.clip_db_id
                      }
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
                      discardResult={
                        discardMutation.isSuccess &&
                        discardMutation.variables === clip.clip_db_id
                          ? discardMutation.data
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
          </div>
        </>
      )}
    </div>
  );
}

function SocialHero({
  streamer,
  isDefaultLayout,
  releaseEnabled,
}: {
  streamer: string;
  isDefaultLayout: boolean;
  releaseEnabled: boolean | null;
}) {
  const t = useT();
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      className="panel-card rounded-2xl p-6 md:p-8 relative overflow-hidden"
    >
      <div className="absolute -top-20 -right-20 h-72 w-72 rounded-full bg-orange/15 blur-3xl pointer-events-none" />
      <div className="absolute -bottom-24 -left-12 h-72 w-72 rounded-full bg-accent/12 blur-3xl pointer-events-none" />
      <div className="relative flex flex-col md:flex-row md:items-end md:justify-between gap-5">
        <div>
          <div className="inline-flex items-center gap-2 text-[11px] uppercase tracking-[0.18em] font-bold text-orange/90 px-2.5 py-1 rounded-full bg-orange/12 border border-orange/30">
            <Sparkles className="w-3.5 h-3.5" />{' '}
            {releaseEnabled === true
              ? t('Clips automatisch posten')
              : releaseEnabled === false
                ? t('Clips sicher vorbereiten')
                : t('Clip-Pipeline')}
          </div>
          <h1 className="display-font font-extrabold text-white mt-3 text-3xl md:text-4xl tracking-tight">
            {t('Social Media für')}{' '}
            <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
              {streamer}
            </span>
          </h1>
          <p className="text-text-secondary mt-2 max-w-2xl text-sm md:text-base">
            {t(
              'Twitch-Clips werden automatisch eingesammelt und als prüfbare 9:16-Vorschau für Shorts, TikTok und Reels aufbereitet. Streamer-Layouts gelten als Standard, einzelne Clips lassen sich anpassen.',
            )}
          </p>
        </div>
        <div className="flex flex-wrap gap-3 text-xs">
          <HeroBadge tone="orange" icon={Film}>
            {isDefaultLayout ? t('Layout: Repo-Default aktiv') : t('Layout: Streamer-Default')}
          </HeroBadge>
          <HeroBadge tone="accent" icon={Calendar}>
            {t('Vorschau · Freigabe · Zeitplan')}
          </HeroBadge>
        </div>
      </div>
    </motion.div>
  );
}

function TestbetriebBanner() {
  const t = useT();
  return (
    <div
      role="status"
      className="flex items-start gap-3 rounded-2xl border border-primary/45 bg-[linear-gradient(120deg,rgba(197,160,89,0.2),rgba(197,160,89,0.06))] px-4 py-3 shadow-[0_12px_35px_-24px_rgba(197,160,89,0.8)]"
    >
      <ShieldAlert className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
      <div>
        <div className="font-bold text-white">{t('Testbetrieb – Veröffentlichung pausiert')}</div>
        <p className="mt-0.5 text-sm leading-relaxed text-text-secondary">
          {t(
            'Clips können vollständig aufbereitet, geprüft und für später freigegeben werden. Es wird nichts an Plattformen gesendet.',
          )}
        </p>
      </div>
    </div>
  );
}

function HeroBadge({
  tone,
  icon: Icon,
  children,
}: {
  tone: 'orange' | 'accent';
  icon: React.ComponentType<{ className?: string }>;
  children: React.ReactNode;
}) {
  const cls = tone === 'orange' ? 'bg-orange/12 text-orange border-orange/30' : 'bg-accent/12 text-accent border-accent/35';
  return (
    <div className={`inline-flex items-center gap-2 px-3 py-2 rounded-xl border font-semibold ${cls}`}>
      <Icon className="w-3.5 h-3.5" />
      {children}
    </div>
  );
}

function StatusFilter({
  value,
  onChange,
}: {
  value: ClipStatus | 'all';
  onChange: (next: ClipStatus | 'all') => void;
}) {
  const t = useT();
  return (
    <div
      role="group"
      aria-label={t('Clipstatus filtern')}
      className="inline-flex flex-wrap rounded-xl border border-border bg-bg/60 p-1 gap-1"
    >
      {STATUS_FILTER_IDS.map((id) => {
        const active = id === value;
        return (
          <button
            key={id}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(id)}
            className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition ${
              active
                ? 'bg-gradient-to-r from-primary to-accent text-on-gold shadow-[0_4px_18px_-6px_rgba(197,160,89,0.55)]'
                : 'text-text-secondary hover:text-white'
            }`}
          >
            {t(statusFilterLabel(id))}
          </button>
        );
      })}
    </div>
  );
}

interface UploadCardProps {
  streamer: string;
  onUpload: (file: File) => void;
  isUploading: boolean;
  /** Kommt als Fehlercode aus dem API-Modul und wird hier erst uebersetzt. */
  uploadError: unknown;
  uploadSuccess: boolean;
}

function UploadCard({ streamer, onUpload, isUploading, uploadError, uploadSuccess }: UploadCardProps) {
  const t = useT();
  const [dragActive, setDragActive] = useState(false);
  const [lokalerFehler, setLokalerFehler] = useState<ManuellerUploadFehler>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const handleFiles = (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const file = files[0];
    const fehler = pruefeManuellenUpload(file);
    if (fehler === 'format') {
      setLokalerFehler(fehler);
      return;
    }
    if (fehler === 'zu_gross') {
      setLokalerFehler(fehler);
      return;
    }
    setLokalerFehler(null);
    onUpload(file);
  };

  const handleDrop = (e: DragEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setDragActive(false);
    handleFiles(e.dataTransfer?.files ?? null);
  };

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Upload className="w-4 h-4 text-accent" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('MP4 hochladen')}
        </h3>
      </div>

      <input
        ref={inputRef}
        type="file"
        accept="video/mp4,.mp4"
        hidden
        onChange={(event) => {
          handleFiles(event.currentTarget.files);
          event.currentTarget.value = '';
        }}
      />
      <button
        type="button"
        disabled={isUploading}
        aria-label={t('MP4 hier ablegen oder auswählen für {streamer}', { streamer })}
        aria-describedby="social-media-upload-hinweis"
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
        onClick={() => inputRef.current?.click()}
        className={`relative w-full cursor-pointer rounded-xl border-2 border-dashed p-6 text-center transition disabled:cursor-wait disabled:opacity-60 ${
          dragActive
            ? 'border-accent bg-accent/10'
            : 'border-border hover:border-accent/50 hover:bg-bg/40'
        }`}
      >
        <Film className="w-8 h-8 text-accent mx-auto mb-2" />
        <span className="block text-sm font-bold text-white">{t('MP4 hier ablegen')}</span>
        <span className="mt-1 block text-xs text-text-secondary">
          {t('oder klicken zum Auswählen · max 200 MB')}
        </span>
        <span
          id="social-media-upload-hinweis"
          className="mt-3 block text-[11px] leading-relaxed text-text-secondary"
        >
          {t(
            'Die Datei wird sicher gespeichert und anschließend mit dem Streamer-Default-Layout aufbereitet.',
          )}
        </span>
      </button>

      {isUploading && (
        <div role="status" aria-live="polite" className="flex items-center gap-2 text-xs text-accent">
          <Loader2 className="w-4 h-4 animate-spin" /> {t('Upload läuft…')}
        </div>
      )}
      {lokalerFehler && (
        <div role="alert" className="text-xs text-danger">
          {lokalerFehler === 'format'
            ? t('Bitte eine MP4-Datei wählen.')
            : t('Die MP4-Datei darf höchstens 200 MB groß sein.')}
        </div>
      )}
      {uploadError ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(uploadError, t)}
        </div>
      ) : null}
      {uploadSuccess && !isUploading && (
        <div role="status" className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" /> {t('Upload erfolgreich. Clip ist in der Pipeline.')}
        </div>
      )}

      <div className="border-t border-border pt-3 space-y-2 text-[11px] text-text-secondary">
        <div className="flex items-center gap-1.5">
          <Calendar className="w-3 h-3" /> {t('Fristprüfung ab 14 Tagen; offene Clips bleiben erhalten')}
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <ShieldAlert className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Freigabe')}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <fieldset className="space-y-2" disabled={gesperrt}>
        <legend className="sr-only">{t('Freigabemodus')}</legend>
        {modi.map((mode) => {
          const texte = APPROVAL_MODE_TEXTE[mode];
          if (!texte) return null;
          const active = aktiv === mode;
          return (
            <label
              key={mode}
              className={`flex w-full cursor-pointer items-start gap-3 rounded-xl border px-4 py-3 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-60 ${
                active
                  ? 'border-primary/60 bg-primary/10'
                  : 'border-border bg-bg/40 hover:border-border-hover'
              }`}
              style={{ transitionProperty: 'border-color, background-color' }}
            >
              <input
                type="radio"
                name="social-media-approval-mode"
                value={mode}
                checked={active}
                onChange={() => onChange(mode)}
                className="mt-0.5 h-4 w-4 shrink-0 accent-primary"
              />
              <div className="min-w-0">
                <div
                  className={`text-sm font-semibold ${active ? 'text-primary' : 'text-white'}`}
                >
                  {t(texte.label)}
                </div>
                <div className="mt-0.5 text-xs text-text-secondary">{t(texte.hinweis)}</div>
              </div>
            </label>
          );
        })}
      </fieldset>
      {error ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(error, t)}
        </div>
      ) : null}
    </div>
  );
}

/** Formatiert einen Termin kurz und lesbar, ohne Sekunden. */
function formatTermin(iso: string | null, locale: string): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleString(locale, {
    weekday: 'short',
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

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
    payload: Partial<
      Omit<
        PlatformScheduleEntry,
        'platform' | 'next_slot' | 'provider_release_blocked' | 'release_block_reason'
      >
    >,
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Calendar className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Zeitplan')}
        </h3>
        {(isSaving || isZeitzoneSaving) && (
          <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />
        )}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <label className="block">
        <span className="text-xs text-text-secondary">{t('Zeitzone des Kanals')}</span>
        <select
          value={zeitzone}
          disabled={zeitzoneGesperrt}
          onChange={(event) => onTimezoneChange(event.target.value)}
          className="mt-1 w-full rounded-lg border border-[color:var(--color-border-strong)] bg-background/80 px-3 py-2 text-sm text-white disabled:opacity-60"
        >
          {zonen.map((zone) => (
            <option key={zone} value={zone}>
              {zone}
            </option>
          ))}
        </select>
      </label>
      <p className="text-sm text-text-secondary">
        {t('Zeiten gelten in {tz}.', { tz: zeitzone })}
      </p>
      {zeitzoneError ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(zeitzoneError, t)}
        </div>
      ) : null}

      <div className="space-y-3">
        {(plan?.platforms ?? []).map((eintrag) => {
          const platformLabel = PLATFORM_LABELS[eintrag.platform] ?? eintrag.platform;
          const termin = formatTermin(eintrag.next_slot, locale);
          const werte = formular[eintrag.platform] ?? {
            postsProWoche: String(eintrag.posts_per_week),
            maxProTag: String(eintrag.max_posts_per_day),
            zeiten: eintrag.post_times.join(', '),
          };
          const wocheFehler = feldFehler[`${eintrag.platform}:woche`];
          const tagFehler = feldFehler[`${eintrag.platform}:tag`];
          const zeitenFehler = feldFehler[`${eintrag.platform}:zeiten`];
          const wocheFehlerId = `zeitplan-${eintrag.platform}-woche-fehler`;
          const tagFehlerId = `zeitplan-${eintrag.platform}-tag-fehler`;
          const zeitenFehlerId = `zeitplan-${eintrag.platform}-zeiten-fehler`;
          const providerGesperrt = eintrag.provider_release_blocked;
          const providerHinweisId = `zeitplan-${eintrag.platform}-provider-hinweis`;
          const autoPostAktiv = eintrag.auto_post && !providerGesperrt;
          // Die Kadenz bleibt sichtbar und aenderbar, auch wenn Auto-Posting
          // aus ist: sonst laesst sie sich nicht vorbereiten und wirkt beim
          // naechsten Einschalten wie aus dem Nichts.
          return (
            <div
              key={eintrag.platform}
              className="rounded-xl border border-border bg-bg/40 px-4 py-3 space-y-3"
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-semibold text-white">
                  {platformLabel}
                </span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={autoPostAktiv}
                  aria-describedby={providerGesperrt ? providerHinweisId : undefined}
                  aria-label={t('Automatisch auf {platform} posten', {
                    platform: platformLabel,
                  })}
                  disabled={gesperrt || providerGesperrt}
                  onClick={() => onChange(eintrag.platform, { auto_post: !eintrag.auto_post })}
                  className={`relative inline-flex h-7 w-12 shrink-0 rounded-full border disabled:opacity-60 ${
                    autoPostAktiv
                      ? 'border-primary/60 bg-primary/30'
                      : 'border-border bg-bg/60'
                  }`}
                  style={{ transitionProperty: 'border-color, background-color' }}
                >
                  <span
                    className={`absolute top-1 h-5 w-5 rounded-full bg-white ${
                      autoPostAktiv ? 'translate-x-6' : 'translate-x-1'
                    }`}
                    style={{ transitionProperty: 'transform' }}
                  />
                </button>
              </div>

              {providerGesperrt && (
                <p id={providerHinweisId} className="text-xs text-warning">
                  {fehlerText(
                    { code: eintrag.release_block_reason ?? 'platform_release_blocked' },
                    t,
                  )}
                </p>
              )}

              <div className="space-y-3">
                {!autoPostAktiv && (
                  <p className="text-xs text-text-secondary">
                    {t('Gilt, sobald Auto-Posting an ist.')}
                  </p>
                )}
                <div className="grid grid-cols-2 gap-3">
                  <label className="block">
                    <span className="text-xs text-text-secondary">{t('Posts pro Woche')}</span>
                    <input
                      type="number"
                      min={0}
                      max={70}
                      step={1}
                      value={werte.postsProWoche}
                      disabled={gesperrt}
                      aria-label={t('Posts pro Woche für {platform}', {
                        platform: platformLabel,
                      })}
                      aria-invalid={Boolean(wocheFehler)}
                      aria-describedby={wocheFehler ? wocheFehlerId : undefined}
                      onChange={(event) =>
                        setzeFeld(eintrag.platform, 'postsProWoche', event.target.value)
                      }
                      onBlur={() => {
                        const pruefung = pruefeGanzeZahlImBereich(
                          werte.postsProWoche,
                          0,
                          70,
                        );
                        const plan = zeitplanFeldVerlassen(
                          pruefung.gueltig
                            ? {
                                gueltig: true,
                                unveraendert: pruefung.wert === eintrag.posts_per_week,
                              }
                            : {
                                gueltig: false,
                                fehler:
                                  pruefung.grund === 'ausserhalb_bereich'
                                    ? FELD_FEHLER.postsProWocheBereich
                                    : FELD_FEHLER.ganzeZahl,
                              },
                        );
                        setzeFehler(`${eintrag.platform}:woche`, plan.fehler);
                        feldAbgeschlossen(eintrag.platform, 'postsProWoche');
                        if (plan.absenden && pruefung.gueltig) {
                          onChange(eintrag.platform, { posts_per_week: pruefung.wert });
                        }
                      }}
                      className="mt-1 w-full rounded-lg border border-[color:var(--color-border-strong)] bg-background/80 px-3 py-2 text-sm text-white"
                    />
                    {wocheFehler && (
                      <span id={wocheFehlerId} role="alert" className="mt-1 block text-xs text-danger">
                        {t(wocheFehler)}
                      </span>
                    )}
                  </label>
                  <label className="block">
                    <span className="text-xs text-text-secondary">{t('Höchstens pro Tag')}</span>
                    <input
                      type="number"
                      min={0}
                      max={10}
                      step={1}
                      value={werte.maxProTag}
                      disabled={gesperrt}
                      aria-label={t('Höchstens pro Tag für {platform}', {
                        platform: platformLabel,
                      })}
                      aria-invalid={Boolean(tagFehler)}
                      aria-describedby={tagFehler ? tagFehlerId : undefined}
                      onChange={(event) =>
                        setzeFeld(eintrag.platform, 'maxProTag', event.target.value)
                      }
                      onBlur={() => {
                        const pruefung = pruefeGanzeZahlImBereich(werte.maxProTag, 0, 10);
                        const plan = zeitplanFeldVerlassen(
                          pruefung.gueltig
                            ? {
                                gueltig: true,
                                unveraendert: pruefung.wert === eintrag.max_posts_per_day,
                              }
                            : {
                                gueltig: false,
                                fehler:
                                  pruefung.grund === 'ausserhalb_bereich'
                                    ? FELD_FEHLER.maxProTagBereich
                                    : FELD_FEHLER.ganzeZahl,
                              },
                        );
                        setzeFehler(`${eintrag.platform}:tag`, plan.fehler);
                        feldAbgeschlossen(eintrag.platform, 'maxProTag');
                        if (plan.absenden && pruefung.gueltig) {
                          onChange(eintrag.platform, { max_posts_per_day: pruefung.wert });
                        }
                      }}
                      className="mt-1 w-full rounded-lg border border-[color:var(--color-border-strong)] bg-background/80 px-3 py-2 text-sm text-white"
                    />
                    {tagFehler && (
                      <span id={tagFehlerId} role="alert" className="mt-1 block text-xs text-danger">
                        {t(tagFehler)}
                      </span>
                    )}
                  </label>
                </div>
                <label className="block">
                  <span className="text-xs text-text-secondary">
                    {t('Uhrzeiten, mit Komma getrennt')}
                  </span>
                  <input
                    type="text"
                    value={werte.zeiten}
                    aria-label={t('Uhrzeiten für {platform}', { platform: platformLabel })}
                    placeholder="18:00, 21:00"
                    disabled={gesperrt}
                    aria-invalid={Boolean(zeitenFehler)}
                    aria-describedby={zeitenFehler ? zeitenFehlerId : undefined}
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
                      zeitenFehler
                        ? 'border-danger'
                        : 'border-[color:var(--color-border-strong)]'
                    }`}
                  />
                  {zeitenFehler && (
                    <span id={zeitenFehlerId} role="alert" className="mt-1 block text-xs text-danger">
                      {t(zeitenFehler)}
                    </span>
                  )}
                </label>
              </div>

              {termin && autoPostAktiv && (
                <div className="text-xs text-text-secondary">
                  {t('Nächster Post: {termin}', { termin })}
                </div>
              )}
            </div>
          );
        })}
      </div>
      {error ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(error, t)}
        </div>
      ) : null}
    </div>
  );
}

/**
 * Auto-Posting je Spielkategorie. Angereichert wird nur Deadlock; bei anderen
 * Kategorien bleiben Titel und Hashtags manuell bearbeitbar.
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Gamepad2 className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Kategorien')}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <div className="space-y-2">
        {(plan?.categories ?? []).map((kategorie) => (
          <label
            key={kategorie.category_key}
            className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3"
          >
            <span>
              <span className="block text-sm font-semibold text-white">
                {t(kategorieLabel(kategorie.category_key, kategorie.display_name))}
              </span>
              <span className="block text-xs text-text-secondary mt-0.5">
                {kategorie.enrichment_enabled
                  ? t('Mit Titel- und Hashtag-Vorschlägen.')
                  : t('Ohne automatische Vorschläge; Metadaten bleiben manuell bearbeitbar.')}
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
      {error ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(error, t)}
        </div>
      ) : null}
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
            : t('{clips} Clips im Pool, das sind rund {tage} Tage bei {proWoche} Posts pro Woche.', {
                clips: pool.verfuegbare_clips,
                tage: pool.reicht_fuer_tage,
                proWoche: pool.posts_pro_woche,
              })}
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
type OauthRueckmeldung =
  | { art: 'ok'; platform: string }
  | { art: 'fehler'; code: string }
  | null;

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
  const standUnbekannt = isLoading || istStandUnbekannt(ladeFehler);
  const gesperrt = istGesperrt({ isLoading, isSaving: isDisconnecting, ladeFehler });
  const rueckmeldung = leseOauthRueckmeldung();

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <ExternalLink className="w-4 h-4 text-orange" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Verbindungen')} · {streamer}
        </h3>
        {isLoading && <Loader2 className="w-4 h-4 text-orange animate-spin ml-auto" />}
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
          const sammelverbindung = connected && !abgelaufen && (status?.uses_global_fallback ?? false);
          const veröffentlichungGesperrt =
            connected &&
            !abgelaufen &&
            (status?.provider_release_blocked ?? !(status?.provider_calls_enabled ?? false));
          const sperrgrund = status?.release_block_reason;
          const ablauf = status?.expires_at
            ? new Date(status.expires_at).toLocaleDateString(locale, {
                day: '2-digit',
                month: '2-digit',
                year: 'numeric',
              })
            : null;

          let zeile: string;
          let tonKlasse = 'text-text-secondary';
          if (isLoading) {
            zeile = t('Verbindungen werden geladen…');
          } else if (standUnbekannt) {
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
              className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <div className="text-sm font-semibold text-white">
                  {PLATFORM_LABELS[platform] ?? platform}
                </div>
                <div className={`break-words text-xs ${tonKlasse}`}>{zeile}</div>
                {!standUnbekannt && veröffentlichungGesperrt && (
                  <div className="text-[11px] text-warning">
                    {sperrgrund
                      ? fehlerText({ code: sperrgrund }, t)
                      : t('Veröffentlichung wartet auf Plattformfreigabe.')}
                  </div>
                )}
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
                  aria-label={t('Trennen: {platform} für {streamer}', {
                    platform: PLATFORM_LABELS[platform] ?? platform,
                    streamer,
                  })}
                  onClick={() => {
                    // Der Kanalname gehoert in die Frage: es gibt eine
                    // Sammelverbindung, und niemand soll aus Versehen alle
                    // Kanaele kappen.
                    const frage = sammelverbindung
                      ? t('{platform} für {streamer} trennen? Der Kanal nutzt die Sammelverbindung.', {
                          platform: PLATFORM_LABELS[platform] ?? platform,
                          streamer,
                        })
                      : t('{platform} für {streamer} trennen?', {
                          platform: PLATFORM_LABELS[platform] ?? platform,
                          streamer,
                        });
                    if (window.confirm(frage)) onDisconnect(platform);
                  }}
                  className="rounded-xl border border-border px-3 py-1.5 text-sm font-semibold text-text-secondary hover:text-white disabled:opacity-40"
                >
                  {t('Trennen')}
                </button>
              ) : (
                <a
                  href={oauthStartUrl(platform, streamer)}
                  aria-label={
                    abgelaufen
                      ? t('Neu verbinden: {platform} für {streamer}', {
                          platform: PLATFORM_LABELS[platform] ?? platform,
                          streamer,
                        })
                      : t('Verbinden: {platform} für {streamer}', {
                          platform: PLATFORM_LABELS[platform] ?? platform,
                          streamer,
                        })
                  }
                  className="rounded-xl border border-orange bg-orange/15 px-3 py-1.5 text-sm font-semibold text-white shrink-0"
                >
                  {abgelaufen ? t('Neu verbinden') : t('Verbinden')}
                </a>
              )}
            </div>
          );
        })}
      </div>

      {error ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(error, t)}
        </div>
      ) : null}
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Archive className="w-4 h-4 text-orange" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('VOD-Archiv')} · {settings?.streamer_login ?? streamer}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-orange animate-spin ml-auto" />}
      </div>

      <LadeFehlerHinweis fehler={ladeFehler} />

      <label className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3">
        <span className="text-sm font-semibold text-white">{t('Automatisch sichern')}</span>
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
              aria-pressed={privacy === option}
              disabled={gesperrt || settings?.privacy_forced}
              onClick={() => onChange({ enabled, privacy: option })}
              className={`rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
                privacy === option
                  ? 'border-orange text-white bg-orange/15'
                  : 'border-border text-text-secondary hover:text-white'
              } disabled:opacity-40 disabled:hover:text-text-secondary`}
            >
              {labels[option]}
            </button>
          ))}
        </div>
      </div>

      {error ? (
        <div role="alert" className="text-xs text-danger">
          {fehlerText(error, t)}
        </div>
      ) : null}
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Languages className="w-4 h-4 text-orange" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">{t('Sprache')}</h3>
      </div>
      <p className="text-sm text-text-secondary">
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
            className={`rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
              language === option
                ? 'border-orange text-white bg-orange/15'
                : 'border-border text-text-secondary hover:text-white'
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
  /** `false` ist der harte Testbetrieb ohne Veröffentlichung. */
  releaseEnabled: boolean | null;
  /** Der manuelle Providerabgleich bestätigt eine externe Veröffentlichung. */
  canReconcile: boolean;
  /** Behält nach einer Aktion einen sinnvollen Fokus und kündigt den Erfolg an. */
  onActionComplete: (message: string) => void;
  editingMode: EditMode | null;
  onOpenEditor: (mode: EditMode) => void;
  onCloseEditor: () => void;
  onDiscard: () => void;
  onSaveOverride: (layout: LayoutPayload) => Promise<void>;
  onResetOverride: () => Promise<void>;
  overridePending: boolean;
  onApprovalDecision: (decision: 'approve' | 'skip' | 'edit', platforms: SocialPlatform[]) => void;
  approvalPending: boolean;
  onCancelScheduled: () => void;
  cancelPending: boolean;
  cancelResult: { cancelled: number; already_running: number } | null;
  discardResult: DiscardClipResult | null;
  /**
   * Plattformen, die die letzte Freigabe an diesem Clip ausgelassen hat, weil
   * dort die Kadenz auf null steht. Ohne diese Zeile quittiert die Oberflaeche
   * eine Freigabe, die auf der gewaehlten Plattform nie stattfindet.
   */
  nichtEingeplant: SocialPlatform[];
  /** Fehler der letzten Aktion an genau diesem Clip. */
  fehler: unknown;
}

function ClipCard({
  clip,
  timezone,
  releaseEnabled,
  canReconcile,
  onActionComplete,
  editingMode,
  onOpenEditor,
  onCloseEditor,
  onDiscard,
  onSaveOverride,
  onResetOverride,
  overridePending,
  onApprovalDecision,
  approvalPending,
  onCancelScheduled,
  cancelPending,
  cancelResult,
  discardResult,
  nichtEingeplant,
  fehler,
}: ClipCardProps) {
  const { t, locale } = useLanguage();
  const status = STATUS_LABELS[clip.status] ?? STATUS_LABELS.pending;
  const sourceLabel = clip.source_kind === 'manual_upload' ? t('Upload') : t('Twitch');
  const enrichmentTopHashtags = clip.enrichment_summary?.top_hashtags ?? [];
  const enrichmentStatus = clip.enrichment_status;

  // Fehlgeschlagene Uploads ohne Grund sind nicht zu gebrauchen: der Grund je
  // Plattform steht am Clip und gehoert auf die Karte.
  const uploadFehler = PLATTFORMEN.map((platform) => ({
    platform,
    text: clip.upload_errors?.[platform] ?? null,
    queueStatus: clip.upload_states?.[platform]?.status ?? null,
  })).filter(
    (eintrag): eintrag is {
      platform: SocialPlatform;
      text: string;
      queueStatus: 'failed' | null;
    } =>
      Boolean(eintrag.text) &&
      (eintrag.queueStatus === 'failed' ||
        (eintrag.queueStatus === null &&
          (clip.status === 'failed' || clip.status === 'published_partial'))),
  );
  const zeigeUploadFehler = uploadFehler.length > 0;

  const termine = PLATTFORMEN.map((platform) => ({
    platform,
    zeit: clip.scheduled_at?.[platform] ?? null,
  })).filter((eintrag): eintrag is { platform: SocialPlatform; zeit: string } => !!eintrag.zeit);
  // Veto-Fenster: solange ein Termin in der Zukunft steht, laesst sich der Post
  // noch stoppen.
  const stoppbar = clip.status === 'approved' && termine.length > 0;
  const unklareUploads = PLATTFORMEN.map((platform) => ({
    platform,
    stand: clip.upload_states?.[platform] ?? null,
  })).filter(
    (eintrag) =>
      eintrag.stand?.reconciliation_required === true ||
      eintrag.stand?.status === 'reconciliation_required',
  );
  const abgleichDaten = unklareUploads.flatMap(({ platform, stand }) =>
    stand?.reconciliation_id && stand.provider_started_at
      ? [
          {
            reconciliation_id: stand.reconciliation_id,
            platform,
            provider_started_at: stand.provider_started_at,
            provider_external_id: stand.provider_external_id,
          },
        ]
      : [],
  );
  const abgleichVollstaendig =
    abgleichDaten.length > 0 && abgleichDaten.length === unklareUploads.length;
  const fehlerZeile = fehlerText(fehler, t);
  const verwerfenHinweis = verwerfenWarnung(discardResult);
  const [selectedPlatforms, setSelectedPlatforms] = useState<SocialPlatform[]>(
    (clip.approval?.approved_platforms ?? []).filter(
      (platform) => !DIREKTFREIGABE_GESPERRT.has(platform),
    ),
  );
  // Twitch raeumt Clip-Thumbnails irgendwann weg; eine 404-URL darf die Kachel
  // nicht mit Alt-Text fluten, sondern faellt auf das Ersatzbild zurueck.
  const [vorschauFehlt, setVorschauFehlt] = useState(false);
  const [preparation, setPreparation] = useState<ClipPreparation | null>(null);
  const editorAusloeserRef = useRef<HTMLButtonElement | null>(null);
  const layoutPanelRef = useRef<HTMLDivElement | null>(null);
  const enrichmentPanelRef = useRef<HTMLDivElement | null>(null);
  const letzterEditorRef = useRef<EditMode | null>(null);
  const layoutPanelId = `clip-${clip.clip_db_id}-layout-editor`;
  const enrichmentPanelId = `clip-${clip.clip_db_id}-metadaten-editor`;
  const queryClient = useQueryClient();
  const abgleichMutation = useMutation({
    mutationFn: () =>
      markClipPublished({
        clipDbId: clip.clip_db_id,
        reconciliations: abgleichDaten,
        streamer: clip.streamer_login,
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      onActionComplete(t('Der Plattformstand wurde abgeglichen.'));
    },
  });
  const layoutFingerprint = JSON.stringify(clip.effective_layout);
  const letzterLayoutFingerprint = useRef(layoutFingerprint);
  useEffect(() => {
    setVorschauFehlt(false);
  }, [clip.thumbnail_url]);
  const vorschauSichtbar = Boolean(clip.thumbnail_url) && !vorschauFehlt;
  const freigabe = freigabeAktion(
    releaseEnabled,
    selectedPlatforms.length,
    approvalPending,
    preparation?.preview_ready === true && Boolean(preparation.preview_url),
  );
  const freigabeHinweisId = `clip-${clip.clip_db_id}-freigabe-hinweis`;
  const tiktokHinweisId = `clip-${clip.clip_db_id}-tiktok-hinweis`;

  useEffect(() => {
    setSelectedPlatforms(
      (clip.approval?.approved_platforms ?? []).filter(
        (platform) => !DIREKTFREIGABE_GESPERRT.has(platform),
      ),
    );
  }, [clip.approval?.approved_platforms, clip.clip_db_id]);

  useEffect(() => {
    if (letzterLayoutFingerprint.current === layoutFingerprint) return;
    letzterLayoutFingerprint.current = layoutFingerprint;
    setPreparation(null);
  }, [layoutFingerprint]);

  useEffect(() => {
    const vorherigerEditor = letzterEditorRef.current;
    if (editingMode === 'layout') layoutPanelRef.current?.focus();
    if (editingMode === 'enrichment') enrichmentPanelRef.current?.focus();
    if (!editingMode && vorherigerEditor) editorAusloeserRef.current?.focus();
    letzterEditorRef.current = editingMode;
  }, [editingMode]);

  const togglePlatform = (platform: SocialPlatform, checked: boolean) => {
    if (DIREKTFREIGABE_GESPERRT.has(platform)) return;
    setSelectedPlatforms((current) => {
      const next = new Set(current);
      if (checked) next.add(platform);
      else next.delete(platform);
      return Array.from(next) as SocialPlatform[];
    });
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="panel-card card-glow group rounded-2xl overflow-hidden flex flex-col"
    >
      <div className="relative aspect-video overflow-hidden bg-[radial-gradient(120%_120%_at_50%_0%,rgba(255,255,255,0.06),transparent_60%)] bg-black/50">
        {vorschauSichtbar ? (
          <img
            src={clip.thumbnail_url ?? ''}
            alt=""
            loading="lazy"
            decoding="async"
            onError={() => setVorschauFehlt(true)}
            className="w-full h-full object-cover transition-transform duration-500 ease-out group-hover:scale-[1.04]"
          />
        ) : (
          // Kein Alt-Text als Ersatzbild: eine tote URL malte sonst den vollen
          // Clip-Titel ueber die schwarze Kachel.
          <div className="w-full h-full flex flex-col items-center justify-center gap-1.5 text-text-secondary">
            <Film className="w-8 h-8 opacity-40" />
            <span className="text-[10px] uppercase tracking-[0.16em]">
              {t('Keine Vorschau')}
            </span>
          </div>
        )}

        <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-black/60 via-black/0 to-black/75" />

        {clip.clip_url && (
          <a
            href={clip.clip_url}
            target="_blank"
            rel="noreferrer"
            title={t('Original ansehen')}
            className="absolute inset-0 grid place-items-center opacity-0 transition-opacity duration-200 group-hover:opacity-100 focus-visible:opacity-100"
          >
            <span className="grid h-12 w-12 place-items-center rounded-full border border-white/25 bg-black/55 backdrop-blur-sm">
              <PlayCircle className="w-7 h-7 text-white" />
            </span>
          </a>
        )}

        <div className="pointer-events-none absolute top-2 left-2 flex items-center gap-1.5">
          <span className={`text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border backdrop-blur-sm ${TONE_BADGE[status.tone]}`}>
            {t(status.label)}
          </span>
          <span className="text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border border-white/15 bg-black/50 text-white/90 backdrop-blur-sm">
            {sourceLabel}
          </span>
        </div>

        <div className="pointer-events-none absolute inset-x-2 bottom-2 flex items-end justify-between gap-2">
          <span className="inline-flex items-center gap-1 rounded bg-black/60 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-white backdrop-blur-sm">
            {formatClipDauer(clip.duration_seconds)}
          </span>
          <span className="inline-flex items-center gap-1.5 rounded bg-black/55 px-1.5 py-0.5 font-mono text-[10px] text-white/85 backdrop-blur-sm">
            <Clock className="w-3 h-3" /> {formatRetention(clip.retention_until, t)}
          </span>
        </div>
      </div>

      <div className="p-4 flex flex-col gap-3 flex-1">
        <div className="space-y-1">
          <h4 className="font-bold text-white line-clamp-2">{clip.title}</h4>
          <p className="text-xs text-text-secondary">
            {clip.streamer_login} ·{' '}
            {t('{views} Aufrufe', { views: (clip.view_count ?? 0).toLocaleString(locale) })}
          </p>
          {clip.layout_override && (
            <p className="text-[11px] text-orange inline-flex items-center gap-1">
              <Pencil className="w-3 h-3" /> {t('Override aktiv')}
            </p>
          )}
        </div>

        {enrichmentTopHashtags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {enrichmentTopHashtags.slice(0, 4).map((tag) => (
              <span
                key={tag}
                className="text-[10px] font-semibold px-1.5 py-0.5 rounded-md bg-accent/10 text-accent border border-accent/30"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}

        <ClipPreparationWorkbench
          clipDbId={clip.clip_db_id}
          clipTitle={clip.title}
          onPreparationChange={setPreparation}
        />

        {zeigeUploadFehler && (
          <div role="alert" className="flex items-start gap-2 text-xs text-danger bg-danger/10 border border-danger/30 rounded-lg p-2.5">
            <AlertCircle className="w-4 h-4 mt-0.5 flex-shrink-0" />
            <div className="space-y-1 min-w-0">
              {uploadFehler.map(({ platform, text }) => (
                <div key={platform} className="break-words">
                  <span className="font-bold">{PLATFORM_LABELS[platform] ?? platform}:</span>{' '}
                  {fehlerText({ code: text }, t)}
                </div>
              ))}
            </div>
          </div>
        )}

        {unklareUploads.length > 0 && (
          <div
            role="alert"
            className="flex items-start gap-2 rounded-lg border border-warning/35 bg-warning/10 p-2.5 text-xs text-warning"
          >
            <ShieldAlert className="mt-0.5 h-4 w-4 flex-shrink-0" />
            <div className="min-w-0 space-y-2">
              <div>
                <p className="font-bold">{t('Ergebnis unklar')}</p>
                <p className="mt-0.5 text-text-secondary">
                  {t(
                    'Der Plattformaufruf wurde gestartet, aber sein Ergebnis konnte nicht sicher bestätigt werden. Prüfe zuerst die Zielplattform.',
                  )}
                </p>
              </div>
              <div className="space-y-1">
                {unklareUploads.map(({ platform, stand }) => (
                  <div key={platform} className="break-all">
                    <span className="font-bold">{PLATFORM_LABELS[platform] ?? platform}</span>
                    {stand?.provider_external_id
                      ? ` · ${t('Plattform-ID')}: ${stand.provider_external_id}`
                      : null}
                  </div>
                ))}
              </div>
              {canReconcile ? (
                <button
                  type="button"
                  disabled={abgleichMutation.isPending || !abgleichVollstaendig}
                  onClick={() => {
                    if (
                      window.confirm(
                        t(
                          'Hast du den Clip auf allen genannten Plattformen als veröffentlicht geprüft?',
                        ),
                      )
                    ) {
                      abgleichMutation.mutate();
                    }
                  }}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-warning/35 bg-warning/15 px-2.5 py-1.5 font-bold text-warning hover:bg-warning/20 disabled:opacity-50"
                >
                  {abgleichMutation.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <CheckCircle2 className="h-3.5 w-3.5" />
                  )}
                  {t('Nach Plattformprüfung bestätigen')}
                </button>
              ) : (
                <p>{t('Nur die Verwaltung kann diesen Plattformstand bestätigen.')}</p>
              )}
              {canReconcile && !abgleichVollstaendig ? (
                <p className="text-danger">
                  {t('Die Abgleichdaten sind unvollständig. Lade die Clip-Liste neu.')}
                </p>
              ) : null}
              {abgleichMutation.isError ? (
                <p role="alert" className="text-danger">{fehlerText(abgleichMutation.error, t)}</p>
              ) : null}
              {abgleichMutation.isSuccess ? (
                <p role="status" aria-live="polite" className="text-success">{t('Der Plattformstand wurde abgeglichen.')}</p>
              ) : null}
            </div>
          </div>
        )}

        {termine.length > 0 && (
          <div className="rounded-lg border border-border bg-bg/30 p-2.5 space-y-1">
            <div className="text-[11px] uppercase tracking-[0.14em] font-bold text-text-secondary inline-flex items-center gap-1.5">
              <CalendarClock className="w-3 h-3" /> {t('Eingeplant')}
            </div>
            {termine.map(({ platform, zeit }) => (
              <div key={platform} className="text-xs text-text-secondary">
                {PLATFORM_LABELS[platform] ?? platform}:{' '}
                <span className="text-white">{formatTerminInZone(zeit, locale, timezone)}</span>
              </div>
            ))}
            {stoppbar && (
              <button
                type="button"
                onClick={onCancelScheduled}
                disabled={cancelPending}
                className="mt-1 inline-flex items-center gap-1.5 rounded-lg border border-danger/30 bg-danger/12 px-2.5 py-1.5 text-xs font-bold text-danger hover:bg-danger/20 disabled:opacity-50"
              >
                {cancelPending ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <XCircle className="w-3.5 h-3.5" />
                )}
                {t('Doch nicht posten')}
              </button>
            )}
          </div>
        )}

        <div className="rounded-xl border border-border bg-bg/30 p-3 space-y-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-[11px] uppercase tracking-[0.14em] font-bold text-orange">
                {t('Freigabe')}
              </p>
              <p className="text-xs text-text-secondary">
                {clip.approval?.state
                  ? t('Status: {state}', {
                      state: t(APPROVAL_STATE_LABELS[clip.approval.state] ?? clip.approval.state),
                    })
                  : t('Bereit zur Prüfung, sobald die Metadaten abgeschlossen sind.')}
              </p>
            </div>
            {approvalPending && <Loader2 className="w-4 h-4 text-orange animate-spin" />}
          </div>
          <div className="grid grid-cols-3 gap-2">
            {(['youtube', 'tiktok', 'instagram'] as const).map((platform) => {
              const direktfreigabeGesperrt = DIREKTFREIGABE_GESPERRT.has(platform);
              return (
                <label
                  key={platform}
                  className={`inline-flex items-center justify-center gap-2 rounded-lg border border-border bg-bg/40 px-2 py-2 text-xs font-semibold text-white ${
                    direktfreigabeGesperrt ? 'cursor-not-allowed opacity-55' : ''
                  }`}
                >
                  <input
                    type="checkbox"
                    aria-describedby={direktfreigabeGesperrt ? tiktokHinweisId : undefined}
                    checked={selectedPlatforms.includes(platform)}
                    disabled={direktfreigabeGesperrt}
                    onChange={(event) => togglePlatform(platform, event.target.checked)}
                    className="h-3.5 w-3.5 accent-orange"
                  />
                  {PLATFORM_LABELS[platform] ?? platform}
                  {direktfreigabeGesperrt && (
                    <LockKeyhole aria-hidden="true" className="h-3.5 w-3.5 text-warning" />
                  )}
                </label>
              );
            })}
          </div>
          <p id={tiktokHinweisId} className="text-[11px] text-text-secondary">
            {t(
              'TikTok bleibt gesperrt, bis Sichtbarkeit, Interaktionen und Zustimmung pro Clip gewählt werden können.',
            )}
          </p>
          {freigabe.hinweis && (
            <p id={freigabeHinweisId} className="text-[11px] text-warning">
              {t(freigabe.hinweis)}
            </p>
          )}
          <div className="grid grid-cols-3 gap-2">
            <button
              type="button"
              aria-describedby={freigabe.hinweis ? freigabeHinweisId : undefined}
              onClick={() => onApprovalDecision('approve', selectedPlatforms)}
              disabled={freigabe.disabled}
              className="inline-flex items-center justify-center gap-1.5 text-xs font-bold px-3 py-2 rounded-lg bg-success/15 text-success border border-success/30 hover:bg-success/20 disabled:opacity-50"
            >
              <CheckCircle2 className="w-3.5 h-3.5" /> {t(freigabe.label)}
            </button>
            <button
              type="button"
              aria-expanded={editingMode === 'enrichment'}
              aria-controls={enrichmentPanelId}
              onClick={(event) => {
                editorAusloeserRef.current = event.currentTarget;
                onApprovalDecision('edit', selectedPlatforms);
                onOpenEditor('enrichment');
              }}
              disabled={approvalPending}
              className="inline-flex items-center justify-center gap-1.5 text-xs font-bold px-3 py-2 rounded-lg bg-warning/15 text-warning border border-warning/30 hover:bg-warning/20 disabled:opacity-50"
            >
              <Pencil className="w-3.5 h-3.5" /> {t('Bearbeiten')}
            </button>
            <button
              type="button"
              onClick={() => onApprovalDecision('skip', selectedPlatforms)}
              disabled={approvalPending}
              className="inline-flex items-center justify-center gap-1.5 text-xs font-bold px-3 py-2 rounded-lg bg-danger/12 text-danger border border-danger/30 hover:bg-danger/20 disabled:opacity-50"
            >
              <Trash2 className="w-3.5 h-3.5" /> {t('Überspringen')}
            </button>
          </div>
        </div>

        <div className="mt-auto flex flex-wrap items-center gap-2 pt-2">
          {clip.clip_url && (
            <a
              href={clip.clip_url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex min-h-6 items-center gap-1.5 text-xs text-text-secondary hover:text-white"
            >
              <ExternalLink className="w-3.5 h-3.5" /> {t('Original')}
            </a>
          )}
          <button
            type="button"
            onClick={onDiscard}
            disabled={clip.status === 'discarded' || !!clip.discarded_at}
            className="ml-auto inline-flex items-center gap-1.5 text-xs font-semibold text-danger hover:text-danger px-2 py-1.5 rounded-lg hover:bg-danger/10 transition disabled:opacity-30"
          >
            <Trash2 className="w-3.5 h-3.5" /> {t('Verwerfen')}
          </button>
          <button
            type="button"
            aria-expanded={editingMode === 'enrichment'}
            aria-controls={enrichmentPanelId}
            onClick={(event) => {
              editorAusloeserRef.current = event.currentTarget;
              onOpenEditor('enrichment');
            }}
            className={`inline-flex items-center gap-1.5 text-xs font-bold px-3 py-1.5 rounded-lg border transition ${
              editingMode === 'enrichment'
                ? 'bg-accent/25 text-accent border-accent/50'
                : 'bg-accent/10 text-accent border-accent/30 hover:bg-accent/20'
            }`}
          >
            <Wand2 className="w-3.5 h-3.5" /> {t('Metadaten')}
            {enrichmentStatus && enrichmentStatus !== 'done' && (
              <span className="text-[9px] uppercase tracking-[0.14em] opacity-80">
                · {t(STATUS_META[enrichmentStatus]?.label ?? enrichmentStatus)}
              </span>
            )}
          </button>
          <button
            type="button"
            aria-expanded={editingMode === 'layout'}
            aria-controls={layoutPanelId}
            onClick={(event) => {
              editorAusloeserRef.current = event.currentTarget;
              onOpenEditor('layout');
            }}
            className={`inline-flex items-center gap-1.5 text-xs font-bold px-3 py-1.5 rounded-lg border transition ${
              editingMode === 'layout'
                ? 'bg-orange/25 text-orange border-orange/50'
                : 'bg-orange/15 text-orange border-orange/30 hover:bg-orange/25'
            }`}
          >
            <Pencil className="w-3.5 h-3.5" /> {t('Layout')}
          </button>
        </div>

        {/* Steht ausserhalb der Terminliste: nach dem Stoppen verschwinden die
            Termine, die Rueckmeldung soll trotzdem stehen bleiben. */}
        {cancelResult && (
          <div role="status" aria-live="polite" className="text-xs text-text-secondary border-t border-border pt-2">
            {cancelResult.already_running > 0
              ? t('Gestoppt, aber {count} Plattform war schon durch.', {
                  count: cancelResult.already_running,
                })
              : t('{count} geplante Posts gestoppt.', { count: cancelResult.cancelled })}
          </div>
        )}

        {/* Eine Freigabe auf eine Plattform mit Kadenz null wird sauber
            quittiert, aber dort passiert nichts. Ohne diese Zeile merkt das
            niemand. */}
        {nichtEingeplant.length > 0 && (
          <div role="status" aria-live="polite" className="text-xs text-orange border-t border-orange/20 pt-2">
            {t('Auf {platforms} passiert nichts, dort steht die Kadenz auf null.', {
              platforms: nichtEingeplant.map((plattform) => PLATFORM_LABELS[plattform] ?? plattform).join(', '),
            })}
          </div>
        )}

        {verwerfenHinweis && (
          <div
            role="alert"
            className="flex items-start gap-2 border-t border-warning/25 pt-2 text-xs text-warning"
          >
            <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            <span>{t(verwerfenHinweis)}</span>
          </div>
        )}

        {fehlerZeile && (
          <div role="alert" className="text-xs text-danger border-t border-danger/20 pt-2">
            {fehlerZeile}
          </div>
        )}
      </div>

      {editingMode === 'layout' && (
        <div
          id={layoutPanelId}
          ref={layoutPanelRef}
          role="region"
          tabIndex={-1}
          aria-label={t('Layout für {title} bearbeiten', { title: clip.title })}
          className="border-t border-border bg-bg/30 p-4"
        >
          <LayoutEditor
            initialLayout={clip.effective_layout}
            isSaving={overridePending}
            saveLabel={t('Override speichern')}
            cancelLabel={t('Schließen')}
            geltungHinweis={t('Gilt nur für diesen Clip.')}
            /* In der Karte ist die Vorschau genau dieser Clip, nichts zum Waehlen. */
            vorschauClips={
              clip.thumbnail_url
                ? [{ id: String(clip.clip_db_id), titel: clip.title, bildUrl: clip.thumbnail_url }]
                : []
            }
            onSave={async (layout) => {
              const vorherigeVorbereitung = preparation;
              setPreparation(null);
              try {
                await onSaveOverride(layout);
                onCloseEditor();
              } catch {
                setPreparation(vorherigeVorbereitung);
                // Der Mutationsfehler bleibt sichtbar an genau dieser Clip-Karte.
              }
            }}
            onCancel={onCloseEditor}
          />
          {clip.layout_override && (
            <div className="mt-3 flex justify-end">
              <button
                type="button"
                disabled={overridePending}
                onClick={async () => {
                  if (window.confirm(t('Override entfernen und Streamer-Default verwenden?'))) {
                    const vorherigeVorbereitung = preparation;
                    setPreparation(null);
                    try {
                      await onResetOverride();
                      onCloseEditor();
                    } catch {
                      setPreparation(vorherigeVorbereitung);
                      // Nicht schließen: der Nutzer soll den fehlgeschlagenen Stand sehen.
                    }
                  }
                }}
                className="text-xs text-text-secondary hover:text-white disabled:opacity-50"
              >
                {t('Override entfernen → Streamer-Default')}
              </button>
            </div>
          )}
        </div>
      )}

      {editingMode === 'enrichment' && (
        <div
          id={enrichmentPanelId}
          ref={enrichmentPanelRef}
          role="region"
          tabIndex={-1}
          aria-label={t('Metadaten für {title} bearbeiten', { title: clip.title })}
          className="border-t border-border bg-bg/30 p-4"
        >
          <EnrichmentPanel clipDbId={clip.clip_db_id} onClose={onCloseEditor} />
        </div>
      )}
    </motion.div>
  );
}
