export type PreviewWorkspaceState = 'ready' | 'loading' | 'error' | 'empty';

export function previewWorkspaceState(
  search: string,
  workspace: 'automation' | 'library' | 'ai',
): PreviewWorkspaceState {
  const fixture = new URLSearchParams(search).get('fixture');
  for (const state of ['loading', 'error', 'empty'] as const) {
    if (fixture === `${workspace}-${state}`) return state;
  }
  return 'ready';
}
