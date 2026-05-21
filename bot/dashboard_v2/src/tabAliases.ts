import type { TabId } from './types/billing';

export interface ResolvedTab {
  tab: TabId;
  sub?: string;
  mode?: string;
}

const TAB_ALIASES: Record<string, ResolvedTab> = {
  overview: { tab: 'overview' },
  streams: { tab: 'streams' },
  audience: { tab: 'audience', sub: 'ueberblick' },
  chat: { tab: 'audience', sub: 'chat' },
  viewers: { tab: 'audience', sub: 'viewer' },
  growth: { tab: 'growth', sub: 'trends' },
  compare: { tab: 'growth', sub: 'vergleich' },
  category: { tab: 'growth', sub: 'markt' },
  experimental: { tab: 'growth', sub: 'experimentell' },
  schedule: { tab: 'planning', sub: 'zeitplan' },
  title: { tab: 'planning', sub: 'titel' },
  planning: { tab: 'planning' },
  coaching: { tab: 'coaching', mode: 'empfehlungen' },
  ai: { tab: 'coaching', mode: 'ki' },
  reports: { tab: 'coaching', mode: 'session' },
  monetization: { tab: 'monetization' },
};

export function resolveTabParam(raw: string | null): ResolvedTab | null {
  if (!raw) return null;
  return TAB_ALIASES[raw] ?? null;
}
