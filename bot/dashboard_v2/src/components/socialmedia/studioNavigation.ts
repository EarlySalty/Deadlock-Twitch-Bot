import type { ScheduleQueueApi } from '@/components/socialmedia/autopilot/schedule-queue';
import type { SocialMediaView, SocialMediaViewTarget } from './labels';

export interface AiEditorMountProps {
  twitchUserId: string;
  streamer: string;
  clipDbId?: number;
  settingsOnly?: boolean;
  scheduleApi?: ScheduleQueueApi;
}

export function socialViewFromSearch(search: string): SocialMediaView {
  const params = new URLSearchParams(search);
  if (params.has('oauth_success') || params.has('oauth_error')) return 'einstellungen';
  const view = params.get('view');
  if (view === 'heute' || view === 'freigabe' || view === 'planen' || view === 'veroeffentlicht' || view === 'einstellungen') return view;
  if (view === 'pool' || view === 'ai' || view === 'review') return 'freigabe';
  if (view === 'plan' || view === 'schedule' || view === 'autopilot' || view === 'library' || view === 'import') return 'planen';
  if (view === 'konten' || view === 'settings' || view === 'layout') return 'einstellungen';
  if (view === 'archiv' || view === 'vod') return 'veroeffentlicht';
  return 'heute';
}

export function socialWorkspaceView(view: SocialMediaViewTarget) {
  const canonical = socialViewFromSearch(`?view=${view}`);
  if (canonical === 'freigabe') return 'pool';
  if (canonical === 'planen') return 'plan';
  if (canonical === 'einstellungen') return 'konten';
  if (canonical === 'veroeffentlicht') return 'archiv';
  return 'heute';
}

export function socialAiClipFromSearch(search: string): number | undefined {
  const value = new URLSearchParams(search).get('clip_db_id');
  if (!value || !/^\d+$/.test(value)) return undefined;
  const id = Number(value);
  return Number.isSafeInteger(id) && id > 0 ? id : undefined;
}

export function socialWorkspaceDepartureAllowed(
  scope: Pick<ParentNode, 'querySelector'>,
  confirmDiscard: () => boolean,
): boolean {
  if (scope.querySelector('[data-saving="true"]')) return false;
  return !scope.querySelector('[data-unsaved="true"]') || confirmDiscard();
}

export function socialViewSearch(search: string, view: SocialMediaViewTarget, clipDbId?: number): string {
  const params = new URLSearchParams(search);
  const canonical = socialViewFromSearch(`?view=${view}`);
  params.set('view', canonical);
  params.delete('oauth_success');
  params.delete('oauth_error');
  if (canonical === 'freigabe' && clipDbId !== undefined) params.set('clip_db_id', String(clipDbId));
  else params.delete('clip_db_id');
  return params.toString();
}
