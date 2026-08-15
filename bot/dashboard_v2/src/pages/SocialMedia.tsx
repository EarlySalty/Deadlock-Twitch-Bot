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
  Wand2,
  SlidersHorizontal,
  Languages,
} from 'lucide-react';
import { KpiCard } from '@/components/cards/KpiCard';
import { useLanguage, useT } from '@/context/LanguageContext';
import { LANGUAGES, LANGUAGE_LABELS, type Language } from '@/i18n/dictionary';
import { AnalyticsTab } from '@/components/socialmedia/AnalyticsTab';
import { LayoutEditor } from '@/components/socialmedia/LayoutEditor';
import { EnrichmentPanel } from '@/components/socialmedia/EnrichmentPanel';
import {
  decideClipApproval,
  SocialMediaForbiddenError,
  discardClip,
  fetchPostingPlan,
  fetchClips,
  fetchVodArchiveSettings,
  fetchPlatformStatus,
  disconnectPlatform,
  oauthStartUrl,
  type PlatformStatus,
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
  type ApprovalMode,
  type ClipPoolForecast,
  type PlatformScheduleEntry,
  type PostingPlan,
  DEFAULT_LAYOUT,
  type ClipStatus,
  type LayoutPayload,
  type SocialClip,
  type SocialPlatform,
  type StreamerLayoutResponse,
  type VodArchivePrivacy,
  type VodArchiveSettings,
} from '@/types/socialMedia';

interface SocialMediaProps {
  streamer: string;
}

/** Deutscher Text als Schluessel: ohne Uebersetzung bleibt er einfach stehen. */
type Translate = (text: string, params?: Record<string, string | number>) => string;

const STATUS_LABELS: Record<ClipStatus, { label: string; tone: 'orange' | 'teal' | 'success' | 'warning' | 'danger' | 'muted' }> = {
  pending: { label: 'Wartend', tone: 'muted' },
  enriched: { label: 'Aufbereitet', tone: 'teal' },
  awaiting_approval: { label: 'Freigabe', tone: 'orange' },
  approved: { label: 'Freigegeben', tone: 'success' },
  editing: { label: 'Bearbeitung', tone: 'warning' },
  skipped: { label: 'Skipped', tone: 'muted' },
  publishing: { label: 'Wird gepostet', tone: 'orange' },
  published_partial: { label: 'Teilveröffentlicht', tone: 'warning' },
  published_all: { label: 'Veröffentlicht', tone: 'success' },
  discarded: { label: 'Verworfen', tone: 'muted' },
  failed: { label: 'Fehler', tone: 'danger' },
};

const TONE_BADGE: Record<string, string> = {
  orange: 'bg-orange/15 text-orange border-orange/35',
  teal: 'bg-teal/15 text-teal border-teal/35',
  success: 'bg-success/15 text-success border-success/35',
  warning: 'bg-warning/15 text-warning border-warning/35',
  danger: 'bg-danger/15 text-danger border-danger/35',
  muted: 'bg-bg/60 text-text-secondary border-border',
};

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
/**
 * Die vier Bereiche der Seite. Getrennt statt alles auf einer Flaeche: die
 * Reihenfolge folgt dem Weg eines Clips, vom Konto ueber den Plan und den Pool
 * bis zum Veroeffentlichten.
 */
type SocialMediaView = 'konten' | 'plan' | 'pool' | 'veroeffentlicht';

