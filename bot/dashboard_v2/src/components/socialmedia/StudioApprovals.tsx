import { useEffect, useId, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { CheckCircle2, Film, MessageSquareText, Sparkles } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import type { SocialClipMitPosting } from '@/api/socialMedia';
import type { PublishPlatform, PublishReviewClient, PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';
import type { AiJobView } from './ai/api';
import type { SocialMediaViewTarget } from './labels';
import type { QueueStage } from './queuePresentation';
import { formatTerminInZone } from './queuePresentation';

const PUBLISH_PLATFORMS: PublishPlatform[] = ['youtube', 'tiktok', 'instagram'];

async function resolveClient(client: PublishReviewClient | undefined): Promise<PublishReviewClient> {
  return client ?? (await import('./autopilot/publish-review/api')).publishReviewClient;
}

function videoPlatforms(snapshot: PublishSnapshot, video: PublishVideo): PublishPlatform[] {
  return [...new Set<PublishPlatform>([...snapshot.target_platforms, ...(video.event?.destinations.map(destination => destination.platform) ?? []), ...video.approvals.map(approval => approval.platform)])]
    .filter(platform => PUBLISH_PLATFORMS.includes(platform));
}

function quickApprovalPlatforms(snapshot: PublishSnapshot, video: PublishVideo): PublishPlatform[] {
  return videoPlatforms(snapshot, video).filter(platform => platform !== 'tiktok'
    && snapshot.connections.some(connection => connection.platform === platform && connection.connected));
}

function revisionVersion(job: AiJobView, videoId: number) {
  return job.versions.find(version => version.id === videoId)
    ?? job.versions.find(version => version.id === job.selected_version_id)
    ?? job.versions[job.versions.length - 1];
}

interface ApprovalContext {
  snapshot: PublishSnapshot;
  clips: SocialClipMitPosting[] | undefined;
  owner?: string;
  client?: PublishReviewClient;
  timezone: string;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
}

export function ApprovalEntries({ videos, ...context }: ApprovalContext & { videos: PublishVideo[] }) {
  return <ul className="studio-approvals">{videos.map(video => <li key={video.id}><ApprovalEntry video={video} {...context} /></li>)}</ul>;
}

function ApprovalEntry({ video, snapshot, clips, owner, client, timezone, onNavigate }: ApprovalContext & { video: PublishVideo }) {
  const { t, locale } = useLanguage();
  const clip = clips?.find(item => item.clip_db_id === video.clip_id);
  const title = clip?.generated_title?.trim() || clip?.title || t('Video {id}', { id: video.id });
  const planned = video.event?.scheduled_at;
  return <article className="studio-approval" aria-label={title}>
    <div className="studio-approval-media">
      {clip?.thumbnail_url ? <img src={clip.thumbnail_url} alt="" loading="lazy" />
        : owner ? <ApprovalVideo owner={owner} videoId={video.id} client={client} /> : <Film className="h-5 w-5" aria-hidden />}
    </div>
    <div className="studio-approval-body">
      <strong title={title}>{title}</strong>
      <span>{planned ? `${t('Geplant für {date}', { date: formatTerminInZone(planned, locale, timezone) })} · ${timezone}` : t('Termin wird nach der Freigabe vergeben')}</span>
      <span className="studio-mode-badge" data-mode={video.mode}>{video.mode === 'ai' ? <Sparkles className="h-3 w-3" aria-hidden /> : <Film className="h-3 w-3" aria-hidden />}{t(video.mode === 'ai' ? 'KI-Schnitt' : 'Standard-Vorlage')}</span>
    </div>
    {owner ? <ApprovalActions video={video} snapshot={snapshot} owner={owner} client={client} onNavigate={onNavigate} />
      : <div className="studio-approval-actions"><button type="button" className="studio-button" onClick={() => onNavigate('clips', 'review')}>{t('Prüfen')}</button></div>}
  </article>;
}

function ApprovalVideo({ owner, videoId, client }: { owner: string; videoId: number; client?: PublishReviewClient }) {
  const { t } = useLanguage();
  const [src, setSrc] = useState<string | null>(() => client?.videoUrl?.(owner, videoId) ?? null);
  useEffect(() => {
    let active = true;
    void resolveClient(client).then(api => { if (active) setSrc(api.videoUrl?.(owner, videoId) ?? null); });
    return () => { active = false; };
  }, [client, owner, videoId]);
  return src ? <video src={src} preload="metadata" muted playsInline aria-label={t('Vorschau Video #{id}', { id: videoId })} /> : <Film className="h-5 w-5" aria-hidden />;
}

function ApprovalActions({ video, snapshot, owner, client, onNavigate }: {
  video: PublishVideo; snapshot: PublishSnapshot; owner: string; client?: PublishReviewClient;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
}) {
  const { t } = useLanguage();
  const queryClient = useQueryClient();
  const feedbackId = useId();
  const [revising, setRevising] = useState(false);
  const [feedback, setFeedback] = useState('');
  const platforms = quickApprovalPlatforms(snapshot, video);
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ['social-media', 'autopilot-publish', owner] });
    void queryClient.invalidateQueries({ queryKey: ['social-ai', 'jobs', owner] });
  };
  const approve = useMutation({
    mutationFn: async () => {
      const api = await resolveClient(client);
      await api.approve(owner, video.id, platforms.map(platform => ({ platform })));
      if (!video.event) await api.schedule(owner, video.id, videoPlatforms(snapshot, video));
    },
    onSuccess: refresh,
  });
  const revise = useMutation({
    mutationFn: async (text: string) => {
      const { fetchAiConsent, openAiVideoEditor, reviseAiVersion } = await import('./ai/api');
      const consent = await fetchAiConsent(owner);
      if (!consent.global_allowed || !consent.external_ai_consent) throw new Error(t('Für Änderungswünsche fehlt die Zustimmung zur externen KI-Verarbeitung.'));
      const job = await openAiVideoEditor(owner, video.id);
      const version = revisionVersion(job, video.id);
      if (!version) throw new Error(t('Für dieses Video ist keine bearbeitbare Version vorhanden.'));
      return reviseAiVersion(owner, job.id, version.id, text);
    },
    onSuccess: () => {
      setRevising(false);
      setFeedback('');
      refresh();
    },
  });
  const busy = approve.isPending || revise.isPending;
  const error = approve.error ?? revise.error;
  return <div className="studio-approval-actions" data-unsaved={feedback.trim() || busy ? 'true' : undefined} data-saving={busy ? 'true' : undefined}>
    <div className="studio-approval-buttons">
      <button type="button" className="studio-button studio-primary" disabled={busy || platforms.length === 0} title={platforms.length === 0 ? t('Kein verbundenes Ziel ohne weitere Angaben verfügbar') : undefined} onClick={() => approve.mutate()}>
        <CheckCircle2 className="h-4 w-4" aria-hidden />{t(approve.isPending ? 'Wird freigegeben …' : 'Freigeben')}
      </button>
      <button type="button" className="studio-button" disabled={busy} aria-expanded={revising} aria-controls={feedbackId}
        onClick={() => video.mode === 'ai' ? setRevising(open => !open) : onNavigate('clips', 'review')}>
        <MessageSquareText className="h-4 w-4" aria-hidden />{t('Änderung wünschen')}
      </button>
    </div>
    {approve.isSuccess && <p className="studio-approval-note" role="status">{t('Freigegeben.')}</p>}
    {revising && <form id={feedbackId} className="studio-approval-feedback" onSubmit={event => { event.preventDefault(); if (feedback.trim()) revise.mutate(feedback.trim()); }}>
      <label>
        <span>{t('Was soll die KI ändern?')}</span>
        <textarea className="studio-input" rows={3} maxLength={2000} value={feedback} disabled={busy}
          placeholder={t('Zum Beispiel: Kürze den Einstieg, aber behalte die ursprüngliche Reaktion.')}
          onChange={event => setFeedback(event.target.value)} />
      </label>
      <div className="studio-approval-buttons">
        <button type="submit" className="studio-button studio-primary" disabled={busy || !feedback.trim()}>{t(revise.isPending ? 'Wird gesendet …' : 'Änderung senden')}</button>
        <button type="button" className="studio-button" disabled={busy} onClick={() => { setRevising(false); setFeedback(''); revise.reset(); }}>{t('Abbrechen')}</button>
      </div>
    </form>}
    {revise.isSuccess && <p className="studio-approval-note" role="status">{t('Änderungswunsch gesendet. Die neue Version erscheint hier zur Freigabe.')}</p>}
    {error && <p className="studio-approval-error" role="alert">{error instanceof Error ? error.message : t('Die Aktion ist fehlgeschlagen.')}</p>}
  </div>;
}
