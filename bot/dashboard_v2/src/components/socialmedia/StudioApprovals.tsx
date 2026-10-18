import { useEffect, useId, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { CalendarPlus, CheckCircle2, Film, FlaskConical, MessageSquareText, Sparkles, Trash2 } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import type { SocialClipMitPosting } from '@/api/socialMedia';
import type { PublishPlatform, PublishReviewClient, PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';
import type { AiJobView } from './ai/api';
import { proposalCost } from './ai/proposalCost';
import type { TestRunApi } from './TestRunControl';
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
  testRunApi?: TestRunApi;
  onTestRunChanged?: () => void;
}

export function ApprovalEntries({ videos, ...context }: ApprovalContext & { videos: PublishVideo[] }) {
  return <ul className="studio-approvals">{videos.map(video => <li key={video.id}><ApprovalEntry video={video} {...context} /></li>)}</ul>;
}

function ApprovalEntry({ video, snapshot, clips, owner, client, timezone, onNavigate, testRunApi, onTestRunChanged }: ApprovalContext & { video: PublishVideo }) {
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
      {video.test_job_id && <span className="studio-mode-badge" data-mode="test"><FlaskConical className="h-3 w-3" aria-hidden />{t('Testlauf')}</span>}
    </div>
    {owner ? <ApprovalActions video={video} snapshot={snapshot} owner={owner} client={client} onNavigate={onNavigate} testRunApi={testRunApi} onTestRunChanged={onTestRunChanged} />
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

function ApprovalActions({ video, snapshot, owner, client, onNavigate, testRunApi, onTestRunChanged }: {
  video: PublishVideo; snapshot: PublishSnapshot; owner: string; client?: PublishReviewClient;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
  testRunApi?: TestRunApi; onTestRunChanged?: () => void;
}) {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const feedbackId = useId();
  const [revising, setRevising] = useState(false);
  const [feedback, setFeedback] = useState('');
  const [pending, setPending] = useState<AiJobView | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const feedbackBytes = new TextEncoder().encode(feedback).length;
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
    onSuccess: () => { refresh(); if (video.test_job_id) onTestRunChanged?.(); },
  });
  const testJobId = video.test_job_id;
  const discard = useMutation({
    mutationFn: async () => {
      if (!testRunApi || !testJobId) throw new Error(t('Die Aktion ist fehlgeschlagen.'));
      await testRunApi.discardTestRun(testJobId, owner);
    },
    onSuccess: () => { refresh(); onTestRunChanged?.(); },
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
    onSuccess: job => {
      setRevising(false);
      setFeedback('');
      setConfirmed(false);
      setPending(job.status === 'awaiting_confirmation' && job.revision_proposal ? job : null);
      refresh();
    },
  });
  const decide = useMutation({
    mutationFn: async ({ id, accept }: { id: number; accept: boolean }) => (await import('./ai/api')).decideAiProposal(owner, id, accept),
    onSuccess: () => {
      setPending(null);
      setConfirmed(false);
      refresh();
    },
  });
  const busy = approve.isPending || revise.isPending || decide.isPending || discard.isPending;
  const error = approve.error ?? revise.error ?? decide.error ?? discard.error;
  const proposal = pending?.revision_proposal;
  return <div className="studio-approval-actions" data-unsaved={feedback.trim() || busy ? 'true' : undefined} data-saving={busy ? 'true' : undefined}>
    <div className="studio-approval-buttons">
      <button type="button" className="studio-button studio-primary" disabled={busy || platforms.length === 0} title={platforms.length === 0 ? t('Kein verbundenes Ziel ohne weitere Angaben verfügbar') : undefined} onClick={() => approve.mutate()}>
        {testJobId ? <CalendarPlus className="h-4 w-4" aria-hidden /> : <CheckCircle2 className="h-4 w-4" aria-hidden />}
        {testJobId ? t(approve.isPending ? 'Wird eingeplant …' : 'Für nächsten Termin einplanen') : t(approve.isPending ? 'Wird freigegeben …' : 'Freigeben')}
      </button>
      <button type="button" className="studio-button" disabled={busy} aria-expanded={revising} aria-controls={feedbackId}
        onClick={() => video.mode === 'ai' ? setRevising(open => !open) : onNavigate('clips', 'review')}>
        <MessageSquareText className="h-4 w-4" aria-hidden />{t('Änderung wünschen')}
      </button>
      {testJobId && testRunApi && <button type="button" className="studio-button" disabled={busy} onClick={() => discard.mutate()}>
        <Trash2 className="h-4 w-4" aria-hidden />{t(discard.isPending ? 'Wird verworfen …' : 'Test verwerfen (Clip zurück in den Vorrat)')}
      </button>}
    </div>
    {videoPlatforms(snapshot, video).includes('tiktok') && <p className="studio-approval-note">{t('TikTok braucht eigene Angaben und wird beim Prüfen freigegeben.')}</p>}
    {approve.isSuccess && <p className="studio-approval-note" role="status">{t(testJobId ? 'Für den nächsten Termin eingeplant.' : 'Freigegeben.')}</p>}
    {revising && <form id={feedbackId} className="studio-approval-feedback" onSubmit={event => { event.preventDefault(); if (feedback.trim() && feedbackBytes <= 2000) revise.mutate(feedback.trim()); }}>
      <label>
        <span>{t('Was soll die KI ändern?')}</span>
        <textarea className="studio-input" rows={3} maxLength={2000} value={feedback} disabled={busy}
          placeholder={t('Zum Beispiel: Kürze den Einstieg, aber behalte die ursprüngliche Reaktion.')}
          onChange={event => setFeedback(event.target.value)} />
      </label>
      {feedbackBytes > 2000 && <p className="studio-approval-error" role="alert">{t('Bitte kürze deine Anweisung, bevor du sie abschickst.')}</p>}
      <div className="studio-approval-buttons">
        <button type="submit" className="studio-button studio-primary" disabled={busy || !feedback.trim() || feedbackBytes > 2000}>{t(revise.isPending ? 'Wird gesendet …' : 'Änderung senden')}</button>
        <button type="button" className="studio-button" disabled={busy} onClick={() => { setRevising(false); setFeedback(''); revise.reset(); }}>{t('Abbrechen')}</button>
      </div>
    </form>}
    {pending && proposal && <div className="studio-approval-feedback" role="group" aria-label={t('Aufwendige Änderung bestätigen')}>
      <strong>{t('Vor dem nächsten Schritt bestätigen')}</strong>
      <p>{proposal.summary}</p>
      <p>{proposal.impact}</p>
      <p>{proposal.reanalyze ? t('Dafür ist eine erneute KI-Analyse nötig.') : t('Der geplante Schnitt verändert die bisherige Fassung deutlich.')}</p>
      <p>{proposalCost(proposal, locale, t)}</p>
      <label><input type="checkbox" checked={confirmed} disabled={busy} onChange={event => setConfirmed(event.target.checked)} />{t('Ich bestätige den vorgeschlagenen nächsten Bearbeitungsschritt und mögliche zusätzliche KI-Kosten.')}</label>
      <div className="studio-approval-buttons">
        <button type="button" className="studio-button studio-primary" disabled={busy || !confirmed} onClick={() => decide.mutate({ id: pending.id, accept: true })}>{t('Änderung bestätigen')}</button>
        <button type="button" className="studio-button" disabled={busy} onClick={() => decide.mutate({ id: pending.id, accept: false })}>{t('Änderung verwerfen')}</button>
      </div>
    </div>}
    {revise.isSuccess && !pending && !decide.isSuccess && <p className="studio-approval-note" role="status">{t('Änderungswunsch gesendet. Die neue Version erscheint hier zur Freigabe.')}</p>}
    {decide.isSuccess && <p className="studio-approval-note" role="status">{t(decide.variables?.accept ? 'Änderung bestätigt. Die Bearbeitung wird fortgesetzt.' : 'Änderung verworfen. Die vorherige Fassung bleibt erhalten.')}</p>}
    {error && <p className="studio-approval-error" role="alert">{error instanceof Error ? error.message : t('Die Aktion ist fehlgeschlagen.')}</p>}
  </div>;
}
