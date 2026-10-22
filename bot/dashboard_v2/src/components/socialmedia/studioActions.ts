import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';
import { PLATFORM_LABELS } from './labels';
import { connectionFact } from './platformPresentation';
import { queueStage, type QueueStage } from './queuePresentation';
import type { SocialPlatform } from '@/types/socialMedia';

type Translate = (key: string, params?: Record<string, string | number>) => string;
interface StudioAction { key: string; title: string; meta: string; tone: 'warning' | 'danger'; view: SocialMediaViewTarget; stage?: QueueStage }

export function studioActions(data: StudioOverviewData, now: number, t: Translate): StudioAction[] {
  const schedule = data.scheduleFailed ? undefined : data.schedule;
  const publish = data.publishFailed ? undefined : data.publish;
  const clips = data.clipsFailed ? [] : data.clips ?? [];
  const platforms = data.platformsFailed ? [] : data.platforms ?? [];
  const rows: StudioAction[] = [];
  if (schedule?.production_hold === 'empty_pool' || schedule?.production_hold === 'no_schedule') rows.push({ key: 'hold', title: t(schedule.production_hold === 'empty_pool' ? 'Keine passenden Clips verfügbar' : 'Wöchentliche Termine fehlen'), meta: t('Der Auto-Pilot wartet auf deine Aktion'), tone: 'warning', view: 'planen' });
  if (schedule?.review_count) rows.push({ key: 'reviews', title: t(schedule.review_count === 1 ? '{count} offene Prüfung' : '{count} offene Prüfungen', { count: schedule.review_count }), meta: t('Auftrag oder Videofreigabe prüfen'), tone: 'warning', view: schedule.jobs.some(job => job.status === 'review') ? 'planen' : 'freigabe' });
  const videoReviews = publish?.videos.filter(video => video.artifact_state === 'present' && video.status === 'review').length ?? 0;
  if (videoReviews) rows.push({ key: 'video-reviews', title: t(videoReviews === 1 ? '{count} fertiges Video prüfen' : '{count} fertige Videos prüfen', { count: videoReviews }), meta: t('Fertige Videos'), tone: 'warning', view: 'freigabe' });
  const clipReviews = clips.filter(clip => queueStage(clip) === 'review').length;
  if (clipReviews) rows.push({ key: 'clips', title: t(clipReviews === 1 ? '{count} Clip freigeben' : '{count} Clips freigeben', { count: clipReviews }), meta: t('Freigabe aus der bisherigen Pipeline'), tone: 'warning', view: 'freigabe', stage: 'review' });
  const failedJobs = schedule?.jobs.filter(job => job.status === 'failed' && job.source !== 'test' && !job.superseded).length ?? 0;
  if (failedJobs) rows.push({ key: 'jobs', title: t(failedJobs === 1 ? '{count} Produktion fehlgeschlagen' : '{count} Produktionen fehlgeschlagen', { count: failedJobs }), meta: t('Auftrag prüfen oder entfernen'), tone: 'danger', view: 'clips', stage: 'failed' });
  const failedClips = clips.filter(clip => queueStage(clip) === 'failed').length;
  if (failedClips) rows.push({ key: 'failed', title: t('{count} Clips mit Fehler', { count: failedClips }), meta: t('Veröffentlichung erneut prüfen'), tone: 'danger', view: 'veroeffentlicht', stage: 'failed' });
  for (const video of publish?.videos ?? []) if (video.event?.destinations.some(destination => ['retry', 'held', 'uncertain', 'failed'].includes(destination.state))) rows.push({ key: `publish-${video.id}`, title: clips.find(clip => clip.clip_db_id === video.clip_id)?.title || t('Video {id}', { id: video.id }), meta: t('Veröffentlichung erneut prüfen'), tone: 'warning', view: 'clips', stage: 'failed' });
  const tiktokDrafts = clips.filter(clip => clip.upload_states?.tiktok === 'waiting_connection').length;
  if (tiktokDrafts) rows.push({ key: 'tiktok-drafts', title: t(tiktokDrafts === 1 ? '{count} TikTok-Entwurf braucht eine Neuverbindung' : '{count} TikTok-Entwürfe brauchen eine Neuverbindung', { count: tiktokDrafts }), meta: t('TikTok neu verbinden'), tone: 'warning', view: 'einstellungen' });
  for (const status of platforms.filter(platform => platform.capabilities?.upload && !(tiktokDrafts && platform.platform === 'tiktok'))) {
    const expiry = status.platform === 'instagram' || !status.automatically_renewed ? status.expires_at : status.refresh_expires_at;
    const expiring = status.reauth_soon || Boolean(expiry && Date.parse(expiry) <= now + 7 * 86400000);
    if (connectionFact(status).state !== 'connected' || expiring) rows.push({ key: `platform-${status.platform}`, title: PLATFORM_LABELS[status.platform as SocialPlatform] ?? status.platform, meta: t(expiring && connectionFact(status).state === 'connected' ? 'Verbindung läuft bald ab' : connectionFact(status).label), tone: 'warning', view: 'einstellungen' });
  }
  return rows;
}
