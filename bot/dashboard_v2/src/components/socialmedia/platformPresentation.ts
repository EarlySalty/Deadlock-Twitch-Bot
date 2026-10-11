import type { PlatformStatus, SocialClipMitPosting } from '../../api/socialMedia';
import type { SocialPlatform } from '../../types/socialMedia';

export type ConnectionLoadState = 'loading' | 'error' | 'ready';
export type StatusTone = 'neutral' | 'success' | 'warning' | 'danger';
export interface PlatformFact {
  state: string;
  label: string;
  tone: StatusTone;
}

const fact = (state: string, label: string, tone: StatusTone = 'neutral'): PlatformFact => ({ state, label, tone });

export function connectionFact(status: PlatformStatus | undefined, loading = false, error = false): PlatformFact {
  if (loading) return fact('loading', 'Wird geladen');
  if (error) return fact('error', 'Status nicht abrufbar', 'danger');
  if (!status) return fact('unknown', 'Zustand unbekannt');
  if (!status.connected) return fact('disconnected', 'Nicht verbunden', 'warning');
  if (status.expired || status.needs_reauth) return fact('expired', 'Neu verbinden', 'warning');
  if (status.authorization?.channel_account_required || (status.platform === 'tiktok' && status.uses_global_fallback)) return fact('channel_required', 'Eigenes Kanalkonto erforderlich', 'warning');
  return fact('connected', 'Verbunden', 'success');
}

export function authorizationFact(status: PlatformStatus | undefined, loading = false, error = false): PlatformFact {
  if (loading) return fact('loading', 'Wird geladen');
  if (error) return fact('error', 'Status nicht abrufbar', 'danger');
  const authorization = status?.authorization;
  if (!authorization || authorization.source !== 'stored') return fact('unknown', 'Upload-Rechte unbekannt');
  if (authorization.upload_scope_status === 'sufficient') return fact('sufficient', 'Upload-Rechte gespeichert', 'success');
  if (authorization.upload_scope_status === 'missing') return fact('missing', 'Upload-Rechte fehlen', 'warning');
  return fact('unknown', 'Upload-Rechte unbekannt');
}

export function approvalFact(clip: SocialClipMitPosting, platform: SocialPlatform): PlatformFact {
  if (clip.approval === undefined) return fact('unknown', 'Freigabestand unbekannt');
  if (clip.approval?.state === 'approved' && clip.approval.approved_platforms.includes(platform)) return fact('approved', 'Clip freigegeben', 'success');
  return fact('not_approved', 'Nicht freigegeben', 'warning');
}

export function transferFact(clip: SocialClipMitPosting, platform: SocialPlatform): PlatformFact {
  const state = clip.upload_states?.[platform];
  if (state === 'inbox') return fact('inbox', 'Im TikTok-Postfach');
  if (state === 'inbox_pending') return fact('pending_confirmation', 'Übertragung noch unbestätigt', 'warning');
  if (state === 'waiting_connection') return fact('waiting_connection', 'Wartet auf Verbindung', 'warning');
  if (state === 'waiting_tiktok_approval') return fact('waiting_consent', 'Wartet auf TikTok-Zustimmung', 'warning');
  if (state === 'waiting_schedule') return fact('waiting_schedule', 'Wartet auf Termin');
  if (state === 'failed' || clip.upload_errors?.[platform]) return fact('failed', 'Übertragung fehlgeschlagen', 'danger');
  if (state === 'uploading' || state === 'processing') return fact('uploading', 'Übertragung läuft');
  if (clip.platform_status[platform] || state === 'completed' || state === 'uploaded') return fact('transferred', 'Übertragen', 'success');
  if (clip.scheduled_at?.[platform] || state === 'pending' || state === 'queued') return fact('queued', 'Eingeplant');
  return fact('unknown', 'Kein Übertragungsstand');
}

export function publicationFact(clip: SocialClipMitPosting, platform: SocialPlatform): PlatformFact {
  const state = clip.publish_states?.[platform];
  const upload = clip.upload_states?.[platform];
  if (state === 'FAILED') return fact('failed', 'Veröffentlichung abgelehnt', 'danger');
  if (platform === 'tiktok' && state === 'PUBLISH_COMPLETE') return fact('confirmed', 'Veröffentlichung bestätigt', 'success');
  if (platform === 'tiktok' && (upload === 'inbox' || upload === 'inbox_pending' || state === 'SEND_TO_USER_INBOX')) return fact('inbox', 'Veröffentlichung in TikTok erforderlich', 'warning');
  if (platform === 'youtube' && clip.platform_status.youtube) {
    if (clip.youtube_visibility === 'private') return fact('private', 'Privat auf YouTube');
    if (clip.youtube_visibility === 'unlisted') return fact('unlisted', 'Nicht gelistet auf YouTube');
    if (clip.youtube_visibility === 'public') return fact('public', 'Öffentlich auf YouTube', 'success');
    return fact('unknown', 'Sichtbarkeit nicht bestätigt');
  }
  if (state === 'PUBLISH_COMPLETE') return fact('confirmed', 'Veröffentlichung bestätigt', 'success');
  if (state === 'PROCESSING_UPLOAD' || state === 'PROCESSING_DOWNLOAD') return fact('processing', 'Wird beim Anbieter verarbeitet');
  return fact('unknown', 'Veröffentlichung nicht bestätigt');
}

export function clipPlatformPresentation(clip: SocialClipMitPosting, platform: SocialPlatform, status: PlatformStatus | undefined, load: ConnectionLoadState) {
  return {
    connection: connectionFact(status, load === 'loading', load === 'error'),
    authorization: authorizationFact(status, load === 'loading', load === 'error'),
    approval: approvalFact(clip, platform),
    transfer: transferFact(clip, platform),
    publication: publicationFact(clip, platform),
  };
}
