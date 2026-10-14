import { StatusChip } from './StatusChip';
import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckCircle2, Archive, Film, Loader2, ExternalLink, Pencil, XCircle, Clapperboard, Crop, MoreHorizontal, Trophy } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';

import { VideoFullscreenButton } from '@/components/socialmedia/VideoFullscreenButton';

import { formatTerminInZone, queueStage } from '@/components/socialmedia/queuePresentation';
import '@/components/socialmedia/studio.css';

import { type SocialClipMitPosting, requestPreview, getPreviewStatus, previewFileUrl, submitClipToContest } from '@/api/socialMedia';
import { fehlerText, uploadWaitText, PLATFORM_LABELS, STATUS_LABELS } from '@/components/socialmedia/labels';
import { type SocialPlatform } from '@/types/socialMedia';

type Translate = (text: string, params?: Record<string, string | number>) => string;

const PLATTFORMEN: SocialPlatform[] = ['youtube', 'tiktok', 'instagram'];

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

interface ClipCardProps {
  clip: SocialClipMitPosting;
  displayTitle?: string;
  now: number;
  onOpenPlatformStatus: () => void;
  onOpenAiEditor: () => void;
  
  timezone: string;
  
  defaultPlatforms: SocialPlatform[];
  availablePlatforms: SocialPlatform[];
  onOpenConnections: () => void;
  onOpenEditor: (mode: EditMode) => void;
  onDiscard: () => void;
  onApprovalDecision: (decision: 'approve' | 'skip' | 'edit', platforms: SocialPlatform[]) => void;
  approvalPending: boolean;
  onCancelScheduled: () => void;
  cancelPending: boolean;
  cancelResult: { cancelled: number; already_running: number } | null;
  nichtEingeplant: SocialPlatform[];
  
  fehler: unknown;
}

