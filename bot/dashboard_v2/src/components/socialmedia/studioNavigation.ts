import type { SocialMediaView } from './labels';

export interface AiEditorMountProps {
  twitchUserId: string;
  streamer: string;
  clipDbId?: number;
}

export function socialViewFromSearch(search: string): SocialMediaView {
  const params = new URLSearchParams(search);
  if (params.has('oauth_success') || params.has('oauth_error')) return 'konten';
  const view = params.get('view');
  if (view === 'plan' || view === 'library' || view === 'layout' || view === 'konten' || view === 'archiv' || view === 'ai') return view;
  if (view === 'schedule' || view === 'autopilot') return 'plan';
  if (view === 'import') return 'library';
  if (view === 'review') return 'ai';
  if (view === 'settings') return 'konten';
  if (view === 'vod') return 'archiv';
  return 'pool';
}

export function socialWorkspaceView(view: SocialMediaView): SocialMediaView {
  return view === 'layout' ? 'konten' : view;
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

export function socialViewSearch(search: string, view: SocialMediaView, clipDbId?: number): string {
  const params = new URLSearchParams(search);
  params.set('view', view);
  params.delete('oauth_success');
  params.delete('oauth_error');
  if (view === 'ai' && clipDbId !== undefined) params.set('clip_db_id', String(clipDbId));
  else params.delete('clip_db_id');
  return params.toString();
}
