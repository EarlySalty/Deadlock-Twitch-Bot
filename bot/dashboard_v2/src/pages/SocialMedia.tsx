import { useEffect, useMemo, useRef, useState, type DragEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import {
  AlertCircle,
  BarChart3,
  CheckCircle2,
  Clock,
  Cog,
  Film,
  HardDrive,
  Loader2,
  ShieldAlert,
  Sparkles,
  Trash2,
  Upload,
  Layers3,
  Calendar,
  ExternalLink,
  Pencil,
  Wand2,
} from 'lucide-react';
import { KpiCard } from '@/components/cards/KpiCard';
import { AnalyticsTab } from '@/components/socialmedia/AnalyticsTab';
import { LayoutEditor } from '@/components/socialmedia/LayoutEditor';
import { EnrichmentPanel } from '@/components/socialmedia/EnrichmentPanel';
import {
  decideClipApproval,
  SocialMediaForbiddenError,
  discardClip,
  fetchAutoApproveSettings,
  fetchClips,
  fetchStreamerLayout,
  saveStreamerLayout,
  saveAutoApproveSettings,
  setClipLayoutOverride,
  uploadClip,
} from '@/api/socialMedia';
import {
  type AutoApproveSettings,
  DEFAULT_LAYOUT,
  type ClipStatus,
  type LayoutPayload,
  type SocialClip,
  type SocialPlatform,
  type StreamerLayoutResponse,
} from '@/types/socialMedia';

interface SocialMediaProps {
  streamer: string;
}

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

function formatRetention(retentionUntil: string | null): string {
  if (!retentionUntil) return '—';
  const target = new Date(retentionUntil);
  const now = new Date();
  const ms = target.getTime() - now.getTime();
  const days = Math.floor(ms / (1000 * 60 * 60 * 24));
  if (days < 0) return 'überfällig';
  if (days === 0) return 'heute';
  if (days === 1) return 'morgen';
  return `${days} Tage`;
}

type EditMode = 'layout' | 'enrichment';
type SocialMediaView = 'pipeline' | 'analytics';

export function SocialMedia({ streamer }: SocialMediaProps) {
  const queryClient = useQueryClient();
  const [statusFilter, setStatusFilter] = useState<ClipStatus | 'all'>('pending');
  const [editingClip, setEditingClip] = useState<{ id: number; mode: EditMode } | null>(null);
  const [activeView, setActiveView] = useState<SocialMediaView>('pipeline');

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

  const autoApproveQuery = useQuery<AutoApproveSettings, Error>({
    queryKey: ['social-media', 'auto-approve-settings'],
    queryFn: () => fetchAutoApproveSettings(),
    enabled: !!streamer,
    retry: (failureCount, err) => {
      if (err instanceof SocialMediaForbiddenError) return false;
      return failureCount < 2;
    },
  });

  const isForbidden =
    layoutQuery.error instanceof SocialMediaForbiddenError ||
    clipsQuery.error instanceof SocialMediaForbiddenError ||
    autoApproveQuery.error instanceof SocialMediaForbiddenError;

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

  const autoApproveMutation = useMutation({
    mutationFn: (payload: AutoApproveSettings) => saveAutoApproveSettings(payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['social-media', 'auto-approve-settings'] });
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
        <h2 className="text-2xl font-bold text-white mb-2">Admin-Zugang erforderlich</h2>
        <p className="text-text-secondary">
          Das Social-Media-Dashboard ist aktuell nur für Admins freigeschaltet. Partner-Streamer
          erhalten Zugriff, sobald die Pipeline ausreichend validiert wurde.
        </p>
      </div>
    );
  }

  if (!streamer) {
    return (
      <div className="panel-card rounded-2xl p-12 text-center max-w-2xl mx-auto mt-12">
        <Film className="w-12 h-12 text-text-secondary mx-auto mb-4" />
        <h2 className="text-2xl font-bold text-white mb-2">Streamer auswählen</h2>
        <p className="text-text-secondary">
          Wähle oben einen Streamer aus, um Layouts, Clips und Uploads zu verwalten.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <SocialHero streamer={streamer} isDefaultLayout={layoutQuery.data?.is_default ?? false} />

      <div className="inline-flex flex-wrap rounded-2xl border border-border bg-bg/60 p-1.5 gap-1.5">
        {[
          { id: 'pipeline' as const, label: 'Pipeline', Icon: Layers3 },
          { id: 'analytics' as const, label: 'Analytics', Icon: BarChart3 },
        ].map(({ id, label, Icon }) => {
          const active = activeView === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => setActiveView(id)}
              className={`inline-flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-semibold transition ${
                active
                  ? 'bg-gradient-to-r from-orange/85 to-teal/70 text-white shadow-[0_6px_24px_-10px_rgba(255,122,24,0.45)]'
                  : 'text-text-secondary hover:text-white'
              }`}
            >
              <Icon className="w-4 h-4" />
              {label}
            </button>
          );
        })}
      </div>

      {activeView === 'analytics' ? (
        <AnalyticsTab streamer={streamer} clips={clipsQuery.data?.items ?? []} />
      ) : (
        <>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
            <KpiCard
              title="Clips in Pipeline"
              value={stats.total}
              icon={Film}
              color="purple"
              subValue={statusFilter === 'all' ? 'alle Stati' : STATUS_LABELS[statusFilter]?.label}
            />
            <KpiCard
              title="Heute veröffentlicht"
              value={stats.publishedToday}
              icon={CheckCircle2}
              color="green"
              subValue="über alle Plattformen"
            />
            <KpiCard
              title="Manuelle Uploads"
              value={stats.manualUploads}
              icon={HardDrive}
              color="yellow"
              subValue="MP4-Drops aus dem Editor"
            />
            <KpiCard
              title="Nächste Retention"
              value={formatRetention(stats.nextRetention)}
              icon={Clock}
              color="blue"
              subValue="14-Tage-Lifecycle"
            />
          </div>

          <AutoApproveCard
            settings={autoApproveQuery.data ?? { youtube: false, tiktok: false, instagram: false }}
            isLoading={autoApproveQuery.isLoading}
            isSaving={autoApproveMutation.isPending}
            error={autoApproveMutation.error as Error | null}
            onChange={(nextSettings) => autoApproveMutation.mutate(nextSettings)}
          />

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
                  saveLabel={`Default für ${streamer} speichern`}
                />
              )}
              {saveLayoutMutation.isError && (
                <div className="text-xs text-danger px-3">
                  Speichern fehlgeschlagen: {(saveLayoutMutation.error as Error).message}
                </div>
              )}
              {saveLayoutMutation.isSuccess && (
                <div className="text-xs text-success px-3">Layout gespeichert.</div>
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
                <Layers3 className="w-5 h-5 text-orange" /> Pipeline
              </h3>
              <StatusFilter value={statusFilter} onChange={setStatusFilter} />
              <div className="ml-auto text-xs text-text-secondary">
                {clipsQuery.isFetching ? 'Aktualisiere…' : `${clipsQuery.data?.items.length ?? 0} Treffer`}
              </div>
            </div>

            {clipsQuery.isLoading ? (
              <div className="panel-card rounded-2xl p-12 flex items-center justify-center">
                <Loader2 className="w-6 h-6 text-orange animate-spin" />
              </div>
            ) : (clipsQuery.data?.items ?? []).length === 0 ? (
              <div className="panel-card rounded-2xl p-12 text-center">
                <AlertCircle className="w-10 h-10 text-text-secondary mx-auto mb-3" />
                <p className="text-white font-bold mb-1">Keine Clips für diesen Filter</p>
                <p className="text-sm text-text-secondary">
                  Sobald neue Twitch-Clips eingehen oder du eine MP4 hochlädst, erscheinen sie hier.
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
                        if (window.confirm(`Clip "${clip.title}" verwerfen?`)) {
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
            <Sparkles className="w-3.5 h-3.5" /> Admin-Tooling · Social Media 2.0
          </div>
          <h1 className="display-font font-extrabold text-white mt-3 text-3xl md:text-4xl tracking-tight">
            Cross-Posting-Pipeline für{' '}
            <span className="bg-gradient-to-r from-orange to-teal bg-clip-text text-transparent">
              {streamer}
            </span>
          </h1>
          <p className="text-text-secondary mt-2 max-w-2xl text-sm md:text-base">
            Twitch-Clips werden automatisch eingesammelt, vertikal aufbereitet und für YT Shorts /
            TikTok / Reels vorbereitet. Layouts pro Streamer als Default, pro Clip override-bar,
            14-Tage-Retention.
          </p>
        </div>
        <div className="flex flex-wrap gap-3 text-xs">
          <HeroBadge tone="orange" icon={Film}>
            {isDefaultLayout ? 'Layout: Repo-Default aktiv' : 'Layout: Streamer-Default'}
          </HeroBadge>
          <HeroBadge tone="teal" icon={Calendar}>
            Phase 3 · Analytics + LLM-Reports
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
                ? 'bg-gradient-to-r from-orange/80 to-teal/70 text-white shadow-[0_4px_18px_-6px_rgba(255,122,24,0.45)]'
                : 'text-text-secondary hover:text-white'
            }`}
          >
            {item.label}
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
  const [dragActive, setDragActive] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const handleFiles = (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const file = files[0];
    if (!file.type.startsWith('video/') && !file.name.toLowerCase().endsWith('.mp4')) {
      alert('Bitte eine MP4-Datei wählen.');
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
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">MP4 hochladen</h3>
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
        <p className="text-sm font-bold text-white">MP4 hier ablegen</p>
        <p className="text-xs text-text-secondary mt-1">oder klicken zum Auswählen · max 200 MB</p>
        <p className="text-[11px] text-text-secondary mt-3 leading-relaxed">
          Datei wird unter <code className="font-mono text-orange">data/clips/uploads/{streamer}/</code> abgelegt
          und automatisch das Streamer-Default-Layout angewendet.
        </p>
      </div>

      {isUploading && (
        <div className="flex items-center gap-2 text-xs text-teal">
          <Loader2 className="w-4 h-4 animate-spin" /> Upload läuft…
        </div>
      )}
      {uploadError && (
        <div className="text-xs text-danger">{uploadError.message}</div>
      )}
      {uploadSuccess && !isUploading && (
        <div className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" /> Upload erfolgreich. Clip ist in der Pipeline.
        </div>
      )}

      <div className="border-t border-border pt-3 space-y-2 text-[11px] text-text-secondary">
        <div className="flex items-center gap-1.5">
          <Calendar className="w-3 h-3" /> Retention: 14 Tage ab Erstellung
        </div>
        <div className="flex items-center gap-1.5">
          <Layers3 className="w-3 h-3" /> Auto-Apply: Streamer-Default-Layout
        </div>
      </div>
    </div>
  );
}

function AutoApproveCard({
  settings,
  isLoading,
  isSaving,
  error,
  onChange,
}: {
  settings: AutoApproveSettings;
  isLoading: boolean;
  isSaving: boolean;
  error: Error | null;
  onChange: (next: AutoApproveSettings) => void;
}) {
  const updateSetting = (platform: keyof AutoApproveSettings, checked: boolean) => {
    onChange({
      ...settings,
      [platform]: checked,
    });
  };

  return (
    <div className="panel-card rounded-2xl p-5 space-y-4">
      <div className="flex items-center gap-2">
        <Cog className="w-4 h-4 text-orange" />
        <h3 className="text-sm font-bold text-white uppercase tracking-[0.14em]">
          Auto-Approve
        </h3>
        {isSaving && <Loader2 className="w-4 h-4 text-orange animate-spin ml-auto" />}
      </div>
      <p className="text-sm text-text-secondary">
        Plattformen mit aktivem Toggle werden nach einer Freigabe automatisch mit in die Queue
        gelegt, auch wenn im Approval-DM kein Häkchen gesetzt wurde.
      </p>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
        {([
          ['youtube', 'YouTube Shorts'],
          ['tiktok', 'TikTok'],
          ['instagram', 'Instagram Reels'],
        ] as const).map(([platform, label]) => (
          <label
            key={platform}
            className="rounded-xl border border-border bg-bg/40 px-4 py-3 flex items-center justify-between gap-3"
          >
            <span className="text-sm font-semibold text-white">{label}</span>
            <input
              type="checkbox"
              checked={settings[platform]}
              disabled={isLoading || isSaving}
              onChange={(event) => updateSetting(platform, event.target.checked)}
              className="h-4 w-4 accent-orange"
            />
          </label>
        ))}
      </div>
      {error && <div className="text-xs text-danger">{error.message}</div>}
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
  const status = STATUS_LABELS[clip.status] ?? STATUS_LABELS.pending;
  const sourceLabel = clip.source_kind === 'manual_upload' ? 'Upload' : 'Twitch';
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
            {status.label}
          </span>
          <span className="text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border bg-bg/70 text-white border-border">
            {sourceLabel}
          </span>
        </div>
        <div className="absolute bottom-2 right-2 inline-flex items-center gap-1.5 text-[10px] font-mono text-white/90 bg-black/55 px-1.5 py-0.5 rounded">
          <Clock className="w-3 h-3" /> {formatRetention(clip.retention_until)}
        </div>
      </div>

      <div className="p-4 flex flex-col gap-3 flex-1">
        <div className="space-y-1">
          <h4 className="font-bold text-white line-clamp-2">{clip.title}</h4>
          <p className="text-xs text-text-secondary">
            {clip.streamer_login} · {(clip.duration_seconds ?? 0).toFixed(0)}s · {(clip.view_count ?? 0).toLocaleString('de-DE')} Views
          </p>
          {clip.layout_override && (
            <p className="text-[11px] text-orange inline-flex items-center gap-1">
              <Pencil className="w-3 h-3" /> Override aktiv
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
                Approval
              </p>
              <p className="text-xs text-text-secondary">
                {clip.approval?.state
                  ? `Status: ${clip.approval.state}`
                  : 'Wird nach abgeschlossenem Enrichment per DM freigegeben.'}
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
              <CheckCircle2 className="w-3.5 h-3.5" /> Posten
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
              <Pencil className="w-3.5 h-3.5" /> Bearbeiten
            </button>
            <button
              type="button"
              onClick={() => onApprovalDecision('skip', selectedPlatforms)}
              disabled={approvalPending}
              className="inline-flex items-center justify-center gap-1.5 text-xs font-bold px-3 py-2 rounded-lg bg-danger/12 text-danger border border-danger/30 hover:bg-danger/20 disabled:opacity-50"
            >
              <Trash2 className="w-3.5 h-3.5" /> Skip
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
              <ExternalLink className="w-3.5 h-3.5" /> Original
            </a>
          )}
          <button
            type="button"
            onClick={onDiscard}
            disabled={clip.status === 'discarded' || !!clip.discarded_at}
            className="ml-auto inline-flex items-center gap-1.5 text-xs font-semibold text-danger hover:text-danger px-2 py-1.5 rounded-lg hover:bg-danger/10 transition disabled:opacity-30"
          >
            <Trash2 className="w-3.5 h-3.5" /> Verwerfen
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
            <Wand2 className="w-3.5 h-3.5" /> Metadaten
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
            <Pencil className="w-3.5 h-3.5" /> Layout
          </button>
        </div>
      </div>

      {editingMode === 'layout' && (
        <div className="border-t border-border p-4 bg-bg/30">
          <LayoutEditor
            initialLayout={clip.effective_layout}
            saveLabel="Override speichern"
            resetLabel="Schließen"
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
                  if (window.confirm('Override entfernen und Streamer-Default verwenden?')) {
                    onResetOverride();
                    onCloseEditor();
                  }
                }}
                className="text-xs text-text-secondary hover:text-white"
              >
                Override entfernen → Streamer-Default
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