export function SocialMedia({ streamer }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const t = useT();
  const [statusFilter, setStatusFilter] = useState<ClipStatus | 'all'>('pending');
  const [editingClip, setEditingClip] = useState<{ id: number; mode: EditMode } | null>(null);
  const [activeView, setActiveView] = useState<SocialMediaView>('pool');

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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
    },
  });

  const overrideMutation = useMutation({
    mutationFn: ({ clipDbId, layout }: { clipDbId: number; layout: LayoutPayload | null }) =>
      setClipLayoutOverride(clipDbId, layout),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
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
      payload: Partial<Omit<PlatformScheduleEntry, 'platform' | 'next_slot'>>;
    }) => savePlatformSchedule(streamer, platform, payload),
    onSuccess: uebernehmePlan,
  });

  const categoryMutation = useMutation({
    mutationFn: ({ categoryKey, autoPost }: { categoryKey: string; autoPost: boolean }) =>
      saveCategoryAutoPost(streamer, categoryKey, autoPost),
    onSuccess: uebernehmePlan,
  });

  const platformStatusQuery = useQuery({
    queryKey: ['social-media', 'platform-status', streamer],
    queryFn: () => fetchPlatformStatus(),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: (platform: string) => disconnectPlatform(platform),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'platform-status', streamer] });
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
    },
    onError: (error) => {
      window.alert((error as Error).message);
    },
  });

  const stats = useMemo(() => {
    const list = clipsQuery.data?.items ?? [];
    const total = clipsQuery.data?.total ?? list.length;
    const publishedToday = list.filter((c) => {
      if (c.status !== 'published_all') return false;
      const created = new Date(c.created_at);
      const now = new Date();
      return (
        created.getUTCFullYear() === now.getUTCFullYear() &&
        created.getUTCMonth() === now.getUTCMonth() &&
        created.getUTCDate() === now.getUTCDate()
      );
    }).length;
    const nextRetention = list
      .map((c) => (c.retention_until ? new Date(c.retention_until).getTime() : null))
      .filter((v): v is number => v !== null)
      .sort((a, b) => a - b)[0];
    const manualUploads = list.filter((c) => c.source_kind === 'manual_upload').length;
    return {
      total,
      publishedToday,
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
      <SocialHero streamer={streamer} isDefaultLayout={layoutQuery.data?.is_default ?? false} />

      <div className="inline-flex flex-wrap rounded-2xl border border-border bg-bg/60 p-1.5 gap-1.5">
        {[
          { id: 'pool' as const, label: 'Clip-Pool', Icon: Layers3 },
          { id: 'plan' as const, label: 'Zeitplan', Icon: Calendar },
          { id: 'veroeffentlicht' as const, label: 'Veröffentlicht', Icon: BarChart3 },
          { id: 'konten' as const, label: 'Konten', Icon: SlidersHorizontal },
        ].map(({ id, label, Icon }) => {
          const active = activeView === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => setActiveView(id)}
              className={`inline-flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-semibold transition ${
                active
                  ? 'bg-gradient-to-r from-orange/85 to-teal/70 text-white shadow-[0_6px_24px_-10px_rgba(201, 168, 106, 0.45)]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              <Icon className="w-4 h-4" />
              {t(label)}
            </button>
          );
        })}
      </div>

      {activeView === 'veroeffentlicht' ? (
        <AnalyticsTab streamer={streamer} clips={clipsQuery.data?.items ?? []} />
      ) : activeView === 'plan' ? (
        <div className="space-y-6">
          <VorratsHinweis pool={postingPlanQuery.data?.pool ?? null} />
          <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
            <ApprovalModeCard
              plan={postingPlanQuery.data ?? null}
              isLoading={postingPlanQuery.isLoading}
              isSaving={approvalModeMutation.isPending}
              error={approvalModeMutation.error as Error | null}
              onChange={(mode) => approvalModeMutation.mutate({ approval_mode: mode })}
            />
            <CategoryCard
              plan={postingPlanQuery.data ?? null}
              isLoading={postingPlanQuery.isLoading}
              isSaving={categoryMutation.isPending}
              error={categoryMutation.error as Error | null}
              onChange={(categoryKey, autoPost) =>
                categoryMutation.mutate({ categoryKey, autoPost })
              }
            />
            <div className="xl:col-span-2">
              <PostingScheduleCard
                plan={postingPlanQuery.data ?? null}
                isLoading={postingPlanQuery.isLoading}
                isSaving={platformScheduleMutation.isPending}
                error={platformScheduleMutation.error as Error | null}
                onChange={(platform, payload) =>
                  platformScheduleMutation.mutate({ platform, payload })
                }
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
            onDisconnect={(platform) => disconnectMutation.mutate(platform)}
            isDisconnecting={disconnectMutation.isPending}
          />
          <VodArchiveCard
            streamer={streamer}
            settings={vodArchiveQuery.data ?? null}
            isLoading={vodArchiveQuery.isLoading}
            isSaving={vodArchiveMutation.isPending}
            error={vodArchiveMutation.error as Error | null}
            onChange={(next) => vodArchiveMutation.mutate(next)}
          />
          <LanguageCard />
        </div>
      ) : (
        <>
          <VorratsHinweis pool={postingPlanQuery.data?.pool ?? null} />
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
              title={t('Heute veröffentlicht')}
              value={stats.publishedToday}
              icon={CheckCircle2}
              color="green"
              subValue={t('über alle Plattformen')}
            />
            <KpiCard
              title={t('Manuelle Uploads')}
              value={stats.manualUploads}
              icon={HardDrive}
              color="yellow"
              subValue={t('MP4-Drops aus dem Editor')}
            />
            <KpiCard
              title={t('Nächste Retention')}
              value={formatRetention(stats.nextRetention, t)}
              icon={Clock}
              color="blue"
              subValue={t('14-Tage-Lifecycle')}
            />
          </div>

          <div className="grid grid-cols-1 xl:grid-cols-[minmax(0,1fr)_360px] gap-6">
            <div className="space-y-4">
              {layoutQuery.isLoading ? (
                <div className="panel-card rounded-2xl p-12 flex items-center justify-center">
                  <Loader2 className="w-6 h-6 text-orange animate-spin" />
                </div>
              ) : (
                <LayoutEditor
                  initialLayout={layoutForEditor}
                  isSaving={saveLayoutMutation.isPending}
                  onSave={(layout) => saveLayoutMutation.mutate(layout)}
                  saveLabel={t('Default für {streamer} speichern', { streamer })}
                />
              )}
              {saveLayoutMutation.isError && (
                <div className="text-xs text-danger px-3">
                  {t('Speichern fehlgeschlagen: {message}', {
                    message: (saveLayoutMutation.error as Error).message,
                  })}
                </div>
              )}
              {saveLayoutMutation.isSuccess && (
                <div className="text-xs text-success px-3">{t('Layout gespeichert.')}</div>
              )}
            </div>

            <UploadCard
              streamer={streamer}
              onUpload={(file) => uploadMutation.mutate(file)}
              isUploading={uploadMutation.isPending}
              uploadError={uploadMutation.error as Error | null}
              uploadSuccess={uploadMutation.isSuccess}
            />
          </div>

          <div className="space-y-4">
            <div className="flex flex-wrap items-center gap-3">
              <h3 className="text-lg font-bold text-white inline-flex items-center gap-2">
                <Layers3 className="w-5 h-5 text-orange" /> {t('Pipeline')}
              </h3>
              <StatusFilter value={statusFilter} onChange={setStatusFilter} />
              <div className="ml-auto text-xs text-text-secondary">
                {clipsQuery.isFetching
                  ? t('Aktualisiere…')
                  : t('{count} Treffer', { count: clipsQuery.data?.items.length ?? 0 })}
              </div>
            </div>

            {clipsQuery.isLoading ? (
              <div className="panel-card rounded-2xl p-12 flex items-center justify-center">
                <Loader2 className="w-6 h-6 text-orange animate-spin" />
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
                      editingMode={editingMode}
                      onOpenEditor={(mode) => setEditingClip({ id: clip.clip_db_id, mode })}
                      onCloseEditor={() => setEditingClip(null)}
                      onDiscard={() => {
                        if (window.confirm(t('Clip "{title}" verwerfen?', { title: clip.title }))) {
                          discardMutation.mutate(clip.clip_db_id);
                        }
                      }}
                      onSaveOverride={(layout) => {
                        overrideMutation.mutate({ clipDbId: clip.clip_db_id, layout });
                      }}
                      onResetOverride={() => {
                        overrideMutation.mutate({ clipDbId: clip.clip_db_id, layout: null });
                      }}
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

function SocialHero({ streamer, isDefaultLayout }: { streamer: string; isDefaultLayout: boolean }) {
  const t = useT();
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      className="panel-card rounded-2xl p-6 md:p-8 relative overflow-hidden"
    >
      <div className="absolute -top-20 -right-20 h-72 w-72 rounded-full bg-orange/15 blur-3xl pointer-events-none" />
      <div className="absolute -bottom-24 -left-12 h-72 w-72 rounded-full bg-teal/12 blur-3xl pointer-events-none" />
      <div className="relative flex flex-col md:flex-row md:items-end md:justify-between gap-5">
        <div>
          <div className="inline-flex items-center gap-2 text-[11px] uppercase tracking-[0.18em] font-bold text-orange/90 px-2.5 py-1 rounded-full bg-orange/12 border border-orange/30">
            <Sparkles className="w-3.5 h-3.5" /> {t('Clips automatisch posten')}
          </div>
          <h1 className="display-font font-extrabold text-white mt-3 text-3xl md:text-4xl tracking-tight">
            {t('Social Media für')}{' '}
            <span className="bg-gradient-to-r from-orange to-teal bg-clip-text text-transparent">
              {streamer}
            </span>
          </h1>
          <p className="text-text-secondary mt-2 max-w-2xl text-sm md:text-base">
            {t(
              'Twitch-Clips werden automatisch eingesammelt, vertikal aufbereitet und für YT Shorts / TikTok / Reels vorbereitet. Layouts pro Streamer als Default, pro Clip override-bar, 14-Tage-Retention.',
            )}
          </p>
        </div>
        <div className="flex flex-wrap gap-3 text-xs">
          <HeroBadge tone="orange" icon={Film}>
            {isDefaultLayout ? t('Layout: Repo-Default aktiv') : t('Layout: Streamer-Default')}
          </HeroBadge>
          <HeroBadge tone="teal" icon={Calendar}>
            {t('Phase 3 · Analytics + LLM-Reports')}
          </HeroBadge>
        </div>
      </div>
    </motion.div>
  );
}

function HeroBadge({
  tone,
  icon: Icon,
  children,
}: {
  tone: 'orange' | 'teal';
  icon: React.ComponentType<{ className?: string }>;
  children: React.ReactNode;
}) {
  const cls = tone === 'orange' ? 'bg-orange/12 text-orange border-orange/30' : 'bg-teal/12 text-teal border-teal/35';
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
  const items: Array<{ id: ClipStatus | 'all'; label: string }> = [
    { id: 'pending', label: 'Wartend' },
    { id: 'enriched', label: 'Aufbereitet' },
    { id: 'awaiting_approval', label: 'Freigabe' },
    { id: 'published_all', label: 'Veröffentlicht' },
    { id: 'discarded', label: 'Verworfen' },
    { id: 'all', label: 'Alle' },
  ];
  return (
    <div className="inline-flex flex-wrap rounded-xl border border-border bg-bg/60 p-1 gap-1">
      {items.map((item) => {
        const active = item.id === value;
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onChange(item.id)}
            className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition ${
              active
                ? 'bg-gradient-to-r from-orange/80 to-teal/70 text-white shadow-[0_4px_18px_-6px_rgba(201, 168, 106, 0.45)]'
                : 'text-text-secondary hover:text-white'
            }`}
          >
            {t(item.label)}
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
  uploadError: Error | null;
  uploadSuccess: boolean;
}

function UploadCard({ streamer, onUpload, isUploading, uploadError, uploadSuccess }: UploadCardProps) {
  const t = useT();
  const [dragActive, setDragActive] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const handleFiles = (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const file = files[0];
    if (!file.type.startsWith('video/') && !file.name.toLowerCase().endsWith('.mp4')) {
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
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Upload className="w-4 h-4 text-teal" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('MP4 hochladen')}
        </h3>
      </div>

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
        onClick={() => inputRef.current?.click()}
        className={`relative cursor-pointer rounded-xl border-2 border-dashed p-6 text-center transition ${
          dragActive
            ? 'border-teal bg-teal/10'
            : 'border-border hover:border-teal/50 hover:bg-bg/40'
        }`}
      >
        <input
          ref={inputRef}
          type="file"
          accept="video/mp4,video/*"
          className="hidden"
          onChange={(e) => handleFiles(e.target.files)}
        />
        <Film className="w-8 h-8 text-teal mx-auto mb-2" />
        <p className="text-sm font-bold text-white">{t('MP4 hier ablegen')}</p>
        <p className="text-xs text-text-secondary mt-1">
          {t('oder klicken zum Auswählen · max 200 MB')}
        </p>
        <p className="text-[11px] text-text-secondary mt-3 leading-relaxed">
          {t('Datei wird unter')}{' '}
          <code className="font-mono text-orange">data/clips/uploads/{streamer}/</code>{' '}
          {t('abgelegt und automatisch das Streamer-Default-Layout angewendet.')}
        </p>
      </div>

      {isUploading && (
        <div className="flex items-center gap-2 text-xs text-teal">
          <Loader2 className="w-4 h-4 animate-spin" /> {t('Upload läuft…')}
        </div>
      )}
      {uploadError && (
        <div className="text-xs text-danger">{uploadError.message}</div>
      )}
      {uploadSuccess && !isUploading && (
        <div className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" /> {t('Upload erfolgreich. Clip ist in der Pipeline.')}
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

/** Beschriftung und Erklaerung der drei Freigabe-Modi. */
const APPROVAL_MODE_TEXTE: Record<ApprovalMode, { label: string; hinweis: string }> = {
  manual: {
    label: 'Nur nach Freigabe',
    hinweis: 'Jeder Clip wartet auf dein Okay.',
  },
  veto_window: {
    label: 'Einspruch bis zum Termin',
    hinweis: 'Clips werden eingeplant. Du kannst sie bis zum Posting stoppen.',
  },
  full_auto: {
    label: 'Vollautomatik',
    hinweis: 'Clips gehen ohne Sichtung raus.',
  },
};

/**
 * Freigabe-Modus des Kanals. Eine Entscheidung mit drei Stufen, deshalb
 * Segment-Knoepfe statt Dropdown: alle Optionen sind gleichzeitig sichtbar.
 */
function ApprovalModeCard({
  plan,
  isLoading,
  isSaving,
  error,
  onChange,
}: {
  plan: PostingPlan | null;
  isLoading: boolean;
  isSaving: boolean;
  error: Error | null;
  onChange: (mode: ApprovalMode) => void;
}) {
  const t = useT();
  const modi = plan?.approval_modes ?? (['manual', 'veto_window', 'full_auto'] as ApprovalMode[]);
  const aktiv = plan?.approval_mode ?? 'manual';

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <ShieldAlert className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Freigabe')}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />}
      </div>
      <div className="space-y-2">
        {modi.map((mode) => {
          const texte = APPROVAL_MODE_TEXTE[mode];
          if (!texte) return null;
          const active = aktiv === mode;
          return (
            <button
              key={mode}
              type="button"
              disabled={isLoading || isSaving}
              onClick={() => onChange(mode)}
              className={`w-full text-left rounded-xl border px-4 py-3 disabled:opacity-60 ${
                active
                  ? 'border-primary/60 bg-primary/10'
                  : 'border-border bg-bg/40 hover:border-border-hover'
              }`}
              style={{ transitionProperty: 'border-color, background-color' }}
            >
              <div
                className={`text-sm font-semibold ${active ? 'text-primary' : 'text-white'}`}
              >
                {t(texte.label)}
              </div>
              <div className="text-xs text-text-secondary mt-0.5">{t(texte.hinweis)}</div>
            </button>
          );
        })}
      </div>
      {error && <div className="text-xs text-danger">{error.message}</div>}
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
function PostingScheduleCard({
  plan,
  isLoading,
  isSaving,
  error,
  onChange,
}: {
  plan: PostingPlan | null;
  isLoading: boolean;
  isSaving: boolean;
  error: Error | null;
  onChange: (
    platform: SocialPlatform,
    payload: Partial<Omit<PlatformScheduleEntry, 'platform' | 'next_slot'>>,
  ) => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const locale = language === 'en' ? 'en-GB' : 'de-DE';

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Calendar className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Zeitplan')}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />}
      </div>
      <p className="text-sm text-text-secondary">
        {t('Zeiten gelten in {tz}.', { tz: plan?.timezone ?? 'Europe/Berlin' })}
      </p>

      <div className="space-y-3">
        {(plan?.platforms ?? []).map((eintrag) => {
          const termin = formatTermin(eintrag.next_slot, locale);
          return (
            <div
              key={eintrag.platform}
              className="rounded-xl border border-border bg-bg/40 px-4 py-3 space-y-3"
            >
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-semibold text-white">
                  {PLATFORM_LABELS[eintrag.platform] ?? eintrag.platform}
                </span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={eintrag.auto_post}
                  aria-label={t('Automatisch posten')}
                  disabled={isLoading || isSaving}
                  onClick={() => onChange(eintrag.platform, { auto_post: !eintrag.auto_post })}
                  className={`relative inline-flex h-7 w-12 shrink-0 rounded-full border disabled:opacity-60 ${
                    eintrag.auto_post
                      ? 'border-primary/60 bg-primary/30'
                      : 'border-border bg-bg/60'
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

              {eintrag.auto_post && (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    <label className="block">
                      <span className="text-xs text-text-secondary">{t('Posts pro Woche')}</span>
                      <input
                        type="number"
                        min={0}
                        max={70}
                        defaultValue={eintrag.posts_per_week}
                        disabled={isSaving}
                        onBlur={(event) => {
                          const wert = Number(event.target.value);
                          if (!Number.isFinite(wert) || wert === eintrag.posts_per_week) return;
                          onChange(eintrag.platform, { posts_per_week: wert });
                        }}
                        className="mt-1 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs text-text-secondary">{t('Höchstens pro Tag')}</span>
                      <input
                        type="number"
                        min={0}
                        max={10}
                        defaultValue={eintrag.max_posts_per_day}
                        disabled={isSaving}
                        onBlur={(event) => {
                          const wert = Number(event.target.value);
                          if (!Number.isFinite(wert) || wert === eintrag.max_posts_per_day) return;
                          onChange(eintrag.platform, { max_posts_per_day: wert });
                        }}
                        className="mt-1 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white"
                      />
                    </label>
                  </div>
                  <label className="block">
                    <span className="text-xs text-text-secondary">
                      {t('Uhrzeiten, mit Komma getrennt')}
                    </span>
                    <input
                      type="text"
                      defaultValue={eintrag.post_times.join(', ')}
                      placeholder="18:00, 21:00"
                      disabled={isSaving}
                      onBlur={(event) => {
                        const zeiten = event.target.value
                          .split(',')
                          .map((wert) => wert.trim())
                          .filter(Boolean);
                        if (zeiten.join(',') === eintrag.post_times.join(',')) return;
                        onChange(eintrag.platform, { post_times: zeiten });
                      }}
                      className="mt-1 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white"
                    />
                  </label>
                  {termin && (
                    <div className="text-xs text-text-secondary">
                      {t('Nächster Post: {termin}', { termin })}
                    </div>
                  )}
                </>
              )}
            </div>
          );
        })}
      </div>
      {error && <div className="text-xs text-danger">{error.message}</div>}
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
  isSaving,
  error,
  onChange,
}: {
  plan: PostingPlan | null;
  isLoading: boolean;
  isSaving: boolean;
  error: Error | null;
  onChange: (categoryKey: string, autoPost: boolean) => void;
}) {
  const t = useT();

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Gamepad2 className="w-4 h-4 text-primary" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Kategorien')}
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-primary animate-spin ml-auto" />}
      </div>
      <div className="space-y-2">
        {(plan?.categories ?? []).map((kategorie) => (
          <label
            key={kategorie.category_key}
            className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3"
          >
            <span>
              <span className="block text-sm font-semibold text-white">
                {kategorie.display_name}
              </span>
              <span className="block text-xs text-text-secondary mt-0.5">
                {kategorie.enrichment_enabled
                  ? t('Mit Titel- und Hashtag-Vorschlägen.')
                  : t('Ohne Vorschläge, Clip geht so raus.')}
              </span>
            </span>
            <input
              type="checkbox"
              checked={kategorie.auto_post}
              disabled={isLoading || isSaving}
              onChange={(event) => onChange(kategorie.category_key, event.target.checked)}
              className="h-4 w-4 shrink-0 accent-primary"
            />
          </label>
        ))}
      </div>
      {error && <div className="text-xs text-danger">{error.message}</div>}
    </div>
  );
}

/**
 * Vorratswarnung. Steht bewusst als eigene betonte Zeile ueber dem Clip-Pool
 * und nicht kleingedruckt in einer Karte: wer keinen Nachschub liefert, hoert
 * irgendwann auf zu posten, ohne es zu merken.
 */
function VorratsHinweis({ pool }: { pool: ClipPoolForecast | null }) {
  const t = useT();
  if (!pool || pool.aktive_plattformen === 0) return null;

  const knapp = pool.warnung;
  return (
    <div
      className={`rounded-xl border px-4 py-3 flex items-center gap-3 ${
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
const PLATFORM_LABELS: Record<string, string> = {
  youtube: 'YouTube',
  tiktok: 'TikTok',
  instagram: 'Instagram',
};

/// Verbindungen zu den Plattformen. Der OAuth-Flow ist ein Redirect auf den
/// Anbieter, deshalb ist "Verbinden" ein Link und kein fetch.
function PlatformConnectionsCard({
  streamer,
  platforms,
  isLoading,
  onDisconnect,
  isDisconnecting,
}: {
  streamer: string;
  platforms: PlatformStatus[];
  isLoading: boolean;
  onDisconnect: (platform: string) => void;
  isDisconnecting: boolean;
}) {
  const t = useT();
  const known = ['youtube', 'tiktok', 'instagram'];
  const byName = new Map(platforms.map((p) => [p.platform, p]));

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <ExternalLink className="w-4 h-4 text-orange" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          {t('Verbindungen')}
        </h3>
        {isLoading && <Loader2 className="w-4 h-4 text-orange animate-spin ml-auto" />}
      </div>

      <div className="space-y-2">
        {known.map((platform) => {
          const status = byName.get(platform);
          const connected = status?.connected ?? false;
          return (
            <div
              key={platform}
              className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <div className="text-sm font-semibold text-white">
                  {PLATFORM_LABELS[platform] ?? platform}
                </div>
                <div className="text-xs text-text-secondary truncate">
                  {/* Der Ablauf des Zugangs wird selbst nachgezogen und ist
                      deshalb keine Meldung wert. */}
                  {connected ? (status?.username ?? t('verbunden')) : t('nicht verbunden')}
                </div>
              </div>
              {connected ? (
                <button
                  type="button"
                  disabled={isDisconnecting}
                  onClick={() => onDisconnect(platform)}
                  className="rounded-xl border border-border px-3 py-1.5 text-sm font-semibold text-text-secondary hover:text-white disabled:opacity-40"
                >
                  {t('Trennen')}
                </button>
              ) : (
                <a
                  href={oauthStartUrl(platform, streamer)}
                  className="rounded-xl border border-orange bg-orange/15 px-3 py-1.5 text-sm font-semibold text-white"
                >
                  {t('Verbinden')}
                </a>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function VodArchiveCard({
  streamer,
  settings,
  isLoading,
  isSaving,
  error,
  onChange,
}: {
  streamer: string;
  settings: VodArchiveSettings | null;
  isLoading: boolean;
  isSaving: boolean;
  error: Error | null;
  onChange: (next: Pick<VodArchiveSettings, 'enabled' | 'privacy'>) => void;
}) {
  const t = useT();
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
      <label className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3">
        <span className="text-sm font-semibold text-white">{t('Automatisch sichern')}</span>
        <input
          type="checkbox"
          checked={enabled}
          disabled={isLoading || isSaving}
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
              disabled={isLoading || isSaving || settings?.privacy_forced}
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

      {error && <div className="text-xs text-danger">{error.message}</div>}
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
  clip: SocialClip;
  editingMode: EditMode | null;
  onOpenEditor: (mode: EditMode) => void;
  onCloseEditor: () => void;
  onDiscard: () => void;
  onSaveOverride: (layout: LayoutPayload) => void;
  onResetOverride: () => void;
  onApprovalDecision: (decision: 'approve' | 'skip' | 'edit', platforms: SocialPlatform[]) => void;
  approvalPending: boolean;
}

function ClipCard({
  clip,
  editingMode,
  onOpenEditor,
  onCloseEditor,
  onDiscard,
  onSaveOverride,
  onResetOverride,
  onApprovalDecision,
  approvalPending,
}: ClipCardProps) {
  const { t, locale } = useLanguage();
  const status = STATUS_LABELS[clip.status] ?? STATUS_LABELS.pending;
  const sourceLabel = clip.source_kind === 'manual_upload' ? t('Upload') : t('Twitch');
  const enrichmentTopHashtags = clip.enrichment_summary?.top_hashtags ?? [];
  const enrichmentStatus = clip.enrichment_status;
  const [selectedPlatforms, setSelectedPlatforms] = useState<SocialPlatform[]>(
    clip.approval?.approved_platforms ?? [],
  );

  useEffect(() => {
    setSelectedPlatforms(clip.approval?.approved_platforms ?? []);
  }, [clip.approval?.approved_platforms, clip.clip_db_id]);

  const togglePlatform = (platform: SocialPlatform, checked: boolean) => {
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
      className="panel-card card-glow rounded-2xl overflow-hidden flex flex-col"
    >
      <div className="relative aspect-video bg-bg overflow-hidden">
        {clip.thumbnail_url ? (
          <img
            src={clip.thumbnail_url}
            alt={clip.title}
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-text-secondary">
            <Film className="w-10 h-10" />
          </div>
        )}
        <div className="absolute top-2 left-2 flex items-center gap-2">
          <span className={`text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border ${TONE_BADGE[status.tone]}`}>
            {t(status.label)}
          </span>
          <span className="text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border bg-bg/70 text-white border-border">
            {sourceLabel}
          </span>
        </div>
        <div className="absolute bottom-2 right-2 inline-flex items-center gap-1.5 text-[10px] font-mono text-white/90 bg-black/55 px-1.5 py-0.5 rounded">
          <Clock className="w-3 h-3" /> {formatRetention(clip.retention_until, t)}
        </div>
      </div>

      <div className="p-4 flex flex-col gap-3 flex-1">
        <div className="space-y-1">
          <h4 className="font-bold text-white line-clamp-2">{clip.title}</h4>
          <p className="text-xs text-text-secondary">
            {clip.streamer_login} · {(clip.duration_seconds ?? 0).toFixed(0)}s ·{' '}
            {t('{views} Views', { views: (clip.view_count ?? 0).toLocaleString(locale) })}
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
                className="text-[10px] font-semibold px-1.5 py-0.5 rounded-md bg-teal/10 text-teal border border-teal/30"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}

        <div className="rounded-xl border border-border bg-bg/30 p-3 space-y-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-[11px] uppercase tracking-[0.14em] font-bold text-orange">
                {t('Approval')}
              </p>
              <p className="text-xs text-text-secondary">
                {clip.approval?.state
                  ? t('Status: {state}', { state: clip.approval.state })
                  : t('Wird nach abgeschlossenem Enrichment per DM freigegeben.')}
              </p>
            </div>
            {approvalPending && <Loader2 className="w-4 h-4 text-orange animate-spin" />}
          </div>
          <div className="grid grid-cols-3 gap-2">
            {([
              ['youtube', 'YT'],
              ['tiktok', 'TT'],
              ['instagram', 'IG'],
            ] as const).map(([platform, label]) => (
              <label
                key={platform}
                className="inline-flex items-center justify-center gap-2 rounded-lg border border-border bg-bg/40 px-2 py-2 text-xs font-semibold text-white"
              >
                <input
                  type="checkbox"
                  checked={selectedPlatforms.includes(platform)}
                  onChange={(event) => togglePlatform(platform, event.target.checked)}
                  className="h-3.5 w-3.5 accent-orange"
                />
                {label}
              </label>
            ))}
          </div>
          <div className="grid grid-cols-3 gap-2">
            <button
              type="button"
              onClick={() => onApprovalDecision('approve', selectedPlatforms)}
              disabled={approvalPending}
              className="inline-flex items-center justify-center gap-1.5 text-xs font-bold px-3 py-2 rounded-lg bg-success/15 text-success border border-success/30 hover:bg-success/20 disabled:opacity-50"
            >
              <CheckCircle2 className="w-3.5 h-3.5" /> {t('Posten')}
            </button>
            <button
              type="button"
              onClick={() => {
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
              <Trash2 className="w-3.5 h-3.5" /> {t('Skip')}
            </button>
          </div>
        </div>

        <div className="flex items-center gap-2 mt-auto pt-2">
          {clip.clip_url && (
            <a
              href={clip.clip_url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1.5 text-xs text-text-secondary hover:text-white"
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
            onClick={() => onOpenEditor('enrichment')}
            className={`inline-flex items-center gap-1.5 text-xs font-bold px-3 py-1.5 rounded-lg border transition ${
              editingMode === 'enrichment'
                ? 'bg-teal/25 text-teal border-teal/50'
                : 'bg-teal/10 text-teal border-teal/30 hover:bg-teal/20'
            }`}
          >
            <Wand2 className="w-3.5 h-3.5" /> {t('Metadaten')}
            {enrichmentStatus && enrichmentStatus !== 'done' && (
              <span className="text-[9px] uppercase tracking-[0.14em] opacity-80">
                · {enrichmentStatus}
              </span>
            )}
          </button>
          <button
            type="button"
            onClick={() => onOpenEditor('layout')}
            className={`inline-flex items-center gap-1.5 text-xs font-bold px-3 py-1.5 rounded-lg border transition ${
              editingMode === 'layout'
                ? 'bg-orange/25 text-orange border-orange/50'
                : 'bg-orange/15 text-orange border-orange/30 hover:bg-orange/25'
            }`}
          >
            <Pencil className="w-3.5 h-3.5" /> {t('Layout')}
          </button>
        </div>
      </div>

      {editingMode === 'layout' && (
        <div className="border-t border-border p-4 bg-bg/30">
          <LayoutEditor
            initialLayout={clip.effective_layout}
            saveLabel={t('Override speichern')}
            resetLabel={t('Schließen')}
            onSave={(layout) => {
              onSaveOverride(layout);
              onCloseEditor();
            }}
            onReset={onCloseEditor}
          />
          {clip.layout_override && (
            <div className="mt-3 flex justify-end">
              <button
                type="button"
                onClick={() => {
                  if (window.confirm(t('Override entfernen und Streamer-Default verwenden?'))) {
                    onResetOverride();
                    onCloseEditor();
                  }
                }}
                className="text-xs text-text-secondary hover:text-white"
              >
                {t('Override entfernen → Streamer-Default')}
              </button>
            </div>
          )}
        </div>
      )}

      {editingMode === 'enrichment' && (
        <div className="border-t border-border p-4 bg-bg/30">
          <EnrichmentPanel clipDbId={clip.clip_db_id} onClose={onCloseEditor} />
        </div>
      )}
    </motion.div>
  );
}
