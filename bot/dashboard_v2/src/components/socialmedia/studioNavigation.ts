import type { SocialMediaView, SocialMediaViewTarget } from './labels';
import type { QueueStage } from './queuePresentation';

export interface AiEditorMountProps {
  twitchUserId: string;
  streamer: string;
  clipDbId?: number;
  settingsOnly?: boolean;
}

const VIEW_ALIASES: Record<string, SocialMediaView> = {
  heute: 'heute',
  clips: 'clips',
  freigabe: 'clips',
  pool: 'clips',
  ai: 'clips',
  review: 'clips',
  veroeffentlicht: 'clips',
  library: 'clips',
  import: 'clips',
  planen: 'planen',
  plan: 'planen',
  schedule: 'planen',
  autopilot: 'planen',
  vod: 'vod',
  archiv: 'vod',
  einstellungen: 'einstellungen',
  konten: 'einstellungen',
  settings: 'einstellungen',
  layout: 'einstellungen',
};

const STAGE_ALIASES: Record<string, QueueStage> = {
  freigabe: 'review',
  review: 'review',
  veroeffentlicht: 'posted',
  vorbereitung: 'preparing',
  aussortiert: 'failed',
};

const STAGES: QueueStage[] = ['all', 'new', 'preparing', 'review', 'planned', 'posted', 'failed', 'archive'];

export function socialViewFromSearch(search: string): SocialMediaView {
  const params = new URLSearchParams(search);
  if (params.has('oauth_success') || params.has('oauth_error')) return 'einstellungen';
  return VIEW_ALIASES[params.get('view') ?? ''] ?? 'heute';
}

export function socialStageFromSearch(search: string): QueueStage | undefined {
  const params = new URLSearchParams(search);
  if (socialViewFromSearch(search) !== 'clips') return undefined;
  const stage = params.get('stage');
  if (stage && (STAGES as string[]).includes(stage)) return stage as QueueStage;
  return STAGE_ALIASES[stage ?? ''] ?? STAGE_ALIASES[params.get('view') ?? ''];
}

export function socialWorkspaceDepartureAllowed(
  scope: Pick<ParentNode, 'querySelector'>,
  confirmDiscard: () => boolean,
): boolean {
  if (scope.querySelector('[data-saving="true"]')) return false;
  return !scope.querySelector('[data-unsaved="true"]') || confirmDiscard();
}

export function socialViewSearch(search: string, view: SocialMediaViewTarget, stage?: QueueStage): string {
  const params = new URLSearchParams(search);
  const canonical = socialViewFromSearch(`?view=${view}`);
  const nextStage = stage ?? socialStageFromSearch(`?view=${view}`);
  params.set('view', canonical);
  params.delete('oauth_success');
  params.delete('oauth_error');
  params.delete('clip_db_id');
  if (canonical === 'clips' && nextStage && nextStage !== 'all') params.set('stage', nextStage);
  else params.delete('stage');
  return params.toString();
}