export function ClipCard({
  clip,
  displayTitle,
  now,
  onOpenPlatformStatus,
  onOpenAiEditor,
  timezone,
  defaultPlatforms,
  availablePlatforms,
  onOpenConnections,
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
  const canDecide = clip.status !== 'approved' && queueStage(clip) === 'review';
  const termine = PLATTFORMEN.flatMap((platform) =>
    clip.scheduled_at?.[platform] && !clip.platform_status[platform] && !['inbox', 'inbox_pending', 'waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.[platform] ?? '')
      ? [{ platform, zeit: clip.scheduled_at[platform] as string }]
      : [],
  );
  const waitingPlatforms = PLATTFORMEN.filter((platform) => clip.upload_states?.[platform] === 'waiting_connection');
  const stoppbar = clip.status === 'approved' && (termine.length > 0 || waitingPlatforms.length > 0
    || ['waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.tiktok ?? ''));
  const terminal =
    clip.status === 'discarded' || clip.status === 'skipped' || Boolean(clip.discarded_at);
  const uploadFehler = PLATTFORMEN.flatMap((platform) =>
    clip.upload_errors?.[platform] && !['waiting_connection', 'waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.[platform] ?? '') ? [{ platform, text: clip.upload_errors[platform] }] : [],
  );
  const tiktokInbox = clip.upload_states?.tiktok === 'inbox';
  const tiktokPending = clip.upload_states?.tiktok === 'inbox_pending';
  const initialPlatforms = clip.approval?.approved_platforms?.length
    ? clip.approval.approved_platforms
    : defaultPlatforms;
  const [selectedPlatforms, setSelectedPlatforms] = useState<SocialPlatform[]>(initialPlatforms);
  const eligiblePlatforms = selectedPlatforms.filter((platform) => availablePlatforms.includes(platform));
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
  const videoRef = useRef<HTMLVideoElement>(null);
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

  const contest = useMutation({
    mutationFn: () => submitClipToContest(clip.clip_db_id),
  });
  const rendering =
    render.isPending || preview.data?.status === 'pending' || preview.data?.status === 'rendering';
  const ready = preview.data?.status === 'ready';
  const fehlerZeile = fehlerText(fehler, t);
  const runMenu = (action: () => void) => {
    const trigger = menu.current?.querySelector('summary');
    menu.current?.removeAttribute('open');
    trigger?.focus();
    action();
  };
  return (
    <article className="studio-clip" aria-label={displayTitle || clip.title}>
      <div className="studio-clip-row">
        <div className="studio-clip-media">
          {ready ? (
            <video
              ref={videoRef}
              controls
              preload="metadata"
              src={previewFileUrl(clip.clip_db_id)}
              aria-label={displayTitle || clip.title}
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
          {ready && <VideoFullscreenButton videoRef={videoRef} />}
          <StatusChip tone={queueStage(clip) === 'failed' ? 'danger' : queueStage(clip) === 'planned' ? 'info' : queueStage(clip) === 'posted' ? 'success' : queueStage(clip) === 'review' ? 'warning' : 'neutral'}>
            {waitingPlatforms.length > 0
              ? t('Wartet auf Verbindung')
              : stoppbar
                ? t('Geplant')
                : tiktokInbox && !clip.platform_status.youtube && !clip.platform_status.instagram
                  ? t('Im TikTok-Postfach')
                  : clip.youtube_visibility === 'private' && clip.platform_status.youtube && !clip.platform_status.tiktok && !clip.platform_status.instagram
                    ? t('Privat auf YouTube')
                    : t(status.label)}
          </StatusChip>
          <h3 className="break-words text-sm font-semibold leading-6">
            {displayTitle || clip.title || t('Clip ohne Titel')}
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
                    checked={eligiblePlatforms.includes(platform)}
                    disabled={!availablePlatforms.includes(platform)}
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
              {eligiblePlatforms.length === 0 && (
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
          {tiktokInbox && (
            <p role="status" className="text-sm text-accent">
              {t('TikTok: Der Clip liegt in deinem Postfach. Öffne TikTok, bearbeite ihn und veröffentliche ihn dort.')}
            </p>
          )}
          {tiktokPending && (
            <p role="status" className="text-sm text-text-secondary">
              {t('Die TikTok-Übertragung ist noch nicht bestätigt. Wir prüfen den Vorgang weiter; ein zweiter Upload bleibt gesperrt.')}
            </p>
          )}
          {['waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.tiktok ?? '') && (
            <p role="status" className="text-sm text-warning">
              {clip.upload_errors?.tiktok}
            </p>
          )}
          {waitingPlatforms.length > 0 && (
            <div role="status" className="space-y-2 text-sm text-warning">
              {waitingPlatforms.map((platform) => (
                <p key={platform}>{uploadWaitText(clip.upload_errors?.[platform], platform, t)}</p>
              ))}
              <button type="button" className="studio-button" onClick={onOpenConnections}>{t('Verbindungen öffnen')}</button>
            </div>
          )}
          {clip.platform_status.youtube && (
            <p role="status" className="text-sm text-text-secondary">
              {clip.youtube_visibility === 'private'
                ? t('YouTube hat dieses Video privat gespeichert. Es ist nicht öffentlich sichtbar.')
                : clip.youtube_visibility === 'unlisted'
                  ? t('YouTube: Nicht gelistet. Das Video ist über den Link erreichbar.')
                  : clip.youtube_visibility === 'public'
                    ? t('YouTube: Öffentlich sichtbar.')
                    : t('YouTube: Upload abgeschlossen, Sichtbarkeit noch nicht bestätigt.')}
            </p>
          )}
          {clip.publish_states?.tiktok && (
            <p role="status" className="text-sm text-text-secondary">
              {t(({
                PROCESSING_UPLOAD: 'TikTok: Die Videodatei wird verarbeitet.',
                PROCESSING_DOWNLOAD: 'TikTok: Die Videodatei wird geladen.',
                PUBLISH_COMPLETE: 'TikTok: Veröffentlichung bestätigt.',
                FAILED: 'TikTok: Veröffentlichung abgelehnt.',
                STATUS_UNAVAILABLE: 'TikTok: Bestätigung steht noch aus.',
                SEND_TO_USER_INBOX: 'TikTok: Frühere Übertragung ins Postfach.',
              } as Record<string, string>)[clip.publish_states.tiktok] ?? 'TikTok: Bestätigung steht noch aus.')}
            </p>
          )}
          {clip.layout_override && <p className="text-xs text-accent">{t('Eigenes Layout')}</p>}
          {uploadFehler.length > 0 && (
            <div role="alert" className="break-words text-sm text-danger">
              {uploadFehler.map(({ platform, text }) => (
                <p key={platform}>
                  {PLATFORM_LABELS[platform]}: {platform === 'tiktok' && text?.includes('unaudited_client_can_only_post_to_private_accounts')
                    ? t('TikTok erlaubt für diese App derzeit nur „Nur ich“. Prüfe die TikTok-Freigabe und plane den Clip erneut ein.')
                    : text?.replace(/^validation failed:\s*/i, '')}
                </p>
              ))}
            </div>
          )}
          {previewActive && preview.data?.status === null && (
            <p role="status" className="text-sm text-text-secondary">{t('Noch keine Vorschau vorhanden. Über „Weitere Aktionen“ kannst du sie rendern.')}</p>
          )}
          {(render.isError || preview.isError || preview.data?.status === 'error') && (
            <p role="alert" className="text-sm text-danger">
              {t('Vorschau konnte nicht gerendert werden.')}
            </p>
          )}
          {cancelResult && (
            <p role="status" className="text-sm text-text-secondary">
              {cancelResult.already_running
                ? t('Geplante Posts gestoppt. Bei {count} Plattformen hat die Übertragung bereits begonnen und lässt sich hier nicht mehr stoppen.', {
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
          {contest.data && (
            <p
              role={contest.data.ok ? 'status' : 'alert'}
              className={`text-sm ${contest.data.ok ? 'text-success' : 'text-danger'}`}
            >
              {t(contest.data.message)}
            </p>
          )}
          {contest.isError && (
            <p role="alert" className="text-sm text-danger">
              {t('Das klappt gerade nicht. Versuch es gleich nochmal.')}
            </p>
          )}
        </div>
        <div className="studio-clip-actions">
          <button type="button" className="studio-button" onClick={onOpenPlatformStatus}>
            {t('Plattformstatus')}
          </button>
          {!canDecide && !terminal && !clip.platform_status.tiktok
            && clip.approval?.state === 'approved'
            && clip.approval.approved_platforms.includes('tiktok')
            && ['pending', 'failed', 'waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.tiktok ?? 'pending') && (
            <button type="button" className="studio-button" disabled={approvalPending}
              onClick={() => onApprovalDecision('approve', clip.approval?.approved_platforms ?? ['tiktok'])}>
              {clip.upload_states?.tiktok === 'waiting_schedule'
                || (clip.upload_states?.tiktok === 'waiting_tiktok_approval'
                  && (!clip.scheduled_at?.tiktok || Date.parse(clip.scheduled_at.tiktok) <= now))
                ? t('TikTok erneut einplanen')
                : t('TikTok-Veröffentlichung vorbereiten')}
            </button>
          )}
          {canDecide ? (
            <>
              <button
                type="button"
                className="studio-primary"
                disabled={approvalPending || eligiblePlatforms.length === 0}
                onClick={() => onApprovalDecision('approve', eligiblePlatforms)}
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
                disabled={terminal || clip.status === 'publishing' || clip.source_kind === 'manual_upload'}
                onClick={() => runMenu(onOpenAiEditor)}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
              >
                <Clapperboard className="h-4 w-4" />
                {t('Mit KI bearbeiten')}
              </button>
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
                <Pencil className="h-4 w-4" />
                {t('Titel, Beschreibung & Hashtags')}
              </button>
              <button
                type="button"
                disabled={preview.isFetching || terminal}
                onClick={() => runMenu(() => { setPreviewActive(true); if (previewActive) void preview.refetch(); })}
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
              >
                <Film className="h-4 w-4" />
                {t('Vorschau prüfen')}
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
              {clip.source_kind !== 'manual_upload' && (
                <button
                  type="button"
                  disabled={contest.isPending || contest.data?.status === 'accepted'}
                  onClick={() => runMenu(() => contest.mutate())}
                  className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-white/5"
                >
                  <Trophy className="h-4 w-4" />
                  {t('Für Clip-Contest einreichen')}
                </button>
              )}
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
